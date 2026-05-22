//! `cortex-core` — Rust SNN simulator + REST/WS server.
//!
//! Boot order:
//!   1. Load `.env` + config.
//!   2. Init tracing.
//!   3. Build DB pool, run embedded migrations.
//!   4. Spawn the engine actor task.
//!   5. Hydrate engine from existing DB rows.
//!   6. Build axum router, install graceful-shutdown handler, serve.

use std::sync::Arc;

use anyhow::Context;
use diesel::prelude::*;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod api;
mod chat;
mod config;
mod cortex_type;
mod db;
mod engine;
mod error;
mod vault;

use crate::api::AppState;
use crate::config::CoreConfig;
use crate::db::models::{EdgeRow, NodeRow};
use crate::db::schema::{edges, nodes};
use crate::engine::{spawn_engine, spawn_spike_persister, spawn_weight_persister};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = CoreConfig::from_env().context("loading config")?;
    init_tracing();

    let bind = cfg.bind_addr();
    tracing::info!(bind = %bind, tick_hz = cfg.tick_hz, "starting cortex-core");

    let pool = db::build_pool(&cfg.database_url).context("building db pool")?;
    db::run_migrations(&pool).context("running migrations")?;
    tracing::info!("migrations applied");

    let (engine, engine_join) = spawn_engine(cfg.tick_hz);
    hydrate_engine(&pool, &engine).await?;

    // Spike persister — drains the spike broadcast into spike_log on
    // a periodic flush (or buffer-full). Fire-and-forget; exits when
    // the broadcast closes on shutdown.
    let _spike_persister = spawn_spike_persister(
        engine.clone(),
        pool.clone(),
        cfg.spike_persist_interval_ms,
        cfg.spike_persist_max_batch,
    );
    tracing::info!(
        spike_interval_ms = cfg.spike_persist_interval_ms,
        spike_max_batch = cfg.spike_persist_max_batch,
        "spike persister spawned"
    );

    // Weight persister — fire-and-forget; exits cleanly when the
    // engine channels close on shutdown.
    let _weight_persister = spawn_weight_persister(
        engine.clone(),
        pool.clone(),
        cfg.weight_persist_interval_ms,
        cfg.weight_persist_epsilon,
    );
    tracing::info!(
        weight_interval_ms = cfg.weight_persist_interval_ms,
        weight_epsilon = cfg.weight_persist_epsilon,
        "weight persister spawned"
    );

    let chat_encoder = chat::make_encoder();
    tracing::info!(encoder = chat_encoder.name(), "chat encoder ready");

    // Clone the engine handle before it moves into AppState so the
    // post-axum shutdown hook can still issue commands after the
    // router stops accepting traffic.
    let engine_for_shutdown = engine.clone();

    let state = AppState {
        pool,
        engine,
        vault_path: Arc::new(cfg.vault_path.clone()),
        chat_encoder,
    };
    let app = api::build_router(state);

    let listener = tokio::net::TcpListener::bind(cfg.bind_addr())
        .await
        .with_context(|| format!("binding {}", cfg.bind_addr()))?;
    tracing::info!(addr = %listener.local_addr()?, "listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve loop")?;

    tracing::info!("axum exited; flushing runtime state to open folder");
    // Best-effort save of neuron dynamic state to the open `.cortex/`
    // folder (no-op when running on transient state). Errors are
    // logged, not propagated — a save failure should not mask the
    // shutdown reason. See vault/ideas/runtime-state-persistence-v1.md.
    match engine_for_shutdown.save_state_to_open_folder().await {
        Ok(Some(n)) => tracing::info!(neurons = n, "state flushed to disk"),
        Ok(None) => tracing::debug!("no folder open; skipping state flush"),
        Err(e) => tracing::warn!(error = %e, "state flush failed on shutdown"),
    }
    // Mirror the same best-effort flush for weights. The periodic
    // persister writes on its own cadence, but it targets Postgres
    // and folder-mode weight flushes are gated by the engine actor —
    // we want to guarantee `latest.cwt` reflects the moment of exit.
    match engine_for_shutdown.flush_weights_to_open_folder().await {
        Ok(Some(n)) => tracing::info!(edges = n, "weights flushed to disk"),
        Ok(None) => tracing::debug!("no folder open; skipping weight flush"),
        Err(e) => tracing::warn!(error = %e, "weight flush failed on shutdown"),
    }

    tracing::info!("waiting for engine task to drain");
    drop(engine_for_shutdown);
    drop(engine_join); // engine task exits when cmd_tx drops; safe to detach.

    Ok(())
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,core=debug"));
    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(false),
        )
        .init();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("shutdown: SIGINT"),
        _ = terminate => tracing::info!("shutdown: SIGTERM"),
    }
}

/// Load existing graph rows from Postgres into the live engine. Cheap at
/// M0 scale; will need streaming later.
///
/// COR-30: trained STDP weights survive restart because `EdgeRow.weight`
/// is forwarded into `engine.ingest_batch`, which passes it to
/// `SimEngine::add_edge` → `StdpSynapse::new(... weight)`. So this
/// loader + `spawn_weight_persister` together form the round-trip:
/// in-memory weights flushed to `edges.weight`, then read back here on
/// the next boot.
async fn hydrate_engine(pool: &db::PgPool, engine: &engine::SimHandle) -> anyhow::Result<()> {
    let (node_rows, edge_rows): (Vec<NodeRow>, Vec<EdgeRow>) = db::run_blocking(pool, |conn| {
        let ns = nodes::table.select(NodeRow::as_select()).load(conn)?;
        let es = edges::table.select(EdgeRow::as_select()).load(conn)?;
        Ok((ns, es))
    })
    .await?;

    let n_ids: Vec<uuid::Uuid> = node_rows.iter().map(|n| n.id).collect();
    let e_tuples: Vec<(uuid::Uuid, uuid::Uuid, uuid::Uuid, f32)> = edge_rows
        .iter()
        .map(|e| (e.id, e.pre_id, e.post_id, e.weight))
        .collect();

    let n = n_ids.len();
    let e = e_tuples.len();
    let _ = engine.ingest_batch(n_ids, e_tuples).await;
    tracing::info!(neurons = n, synapses = e, "engine hydrated from db");
    Ok(())
}

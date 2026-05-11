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
mod config;
mod db;
mod domain;
mod engine;
mod error;
mod vault;

use crate::api::AppState;
use crate::config::CoreConfig;
use crate::db::models::{EdgeRow, NodeRow};
use crate::db::schema::{edges, nodes};
use crate::engine::spawn_engine;

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

    let state = AppState {
        pool,
        engine,
        vault_path: Arc::new(cfg.vault_path.clone()),
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

    tracing::info!("axum exited; waiting for engine task to drain");
    drop(engine_join); // engine task exits when cmd_tx drops; safe to detach.

    Ok(())
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,core=debug"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_target(true).with_thread_ids(false))
        .init();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
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
async fn hydrate_engine(pool: &db::PgPool, engine: &engine::SimHandle) -> anyhow::Result<()> {
    let (node_rows, edge_rows): (Vec<NodeRow>, Vec<EdgeRow>) = db::run_blocking(pool, |conn| {
        let ns = nodes::table.select(NodeRow::as_select()).load(conn)?;
        let es = edges::table.select(EdgeRow::as_select()).load(conn)?;
        Ok((ns, es))
    }).await?;

    let n_ids: Vec<uuid::Uuid> = node_rows.iter().map(|n| n.id).collect();
    let e_tuples: Vec<(uuid::Uuid, uuid::Uuid, uuid::Uuid, f32)> = edge_rows.iter()
        .map(|e| (e.id, e.pre_id, e.post_id, e.weight))
        .collect();

    let n = n_ids.len();
    let e = e_tuples.len();
    let _ = engine.ingest_batch(n_ids, e_tuples).await;
    tracing::info!(neurons = n, synapses = e, "engine hydrated from db");
    Ok(())
}

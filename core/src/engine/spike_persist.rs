//! Batched spike persister.
//!
//! Subscribes to the engine spike broadcast, buffers events, and bulk-
//! inserts them into `spike_log` on a periodic flush or when the buffer
//! exceeds `max_batch`. The 200Hz tick rate produces a high spike volume
//! during active stimulation, so single-row inserts would dominate the
//! DB pool; one INSERT per ~250ms keeps round-trips bounded.
//!
//! `RecvError::Lagged(n)` is warned and swallowed — the engine is
//! allowed to outpace the persister for short windows (an ingestion
//! storm, for example); losing some events is preferable to wedging
//! the engine on backpressure.

use std::time::Duration;

use diesel::prelude::*;
use tokio::sync::broadcast::error::RecvError;

use crate::db::models::NewSpike;
use crate::db::schema::spike_log;
use crate::db::{run_blocking, PgPool};
use crate::engine::SimHandle;
use cortex_snn::engine::events::SpikeFrame;

/// Spawn the spike persister.
///
/// `interval_ms` is the timer-driven flush cadence (default 250ms);
/// `max_batch` triggers an early flush when the buffer would otherwise
/// grow unbounded (default 2000).
pub fn spawn_spike_persister(
    handle: SimHandle,
    pool: PgPool,
    interval_ms: u64,
    max_batch: usize,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut rx = handle.spikes.subscribe();
        let mut buf: Vec<NewSpike> = Vec::with_capacity(max_batch.max(64));
        let mut ticker = tokio::time::interval(Duration::from_millis(interval_ms));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                biased;

                _ = ticker.tick() => {
                    if buf.is_empty() { continue; }
                    flush_spikes(&pool, std::mem::take(&mut buf)).await;
                }

                frame = rx.recv() => {
                    match frame {
                        Ok(SpikeFrame { events, .. }) => {
                            for ev in events {
                                buf.push(NewSpike { node_id: ev.node_id, t_ms: ev.t_ms });
                            }
                            if buf.len() >= max_batch {
                                flush_spikes(&pool, std::mem::take(&mut buf)).await;
                            }
                        }
                        Err(RecvError::Lagged(n)) => {
                            tracing::warn!(skipped = n, "spike persister lagged broadcast");
                        }
                        Err(RecvError::Closed) => {
                            // Final drain before exiting.
                            if !buf.is_empty() {
                                flush_spikes(&pool, std::mem::take(&mut buf)).await;
                            }
                            tracing::info!("spike persister: broadcast closed, exiting");
                            return;
                        }
                    }
                }
            }
        }
    })
}

async fn flush_spikes(pool: &PgPool, rows: Vec<NewSpike>) {
    let n = rows.len();
    if n == 0 { return; }
    let result = run_blocking(pool, move |conn| {
        diesel::insert_into(spike_log::table)
            .values(&rows)
            .execute(conn)
            .map_err(crate::error::CoreError::from)
    })
    .await;
    match result {
        Ok(inserted) => tracing::debug!(inserted, batched = n, "spike persister flushed"),
        Err(e) => tracing::warn!(error = %e, batched = n, "spike persister: flush failed"),
    }
}

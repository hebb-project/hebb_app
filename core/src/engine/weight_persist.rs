//! Periodic STDP weight persister.
//!
//! Runs as a sibling task to the engine actor. Pulls `weight_snapshot`
//! on a cadence, diffs against the last persisted set, and issues a
//! single transaction with one `UPDATE edges SET weight = ?, updated_at
//! = ? WHERE id = ?` per changed edge. The diff threshold filters STDP
//! trace noise so stable edges don't trigger writes every tick.
//!
//! Paired with the existing `hydrate_engine` loader in `main.rs`, this
//! closes the loop: in-memory weights → `edges.weight` → loaded back on
//! the next boot via `SimEngine::add_edge(..., weight)`. No additional
//! startup wiring needed.
//!
//! Folder mode: when a `.cortex/` folder is open, the persister asks
//! the engine actor to write a full weight snapshot to
//! `weights/{type}/latest.cwt` instead of issuing per-edge UPDATEs
//! against Postgres. The .cwt writer is atomic (write-temp + fsync +
//! rename) so a crash mid-flush leaves the previous good snapshot in
//! place. The Postgres path remains the source of truth for KG
//! networks and for any engine running without an open folder.

use std::collections::HashMap;
use std::time::Duration;

use diesel::prelude::*;
use uuid::Uuid;

use crate::db::schema::edges;
use crate::db::{run_blocking, PgPool};
use crate::engine::SimHandle;

/// Spawn the periodic weight persister.
///
/// `interval_ms` controls cadence; `epsilon` is the minimum absolute
/// change since the last persisted weight required to issue an UPDATE.
/// Defaults (7s / 0.001) come from `CoreConfig`.
///
/// On the very first tick `last` is empty, so every edge with a finite
/// weight is treated as changed — this is intentional. It lets the
/// row's `updated_at` advance after startup, which is a useful liveness
/// signal even when in-memory and on-disk weights happen to match.
pub fn spawn_weight_persister(
    handle: SimHandle,
    pool: PgPool,
    interval_ms: u64,
    epsilon: f32,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut last: HashMap<Uuid, f32> = HashMap::new();
        let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Discard the immediate first tick — wait one full period before
        // the first flush so startup hydration settles.
        interval.tick().await;

        loop {
            interval.tick().await;

            // Folder mode: write a full snapshot to disk via the engine
            // actor. The actor owns the Cortex handle so we don't race
            // with API-driven topology edits. We skip the per-edge diff
            // because .cwt is rewritten in full anyway — the writer is
            // already atomic and the file size at M0 scale is small.
            match handle.flush_weights_to_open_folder().await {
                Ok(Some(n)) => {
                    tracing::debug!(written = n, "weight persister flushed to .cortex/");
                    continue;
                }
                Ok(None) => {
                    // Fall through to the Postgres path below.
                }
                Err(e) => {
                    tracing::warn!(error = %e, "weight persister: folder flush failed, will retry");
                    continue;
                }
            }

            let snap = match handle.weight_snapshot().await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, "weight persister: engine offline, exiting");
                    return;
                }
            };

            let mut changed: Vec<(Uuid, f32)> = Vec::new();
            for (id, w) in &snap {
                let is_changed = match last.get(id).copied() {
                    None => true,
                    Some(p) => (w - p).abs() >= epsilon,
                };
                if is_changed {
                    changed.push((*id, *w));
                }
            }

            if changed.is_empty() {
                continue;
            }

            let n = changed.len();
            let rows = changed.clone();
            let result = run_blocking(&pool, move |conn| {
                use diesel::Connection;
                conn.transaction::<usize, diesel::result::Error, _>(|tx| {
                    let mut written = 0usize;
                    for (id, w) in &rows {
                        let now = chrono::Utc::now();
                        let n = diesel::update(edges::table.filter(edges::id.eq(id)))
                            .set((edges::weight.eq(*w), edges::updated_at.eq(now)))
                            .execute(tx)?;
                        written += n;
                    }
                    Ok(written)
                })
                .map_err(crate::error::CoreError::from)
            })
            .await;

            match result {
                Ok(written) => {
                    // Commit the in-memory `last` only after the
                    // transaction succeeded — otherwise a retry will
                    // never see these edges as "changed" again.
                    for (id, w) in &changed {
                        last.insert(*id, *w);
                    }
                    tracing::debug!(
                        candidates = n,
                        written,
                        total_edges = snap.len(),
                        "weight persister flushed"
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, "weight persister: flush failed, will retry");
                }
            }
        }
    })
}

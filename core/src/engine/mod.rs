//! Engine actor: a single tokio task owns the [`cortex_snn::SimEngine`]
//! and mutates it only via mpsc commands. Read-only views are served
//! either via the broadcast channel (spike stream) or by a `Snapshot`
//! command.
//!
//! This module is the *concurrency wrapper* around the substrate; the
//! deterministic simulator itself lives in the `cortex-snn` crate.
//! `core`'s job is to make it usable from axum handlers and to plumb
//! state in and out of the DB.

pub mod spike_persist;
pub mod weight_persist;

// Re-export the wire-event types so the rest of `core` (api/weights.rs,
// the WS handler, etc.) doesn't need to know they originate in
// cortex-snn. Lets us swap the substrate's serialization layer later
// without churn across the handler layer.
pub use cortex_snn::engine::events::{SpikeEvent, SpikeFrame, WeightDelta, WeightFrame};
pub use cortex_snn::engine::sim::SimEngine;
pub use spike_persist::spawn_spike_persister;
pub use weight_persist::spawn_weight_persister;

use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

use crate::cortex_type::CortexType;
use cortex_snn::NeuronKind;

/// Public, cloneable handle to the running engine task. Embed in axum's
/// `AppState`.
#[derive(Clone)]
pub struct SimHandle {
    cmd_tx: mpsc::Sender<EngineCommand>,
    pub spikes: broadcast::Sender<SpikeFrame>,
    pub weights: broadcast::Sender<WeightFrame>,
}

pub struct EngineSnapshot {
    pub t_ms: f64,
    pub n_neurons: usize,
    pub n_synapses: usize,
}

pub enum EngineCommand {
    AddNode(Uuid),
    AddEdge { edge_id: Uuid, pre: Uuid, post: Uuid, weight: f32 },
    IngestBatch {
        nodes: Vec<Uuid>,
        /// (edge_id, pre_id, post_id, initial_weight)
        edges: Vec<(Uuid, Uuid, Uuid, f32)>,
        reply: oneshot::Sender<()>,
    },
    Stimulate { node_id: Uuid, current: f32, duration_ms: f32 },
    Snapshot(oneshot::Sender<EngineSnapshot>),
    /// One-shot weight snapshot: returns Vec<(edge_id, weight)>.
    WeightSnapshot(oneshot::Sender<Vec<(Uuid, f32)>>),
    /// Swap the engine to a different cortex type. Wipes neuron + synapse
    /// state — you can't mix LIF and HH neurons in the same `SimEngine`
    /// without ambiguous input-current units, so reconfigure is a reset.
    /// Reply fires after the swap is in effect; callers should re-ingest
    /// topology afterward.
    Configure { cortex_type: CortexType, reply: oneshot::Sender<()> },
    /// Return the currently-active cortex type (for `GET /api/cortex`).
    GetType(oneshot::Sender<CortexType>),
}

impl SimHandle {
    pub async fn add_node(&self, id: Uuid) -> Result<(), &'static str> {
        self.cmd_tx.send(EngineCommand::AddNode(id)).await
            .map_err(|_| "engine offline")
    }

    pub async fn add_edge(&self, edge_id: Uuid, pre: Uuid, post: Uuid, weight: f32)
        -> Result<(), &'static str>
    {
        self.cmd_tx.send(EngineCommand::AddEdge { edge_id, pre, post, weight }).await
            .map_err(|_| "engine offline")
    }

    pub async fn ingest_batch(
        &self,
        nodes: Vec<Uuid>,
        edges: Vec<(Uuid, Uuid, Uuid, f32)>,
    ) -> Result<(), &'static str> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::IngestBatch { nodes, edges, reply: tx })
            .await
            .map_err(|_| "engine offline")?;
        rx.await.map_err(|_| "engine dropped reply")
    }

    pub async fn stimulate(&self, node_id: Uuid, current: f32, duration_ms: f32)
        -> Result<(), &'static str>
    {
        self.cmd_tx
            .send(EngineCommand::Stimulate { node_id, current, duration_ms })
            .await
            .map_err(|_| "engine offline")
    }

    pub async fn snapshot(&self) -> Result<EngineSnapshot, &'static str> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx.send(EngineCommand::Snapshot(tx)).await
            .map_err(|_| "engine offline")?;
        rx.await.map_err(|_| "engine dropped reply")
    }

    pub async fn weight_snapshot(&self) -> Result<Vec<(Uuid, f32)>, &'static str> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx.send(EngineCommand::WeightSnapshot(tx)).await
            .map_err(|_| "engine offline")?;
        rx.await.map_err(|_| "engine dropped reply")
    }

    /// Wipe the engine and re-seed it as `cortex_type`. The caller is
    /// responsible for re-ingesting nodes/edges afterward (the actor
    /// doesn't replay DB state on its own — that's `hydrate_engine`'s
    /// job at startup).
    pub async fn configure(&self, cortex_type: CortexType) -> Result<(), &'static str> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::Configure { cortex_type, reply: tx })
            .await
            .map_err(|_| "engine offline")?;
        rx.await.map_err(|_| "engine dropped reply")
    }

    pub async fn cortex_type(&self) -> Result<CortexType, &'static str> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx.send(EngineCommand::GetType(tx)).await
            .map_err(|_| "engine offline")?;
        rx.await.map_err(|_| "engine dropped reply")
    }
}

/// Spawn the engine task. Returns a handle plus a join handle for
/// graceful shutdown coordination.
pub fn spawn_engine(tick_hz: u32) -> (SimHandle, tokio::task::JoinHandle<()>) {
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<EngineCommand>(256);
    let (spike_tx, _) = broadcast::channel::<SpikeFrame>(1024);
    let (weight_tx, _) = broadcast::channel::<WeightFrame>(256);

    let handle = SimHandle {
        cmd_tx,
        spikes: spike_tx.clone(),
        weights: weight_tx.clone(),
    };

    spawn_weight_watcher(handle.clone(), weight_tx);

    let dt_ms = 1000.0 / tick_hz as f32;
    let tick_dur = Duration::from_secs_f32(dt_ms / 1000.0);

    let join = tokio::spawn(async move {
        let mut engine = SimEngine::new();
        // The actor's single source of truth for cortex type. Defaults
        // to LIF — preserves the M0 boot behavior; `Configure` replaces
        // both the engine and this value atomically. `AddNode` /
        // `IngestBatch` consult this rather than calling
        // `add_neuron` (which would hard-code LIF) so a freshly-
        // configured HH engine gets HH neurons from the first insert.
        let mut current_type: CortexType = CortexType::default();
        let mut current_kind: NeuronKind = current_type.neuron_kind();
        let mut ticker = tokio::time::interval(tick_dur);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let frame = engine.tick(dt_ms);
                    if !frame.events.is_empty() {
                        // best-effort broadcast; ignore lagged receivers
                        let _ = spike_tx.send(frame);
                    }
                }
                maybe_cmd = cmd_rx.recv() => {
                    let Some(cmd) = maybe_cmd else { break; };
                    match cmd {
                        EngineCommand::AddNode(id) =>
                            engine.add_neuron_with_kind(id, &current_kind),
                        EngineCommand::AddEdge { edge_id, pre, post, weight } =>
                            engine.add_edge(edge_id, pre, post, weight),
                        EngineCommand::IngestBatch { nodes, edges, reply } => {
                            for id in nodes {
                                engine.add_neuron_with_kind(id, &current_kind);
                            }
                            for (edge_id, pre, post, w) in edges {
                                engine.add_edge(edge_id, pre, post, w);
                            }
                            let _ = reply.send(());
                        }
                        EngineCommand::Stimulate { node_id, current, duration_ms } =>
                            engine.inject(node_id, current, duration_ms),
                        EngineCommand::Snapshot(reply) => {
                            let snap = EngineSnapshot {
                                t_ms: engine.t_ms,
                                n_neurons: engine.n_neurons(),
                                n_synapses: engine.n_synapses(),
                            };
                            let _ = reply.send(snap);
                        }
                        EngineCommand::WeightSnapshot(reply) => {
                            let _ = reply.send(engine.weight_snapshot());
                        }
                        EngineCommand::Configure { cortex_type, reply } => {
                            tracing::info!(
                                from = %current_type.slug(),
                                to = %cortex_type.slug(),
                                "reconfiguring engine; wiping state"
                            );
                            engine = SimEngine::new();
                            current_kind = cortex_type.neuron_kind();
                            current_type = cortex_type;
                            let _ = reply.send(());
                        }
                        EngineCommand::GetType(reply) => {
                            let _ = reply.send(current_type.clone());
                        }
                    }
                }
            }
        }

        tracing::info!("engine task shut down cleanly");
    });

    (handle, join)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_snn::{HhConfig, HhIntegrator};

    /// Configure the engine to HH, drive it via inject() + tick(), and
    /// assert a spike escapes the broadcast. Proves the actor wiring
    /// (Configure → AddNode-uses-current-kind → SimEngine HH path) works
    /// end-to-end. Uses a fine-grained tick rate (10 kHz → dt = 0.1 ms)
    /// because Euler at the default 1 kHz wouldn't be stable for HH.
    #[tokio::test]
    async fn engine_reconfigured_to_hh_emits_spikes() {
        let (handle, _join) = spawn_engine(10_000);
        let mut spikes = handle.spikes.subscribe();
        let cfg = HhConfig { integrator: HhIntegrator::Rk4, ..HhConfig::default() };
        handle.configure(CortexType::Hh { config: cfg }).await.unwrap();

        let id = Uuid::new_v4();
        handle.add_node(id).await.unwrap();
        handle.stimulate(id, 10.0, 200.0).await.unwrap();

        // Drain spike broadcast for up to ~1 s wall-clock; with 10 kHz
        // ticks the HH neuron should fire within tens of ms simulated.
        let timeout = tokio::time::Duration::from_secs(1);
        let got = tokio::time::timeout(timeout, async {
            loop {
                let frame = spikes.recv().await.unwrap();
                if frame.events.iter().any(|e| e.node_id == id) {
                    return true;
                }
            }
        })
        .await
        .unwrap_or(false);
        assert!(got, "HH neuron should spike after Configure + Stimulate");

        // Round-trip the type query.
        let ct = handle.cortex_type().await.unwrap();
        assert_eq!(ct.slug(), "hh");
    }
}

/// Periodically diff the live weight set against the last published one
/// and broadcast deltas. Runs as a sibling task to the engine — reads
/// state through `SimHandle::weight_snapshot` (one mpsc round-trip) so
/// it doesn't share memory with the engine.
///
/// `WEIGHT_TICK_MS` controls the broadcast rate; `WEIGHT_EPSILON` the
/// minimum change to publish (filters STDP trace noise on edges that
/// are essentially stable).
fn spawn_weight_watcher(handle: SimHandle, tx: broadcast::Sender<WeightFrame>) {
    const WEIGHT_TICK_MS: u64 = 250;
    const WEIGHT_EPSILON: f32 = 0.0015;
    const FORCE_FULL_EVERY: u32 = 40; // ~10s — keeps late subscribers honest.

    tokio::spawn(async move {
        let mut last: std::collections::HashMap<Uuid, f32> = std::collections::HashMap::new();
        let mut since_full: u32 = u32::MAX; // force a full first frame.
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(WEIGHT_TICK_MS));

        loop {
            interval.tick().await;
            let snap = match handle.weight_snapshot().await {
                Ok(s) => s,
                Err(_) => break,
            };
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0);

            let send_full = since_full >= FORCE_FULL_EVERY;
            let deltas: Vec<WeightDelta> = if send_full {
                snap.iter().map(|(id, w)| WeightDelta { edge_id: *id, w: *w }).collect()
            } else {
                snap.iter()
                    .filter_map(|(id, w)| {
                        let prev = last.get(id).copied().unwrap_or(f32::NAN);
                        if !prev.is_finite() || (w - prev).abs() >= WEIGHT_EPSILON {
                            Some(WeightDelta { edge_id: *id, w: *w })
                        } else { None }
                    })
                    .collect()
            };

            // Refresh `last` snapshot.
            last.clear();
            for (id, w) in &snap { last.insert(*id, *w); }

            if deltas.is_empty() && !send_full { continue; }

            let frame = if send_full {
                since_full = 0;
                WeightFrame::snapshot(now, deltas)
            } else {
                since_full = since_full.saturating_add(1);
                WeightFrame::delta(now, deltas)
            };
            let _ = tx.send(frame);
        }
    });
}

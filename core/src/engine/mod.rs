//! Engine actor: a single tokio task owns `SimEngine` and mutates it only
//! via mpsc commands. Read-only views are served either via the broadcast
//! channel (spike stream) or by a `Snapshot` command.

pub mod events;
pub mod sim;
pub mod spike_persist;
pub mod weight_persist;

pub use events::{SpikeEvent, SpikeFrame, WeightDelta, WeightFrame};
pub use sim::SimEngine;
pub use spike_persist::spawn_spike_persister;
pub use weight_persist::spawn_weight_persister;

use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

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
                        EngineCommand::AddNode(id) => engine.add_neuron(id),
                        EngineCommand::AddEdge { edge_id, pre, post, weight } =>
                            engine.add_edge(edge_id, pre, post, weight),
                        EngineCommand::IngestBatch { nodes, edges, reply } => {
                            for id in nodes { engine.add_neuron(id); }
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
                    }
                }
            }
        }

        tracing::info!("engine task shut down cleanly");
    });

    (handle, join)
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

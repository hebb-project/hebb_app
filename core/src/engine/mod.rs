//! Engine actor: a single tokio task owns `SimEngine` and mutates it only
//! via mpsc commands. Read-only views are served either via the broadcast
//! channel (spike stream) or by a `Snapshot` command.

pub mod events;
pub mod sim;

pub use events::{SpikeEvent, SpikeFrame};
pub use sim::SimEngine;

use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

/// Public, cloneable handle to the running engine task. Embed in axum's
/// `AppState`.
#[derive(Clone)]
pub struct SimHandle {
    cmd_tx: mpsc::Sender<EngineCommand>,
    pub spikes: broadcast::Sender<SpikeFrame>,
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

    let handle = SimHandle { cmd_tx, spikes: spike_tx.clone() };

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

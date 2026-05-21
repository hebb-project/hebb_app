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

use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

use crate::cortex_type::CortexType;
use cortex_snn::format::topology::TopologyFile;
use cortex_snn::NeuronKind;
use cortex_snn::{AddNeuron as CortexAddNeuron, AddSynapse as CortexAddSynapse, Cortex};

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
    /// Open a `.cortex/` folder and hydrate the engine from its
    /// `topology.json` + `weights/{type}/latest.cwt`. Wipes prior in-
    /// memory state (same destructive contract as `Configure`). The
    /// reply carries a summary so the caller can confirm sizes without
    /// a follow-up snapshot.
    Open {
        folder: PathBuf,
        reply: oneshot::Sender<Result<OpenSummary, String>>,
    },
    /// Return the path of the currently-open `.cortex/` folder, if any.
    /// `None` means the engine is running on transient state (a bare
    /// `Configure` was used, no folder ever opened).
    GetFolder(oneshot::Sender<Option<PathBuf>>),
    /// List all currently-loaded neuron IDs. Drives `GET /api/nodes`.
    ListNeurons(oneshot::Sender<Vec<Uuid>>),
    /// Get the introspectable params for a neuron. `None` if the
    /// neuron isn't in the engine.
    GetNodeParams {
        node_id: Uuid,
        reply: oneshot::Sender<Option<serde_json::Value>>,
    },
    /// Bulk-fetch every neuron's params keyed by ID. Drives the
    /// dump endpoint used by the agent harness as a one-shot
    /// "what's in this network right now" call.
    GetAllNodeParams(oneshot::Sender<Vec<(Uuid, serde_json::Value)>>),
    /// Mutate one parameter on one neuron. Returns the new full
    /// param set on success, the substrate's ParamError text on
    /// failure (validation, type, range, unknown key).
    SetNodeParam {
        node_id: Uuid,
        key: String,
        value: serde_json::Value,
        reply: oneshot::Sender<Result<serde_json::Value, String>>,
    },
    /// Add a neuron to the currently-open `.cortex/` folder and to the
    /// in-memory SimEngine. Fails if no folder is open — call sites
    /// check `current_folder()` first and route to the legacy
    /// Postgres+engine path when None.
    AddNeuronToFolder {
        label: String,
        metadata: serde_json::Value,
        reply: oneshot::Sender<Result<FolderNodeRecord, String>>,
    },
    /// Add a synapse to the open folder and to SimEngine.
    AddSynapseToFolder {
        pre: Uuid,
        post: Uuid,
        weight: f32,
        metadata: serde_json::Value,
        reply: oneshot::Sender<Result<FolderEdgeRecord, String>>,
    },
    /// Remove a neuron (cascades to incident edges) from disk and from
    /// SimEngine. Returns `None` if the neuron wasn't in the topology.
    RemoveNeuronFromFolder {
        node_id: Uuid,
        reply: oneshot::Sender<Result<Option<FolderRemoveSummary>, String>>,
    },
    /// Remove one synapse from disk and from SimEngine.
    RemoveSynapseFromFolder {
        edge_id: Uuid,
        reply: oneshot::Sender<Result<bool, String>>,
    },
}

/// Returned by `AddNeuronToFolder` — enough fields to render a
/// `CortexNode`-shaped JSON response without re-reading topology.json.
#[derive(Debug, Clone)]
pub struct FolderNodeRecord {
    pub id: Uuid,
    pub label: String,
    pub node_type: String,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct FolderEdgeRecord {
    pub id: Uuid,
    pub pre_id: Uuid,
    pub post_id: Uuid,
    pub weight: f32,
    pub edge_type: String,
}

#[derive(Debug, Clone, Copy)]
pub struct FolderRemoveSummary {
    /// Number of incident edges cascaded by a neuron removal.
    pub cascaded_edges: usize,
}

/// Returned to a caller of `Open`. Lets the REST handler render a
/// useful response without a round-trip back into the actor.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OpenSummary {
    pub folder: String,
    pub cortex_type: String,
    pub name: String,
    pub n_nodes: usize,
    pub n_edges: usize,
    pub weights_loaded: usize,
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

    /// Open a `.cortex/` folder and hydrate the engine from disk.
    /// Returns the substrate's `Display` error text on failure so
    /// handlers can pass it straight to the user.
    pub async fn open(&self, folder: PathBuf) -> Result<OpenSummary, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::Open { folder, reply: tx })
            .await
            .map_err(|_| "engine offline".to_string())?;
        rx.await.map_err(|_| "engine dropped reply".to_string())?
    }

    pub async fn current_folder(&self) -> Result<Option<PathBuf>, &'static str> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx.send(EngineCommand::GetFolder(tx)).await
            .map_err(|_| "engine offline")?;
        rx.await.map_err(|_| "engine dropped reply")
    }

    pub async fn list_neurons(&self) -> Result<Vec<Uuid>, &'static str> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx.send(EngineCommand::ListNeurons(tx)).await
            .map_err(|_| "engine offline")?;
        rx.await.map_err(|_| "engine dropped reply")
    }

    pub async fn get_node_params(&self, node_id: Uuid)
        -> Result<Option<serde_json::Value>, &'static str>
    {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::GetNodeParams { node_id, reply: tx })
            .await
            .map_err(|_| "engine offline")?;
        rx.await.map_err(|_| "engine dropped reply")
    }

    pub async fn get_all_node_params(&self)
        -> Result<Vec<(Uuid, serde_json::Value)>, &'static str>
    {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx.send(EngineCommand::GetAllNodeParams(tx)).await
            .map_err(|_| "engine offline")?;
        rx.await.map_err(|_| "engine dropped reply")
    }

    pub async fn set_node_param(
        &self,
        node_id: Uuid,
        key: String,
        value: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::SetNodeParam { node_id, key, value, reply: tx })
            .await
            .map_err(|_| "engine offline".to_string())?;
        rx.await.map_err(|_| "engine dropped reply".to_string())?
    }

    /// Append a neuron to the open `.cortex/` folder and to the engine.
    /// Caller must have verified `current_folder().is_some()` — the
    /// actor returns an error string otherwise.
    pub async fn add_neuron_to_folder(
        &self,
        label: String,
        metadata: serde_json::Value,
    ) -> Result<FolderNodeRecord, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::AddNeuronToFolder { label, metadata, reply: tx })
            .await
            .map_err(|_| "engine offline".to_string())?;
        rx.await.map_err(|_| "engine dropped reply".to_string())?
    }

    pub async fn add_synapse_to_folder(
        &self,
        pre: Uuid,
        post: Uuid,
        weight: f32,
        metadata: serde_json::Value,
    ) -> Result<FolderEdgeRecord, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::AddSynapseToFolder {
                pre,
                post,
                weight,
                metadata,
                reply: tx,
            })
            .await
            .map_err(|_| "engine offline".to_string())?;
        rx.await.map_err(|_| "engine dropped reply".to_string())?
    }

    pub async fn remove_neuron_from_folder(
        &self,
        node_id: Uuid,
    ) -> Result<Option<FolderRemoveSummary>, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::RemoveNeuronFromFolder { node_id, reply: tx })
            .await
            .map_err(|_| "engine offline".to_string())?;
        rx.await.map_err(|_| "engine dropped reply".to_string())?
    }

    pub async fn remove_synapse_from_folder(
        &self,
        edge_id: Uuid,
    ) -> Result<bool, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::RemoveSynapseFromFolder { edge_id, reply: tx })
            .await
            .map_err(|_| "engine offline".to_string())?;
        rx.await.map_err(|_| "engine dropped reply".to_string())?
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
        // The folder this engine instance was last hydrated from, if
        // any. `None` means the engine is running on transient state
        // built via `Configure` + `AddNode` calls — same shape as M0.
        let mut current_folder: Option<PathBuf> = None;
        // Live handle to the open `.cortex/` folder. When `Some`, every
        // topology mutation must flow through it so `topology.json`
        // stays in lockstep with SimEngine. The actor is the sole owner
        // — no other task touches the handle, so we don't need a lock.
        let mut current_cortex: Option<Cortex> = None;
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
                            // A bare Configure detaches from any
                            // previously-open folder — the engine is
                            // now running on transient memory only.
                            current_folder = None;
                            current_cortex = None;
                            let _ = reply.send(());
                        }
                        EngineCommand::GetType(reply) => {
                            let _ = reply.send(current_type.clone());
                        }
                        EngineCommand::Open { folder, reply } => {
                            match open_folder_into_engine(&folder) {
                                Ok((new_engine, new_type, new_kind, new_cortex, summary)) => {
                                    tracing::info!(
                                        folder = %folder.display(),
                                        cortex_type = %new_type.slug(),
                                        n_nodes = summary.n_nodes,
                                        n_edges = summary.n_edges,
                                        weights_loaded = summary.weights_loaded,
                                        "engine hydrated from .cortex/ folder"
                                    );
                                    engine = new_engine;
                                    current_type = new_type;
                                    current_kind = new_kind;
                                    current_folder = Some(folder);
                                    current_cortex = Some(new_cortex);
                                    let _ = reply.send(Ok(summary));
                                }
                                Err(msg) => {
                                    tracing::warn!(
                                        folder = %folder.display(),
                                        error = %msg,
                                        "engine open failed"
                                    );
                                    let _ = reply.send(Err(msg));
                                }
                            }
                        }
                        EngineCommand::GetFolder(reply) => {
                            let _ = reply.send(current_folder.clone());
                        }
                        EngineCommand::ListNeurons(reply) => {
                            let _ = reply.send(engine.list_neurons());
                        }
                        EngineCommand::GetNodeParams { node_id, reply } => {
                            let _ = reply.send(engine.neuron_params(node_id));
                        }
                        EngineCommand::GetAllNodeParams(reply) => {
                            let _ = reply.send(engine.all_neuron_params());
                        }
                        EngineCommand::SetNodeParam { node_id, key, value, reply } => {
                            let result = engine
                                .set_neuron_param(node_id, &key, &value)
                                .map_err(|e| e.to_string());
                            let _ = reply.send(result);
                        }
                        EngineCommand::AddNeuronToFolder { label, metadata, reply } => {
                            let result = match current_cortex.as_mut() {
                                None => Err(
                                    "no .cortex/ folder open; route writes through Postgres path"
                                        .to_string(),
                                ),
                                Some(cortex) => {
                                    let spec = CortexAddNeuron {
                                        id: None,
                                        label: label.clone(),
                                        kind: None,
                                        metadata: Some(metadata.clone()),
                                        init_state: None,
                                    };
                                    match cortex.add_neuron(spec) {
                                        Ok(id) => {
                                            // Mirror into SimEngine so the simulator
                                            // sees the new neuron without reopening.
                                            engine.add_neuron_with_kind(id, &current_kind);
                                            Ok(FolderNodeRecord {
                                                id,
                                                label,
                                                node_type: current_type.slug().to_string(),
                                                metadata,
                                            })
                                        }
                                        Err(e) => Err(e.to_string()),
                                    }
                                }
                            };
                            let _ = reply.send(result);
                        }
                        EngineCommand::AddSynapseToFolder {
                            pre,
                            post,
                            weight,
                            metadata,
                            reply,
                        } => {
                            let result = match current_cortex.as_mut() {
                                None => Err(
                                    "no .cortex/ folder open; route writes through Postgres path"
                                        .to_string(),
                                ),
                                Some(cortex) => {
                                    let spec = CortexAddSynapse {
                                        id: None,
                                        pre,
                                        post,
                                        kind: None,
                                        init_weight: weight,
                                        delay_ms: None,
                                        metadata: Some(metadata.clone()),
                                    };
                                    let edge_type = cortex
                                        .topology()
                                        .defaults
                                        .synapse
                                        .kind
                                        .clone();
                                    match cortex.add_synapse(spec) {
                                        Ok(id) => {
                                            engine.add_edge(id, pre, post, weight);
                                            Ok(FolderEdgeRecord {
                                                id,
                                                pre_id: pre,
                                                post_id: post,
                                                weight,
                                                edge_type,
                                            })
                                        }
                                        Err(e) => Err(e.to_string()),
                                    }
                                }
                            };
                            let _ = reply.send(result);
                        }
                        EngineCommand::RemoveNeuronFromFolder { node_id, reply } => {
                            let result = match current_cortex.as_mut() {
                                None => Err(
                                    "no .cortex/ folder open; route writes through Postgres path"
                                        .to_string(),
                                ),
                                Some(cortex) => {
                                    let present = cortex
                                        .topology()
                                        .nodes
                                        .iter()
                                        .any(|n| n.id == node_id);
                                    if !present {
                                        Ok(None)
                                    } else {
                                        match cortex.remove_neuron(node_id) {
                                            Ok(cascaded) => {
                                                engine.remove_neuron(node_id);
                                                Ok(Some(FolderRemoveSummary {
                                                    cascaded_edges: cascaded,
                                                }))
                                            }
                                            Err(e) => Err(e.to_string()),
                                        }
                                    }
                                }
                            };
                            let _ = reply.send(result);
                        }
                        EngineCommand::RemoveSynapseFromFolder { edge_id, reply } => {
                            let result = match current_cortex.as_mut() {
                                None => Err(
                                    "no .cortex/ folder open; route writes through Postgres path"
                                        .to_string(),
                                ),
                                Some(cortex) => match cortex.remove_synapse(edge_id) {
                                    Ok(true) => {
                                        engine.remove_synapse(edge_id);
                                        Ok(true)
                                    }
                                    Ok(false) => Ok(false),
                                    Err(e) => Err(e.to_string()),
                                },
                            };
                            let _ = reply.send(result);
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

    /// Build a tiny HH `.cortex/` folder via the substrate, then ask
    /// the actor to open it. Asserts the actor reports the right
    /// neuron/edge counts and accepts a stimulate command on the
    /// hydrated neuron afterward. Proves the on-disk → in-memory
    /// pipeline works end-to-end from core's perspective.
    #[tokio::test]
    async fn engine_opens_cortex_folder_from_disk() {
        use cortex_snn::format::topology::{NeuronSpec, SynapseSpec, TopologyDefaults};
        use cortex_snn::{AddNeuron, AddSynapse, CreateOptions};
        use std::time::SystemTime;

        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("core-engine-open-{nanos}"));

        // Build a 2-neuron HH folder.
        let mut cx = cortex_snn::Cortex::create(
            &root,
            CreateOptions {
                name: "Open Test".into(),
                cortex_type: "hh".into(),
                source_root: root.display().to_string(),
                now_rfc3339: "2026-05-21T12:00:00Z".into(),
                defaults: TopologyDefaults {
                    neuron: NeuronSpec::hh(None),
                    synapse: SynapseSpec::stdp(),
                },
                hh_config: None,
            },
        )
        .unwrap();
        let a = cx.add_neuron(AddNeuron { label: "a".into(), ..Default::default() }).unwrap();
        let b = cx.add_neuron(AddNeuron { label: "b".into(), ..Default::default() }).unwrap();
        cx.add_synapse(AddSynapse {
            id: None,
            pre: a,
            post: b,
            kind: None,
            init_weight: 0.4,
            delay_ms: None,
            metadata: None,
        })
        .unwrap();
        drop(cx);

        let (handle, _join) = spawn_engine(1000);
        let summary = handle.open(root.clone()).await.expect("open should succeed");
        assert_eq!(summary.cortex_type, "hh");
        assert_eq!(summary.n_nodes, 2);
        assert_eq!(summary.n_edges, 1);

        // Engine should now report the right cortex type via the
        // existing query, *and* current_folder should be set.
        let ct = handle.cortex_type().await.unwrap();
        assert_eq!(ct.slug(), "hh");
        let f = handle.current_folder().await.unwrap();
        assert_eq!(f.as_deref(), Some(root.as_path()));

        // Stimulating one of the hydrated neurons should not error
        // (proves the engine knows about the neuron by id).
        handle.stimulate(a, 5.0, 10.0).await.unwrap();

        std::fs::remove_dir_all(&root).ok();
    }

    /// Opening a non-existent folder must return a clean error, not
    /// crash the actor.
    #[tokio::test]
    async fn engine_open_missing_folder_returns_error() {
        let (handle, _join) = spawn_engine(1000);
        let bogus = std::path::PathBuf::from("/tmp/this-path-does-not-exist-cortex-test");
        let err = handle.open(bogus).await.unwrap_err();
        assert!(!err.is_empty(), "error should have a non-empty message");
    }

    /// After a successful Open, a subsequent bare Configure should
    /// detach from the folder and reset `current_folder` to None.
    #[tokio::test]
    async fn configure_after_open_clears_folder() {
        use cortex_snn::format::topology::{NeuronSpec, SynapseSpec, TopologyDefaults};
        use cortex_snn::CreateOptions;
        use std::time::SystemTime;

        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("core-configure-detach-{nanos}"));
        let _cx = cortex_snn::Cortex::create(
            &root,
            CreateOptions {
                name: "Detach".into(),
                cortex_type: "lif".into(),
                source_root: root.display().to_string(),
                now_rfc3339: "2026-05-21T12:00:00Z".into(),
                defaults: TopologyDefaults {
                    neuron: NeuronSpec::lif(),
                    synapse: SynapseSpec::stdp(),
                },
                hh_config: None,
            },
        )
        .unwrap();

        let (handle, _join) = spawn_engine(1000);
        handle.open(root.clone()).await.unwrap();
        assert!(handle.current_folder().await.unwrap().is_some());

        handle.configure(CortexType::Lif).await.unwrap();
        assert!(handle.current_folder().await.unwrap().is_none());

        std::fs::remove_dir_all(&root).ok();
    }
}

/// Open a `.cortex/` folder via the substrate's [`cortex_snn::Cortex`]
/// handle and produce a fresh `SimEngine` populated from disk. Pure
/// sync (uses `std::fs` through the substrate), so the actor wraps it
/// in `tokio::task::block_in_place` … actually no — the disk reads are
/// small (a JSON topology and a binary weights file), well under the
/// "block the runtime for a few ms" budget, and wrapping in
/// `spawn_blocking` would force ownership gymnastics across an async
/// boundary the actor doesn't need. Keep it sync; revisit if folders
/// grow to 100k-edge scale.
fn open_folder_into_engine(
    folder: &std::path::Path,
) -> Result<(SimEngine, CortexType, NeuronKind, Cortex, OpenSummary), String> {
    let cortex = cortex_snn::Cortex::open(folder).map_err(|e| e.to_string())?;
    let topology: &TopologyFile = cortex.topology();
    let metadata = cortex.metadata();

    let cortex_type = CortexType::from_slug_and_config(
        &metadata.cortex_type,
        metadata.hh_config.as_ref(),
    )
    .ok_or_else(|| format!(
        "unknown cortex_type '{}' in folder {}",
        metadata.cortex_type,
        folder.display(),
    ))?;
    let kind = cortex_type.neuron_kind();

    let mut engine = SimEngine::new();
    for n in &topology.nodes {
        // Per-node `kind` overrides are accepted by the format but the
        // actor currently runs a single neuron kind. If the override
        // disagrees with the default we surface it rather than
        // silently using the default — the user's intent should be
        // honored or rejected, never quietly ignored.
        let effective = topology.effective_neuron(n);
        if effective.kind != topology.defaults.neuron.kind {
            return Err(format!(
                "node {} requests neuron kind '{}' but engine currently \
                 runs a single kind '{}' per folder; \
                 mixed-kind networks are not supported in v1",
                n.id,
                effective.kind,
                topology.defaults.neuron.kind,
            ));
        }
        engine.add_neuron_with_kind(n.id, &kind);
    }
    for e in &topology.edges {
        engine.add_edge(e.id, e.pre, e.post, e.init_weight);
    }

    // Apply persisted weights on top of the init_weight values. Missing
    // weights file is fine — a freshly-created folder has none.
    let weights = cortex.load_weights().map_err(|e| e.to_string())?;
    let weights_loaded = weights.len();
    for (edge_id, w) in weights {
        // SimEngine's add_edge already set an initial weight; we don't
        // have a `set_weight` on the engine itself, so the cleanest
        // path is to update the synapse directly via a future
        // `set_weight` method. For now, recreating the edge with the
        // loaded weight matches the existing `add_edge` semantics
        // (idempotent-by-id is enforced inside SimEngine via the
        // hash-map insert).
        //
        // TODO(PR C2): expose `SimEngine::set_edge_weight(edge_id, w)`
        // and call it here. The current path works because add_edge
        // overwrites a same-id edge.
        //
        // Find the edge's pre/post from topology so we can reissue.
        if let Some(spec) = topology.edges.iter().find(|te| te.id == edge_id) {
            engine.add_edge(edge_id, spec.pre, spec.post, w);
        }
    }

    let summary = OpenSummary {
        folder: folder.display().to_string(),
        cortex_type: metadata.cortex_type.clone(),
        name: metadata.name.clone(),
        n_nodes: topology.nodes.len(),
        n_edges: topology.edges.len(),
        weights_loaded,
    };
    Ok((engine, cortex_type, kind, cortex, summary))
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

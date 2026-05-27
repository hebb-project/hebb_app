//! Engine actor: a single tokio task owns the [`hebb::SimEngine`]
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
pub use hebb::engine::events::{SpikeFrame, WeightDelta, WeightFrame};
pub use hebb::engine::sim::SimEngine;
pub use spike_persist::spawn_spike_persister;
pub use weight_persist::spawn_weight_persister;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

use crate::cortex_type::CortexType;
use hebb::format::topology::TopologyFile;
use hebb::seeds::Seed;
use hebb::NeuronKind;
use hebb::{AddNeuron as CortexAddNeuron, AddSynapse as CortexAddSynapse, Cortex};

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

#[derive(Debug, Clone, serde::Serialize)]
pub struct EngineGraphSnapshot {
    pub t_ms: f64,
    pub nodes: Vec<EngineGraphNode>,
    pub edges: Vec<EngineGraphEdge>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EngineGraphNode {
    pub id: Uuid,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EngineGraphEdge {
    pub id: Uuid,
    pub pre_id: Uuid,
    pub post_id: Uuid,
    pub weight: f32,
    pub edge_type: &'static str,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct EngineSeedReport {
    pub added_nodes: usize,
    pub added_edges: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EngineRunSummary {
    pub requested_duration_ms: f32,
    pub duration_ms: f32,
    pub dt_ms: f32,
    pub steps: usize,
    pub t_start_ms: f64,
    pub t_end_ms: f64,
    pub total_spikes: usize,
    pub per_neuron: Vec<EngineRunNeuronCount>,
    pub cancelled: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EngineRunNeuronCount {
    pub node_id: Uuid,
    pub spikes: usize,
}

pub enum EngineCommand {
    AddNode(Uuid),
    AddEdge {
        edge_id: Uuid,
        pre: Uuid,
        post: Uuid,
        weight: f32,
    },
    IngestBatch {
        nodes: Vec<Uuid>,
        /// (edge_id, pre_id, post_id, initial_weight)
        edges: Vec<(Uuid, Uuid, Uuid, f32)>,
        reply: oneshot::Sender<()>,
    },
    ApplySeed {
        seed: Seed,
        reply: oneshot::Sender<Result<EngineSeedReport, String>>,
    },
    Stimulate {
        node_id: Uuid,
        current: f32,
        duration_ms: f32,
    },
    ForceSpike {
        node_id: Uuid,
        reply: oneshot::Sender<Result<(), String>>,
    },
    RemoveNeuron {
        node_id: Uuid,
        reply: oneshot::Sender<Result<Option<FolderRemoveSummary>, String>>,
    },
    RemoveSynapse {
        edge_id: Uuid,
        reply: oneshot::Sender<Result<bool, String>>,
    },
    RunFor {
        duration_ms: f32,
        dt_ms: f32,
        reply: oneshot::Sender<Result<EngineRunSummary, String>>,
    },
    Snapshot(oneshot::Sender<EngineSnapshot>),
    /// Live topology snapshot with synapse endpoints. This is the
    /// read-only graph view the agent harness needs without going
    /// through the HTTP layer.
    GraphSnapshot(oneshot::Sender<EngineGraphSnapshot>),
    /// One-shot weight snapshot: returns Vec<(edge_id, weight)>.
    WeightSnapshot(oneshot::Sender<Vec<(Uuid, f32)>>),
    /// One-shot membrane-potential snapshot for the `/ws/voltage` stream.
    /// `nodes = Some(ids)` samples only those (preserving order, skipping
    /// unknown); `None` samples every neuron. Reply carries the engine
    /// clock so each frame is self-consistent: `(t_ms, [(node_id, v_mV)])`.
    VoltageSnapshot {
        nodes: Option<Vec<Uuid>>,
        reply: oneshot::Sender<(f64, Vec<(Uuid, f32)>)>,
    },
    /// Swap the engine to a different cortex type. Wipes neuron + synapse
    /// state — you can't mix LIF and HH neurons in the same `SimEngine`
    /// without ambiguous input-current units, so reconfigure is a reset.
    /// Reply fires after the swap is in effect; callers should re-ingest
    /// topology afterward.
    Configure {
        cortex_type: CortexType,
        reply: oneshot::Sender<()>,
    },
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
    /// List all currently-loaded synapse edge IDs. Drives
    /// `GET /api/synapses`.
    ListSynapses(oneshot::Sender<Vec<Uuid>>),
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
    /// Get the introspectable params for a synapse. `None` if the
    /// edge isn't in the engine.
    GetSynapseParams {
        edge_id: Uuid,
        reply: oneshot::Sender<Option<serde_json::Value>>,
    },
    /// Bulk-fetch every synapse's params keyed by stable edge ID.
    GetAllSynapseParams(oneshot::Sender<Vec<(Uuid, serde_json::Value)>>),
    /// Mutate one parameter on one synapse.
    SetSynapseParam {
        edge_id: Uuid,
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
    /// Snapshot the live weights and persist them to
    /// `weights/{type}/latest.cwt` via the open `Cortex` handle.
    /// Returns `Ok(None)` when no folder is open — the caller should
    /// fall back to the Postgres path.
    FlushWeightsToOpenFolder {
        reply: oneshot::Sender<Result<Option<usize>, String>>,
    },
    /// Snapshot every neuron's dynamic state and persist it to
    /// `state/{type}/latest.json` via the open `Cortex` handle.
    /// Returns `Ok(None)` when no folder is open (engine is running on
    /// transient state). The number returned on success is the count
    /// of neuron records written — useful for logging "saved state
    /// for N neurons" on shutdown without a second round-trip.
    SaveStateToOpenFolder {
        reply: oneshot::Sender<Result<Option<usize>, String>>,
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
        self.cmd_tx
            .send(EngineCommand::AddNode(id))
            .await
            .map_err(|_| "engine offline")
    }

    pub async fn add_edge(
        &self,
        edge_id: Uuid,
        pre: Uuid,
        post: Uuid,
        weight: f32,
    ) -> Result<(), &'static str> {
        self.cmd_tx
            .send(EngineCommand::AddEdge {
                edge_id,
                pre,
                post,
                weight,
            })
            .await
            .map_err(|_| "engine offline")
    }

    pub async fn ingest_batch(
        &self,
        nodes: Vec<Uuid>,
        edges: Vec<(Uuid, Uuid, Uuid, f32)>,
    ) -> Result<(), &'static str> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::IngestBatch {
                nodes,
                edges,
                reply: tx,
            })
            .await
            .map_err(|_| "engine offline")?;
        rx.await.map_err(|_| "engine dropped reply")
    }

    pub async fn apply_seed(&self, seed: Seed) -> Result<EngineSeedReport, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::ApplySeed { seed, reply: tx })
            .await
            .map_err(|_| "engine offline".to_string())?;
        rx.await.map_err(|_| "engine dropped reply".to_string())?
    }

    pub async fn stimulate(
        &self,
        node_id: Uuid,
        current: f32,
        duration_ms: f32,
    ) -> Result<(), &'static str> {
        self.cmd_tx
            .send(EngineCommand::Stimulate {
                node_id,
                current,
                duration_ms,
            })
            .await
            .map_err(|_| "engine offline")
    }

    pub async fn force_spike(&self, node_id: Uuid) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::ForceSpike { node_id, reply: tx })
            .await
            .map_err(|_| "engine offline".to_string())?;
        rx.await.map_err(|_| "engine dropped reply".to_string())?
    }

    pub async fn run_for(&self, duration_ms: f32, dt_ms: f32) -> Result<EngineRunSummary, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::RunFor {
                duration_ms,
                dt_ms,
                reply: tx,
            })
            .await
            .map_err(|_| "engine offline".to_string())?;
        rx.await.map_err(|_| "engine dropped reply".to_string())?
    }

    pub async fn snapshot(&self) -> Result<EngineSnapshot, &'static str> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::Snapshot(tx))
            .await
            .map_err(|_| "engine offline")?;
        rx.await.map_err(|_| "engine dropped reply")
    }

    pub async fn graph_snapshot(&self) -> Result<EngineGraphSnapshot, &'static str> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::GraphSnapshot(tx))
            .await
            .map_err(|_| "engine offline")?;
        rx.await.map_err(|_| "engine dropped reply")
    }

    pub async fn weight_snapshot(&self) -> Result<Vec<(Uuid, f32)>, &'static str> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::WeightSnapshot(tx))
            .await
            .map_err(|_| "engine offline")?;
        rx.await.map_err(|_| "engine dropped reply")
    }

    /// Sample membrane potentials for the live voltage stream. `nodes`
    /// filters to a specific set (the UI's selected neurons); `None`
    /// samples all.
    pub async fn voltage_snapshot(
        &self,
        nodes: Option<Vec<Uuid>>,
    ) -> Result<(f64, Vec<(Uuid, f32)>), &'static str> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::VoltageSnapshot { nodes, reply: tx })
            .await
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
            .send(EngineCommand::Configure {
                cortex_type,
                reply: tx,
            })
            .await
            .map_err(|_| "engine offline")?;
        rx.await.map_err(|_| "engine dropped reply")
    }

    pub async fn cortex_type(&self) -> Result<CortexType, &'static str> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::GetType(tx))
            .await
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
        self.cmd_tx
            .send(EngineCommand::GetFolder(tx))
            .await
            .map_err(|_| "engine offline")?;
        rx.await.map_err(|_| "engine dropped reply")
    }

    pub async fn list_neurons(&self) -> Result<Vec<Uuid>, &'static str> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::ListNeurons(tx))
            .await
            .map_err(|_| "engine offline")?;
        rx.await.map_err(|_| "engine dropped reply")
    }

    pub async fn list_synapses(&self) -> Result<Vec<Uuid>, &'static str> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::ListSynapses(tx))
            .await
            .map_err(|_| "engine offline")?;
        rx.await.map_err(|_| "engine dropped reply")
    }

    pub async fn get_node_params(
        &self,
        node_id: Uuid,
    ) -> Result<Option<serde_json::Value>, &'static str> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::GetNodeParams { node_id, reply: tx })
            .await
            .map_err(|_| "engine offline")?;
        rx.await.map_err(|_| "engine dropped reply")
    }

    pub async fn get_all_node_params(
        &self,
    ) -> Result<Vec<(Uuid, serde_json::Value)>, &'static str> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::GetAllNodeParams(tx))
            .await
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
            .send(EngineCommand::SetNodeParam {
                node_id,
                key,
                value,
                reply: tx,
            })
            .await
            .map_err(|_| "engine offline".to_string())?;
        rx.await.map_err(|_| "engine dropped reply".to_string())?
    }

    pub async fn get_synapse_params(
        &self,
        edge_id: Uuid,
    ) -> Result<Option<serde_json::Value>, &'static str> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::GetSynapseParams { edge_id, reply: tx })
            .await
            .map_err(|_| "engine offline")?;
        rx.await.map_err(|_| "engine dropped reply")
    }

    pub async fn get_all_synapse_params(
        &self,
    ) -> Result<Vec<(Uuid, serde_json::Value)>, &'static str> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::GetAllSynapseParams(tx))
            .await
            .map_err(|_| "engine offline")?;
        rx.await.map_err(|_| "engine dropped reply")
    }

    pub async fn set_synapse_param(
        &self,
        edge_id: Uuid,
        key: String,
        value: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::SetSynapseParam {
                edge_id,
                key,
                value,
                reply: tx,
            })
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
            .send(EngineCommand::AddNeuronToFolder {
                label,
                metadata,
                reply: tx,
            })
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

    pub async fn remove_neuron(
        &self,
        node_id: Uuid,
    ) -> Result<Option<FolderRemoveSummary>, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::RemoveNeuron { node_id, reply: tx })
            .await
            .map_err(|_| "engine offline".to_string())?;
        rx.await.map_err(|_| "engine dropped reply".to_string())?
    }

    pub async fn remove_synapse(&self, edge_id: Uuid) -> Result<bool, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::RemoveSynapse { edge_id, reply: tx })
            .await
            .map_err(|_| "engine offline".to_string())?;
        rx.await.map_err(|_| "engine dropped reply".to_string())?
    }

    pub async fn remove_synapse_from_folder(&self, edge_id: Uuid) -> Result<bool, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::RemoveSynapseFromFolder { edge_id, reply: tx })
            .await
            .map_err(|_| "engine offline".to_string())?;
        rx.await.map_err(|_| "engine dropped reply".to_string())?
    }

    /// Persist the current weight set to the open `.cortex/` folder.
    /// `Ok(Some(n))` means n edges were written; `Ok(None)` means no
    /// folder is open and the caller should fall back to its DB path.
    pub async fn flush_weights_to_open_folder(&self) -> Result<Option<usize>, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::FlushWeightsToOpenFolder { reply: tx })
            .await
            .map_err(|_| "engine offline".to_string())?;
        rx.await.map_err(|_| "engine dropped reply".to_string())?
    }

    /// Persist the current neuron dynamic state to the open `.cortex/`
    /// folder. `Ok(Some(n))` is the count of neuron records written;
    /// `Ok(None)` means no folder is open and the caller should treat
    /// the request as a no-op. Intended for graceful-shutdown flushes.
    pub async fn save_state_to_open_folder(&self) -> Result<Option<usize>, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(EngineCommand::SaveStateToOpenFolder { reply: tx })
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
                        EngineCommand::ApplySeed { seed, reply } => {
                            let result = match current_cortex.as_mut() {
                                Some(cortex) => {
                                    let report = cortex
                                        .apply_seed(seed.clone())
                                        .map(|report| EngineSeedReport {
                                            added_nodes: report.added_nodes,
                                            added_edges: report.added_edges,
                                        })
                                        .map_err(|e| e.to_string());
                                    if report.is_ok() {
                                        for node in &seed.nodes {
                                            engine.add_neuron_with_kind(node.id, &current_kind);
                                        }
                                        for edge in &seed.edges {
                                            engine.add_edge(edge.id, edge.pre, edge.post, edge.init_weight);
                                        }
                                    }
                                    report
                                }
                                None => {
                                    let nodes_before = engine.n_neurons();
                                    let edges_before = engine.n_synapses();
                                    for node in seed.nodes {
                                        engine.add_neuron_with_kind(node.id, &current_kind);
                                    }
                                    for edge in seed.edges {
                                        engine.add_edge(edge.id, edge.pre, edge.post, edge.init_weight);
                                    }
                                    Ok(EngineSeedReport {
                                        added_nodes: engine.n_neurons() - nodes_before,
                                        added_edges: engine.n_synapses() - edges_before,
                                    })
                                }
                            };
                            let _ = reply.send(result);
                        }
                        EngineCommand::Stimulate { node_id, current, duration_ms } =>
                            engine.inject(node_id, current, duration_ms),
                        EngineCommand::ForceSpike { node_id, reply } => {
                            let result = if engine.neurons.contains_key(&node_id) {
                                engine.fired_prev.insert(node_id);
                                let frame = SpikeFrame::new(
                                    engine.t_ms,
                                    vec![hebb::engine::events::SpikeEvent {
                                        node_id,
                                        t_ms: engine.t_ms,
                                    }],
                                );
                                let _ = spike_tx.send(frame);
                                Ok(())
                            } else {
                                Err(format!("node {node_id} not in engine"))
                            };
                            let _ = reply.send(result);
                        }
                        EngineCommand::RunFor {
                            duration_ms,
                            dt_ms,
                            reply,
                        } => {
                            let result = run_engine_for(
                                &mut engine,
                                &spike_tx,
                                duration_ms,
                                dt_ms,
                            );
                            let _ = reply.send(result);
                        }
                        EngineCommand::RemoveNeuron { node_id, reply } => {
                            let result = match current_cortex.as_mut() {
                                Some(cortex) => {
                                    let present =
                                        cortex.topology().nodes.iter().any(|n| n.id == node_id);
                                    if !present {
                                        Ok(None)
                                    } else {
                                        match cortex.remove_neuron(node_id) {
                                            Ok(cascaded_edges) => {
                                                engine.remove_neuron(node_id);
                                                Ok(Some(FolderRemoveSummary { cascaded_edges }))
                                            }
                                            Err(e) => Err(e.to_string()),
                                        }
                                    }
                                }
                                None => {
                                    if engine.neurons.contains_key(&node_id) {
                                        let cascaded_edges = engine.remove_neuron(node_id);
                                        Ok(Some(FolderRemoveSummary { cascaded_edges }))
                                    } else {
                                        Ok(None)
                                    }
                                }
                            };
                            let _ = reply.send(result);
                        }
                        EngineCommand::RemoveSynapse { edge_id, reply } => {
                            let result = match current_cortex.as_mut() {
                                Some(cortex) => match cortex.remove_synapse(edge_id) {
                                    Ok(true) => {
                                        engine.remove_synapse(edge_id);
                                        Ok(true)
                                    }
                                    Ok(false) => Ok(false),
                                    Err(e) => Err(e.to_string()),
                                },
                                None => Ok(engine.remove_synapse(edge_id)),
                            };
                            let _ = reply.send(result);
                        }
                        EngineCommand::Snapshot(reply) => {
                            let snap = EngineSnapshot {
                                t_ms: engine.t_ms,
                                n_neurons: engine.n_neurons(),
                                n_synapses: engine.n_synapses(),
                            };
                            let _ = reply.send(snap);
                        }
                        EngineCommand::GraphSnapshot(reply) => {
                            let nodes = engine
                                .list_neurons()
                                .into_iter()
                                .map(|id| EngineGraphNode { id })
                                .collect();
                            let edges = engine
                                .synapses
                                .iter()
                                .map(|synapse| EngineGraphEdge {
                                    id: synapse.id(),
                                    pre_id: synapse.pre_id(),
                                    post_id: synapse.post_id(),
                                    weight: synapse.weight(),
                                    edge_type: synapse.kind_name(),
                                })
                                .collect();
                            let _ = reply.send(EngineGraphSnapshot {
                                t_ms: engine.t_ms,
                                nodes,
                                edges,
                            });
                        }
                        EngineCommand::WeightSnapshot(reply) => {
                            let _ = reply.send(engine.weight_snapshot());
                        }
                        EngineCommand::VoltageSnapshot { nodes, reply } => {
                            let samples = engine.sample_voltages(nodes.as_deref());
                            let _ = reply.send((engine.t_ms, samples));
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
                        EngineCommand::ListSynapses(reply) => {
                            let _ = reply.send(engine.list_synapses());
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
                        EngineCommand::GetSynapseParams { edge_id, reply } => {
                            let _ = reply.send(engine.synapse_params(edge_id));
                        }
                        EngineCommand::GetAllSynapseParams(reply) => {
                            let _ = reply.send(engine.all_synapse_params());
                        }
                        EngineCommand::SetSynapseParam { edge_id, key, value, reply } => {
                            let result = engine
                                .set_synapse_param(edge_id, &key, &value)
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
                        EngineCommand::FlushWeightsToOpenFolder { reply } => {
                            let result = match current_cortex.as_ref() {
                                None => Ok(None),
                                Some(cortex) => {
                                    let snap = engine.weight_snapshot();
                                    let n = snap.len();
                                    cortex
                                        .persist_weights(snap)
                                        .map(|_| Some(n))
                                        .map_err(|e| e.to_string())
                                }
                            };
                            let _ = reply.send(result);
                        }
                        EngineCommand::SaveStateToOpenFolder { reply } => {
                            let result = match current_cortex.as_ref() {
                                None => Ok(None),
                                Some(cortex) => {
                                    let snap = engine.snapshot_neuron_state();
                                    let n = snap.len();
                                    cortex
                                        .persist_state(snap)
                                        .map(|_| Some(n))
                                        .map_err(|e| e.to_string())
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
    use hebb::{HhConfig, HhIntegrator};

    /// Configure the engine to HH, drive it via inject() + tick(), and
    /// assert a spike escapes the broadcast. Proves the actor wiring
    /// (Configure → AddNode-uses-current-kind → SimEngine HH path) works
    /// end-to-end. Uses a fine-grained tick rate (10 kHz → dt = 0.1 ms)
    /// because Euler at the default 1 kHz wouldn't be stable for HH.
    #[tokio::test]
    async fn engine_reconfigured_to_hh_emits_spikes() {
        let (handle, _join) = spawn_engine(10_000);
        let mut spikes = handle.spikes.subscribe();
        let cfg = HhConfig {
            integrator: HhIntegrator::Rk4,
            ..HhConfig::default()
        };
        handle
            .configure(CortexType::Hh { config: cfg })
            .await
            .unwrap();

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
        use hebb::format::topology::{NeuronSpec, SynapseSpec, TopologyDefaults};
        use hebb::{AddNeuron, AddSynapse, CreateOptions};
        use std::time::SystemTime;

        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("core-engine-open-{nanos}"));

        // Build a 2-neuron HH folder.
        let mut cx = hebb::Cortex::create(
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
        let a = cx
            .add_neuron(AddNeuron {
                label: "a".into(),
                ..Default::default()
            })
            .unwrap();
        let b = cx
            .add_neuron(AddNeuron {
                label: "b".into(),
                ..Default::default()
            })
            .unwrap();
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
        let summary = handle
            .open(root.clone())
            .await
            .expect("open should succeed");
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

    /// Round-trip: open a freshly-created folder, push a neuron through
    /// the folder-write command, drop the engine, spawn a new engine,
    /// open the same folder — the neuron must persist via topology.json
    /// without any DB involvement.
    #[tokio::test]
    async fn folder_neuron_write_persists_across_engine_restart() {
        use hebb::format::topology::{NeuronSpec, SynapseSpec, TopologyDefaults};
        use hebb::CreateOptions;
        use std::time::SystemTime;

        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("core-folder-write-persist-{nanos}"));
        hebb::Cortex::create(
            &root,
            CreateOptions {
                name: "Persist".into(),
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

        // Engine #1: open + add.
        let (handle, _join) = spawn_engine(1000);
        handle.open(root.clone()).await.unwrap();
        let rec = handle
            .add_neuron_to_folder(
                "first-manual".to_string(),
                serde_json::json!({ "x": 1.0, "y": 2.0 }),
            )
            .await
            .unwrap();
        // Drop the handle — actor's owned Cortex goes with it.
        drop(handle);

        // Engine #2: a brand-new actor opens the same folder and must
        // see the persisted neuron.
        let (handle2, _join2) = spawn_engine(1000);
        let summary = handle2.open(root.clone()).await.unwrap();
        assert_eq!(summary.n_nodes, 1, "neuron should survive restart on disk");
        let neurons = handle2.list_neurons().await.unwrap();
        assert!(
            neurons.iter().any(|id| *id == rec.id),
            "hydrated SimEngine should contain the persisted neuron"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    /// Cross-network isolation: writes against folder A must not leak
    /// into folder B. The actor switches Cortex handles atomically on
    /// Open, so opening B after writing to A should show zero nodes
    /// from A in B, and reopening A still shows the original write.
    #[tokio::test]
    async fn folder_writes_isolated_across_networks() {
        use hebb::format::topology::{NeuronSpec, SynapseSpec, TopologyDefaults};
        use hebb::CreateOptions;
        use std::time::SystemTime;

        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root_a = std::env::temp_dir().join(format!("core-folder-isolation-a-{nanos}"));
        let root_b = std::env::temp_dir().join(format!("core-folder-isolation-b-{nanos}"));
        for r in [&root_a, &root_b] {
            hebb::Cortex::create(
                r,
                CreateOptions {
                    name: r.file_name().unwrap().to_string_lossy().to_string(),
                    cortex_type: "lif".into(),
                    source_root: r.display().to_string(),
                    now_rfc3339: "2026-05-21T12:00:00Z".into(),
                    defaults: TopologyDefaults {
                        neuron: NeuronSpec::lif(),
                        synapse: SynapseSpec::stdp(),
                    },
                    hh_config: None,
                },
            )
            .unwrap();
        }

        let (handle, _join) = spawn_engine(1000);

        handle.open(root_a.clone()).await.unwrap();
        let a_rec = handle
            .add_neuron_to_folder("a-only".into(), serde_json::json!({}))
            .await
            .unwrap();

        let b_summary = handle.open(root_b.clone()).await.unwrap();
        assert_eq!(
            b_summary.n_nodes, 0,
            "folder B must not see folder A's neuron"
        );
        let b_neurons = handle.list_neurons().await.unwrap();
        assert!(
            b_neurons.iter().all(|id| *id != a_rec.id),
            "SimEngine must be wiped on open-folder swap"
        );

        // Reopening A should still find the original neuron — the disk
        // file owns the persistence, not the engine.
        let a_summary = handle.open(root_a.clone()).await.unwrap();
        assert_eq!(
            a_summary.n_nodes, 1,
            "folder A's neuron must still be on disk"
        );

        std::fs::remove_dir_all(&root_a).ok();
        std::fs::remove_dir_all(&root_b).ok();
    }

    /// Synapse + remove path: add neuron, add synapse, remove neuron —
    /// the cascade must drop the incident edge in both the topology
    /// file and the live SimEngine, and a fresh open must reflect the
    /// final state.
    #[tokio::test]
    async fn folder_synapse_and_cascade_round_trip() {
        use hebb::format::topology::{NeuronSpec, SynapseSpec, TopologyDefaults};
        use hebb::CreateOptions;
        use std::time::SystemTime;

        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("core-folder-cascade-{nanos}"));
        hebb::Cortex::create(
            &root,
            CreateOptions {
                name: "Cascade".into(),
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
        let a = handle
            .add_neuron_to_folder("a".into(), serde_json::json!({}))
            .await
            .unwrap();
        let b = handle
            .add_neuron_to_folder("b".into(), serde_json::json!({}))
            .await
            .unwrap();
        let edge = handle
            .add_synapse_to_folder(a.id, b.id, 0.4, serde_json::json!({}))
            .await
            .unwrap();

        // Sanity: synapse exists in both engine and disk before removal.
        let snap = handle.snapshot().await.unwrap();
        assert_eq!(snap.n_neurons, 2);
        assert_eq!(snap.n_synapses, 1);

        // Removing the pre-neuron must cascade the edge.
        let removed = handle
            .remove_neuron_from_folder(a.id)
            .await
            .unwrap()
            .expect("neuron was present");
        assert_eq!(removed.cascaded_edges, 1);

        let snap2 = handle.snapshot().await.unwrap();
        assert_eq!(snap2.n_neurons, 1, "removed neuron leaves only b");
        assert_eq!(snap2.n_synapses, 0, "incident edge cascaded in engine");

        // Reopen with a fresh engine and confirm disk matches.
        drop(handle);
        let (handle2, _join2) = spawn_engine(1000);
        let summary = handle2.open(root.clone()).await.unwrap();
        assert_eq!(summary.n_nodes, 1);
        assert_eq!(summary.n_edges, 0);

        // Removing the (already-cascaded) edge id now returns false.
        let again = handle2.remove_synapse_from_folder(edge.id).await.unwrap();
        assert!(!again);

        std::fs::remove_dir_all(&root).ok();
    }

    /// Weight write-through: changing a synapse weight in SimEngine
    /// then flushing should land in `weights/{type}/latest.cwt`.
    /// `FlushWeightsToOpenFolder` reports `Ok(None)` when no folder is
    /// open, signaling the persister to take its Postgres path.
    #[tokio::test]
    async fn folder_weight_flush_persists_to_disk_and_noops_without_folder() {
        use hebb::format::topology::{NeuronSpec, SynapseSpec, TopologyDefaults};
        use hebb::CreateOptions;
        use std::time::SystemTime;

        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("core-folder-weight-flush-{nanos}"));
        hebb::Cortex::create(
            &root,
            CreateOptions {
                name: "WeightFlush".into(),
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

        // Pre-open: flush must report Ok(None) so the persister falls
        // back to its Postgres path.
        assert_eq!(handle.flush_weights_to_open_folder().await.unwrap(), None);

        handle.open(root.clone()).await.unwrap();
        let a = handle
            .add_neuron_to_folder("a".into(), serde_json::json!({}))
            .await
            .unwrap();
        let b = handle
            .add_neuron_to_folder("b".into(), serde_json::json!({}))
            .await
            .unwrap();
        let edge = handle
            .add_synapse_to_folder(a.id, b.id, 0.42, serde_json::json!({}))
            .await
            .unwrap();

        // First flush — must succeed and report one written record.
        let written = handle.flush_weights_to_open_folder().await.unwrap();
        assert_eq!(written, Some(1));

        // Reopen the folder via cortex_snn directly and confirm the
        // weight landed on disk with the same id and value.
        drop(handle);
        let cx = hebb::Cortex::open(&root).unwrap();
        let weights = cx.load_weights().unwrap();
        assert_eq!(weights.len(), 1);
        assert_eq!(weights[0].0, edge.id);
        assert!((weights[0].1 - 0.42).abs() < 1e-6);

        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn engine_synapse_params_round_trip() {
        let (handle, _join) = spawn_engine(1000);
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let edge = Uuid::new_v4();

        handle.add_edge(edge, a, b, 0.4).await.unwrap();

        assert_eq!(handle.list_synapses().await.unwrap(), vec![edge]);
        let params = handle.get_synapse_params(edge).await.unwrap().unwrap();
        assert!((params["weight"].as_f64().unwrap() - 0.4).abs() < 1e-6);
        assert_eq!(handle.get_all_synapse_params().await.unwrap().len(), 1);

        let after = handle
            .set_synapse_param(edge, "weight".into(), serde_json::json!(0.8))
            .await
            .unwrap();
        assert!((after["weight"].as_f64().unwrap() - 0.8).abs() < 1e-6);
        assert_eq!(handle.weight_snapshot().await.unwrap(), vec![(edge, 0.8)]);

        let err = handle
            .set_synapse_param(edge, "tau_plus".into(), serde_json::json!(0.0))
            .await
            .unwrap_err();
        assert!(err.contains("out of range"), "got: {err}");
    }

    /// Folder-write commands must refuse cleanly when no folder is open.
    #[tokio::test]
    async fn folder_write_without_open_returns_error() {
        let (handle, _join) = spawn_engine(1000);
        let err = handle
            .add_neuron_to_folder("x".into(), serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(err.contains("no .cortex/ folder open"), "got: {err}");
    }

    /// After a successful Open, a subsequent bare Configure should
    /// detach from the folder and reset `current_folder` to None.
    #[tokio::test]
    async fn configure_after_open_clears_folder() {
        use hebb::format::topology::{NeuronSpec, SynapseSpec, TopologyDefaults};
        use hebb::CreateOptions;
        use std::time::SystemTime;

        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("core-configure-detach-{nanos}"));
        let _cx = hebb::Cortex::create(
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

    /// Open → mutate neuron param via set_node_param → save state →
    /// drop engine → reopen with fresh engine → the mutated v must
    /// survive. Anchors the warm-resume contract from
    /// vault/ideas/runtime-state-persistence-v1.md.
    #[tokio::test]
    async fn state_save_then_reopen_round_trip() {
        use hebb::format::topology::{NeuronSpec, SynapseSpec, TopologyDefaults};
        use hebb::CreateOptions;
        use std::time::SystemTime;

        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("core-state-roundtrip-{nanos}"));
        hebb::Cortex::create(
            &root,
            CreateOptions {
                name: "StateRT".into(),
                cortex_type: "lif".into(),
                source_root: root.display().to_string(),
                now_rfc3339: "2026-05-22T00:00:00Z".into(),
                defaults: TopologyDefaults {
                    neuron: NeuronSpec::lif(),
                    synapse: SynapseSpec::stdp(),
                },
                hh_config: None,
            },
        )
        .unwrap();

        // Engine #1: open, add a neuron, drive its membrane to a non-
        // default value, save state, drop.
        let (h1, _j1) = spawn_engine(1000);
        h1.open(root.clone()).await.unwrap();
        let n = h1
            .add_neuron_to_folder("n".into(), serde_json::json!({}))
            .await
            .unwrap();
        h1.set_node_param(n.id, "v".into(), serde_json::json!(-55.5_f64))
            .await
            .unwrap();
        // The engine ticks on its own (LIF leak), so the live `v` will
        // drift between set_node_param and save. Capture the *actual*
        // snapshot value via the same code path the save uses, then
        // assert that's what survives restart — the contract is "what
        // was on disk at save time hydrates", not "what the caller
        // last set".
        let written = h1.save_state_to_open_folder().await.unwrap();
        assert_eq!(
            written,
            Some(1),
            "save_state should report one persisted neuron"
        );
        let persisted = hebb::Cortex::open(&root)
            .unwrap()
            .load_state()
            .unwrap()
            .expect("state file written");
        let expected_v = persisted
            .neurons
            .get(&n.id)
            .and_then(|v| v["v"].as_f64())
            .expect("v in persisted state");
        drop(h1);

        // Engine #2: reopen and assert the membrane voltage survived
        // the restart via state/{type}/latest.json.
        let (h2, _j2) = spawn_engine(1000);
        let summary = h2.open(root.clone()).await.unwrap();
        assert_eq!(summary.n_nodes, 1);
        let restored = h2.get_node_params(n.id).await.unwrap().unwrap();
        let got_v = restored["v"].as_f64().expect("v in params");
        // Tolerance covers the engine's first post-open tick. We're
        // proving "state loaded from disk replaced the default", not
        // bit-exact reproduction — the default v is -65, the saved
        // value should be in the -55..-56 range we drove it to.
        assert!(
            (got_v - expected_v).abs() < 0.5,
            "membrane voltage should hydrate near the saved value; \
             got {got_v}, saved {expected_v}",
        );
        assert!(
            got_v > -60.0,
            "should not have fallen back to the LIF default (-65)",
        );

        std::fs::remove_dir_all(&root).ok();
    }

    /// Saving state with no folder open must be a no-op that returns
    /// Ok(None), mirroring FlushWeightsToOpenFolder's transient-state
    /// contract. The shutdown hook depends on this — it can't know
    /// whether the engine was hydrated from a folder.
    #[tokio::test]
    async fn save_state_without_open_is_noop() {
        let (h, _j) = spawn_engine(1000);
        let res = h.save_state_to_open_folder().await.unwrap();
        assert_eq!(res, None);
    }

    /// Opening a folder whose state file declares the wrong cortex_type
    /// must fail closed — silently overlaying HH state onto an LIF
    /// network would corrupt without diagnosis.
    #[tokio::test]
    async fn open_rejects_mismatched_state_cortex_type() {
        use hebb::format::state::StateFile;
        use hebb::format::topology::{NeuronSpec, SynapseSpec, TopologyDefaults};
        use hebb::CreateOptions;
        use std::time::SystemTime;

        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("core-state-mismatch-{nanos}"));
        hebb::Cortex::create(
            &root,
            CreateOptions {
                name: "Mismatch".into(),
                cortex_type: "lif".into(),
                source_root: root.display().to_string(),
                now_rfc3339: "2026-05-22T00:00:00Z".into(),
                defaults: TopologyDefaults {
                    neuron: NeuronSpec::lif(),
                    synapse: SynapseSpec::stdp(),
                },
                hh_config: None,
            },
        )
        .unwrap();

        // Hand-write a state file claiming to be HH — write it to the
        // LIF folder's state path. Open must refuse.
        let bogus = StateFile::empty("hh");
        let bytes = bogus.to_json_bytes().unwrap();
        let state_path = hebb::disk::state_path(&root, &bogus.cortex_type);
        std::fs::create_dir_all(state_path.parent().unwrap()).unwrap();
        std::fs::write(&state_path, bytes).unwrap();
        // The state path the LIF reader actually consults — copy too.
        let lif_state_path = hebb::disk::state_path(&root, "lif");
        std::fs::create_dir_all(lif_state_path.parent().unwrap()).unwrap();
        let mut bad = StateFile::empty("hh");
        bad.neurons
            .insert(Uuid::new_v4(), serde_json::json!({"v": -60.0}));
        std::fs::write(&lif_state_path, bad.to_json_bytes().unwrap()).unwrap();

        let (h, _j) = spawn_engine(1000);
        let err = h.open(root.clone()).await.unwrap_err();
        assert!(
            err.contains("cortex_type"),
            "expected mismatch error, got: {err}",
        );

        std::fs::remove_dir_all(&root).ok();
    }

    /// State for a neuron id that's no longer in topology must be
    /// silently skipped on open (warn, don't error). v1 tolerates this
    /// drift so a topology edit + reopen doesn't trip the user.
    #[tokio::test]
    async fn open_tolerates_state_for_deleted_node() {
        use hebb::format::state::StateFile;
        use hebb::format::topology::{NeuronSpec, SynapseSpec, TopologyDefaults};
        use hebb::CreateOptions;
        use std::time::SystemTime;

        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("core-state-orphan-{nanos}"));
        hebb::Cortex::create(
            &root,
            CreateOptions {
                name: "Orphan".into(),
                cortex_type: "lif".into(),
                source_root: root.display().to_string(),
                now_rfc3339: "2026-05-22T00:00:00Z".into(),
                defaults: TopologyDefaults {
                    neuron: NeuronSpec::lif(),
                    synapse: SynapseSpec::stdp(),
                },
                hh_config: None,
            },
        )
        .unwrap();

        // Write state referencing a neuron that doesn't exist in topology.
        let mut s = StateFile::empty("lif");
        s.neurons
            .insert(Uuid::new_v4(), serde_json::json!({"v": -60.0}));
        let path = hebb::disk::state_path(&root, "lif");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, s.to_json_bytes().unwrap()).unwrap();

        let (h, _j) = spawn_engine(1000);
        let summary = h
            .open(root.clone())
            .await
            .expect("orphan state should not fail open");
        assert_eq!(summary.n_nodes, 0);

        std::fs::remove_dir_all(&root).ok();
    }
}

/// Open a `.cortex/` folder via the substrate's [`hebb::Cortex`]
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
    let cortex = hebb::Cortex::open(folder).map_err(|e| e.to_string())?;
    let topology: &TopologyFile = cortex.topology();
    let metadata = cortex.metadata();

    let cortex_type =
        CortexType::from_slug_and_config(&metadata.cortex_type, metadata.hh_config.as_ref())
            .ok_or_else(|| {
                format!(
                    "unknown cortex_type '{}' in folder {}",
                    metadata.cortex_type,
                    folder.display(),
                )
            })?;
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
                n.id, effective.kind, topology.defaults.neuron.kind,
            ));
        }
        engine.add_neuron_with_kind(n.id, &kind);
    }
    for e in &topology.edges {
        // Honor the per-edge synapse kind from topology (stdp vs
        // plastic-synapse). Unknown kinds fail the open rather than
        // silently falling back to STDP — a typo in topology.json should
        // be loud. See hebb::SynapseKind.
        let spec = topology.effective_synapse(e);
        let kind =
            hebb::SynapseKind::from_spec(spec).map_err(|m| format!("edge {}: {}", e.id, m))?;
        engine.add_edge_with_kind(e.id, e.pre, e.post, e.init_weight, &kind);
    }

    // Apply persisted weights on top of the init_weight values. Missing
    // weights file is fine — a freshly-created folder has none.
    let weights = cortex.load_weights().map_err(|e| e.to_string())?;
    let weights_loaded = weights.len();
    for (edge_id, w) in weights {
        // Overwrite the weight in place. The previous implementation
        // called `engine.add_edge(...)` here, which silently downgraded
        // any non-STDP synapse (e.g. PlasticSynapse) back to STDP — the
        // installed kind from `add_edge_with_kind` above was discarded.
        // `set_edge_weight` preserves the kind.
        engine.set_edge_weight(edge_id, w);
    }

    // Apply persisted dynamic state on top of impl defaults. Missing
    // state file is fine — fresh / never-shut-down folders don't have
    // one. A present file with a `cortex_type` that disagrees with the
    // metadata is a hard error — overlaying HH state onto an LIF
    // network would silently corrupt; refuse instead.
    if let Some(state) = cortex.load_state().map_err(|e| e.to_string())? {
        if state.cortex_type != metadata.cortex_type {
            return Err(format!(
                "state file declares cortex_type '{}' but metadata says '{}' \
                 in folder {}",
                state.cortex_type,
                metadata.cortex_type,
                folder.display(),
            ));
        }
        let mut skipped_unknown_keys = 0usize;
        let mut missing_nodes = 0usize;
        for (id, value) in &state.neurons {
            if !topology.nodes.iter().any(|n| n.id == *id) {
                // State for a deleted node — warn-and-skip, don't
                // error. v1 intentionally tolerates topology drift.
                missing_nodes += 1;
                continue;
            }
            skipped_unknown_keys += engine
                .restore_neuron_state(*id, value)
                .map_err(|e| e.to_string())?;
        }
        if missing_nodes > 0 {
            tracing::warn!(
                folder = %folder.display(),
                missing = missing_nodes,
                "state file references nodes no longer in topology; skipped"
            );
        }
        if skipped_unknown_keys > 0 {
            tracing::debug!(
                folder = %folder.display(),
                skipped = skipped_unknown_keys,
                "ignored unknown state fields (forward-compat)"
            );
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

fn run_engine_for(
    engine: &mut SimEngine,
    spike_tx: &broadcast::Sender<SpikeFrame>,
    duration_ms: f32,
    dt_ms: f32,
) -> Result<EngineRunSummary, String> {
    if !duration_ms.is_finite() || !dt_ms.is_finite() {
        return Err("duration_ms and dt_ms must be finite".into());
    }
    if duration_ms < 0.0 {
        return Err("duration_ms must be >= 0".into());
    }
    if dt_ms <= 0.0 {
        return Err("dt_ms must be > 0".into());
    }

    let t_start_ms = engine.t_ms;
    let steps = (duration_ms / dt_ms).ceil() as usize;
    let mut counts: BTreeMap<Uuid, usize> = BTreeMap::new();
    let mut total_spikes = 0;

    for step in 0..steps {
        let elapsed = step as f32 * dt_ms;
        let remaining = (duration_ms - elapsed).max(0.0);
        let step_dt = remaining.min(dt_ms);
        if step_dt <= 0.0 {
            break;
        }
        let frame = engine.tick(step_dt);
        if !frame.events.is_empty() {
            total_spikes += frame.events.len();
            for event in &frame.events {
                *counts.entry(event.node_id).or_insert(0) += 1;
            }
            let _ = spike_tx.send(frame);
        }
    }

    Ok(EngineRunSummary {
        requested_duration_ms: duration_ms,
        duration_ms,
        dt_ms,
        steps,
        t_start_ms,
        t_end_ms: engine.t_ms,
        total_spikes,
        per_neuron: counts
            .into_iter()
            .map(|(node_id, spikes)| EngineRunNeuronCount { node_id, spikes })
            .collect(),
        cancelled: false,
    })
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
                snap.iter()
                    .map(|(id, w)| WeightDelta {
                        edge_id: *id,
                        w: *w,
                    })
                    .collect()
            } else {
                snap.iter()
                    .filter_map(|(id, w)| {
                        let prev = last.get(id).copied().unwrap_or(f32::NAN);
                        if !prev.is_finite() || (w - prev).abs() >= WEIGHT_EPSILON {
                            Some(WeightDelta {
                                edge_id: *id,
                                w: *w,
                            })
                        } else {
                            None
                        }
                    })
                    .collect()
            };

            // Refresh `last` snapshot.
            last.clear();
            for (id, w) in &snap {
                last.insert(*id, *w);
            }

            if deltas.is_empty() && !send_full {
                continue;
            }

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

//! `/api/{nodes,synapses}/:id/params` — live parameter introspection.
//!
//! This is the substrate the agent harness drives: an LLM (or the
//! desktop's parameter panel, or a Python script) reads neuron/synapse
//! state through `GET`, mutates it through `PATCH`. The engine actor owns
//! every write so the simulator's invariants stay intact regardless
//! of how many surfaces are reading concurrently.
//!
//! Substrate validation errors (`ParamError`) surface as 400
//! `bad_request` with the substrate's own `Display` text. The
//! `set_param` impls on `LifNeuron` and `HhNeuron` are deliberately
//! strict about ranges, types, and NaN/Inf — there is no path for an
//! agent to push a neuron into an unphysical state.

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use super::{ok, AppState};
use crate::error::{CoreError, CoreResult};

/// `GET /api/nodes` — list every neuron ID currently in the engine.
pub async fn list_nodes(State(s): State<AppState>) -> CoreResult<Json<serde_json::Value>> {
    let ids = s
        .engine
        .list_neurons()
        .await
        .map_err(|m| CoreError::EngineOffline(m.into()))?;
    let strs: Vec<String> = ids.iter().map(|u| u.to_string()).collect();
    Ok(ok(
        serde_json::json!({ "node_ids": strs, "count": strs.len() }),
    ))
}

/// `GET /api/synapses` — list every synapse edge ID currently in the engine.
pub async fn list_synapses(State(s): State<AppState>) -> CoreResult<Json<serde_json::Value>> {
    let ids = s
        .engine
        .list_synapses()
        .await
        .map_err(|m| CoreError::EngineOffline(m.into()))?;
    let strs: Vec<String> = ids.iter().map(|u| u.to_string()).collect();
    Ok(ok(
        serde_json::json!({ "synapse_ids": strs, "count": strs.len() }),
    ))
}

/// `GET /api/nodes/params` — bulk-fetch every neuron's params keyed
/// by ID. Cheaper than fanning out N GET requests when the agent or
/// UI wants a "what's in this network" snapshot.
pub async fn get_all_params(State(s): State<AppState>) -> CoreResult<Json<serde_json::Value>> {
    let all = s
        .engine
        .get_all_node_params()
        .await
        .map_err(|m| CoreError::EngineOffline(m.into()))?;
    let map: serde_json::Map<String, serde_json::Value> = all
        .into_iter()
        .map(|(id, params)| (id.to_string(), params))
        .collect();
    Ok(ok(serde_json::Value::Object(map)))
}

/// `GET /api/synapses/params` — bulk-fetch every synapse's params keyed
/// by stable edge ID.
pub async fn get_all_synapse_params(
    State(s): State<AppState>,
) -> CoreResult<Json<serde_json::Value>> {
    let all = s
        .engine
        .get_all_synapse_params()
        .await
        .map_err(|m| CoreError::EngineOffline(m.into()))?;
    let map: serde_json::Map<String, serde_json::Value> = all
        .into_iter()
        .map(|(id, params)| (id.to_string(), params))
        .collect();
    Ok(ok(serde_json::Value::Object(map)))
}

/// `GET /api/nodes/:id/params` — read one neuron's introspectable
/// params. 404 if the neuron isn't in the engine.
pub async fn get_node_params(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> CoreResult<Json<serde_json::Value>> {
    let node_id = parse_id("node id", &id)?;
    let params = s
        .engine
        .get_node_params(node_id)
        .await
        .map_err(|m| CoreError::EngineOffline(m.into()))?;
    match params {
        Some(p) => Ok(ok(p)),
        None => Err(CoreError::NotFound(format!("node {id} not in engine"))),
    }
}

/// `GET /api/synapses/:id/params` — read one synapse's introspectable
/// params. 404 if the edge isn't in the engine.
pub async fn get_synapse_params(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> CoreResult<Json<serde_json::Value>> {
    let edge_id = parse_id("synapse id", &id)?;
    let params = s
        .engine
        .get_synapse_params(edge_id)
        .await
        .map_err(|m| CoreError::EngineOffline(m.into()))?;
    match params {
        Some(p) => Ok(ok(p)),
        None => Err(CoreError::NotFound(format!("synapse {id} not in engine"))),
    }
}

#[derive(Debug, Deserialize)]
pub struct SetParamBody {
    /// Parameter name. Examples: `"v_thresh"`, `"tau_m"` (LIF);
    /// `"v"`, `"m"`, `"g_na"`, `"integrator"` (HH);
    /// `"weight"`, `"a_plus"`, `"tau_plus"` (STDP).
    pub key: String,
    /// New value as raw JSON. The substrate validates type + range —
    /// passing a string when a number is expected (or NaN, or
    /// out-of-range) returns a 400 with substrate-supplied text.
    pub value: serde_json::Value,
}

/// `PATCH /api/nodes/:id/params { key, value }` — mutate one
/// parameter. Returns the *new* full param set so the caller can
/// confirm the write without a follow-up GET.
pub async fn set_node_param(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SetParamBody>,
) -> CoreResult<Json<serde_json::Value>> {
    let node_id = parse_id("node id", &id)?;
    let after = s
        .engine
        .set_node_param(node_id, body.key, body.value)
        .await
        .map_err(CoreError::BadRequest)?;
    Ok(ok(after))
}

/// `PATCH /api/synapses/:id/params { key, value }` — mutate one
/// synapse parameter. Returns the new full param set.
pub async fn set_synapse_param(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SetParamBody>,
) -> CoreResult<Json<serde_json::Value>> {
    let edge_id = parse_id("synapse id", &id)?;
    let after = s
        .engine
        .set_synapse_param(edge_id, body.key, body.value)
        .await
        .map_err(CoreError::BadRequest)?;
    Ok(ok(after))
}

fn parse_id(label: &str, id: &str) -> CoreResult<Uuid> {
    Uuid::parse_str(id).map_err(|e| CoreError::BadRequest(format!("{label}: {e}")))
}

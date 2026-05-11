use axum::extract::State;
use serde_json::json;

use super::AppState;
use crate::error::CoreResult;

pub async fn get_health(State(s): State<AppState>) -> CoreResult<axum::Json<serde_json::Value>> {
    let snap = s.engine.snapshot().await.ok();
    Ok(super::ok(json!({
        "status": "ok",
        "engine": snap.map(|s| json!({
            "t_ms": s.t_ms,
            "n_neurons": s.n_neurons,
            "n_synapses": s.n_synapses,
        })),
    })))
}

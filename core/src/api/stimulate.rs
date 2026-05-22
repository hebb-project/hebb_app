use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use super::{ok, AppState};
use crate::error::{CoreError, CoreResult};

#[derive(Debug, Deserialize)]
pub struct StimulateBody {
    pub node_id: Uuid,
    pub current: f32,
    #[serde(default = "default_duration")]
    pub duration_ms: f32,
}

fn default_duration() -> f32 {
    20.0
}

pub async fn post_stimulate(
    State(s): State<AppState>,
    Json(body): Json<StimulateBody>,
) -> CoreResult<Json<serde_json::Value>> {
    if !body.current.is_finite() || !body.duration_ms.is_finite() {
        return Err(CoreError::BadRequest("non-finite current/duration".into()));
    }
    if body.duration_ms <= 0.0 {
        return Err(CoreError::BadRequest("duration_ms must be > 0".into()));
    }
    s.engine
        .stimulate(body.node_id, body.current, body.duration_ms)
        .await
        .map_err(|m| CoreError::EngineOffline(m.into()))?;
    Ok(ok(serde_json::json!({
        "node_id": body.node_id,
        "current": body.current,
        "duration_ms": body.duration_ms,
    })))
}

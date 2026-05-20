//! `/api/cortex` — cortex-type configuration surface.
//!
//! The frontend posts a cortex type when the user opens a network; the
//! engine re-seeds itself accordingly. `GET` returns the current type
//! so a reconnecting client can pick the right viz without re-asking
//! the user.
//!
//! Reconfigure is *destructive* — wiping in-memory state is the only
//! way to keep neuron-kind uniformity inside `SimEngine`. The DB is
//! the authoritative topology; the desktop is expected to re-ingest
//! (or skip ingestion) after a successful configure.

use axum::extract::State;
use axum::Json;

use super::{ok, AppState};
use crate::cortex_type::CortexType;
use crate::error::{CoreError, CoreResult};

pub async fn post_configure(
    State(s): State<AppState>,
    Json(body): Json<CortexType>,
) -> CoreResult<Json<serde_json::Value>> {
    let slug = body.slug();
    s.engine
        .configure(body)
        .await
        .map_err(|m| CoreError::EngineOffline(m.into()))?;
    tracing::info!(cortex_type = %slug, "engine reconfigured");
    Ok(ok(serde_json::json!({ "cortex_type": slug })))
}

pub async fn get_current(
    State(s): State<AppState>,
) -> CoreResult<Json<serde_json::Value>> {
    let ct = s.engine
        .cortex_type()
        .await
        .map_err(|m| CoreError::EngineOffline(m.into()))?;
    Ok(ok(ct))
}

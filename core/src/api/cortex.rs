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

use std::path::PathBuf;

use axum::extract::State;
use axum::Json;
use serde::Deserialize;

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

pub async fn get_current(State(s): State<AppState>) -> CoreResult<Json<serde_json::Value>> {
    let ct = s
        .engine
        .cortex_type()
        .await
        .map_err(|m| CoreError::EngineOffline(m.into()))?;
    Ok(ok(ct))
}

#[derive(Debug, Deserialize)]
pub struct OpenBody {
    /// Absolute path to a `.cortex/` folder. Must already exist and
    /// contain a valid `metadata.json` + `topology.json`. The desktop
    /// supplies this directly from the user's Open dialog; the Python
    /// library passes the path it just created with `Cortex.create`.
    pub folder: String,
}

/// `POST /api/cortex/open` — hydrate the engine from a `.cortex/`
/// folder on disk. Replaces any prior in-memory state. Surfaces the
/// substrate's validation errors verbatim so clients can render them
/// to the user.
pub async fn post_open(
    State(s): State<AppState>,
    Json(body): Json<OpenBody>,
) -> CoreResult<Json<serde_json::Value>> {
    let folder = PathBuf::from(body.folder);
    let summary = s
        .engine
        .open(folder)
        .await
        .map_err(CoreError::EngineOffline)?;
    Ok(ok(
        serde_json::to_value(summary).unwrap_or(serde_json::Value::Null)
    ))
}

/// `GET /api/cortex/folder` — the path of the currently-open folder,
/// or null if the engine is running on transient state.
pub async fn get_folder(State(s): State<AppState>) -> CoreResult<Json<serde_json::Value>> {
    let folder = s
        .engine
        .current_folder()
        .await
        .map_err(|m| CoreError::EngineOffline(m.into()))?;
    Ok(ok(serde_json::json!({
        "folder": folder.map(|p| p.display().to_string()),
    })))
}

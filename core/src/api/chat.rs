//! `POST /api/chat` — Rust-native chat pipeline.
//!
//! Same flow the Python bridge runs, but in-process:
//!
//! 1. Load the node catalogue (labels + body_text) from the DB.
//! 2. Ask the configured [`super::AppState::chat_encoder`] to pick
//!    seed nodes + currents.
//! 3. Subscribe to the engine's spike broadcast *before* injecting,
//!    so the first cascade frame isn't missed.
//! 4. Inject every stimulation in parallel.
//! 5. Collect spikes for `ACTIVATION_TAP_MS` and tally per-node counts.
//! 6. Hand stim + activation back to the encoder for reply synthesis.
//!
//! No HTTP hop, no WS handshake, no Python sidecar. The desktop app
//! ships the binary that does this — that's the whole point of
//! Phase 2.

use std::collections::HashMap;
use std::time::Duration;

use axum::extract::State;
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;
use tokio::time::{timeout_at, Instant};
use uuid::Uuid;

use super::{ok, AppState};
use crate::chat::{Activation, NodeRef, Stimulus};
use crate::db::models::NodeRow;
use crate::db::run_blocking;
use crate::db::schema::nodes;
use crate::error::{CoreError, CoreResult};

/// How long we wait for spikes after stimulation before tallying. The
/// Python bridge uses 700 ms; matched here so behavior is comparable
/// across the two paths during the transition.
const ACTIVATION_TAP_MS: u64 = 700;

/// Hard ceiling on activations reported back to the caller. Keeps the
/// response payload small and matches the bridge's `.most_common(12)`.
const MAX_ACTIVATIONS: usize = 12;

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub reply: String,
    pub encoder: String,
    pub stimulated: Vec<Stimulus>,
    pub activated: Vec<Activation>,
}

pub async fn post_chat(
    State(s): State<AppState>,
    Json(body): Json<ChatRequest>,
) -> CoreResult<Json<serde_json::Value>> {
    let message = body.message.trim();
    if message.is_empty() {
        return Err(CoreError::BadRequest("message must be non-empty".into()));
    }

    // 1. Load nodes from the DB.
    let rows: Vec<NodeRow> = run_blocking(&s.pool, |conn| {
        Ok(nodes::table.select(NodeRow::as_select()).load(conn)?)
    })
    .await?;
    if rows.is_empty() {
        return Err(CoreError::BadRequest(
            "cortex has no nodes — ingest a vault first".into(),
        ));
    }

    let candidates: Vec<NodeRef> = rows.iter().map(node_row_to_ref).collect();
    let label_by_id: HashMap<Uuid, String> = rows.iter().map(|n| (n.id, n.label.clone())).collect();

    // 2. Encoder picks seeds.
    let encoder = s.chat_encoder.clone();
    let stimuli = encoder.encode(message, &candidates).await;

    // 3. Subscribe before injecting so the first cascade frame can't
    //    slip past us between the `stimulate` call and the receiver
    //    being awake.
    let mut spike_rx = s.engine.spikes.subscribe();
    let deadline = Instant::now() + Duration::from_millis(ACTIVATION_TAP_MS);
    let tap_task = tokio::spawn(async move {
        let mut counts: HashMap<Uuid, u32> = HashMap::new();
        loop {
            match timeout_at(deadline, spike_rx.recv()).await {
                Ok(Ok(frame)) => {
                    for ev in frame.events {
                        *counts.entry(ev.node_id).or_insert(0) += 1;
                    }
                }
                // Subscriber lagged behind: continue tallying with the
                // newer frames rather than aborting the chat.
                Ok(Err(RecvError::Lagged(n))) => {
                    tracing::warn!(skipped = n, "chat spike tap lagged");
                }
                Ok(Err(RecvError::Closed)) => break,
                Err(_) => break, // deadline reached
            }
        }
        counts
    });

    // 4. Inject all stimulations in parallel. Engine handle is cheap
    //    to clone (it's an mpsc Sender).
    let inject_results = futures_util::future::join_all(stimuli.iter().map(|stim| {
        let engine = s.engine.clone();
        let stim = stim.clone();
        async move {
            engine
                .stimulate(stim.node_id, stim.current, stim.duration_ms)
                .await
        }
    }))
    .await;
    for (i, r) in inject_results.into_iter().enumerate() {
        if let Err(e) = r {
            tracing::warn!(
                stim_index = i,
                error = e,
                "stimulate failed (engine offline?); continuing"
            );
        }
    }

    // 5. Wait for the tap window.
    let counts = tap_task.await.unwrap_or_default();

    // Sort by spike count descending, drop the seeds themselves so the
    // activation list highlights *propagation*, not the input.
    let stim_set: std::collections::HashSet<Uuid> = stimuli.iter().map(|s| s.node_id).collect();
    let mut activated: Vec<Activation> = counts
        .into_iter()
        .filter(|(id, _)| !stim_set.contains(id))
        .filter_map(|(id, count)| {
            label_by_id.get(&id).map(|label| Activation {
                node_id: id,
                label: label.clone(),
                spike_count: count,
            })
        })
        .collect();
    activated.sort_by(|a, b| {
        b.spike_count
            .cmp(&a.spike_count)
            .then_with(|| a.label.cmp(&b.label))
    });
    activated.truncate(MAX_ACTIVATIONS);

    // 6. Synthesize the reply.
    let reply = encoder
        .synthesize_reply(message, &stimuli, &activated)
        .await;

    Ok(ok(ChatResponse {
        reply,
        encoder: encoder.name().to_string(),
        stimulated: stimuli,
        activated,
    }))
}

fn node_row_to_ref(n: &NodeRow) -> NodeRef {
    // Encoders only need text — strip metadata to the body text the
    // vault parser stored. Fall back to body_excerpt if body_text isn't
    // present (older ingests).
    let body = n
        .metadata
        .get("body_text")
        .or_else(|| n.metadata.get("body_excerpt"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    NodeRef {
        id: n.id,
        label: n.label.clone(),
        node_type: n.node_type.clone(),
        body,
    }
}

// Make ChatResponse serializable through axum::Json + our envelope.
impl From<ChatResponse> for serde_json::Value {
    fn from(v: ChatResponse) -> Self {
        serde_json::to_value(v).unwrap_or(serde_json::Value::Null)
    }
}

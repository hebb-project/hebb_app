//! In-process chat pipeline: query → stimulate cortex → tap spikes → reply.
//!
//! This is the Rust port of `experiments/bridge/service.py`, lifted up
//! into `core` so the desktop app doesn't need a Python sidecar to
//! answer chat. The bridge stays in the repo as the fast-iteration
//! research path; this module is the shipping one.
//!
//! ## Encoder ladder
//!
//! Built at startup based on environment:
//!
//! | Encoder      | Trigger                              | What it does |
//! |--------------|--------------------------------------|--------------|
//! | `anthropic`  | `ANTHROPIC_API_KEY` present          | Asks Haiku to score each candidate label for relevance, like the Python bridge. |
//! | `lexical`    | always (default fallback)            | Token-overlap scoring against labels + body. Zero deps, deterministic, no API key — the "downloaded desktop just works" path. |
//!
//! Adding a future `embedding` encoder (bundled ONNX MiniLM via
//! `fastembed-rs` / `ort`) means a new module + a clause in
//! [`make_encoder`]. The trait surface is intentionally narrow so
//! that addition is additive, not a refactor.

pub mod anthropic;
pub mod lexical;

use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use uuid::Uuid;

/// What an encoder needs to know about a candidate node. Decoupled from
/// `db::models::NodeRow` so encoders can be unit-tested without the DB.
#[derive(Debug, Clone)]
pub struct NodeRef {
    pub id: Uuid,
    pub label: String,
    pub node_type: String,
    /// Pre-extracted body text for lexical / dense scoring. Optional —
    /// the Anthropic encoder ignores it.
    pub body: String,
}

/// One stimulation directive from the encoder. Mirrors the Python
/// bridge's `Stimulus`. Currents are clamped to [0, MAX_CURRENT] by
/// the encoder; the handler does not re-clamp.
#[derive(Debug, Clone, Serialize)]
pub struct Stimulus {
    pub node_id: Uuid,
    pub label: String,
    pub current: f32,
    pub duration_ms: f32,
    pub score: f32,
}

/// What lit up after the cascade.
#[derive(Debug, Clone, Serialize)]
pub struct Activation {
    pub node_id: Uuid,
    pub label: String,
    pub spike_count: u32,
}

/// The encoder API. Two-method trait: pick seeds, then verbalize the
/// resulting cascade. Implementations are cheap-to-construct so the
/// factory can rebuild on env change (no warm caches yet at this
/// layer; embedding encoders will introduce that and own their own
/// state).
#[async_trait]
pub trait ChatEncoder: Send + Sync {
    fn name(&self) -> &'static str;

    async fn encode(&self, message: &str, nodes: &[NodeRef]) -> Vec<Stimulus>;

    async fn synthesize_reply(
        &self,
        message: &str,
        stimulated: &[Stimulus],
        activated: &[Activation],
    ) -> String;
}

/// Type-erased encoder. Boxed so the handler can hold one trait object
/// regardless of which concrete encoder the factory chose.
pub type ArcEncoder = Arc<dyn ChatEncoder + Send + Sync + 'static>;

/// Build the configured encoder. Falls back to lexical when no API
/// keys are present so the downloaded desktop is functional with zero
/// setup. Override with `CORTEX_CHAT_ENCODER={anthropic,lexical}`.
pub fn make_encoder() -> ArcEncoder {
    let explicit = std::env::var("CORTEX_CHAT_ENCODER").ok();
    let choice = explicit.as_deref().unwrap_or_else(|| {
        if std::env::var("ANTHROPIC_API_KEY").is_ok() {
            "anthropic"
        } else {
            "lexical"
        }
    });
    match choice {
        "anthropic" => match anthropic::AnthropicEncoder::from_env() {
            Ok(enc) => Arc::new(enc),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "anthropic encoder unavailable; falling back to lexical"
                );
                Arc::new(lexical::LexicalEncoder::default())
            }
        },
        "lexical" => Arc::new(lexical::LexicalEncoder::default()),
        other => {
            tracing::warn!(other, "unknown CORTEX_CHAT_ENCODER, using lexical");
            Arc::new(lexical::LexicalEncoder::default())
        }
    }
}

/// Format a deterministic reply describing what happened in the
/// cortex. Used by encoders that don't have an LLM available; also
/// useful as a debug view.
pub fn deterministic_reply(stimulated: &[Stimulus], activated: &[Activation]) -> String {
    if stimulated.is_empty() {
        return "Nothing relevant fired. The cortex doesn't have a concept matching this query yet.".into();
    }
    let stim_labels: Vec<&str> = stimulated.iter().map(|s| s.label.as_str()).take(4).collect();
    let act_str = if activated.is_empty() {
        "cortex stayed quiet — stimulated nodes didn't propagate enough to ignite their neighbors".to_string()
    } else {
        activated
            .iter()
            .take(5)
            .map(|a| format!("{} (×{})", a.label, a.spike_count))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!("Stimulated [{}]. Activated: {}.", stim_labels.join(", "), act_str)
}

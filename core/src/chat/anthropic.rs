//! Anthropic-backed chat encoder.
//!
//! Mirrors `experiments/bridge/encoders.py::HaikuLabelEncoder`: asks
//! Claude Haiku to score each candidate node label for relevance to
//! the user message; the scored labels become stimulation currents.
//! Reply synthesis uses a second Claude call describing what actually
//! fired in the cortex.
//!
//! Hand-rolled `reqwest` instead of pulling in the community
//! `anthropic` crate. The API surface we need is tiny (one POST to
//! `/v1/messages`), and dropping a dep keeps the desktop binary
//! smaller for bundling.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Activation, ChatEncoder, NodeRef, Stimulus};

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MODEL: &str = "claude-haiku-4-5-20251001";
const ANTHROPIC_ENDPOINT: &str = "https://api.anthropic.com/v1/messages";

const ENCODE_SYSTEM_PROMPT: &str = "You score how relevant each concept node in a knowledge graph is to a user message. The graph is the user's notes, ingested as a spiking neural network. Your relevance scores will be turned into input currents that ignite those nodes, so the cortex will physically light up the concepts you choose.\n\nReturn STRICT JSON: a list of {\"label\": <string from the candidate list>, \"score\": <float 0..1>} objects. Include only nodes that are clearly relevant (score >= 0.3). Omit irrelevant ones. No prose, no markdown fences.";

pub struct AnthropicEncoder {
    api_key: String,
    model: String,
    client: reqwest::Client,
    pub top_k: usize,
    pub max_current: f32,
    pub duration_ms: f32,
}

impl AnthropicEncoder {
    pub fn from_env() -> Result<Self, String> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| "ANTHROPIC_API_KEY not set".to_string())?;
        let model =
            std::env::var("CORTEX_CHAT_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        let client = reqwest::Client::builder()
            // The Anthropic call is the long pole on a chat round-trip.
            // 30s gives Haiku room without letting a hung connection
            // pin a desktop user's UI forever.
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| format!("building http client: {e}"))?;
        Ok(Self {
            api_key,
            model,
            client,
            top_k: 6,
            max_current: 45.0,
            duration_ms: 500.0,
        })
    }

    async fn call_messages(
        &self,
        system: Option<&str>,
        user: &str,
        max_tokens: u32,
    ) -> Result<String, String> {
        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "messages": [{"role": "user", "content": user}],
        });
        if let Some(s) = system {
            body["system"] = serde_json::Value::String(s.to_string());
        }
        let resp = self
            .client
            .post(ANTHROPIC_ENDPOINT)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("anthropic request failed: {e}"))?;

        let status = resp.status();
        let raw = resp
            .text()
            .await
            .map_err(|e| format!("anthropic response body: {e}"))?;
        if !status.is_success() {
            return Err(format!("anthropic http {status}: {raw}"));
        }
        let parsed: AnthropicMessage = serde_json::from_str(&raw)
            .map_err(|e| format!("parsing anthropic response: {e}; body was: {raw}"))?;
        let text = parsed
            .content
            .into_iter()
            .filter_map(|b| if b.kind == "text" { Some(b.text) } else { None })
            .collect::<String>();
        Ok(text)
    }
}

#[async_trait]
impl ChatEncoder for AnthropicEncoder {
    fn name(&self) -> &'static str {
        "anthropic"
    }

    async fn encode(&self, message: &str, nodes: &[NodeRef]) -> Vec<Stimulus> {
        if nodes.is_empty() {
            return Vec::new();
        }

        // Compact catalogue grouped by node type — keeps the prompt
        // cheap. `BTreeMap` gives a stable order so the same vault
        // produces the same prompt across runs (good for caching, good
        // for reproducibility).
        let mut by_type: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for n in nodes {
            by_type
                .entry(n.node_type.as_str())
                .or_default()
                .push(n.label.as_str());
        }
        let mut catalogue = String::new();
        for (t, labels) in &mut by_type {
            labels.sort_unstable();
            catalogue.push_str(&format!("  {t}: {}\n", labels.join(", ")));
        }

        let user_msg = format!(
            "Candidate nodes (grouped by type):\n{catalogue}\nUser message: \"\"\"{message}\"\"\"\n\nJSON only."
        );

        let text = match self
            .call_messages(Some(ENCODE_SYSTEM_PROMPT), &user_msg, 512)
            .await
        {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(error = %e, "anthropic encode call failed; returning no stimuli");
                return Vec::new();
            }
        };

        let scores = parse_label_scores(&text);
        let by_label: BTreeMap<&str, &NodeRef> =
            nodes.iter().map(|n| (n.label.as_str(), n)).collect();
        let mut scored: Vec<(Uuid, String, f32)> = scores
            .into_iter()
            .filter_map(|(label, score)| {
                by_label
                    .get(label.as_str())
                    .map(|n| (n.id, n.label.clone(), score))
            })
            .collect();
        scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(self.top_k);

        scored
            .into_iter()
            .map(|(id, label, score)| Stimulus {
                node_id: id,
                label,
                current: self.max_current * score.clamp(0.0, 1.0),
                duration_ms: self.duration_ms,
                score,
            })
            .collect()
    }

    async fn synthesize_reply(
        &self,
        message: &str,
        stimulated: &[Stimulus],
        activated: &[Activation],
    ) -> String {
        let stim_block = if stimulated.is_empty() {
            "  (nothing — no relevant concepts found)".to_string()
        } else {
            stimulated
                .iter()
                .map(|s| {
                    format!(
                        "  - {} (score {:.2}, {:.1} current)",
                        s.label, s.score, s.current
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let act_block = if activated.is_empty() {
            "  (nothing — cortex stayed quiet)".to_string()
        } else {
            activated
                .iter()
                .map(|a| format!("  - {}: {} spikes", a.label, a.spike_count))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let prompt = format!(
            "You are the verbal interface to a spiking-neural-network cortex built from the user's knowledge graph. You don't speak for the user — you report what just happened in the cortex.\n\nThe user said: {message:?}\n\nTo ignite the cortex you stimulated:\n{stim_block}\n\nWithin ~700 ms after stimulation, these nodes activated (spike counts indicate how strongly they lit up):\n{act_block}\n\nWrite a short, candid reply (1-3 sentences). Reference what *physically* happened — which concepts lit up, which paths conducted, what didn't fire. Voice: a scientific assistant narrating instruments. Don't claim to \"think\"; describe propagation. Don't hallucinate connections the spike list doesn't support."
        );

        match self.call_messages(None, &prompt, 400).await {
            Ok(text) if !text.trim().is_empty() => text,
            Ok(_) => super::deterministic_reply(stimulated, activated),
            Err(e) => {
                tracing::warn!(error = %e, "anthropic reply call failed; deterministic fallback");
                super::deterministic_reply(stimulated, activated)
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct AnthropicMessage {
    content: Vec<AnthropicBlock>,
}

#[derive(Debug, Deserialize)]
struct AnthropicBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct LabelScore {
    label: String,
    #[serde(default)]
    score: f32,
}

/// Extract `[{label, score}, ...]` from a model JSON response.
/// Tolerates markdown fences and leading/trailing prose since models
/// occasionally fence JSON even when asked not to.
fn parse_label_scores(text: &str) -> Vec<(String, f32)> {
    let trimmed = text.trim();
    let body = strip_code_fence(trimmed).unwrap_or(trimmed);
    let candidate = match serde_json::from_str::<Vec<LabelScore>>(body) {
        Ok(v) => Some(v),
        Err(_) => {
            // Last-resort: find the first `[` ... `]` block and re-parse.
            match (body.find('['), body.rfind(']')) {
                (Some(start), Some(end)) if end > start => {
                    serde_json::from_str::<Vec<LabelScore>>(&body[start..=end]).ok()
                }
                _ => None,
            }
        }
    };
    candidate
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.label, s.score.clamp(0.0, 1.0)))
        .collect()
}

fn strip_code_fence(s: &str) -> Option<&str> {
    let s = s
        .strip_prefix("```json")
        .or_else(|| s.strip_prefix("```"))?;
    let s = s.trim_start_matches('\n');
    let s = s.strip_suffix("```").unwrap_or(s);
    Some(s.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clean_json() {
        let raw = r#"[{"label":"hannibal","score":0.9},{"label":"rome","score":0.5}]"#;
        let out = parse_label_scores(raw);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, "hannibal");
        assert!((out[0].1 - 0.9).abs() < 1e-6);
    }

    #[test]
    fn tolerates_markdown_fence() {
        let raw = "```json\n[{\"label\":\"hannibal\",\"score\":0.8}]\n```";
        let out = parse_label_scores(raw);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "hannibal");
    }

    #[test]
    fn clamps_score() {
        let raw = r#"[{"label":"x","score":2.0},{"label":"y","score":-0.5}]"#;
        let out = parse_label_scores(raw);
        assert_eq!(out[0].1, 1.0);
        assert_eq!(out[1].1, 0.0);
    }

    #[test]
    fn ignores_surrounding_prose() {
        let raw = "Here you go: [{\"label\":\"a\",\"score\":0.4}] cheers";
        let out = parse_label_scores(raw);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "a");
    }
}

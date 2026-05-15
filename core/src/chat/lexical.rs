//! Lexical (token-overlap) chat encoder.
//!
//! Default encoder when no API key is available. Scores candidate
//! nodes by how many query terms appear in their label or body, then
//! emits the top-k as stimuli. Reply is deterministic — describes
//! what fired without LLM synthesis.
//!
//! Deliberately simple. It is *not* trying to be a great retriever;
//! its job is to make a downloaded desktop usable with zero setup
//! while a more capable encoder (Anthropic, or a local ONNX embedding)
//! is plugged in above it via [`super::make_encoder`].

use async_trait::async_trait;
use uuid::Uuid;

use super::{deterministic_reply, Activation, ChatEncoder, NodeRef, Stimulus};

pub struct LexicalEncoder {
    pub top_k: usize,
    pub max_current: f32,
    pub duration_ms: f32,
    pub min_score: f32,
}

impl Default for LexicalEncoder {
    fn default() -> Self {
        Self {
            top_k: 6,
            max_current: 45.0,
            duration_ms: 500.0,
            min_score: 0.05,
        }
    }
}

#[async_trait]
impl ChatEncoder for LexicalEncoder {
    fn name(&self) -> &'static str {
        "lexical"
    }

    async fn encode(&self, message: &str, nodes: &[NodeRef]) -> Vec<Stimulus> {
        let terms = query_terms(message);
        if terms.is_empty() || nodes.is_empty() {
            return Vec::new();
        }

        // Score each node. Label hits dominate body hits because the
        // label is the canonical concept name; body matches are
        // secondary evidence.
        let mut scored: Vec<(usize, f32)> = nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| {
                let label = n.label.to_lowercase();
                let body = n.body.to_lowercase();
                let mut hits = 0.0_f32;
                let mut term_count = 0.0_f32;
                for term in &terms {
                    term_count += 1.0;
                    if label.contains(term) {
                        hits += 1.0;
                    } else if body.contains(term) {
                        hits += 0.4;
                    }
                }
                if hits <= 0.0 {
                    return None;
                }
                // Normalize to [0, 1]: each query term contributes up
                // to 1.0 (label) or 0.4 (body).
                let score = (hits / term_count).min(1.0);
                if score < self.min_score {
                    return None;
                }
                Some((i, score))
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(self.top_k);

        scored
            .into_iter()
            .map(|(i, score)| {
                let n = &nodes[i];
                Stimulus {
                    node_id: n.id,
                    label: n.label.clone(),
                    current: self.max_current * score.clamp(0.0, 1.0),
                    duration_ms: self.duration_ms,
                    score,
                }
            })
            .collect()
    }

    async fn synthesize_reply(
        &self,
        _message: &str,
        stimulated: &[Stimulus],
        activated: &[Activation],
    ) -> String {
        deterministic_reply(stimulated, activated)
    }
}

/// Lower-case, strip punctuation, drop stopwords + 1-char tokens.
/// Stopword list intentionally mirrors `api::search::STOPWORDS` so the
/// chat encoder and the vault search behave consistently.
fn query_terms(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .map(str::trim)
        .filter(|s| s.len() > 1 && !STOPWORDS.contains(s))
        .map(ToOwned::to_owned)
        .collect()
}

const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "how",
    "in", "is", "it", "of", "on", "or", "the", "to", "was", "were", "what",
    "when", "where", "who", "whose", "why", "with", "tell", "me", "about",
    "did", "do", "does",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn node(label: &str, body: &str, node_type: &str) -> NodeRef {
        NodeRef {
            id: Uuid::new_v4(),
            label: label.into(),
            node_type: node_type.into(),
            body: body.into(),
        }
    }

    #[tokio::test]
    async fn label_hits_outrank_body_hits() {
        let enc = LexicalEncoder::default();
        let nodes = vec![
            node("hannibal", "carthaginian general", "person"),
            node("rome", "hannibal led an army against rome", "place"),
            node("plato", "philosopher", "person"),
        ];
        let out = enc.encode("Who was Hannibal?", &nodes).await;
        assert!(!out.is_empty());
        assert_eq!(out[0].label, "hannibal");
    }

    #[tokio::test]
    async fn empty_query_returns_no_stimuli() {
        let enc = LexicalEncoder::default();
        let nodes = vec![node("hannibal", "x", "person")];
        let out = enc.encode("the of an", &nodes).await; // all stopwords
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn reply_describes_activation_when_present() {
        let enc = LexicalEncoder::default();
        let stim = vec![Stimulus {
            node_id: Uuid::new_v4(),
            label: "hannibal".into(),
            current: 40.0,
            duration_ms: 500.0,
            score: 0.9,
        }];
        let act = vec![Activation {
            node_id: Uuid::new_v4(),
            label: "carthage".into(),
            spike_count: 7,
        }];
        let r = enc.synthesize_reply("x", &stim, &act).await;
        assert!(r.contains("hannibal") && r.contains("carthage"));
    }

    #[tokio::test]
    async fn reply_calls_out_silence_when_no_activation() {
        let enc = LexicalEncoder::default();
        let stim = vec![Stimulus {
            node_id: Uuid::new_v4(),
            label: "hannibal".into(),
            current: 40.0,
            duration_ms: 500.0,
            score: 0.9,
        }];
        let r = enc.synthesize_reply("x", &stim, &[]).await;
        assert!(r.to_lowercase().contains("quiet"));
    }
}

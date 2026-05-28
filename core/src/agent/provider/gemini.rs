//! Gemini provider (`#9`) — implements [`LlmProvider`] over `reqwest`.
//!
//! Talks to the Generative Language API `:generateContent` endpoint, no SDK
//! (same posture as `chat/anthropic.rs`). The conversion between our
//! provider-neutral types and Gemini's wire format lives in the free
//! functions [`to_wire_request`] / [`from_wire_response`], which are pure and
//! unit-tested without a network; [`GeminiProvider::complete`] is the thin
//! HTTP shell around them.
//!
//! Mapping notes:
//! - `System` turns are lifted into `system_instruction`.
//! - `User`/`Assistant` map to Gemini roles `user`/`model`.
//! - Assistant `tool_calls` become `functionCall` parts; `Tool` results
//!   become `functionResponse` parts (role `user`), routed **by name** since
//!   Gemini has no tool-call ids — hence [`ChatMessage::name`].
//! - Gemini doesn't return tool-call ids, so we synthesize `call_<i>`.
//! - Streaming is `#10`; this is the single-shot path.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{
    ChatMessage, ChatRequest, ChatResponse, FinishReason, LlmProvider, ProviderError, Role,
    TokenUsage, ToolCall,
};

const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

/// Gemini provider. Holds a reusable `reqwest::Client`; the API key is passed
/// per call (never stored here) per the keychain design.
pub struct GeminiProvider {
    client: reqwest::Client,
    base_url: String,
}

impl GeminiProvider {
    pub fn new() -> Self {
        Self { client: reqwest::Client::new(), base_url: DEFAULT_BASE_URL.to_string() }
    }

    /// Override the base URL (used by tests to point at a local mock server).
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

impl Default for GeminiProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LlmProvider for GeminiProvider {
    fn id(&self) -> &'static str {
        "gemini"
    }

    async fn complete(
        &self,
        request: &ChatRequest,
        api_key: &str,
    ) -> Result<ChatResponse, ProviderError> {
        let url = format!("{}/models/{}:generateContent", self.base_url, request.model);
        let body = to_wire_request(request);

        let resp = self
            .client
            .post(&url)
            // Key as a header (never in the URL/query) so it can't leak into
            // request logs or proxies that record URLs.
            .header("x-goog-api-key", api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Transport(e.to_string()))?;

        let status = resp.status();
        let retry_after = parse_retry_after(resp.headers());
        let text = resp
            .text()
            .await
            .map_err(|e| ProviderError::Transport(e.to_string()))?;

        if !status.is_success() {
            return Err(classify_http_error(status.as_u16(), &text, retry_after));
        }

        let wire: GeminiResponseBody = serde_json::from_str(&text)
            .map_err(|e| ProviderError::Decode(format!("{e}: {text}")))?;
        from_wire_response(wire)
    }
}

/// Map a non-2xx HTTP response to a typed error, pulling Gemini's
/// `error.message` when present. The retry classifier ([A6]) decides
/// what's worth a second attempt — this just picks the variant; see
/// [`ProviderError::is_retryable`].
fn classify_http_error(
    status: u16,
    body: &str,
    retry_after: Option<std::time::Duration>,
) -> ProviderError {
    let message = serde_json::from_str::<GeminiErrorEnvelope>(body)
        .ok()
        .map(|e| e.error.message)
        .unwrap_or_else(|| body.to_string());
    match status {
        400 => ProviderError::InvalidRequest(message),
        401 | 403 => ProviderError::Auth(message),
        429 => ProviderError::RateLimit {
            message,
            retry_after,
        },
        500..=599 => ProviderError::Transient(format!("HTTP {status}: {message}")),
        _ => ProviderError::Provider(format!("HTTP {status}: {message}")),
    }
}

/// Parse a `Retry-After` header value as seconds. Gemini sends an integer
/// seconds count rather than the HTTP-date alternative the spec permits,
/// so we only handle the seconds case — an unparseable header yields
/// `None` and the retry loop falls back to exponential backoff.
fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<std::time::Duration> {
    let raw = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    let secs: u64 = raw.trim().parse().ok()?;
    Some(std::time::Duration::from_secs(secs))
}

// ---------------------------------------------------------------------------
// Neutral -> Gemini wire
// ---------------------------------------------------------------------------

/// Build the Gemini request body from a neutral [`ChatRequest`].
fn to_wire_request(req: &ChatRequest) -> GeminiRequestBody {
    let mut system_parts: Vec<GeminiPart> = Vec::new();
    let mut contents: Vec<GeminiContent> = Vec::new();

    for msg in &req.messages {
        match msg.role {
            Role::System => {
                if !msg.content.is_empty() {
                    system_parts.push(GeminiPart::text(&msg.content));
                }
            }
            Role::User => {
                contents.push(GeminiContent::new("user", vec![GeminiPart::text(&msg.content)]));
            }
            Role::Assistant => {
                let mut parts = Vec::new();
                if !msg.content.is_empty() {
                    parts.push(GeminiPart::text(&msg.content));
                }
                for call in &msg.tool_calls {
                    parts.push(GeminiPart::function_call(&call.name, call.arguments.clone()));
                }
                contents.push(GeminiContent::new("model", parts));
            }
            Role::Tool => {
                // Gemini correlates by function name. Prefer the explicit
                // name; fall back to the id if a caller omitted it.
                let name = msg
                    .name
                    .clone()
                    .or_else(|| msg.tool_call_id.clone())
                    .unwrap_or_default();
                contents.push(GeminiContent::new(
                    "user",
                    vec![GeminiPart::function_response(&name, tool_response_object(&msg.content))],
                ));
            }
        }
    }

    let system_instruction =
        (!system_parts.is_empty()).then(|| GeminiContent { role: None, parts: system_parts });

    let tools = (!req.tools.is_empty()).then(|| {
        vec![GeminiTool {
            function_declarations: req
                .tools
                .iter()
                .map(|t| GeminiFunctionDeclaration {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                })
                .collect(),
        }]
    });

    let generation_config = GeminiGenerationConfig {
        temperature: req.config.temperature,
        max_output_tokens: req.config.max_output_tokens,
        top_p: req.config.top_p,
    };

    GeminiRequestBody { contents, system_instruction, tools, generation_config }
}

/// Gemini's `functionResponse.response` must be a JSON object. If the tool
/// result text is already a JSON object, pass it through; otherwise wrap it.
fn tool_response_object(content: &str) -> serde_json::Value {
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(v @ serde_json::Value::Object(_)) => v,
        Ok(other) => serde_json::json!({ "result": other }),
        Err(_) => serde_json::json!({ "result": content }),
    }
}

// ---------------------------------------------------------------------------
// Gemini wire -> neutral
// ---------------------------------------------------------------------------

/// Convert a parsed Gemini response into a neutral [`ChatResponse`].
fn from_wire_response(body: GeminiResponseBody) -> Result<ChatResponse, ProviderError> {
    let candidate = body.candidates.into_iter().next().ok_or_else(|| {
        // No candidate usually means the prompt was blocked.
        let reason = body
            .prompt_feedback
            .and_then(|f| f.block_reason)
            .unwrap_or_else(|| "no candidates returned".to_string());
        ProviderError::Provider(reason)
    })?;

    let mut text = String::new();
    let mut tool_calls = Vec::new();
    for part in candidate.content.map(|c| c.parts).unwrap_or_default() {
        if let Some(t) = part.text {
            text.push_str(&t);
        }
        if let Some(fc) = part.function_call {
            tool_calls.push(ToolCall {
                id: format!("call_{}", tool_calls.len()),
                name: fc.name,
                arguments: fc.args.unwrap_or_else(|| serde_json::json!({})),
            });
        }
    }

    let usage = body
        .usage_metadata
        .map(|u| TokenUsage::new(u.prompt_token_count, u.candidates_token_count))
        .unwrap_or_default();

    // If the model emitted tool calls, that's the branch the loop cares about,
    // regardless of the textual finish reason Gemini reports.
    let finish_reason = if !tool_calls.is_empty() {
        FinishReason::ToolCalls
    } else {
        map_finish_reason(candidate.finish_reason.as_deref())
    };

    Ok(ChatResponse { text, tool_calls, usage, finish_reason })
}

fn map_finish_reason(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("STOP") => FinishReason::Stop,
        Some("MAX_TOKENS") => FinishReason::Length,
        Some("SAFETY") | Some("RECITATION") | Some("BLOCKLIST") | Some("PROHIBITED_CONTENT") => {
            FinishReason::ContentFilter
        }
        _ => FinishReason::Other,
    }
}

// ---------------------------------------------------------------------------
// Wire structs
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct GeminiRequestBody {
    contents: Vec<GeminiContent>,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiTool>>,
    #[serde(rename = "generationConfig")]
    generation_config: GeminiGenerationConfig,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    parts: Vec<GeminiPart>,
}

impl GeminiContent {
    fn new(role: &str, parts: Vec<GeminiPart>) -> Self {
        Self { role: Some(role.to_string()), parts }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct GeminiPart {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(rename = "functionCall", skip_serializing_if = "Option::is_none")]
    function_call: Option<GeminiFunctionCall>,
    #[serde(rename = "functionResponse", skip_serializing_if = "Option::is_none")]
    function_response: Option<GeminiFunctionResponse>,
}

impl GeminiPart {
    fn text(s: &str) -> Self {
        Self { text: Some(s.to_string()), ..Default::default() }
    }
    fn function_call(name: &str, args: serde_json::Value) -> Self {
        Self {
            function_call: Some(GeminiFunctionCall { name: name.to_string(), args: Some(args) }),
            ..Default::default()
        }
    }
    fn function_response(name: &str, response: serde_json::Value) -> Self {
        Self {
            function_response: Some(GeminiFunctionResponse { name: name.to_string(), response }),
            ..Default::default()
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiFunctionCall {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    args: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiFunctionResponse {
    name: String,
    response: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct GeminiTool {
    #[serde(rename = "functionDeclarations")]
    function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Debug, Serialize)]
struct GeminiFunctionDeclaration {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(rename = "maxOutputTokens", skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(rename = "topP", skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponseBody {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsage>,
    #[serde(rename = "promptFeedback")]
    prompt_feedback: Option<GeminiPromptFeedback>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiUsage {
    #[serde(rename = "promptTokenCount", default)]
    prompt_token_count: u32,
    #[serde(rename = "candidatesTokenCount", default)]
    candidates_token_count: u32,
}

#[derive(Debug, Deserialize)]
struct GeminiPromptFeedback {
    #[serde(rename = "blockReason")]
    block_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiErrorEnvelope {
    error: GeminiErrorBody,
}

#[derive(Debug, Deserialize)]
struct GeminiErrorBody {
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::provider::{GenerationConfig, ToolDeclaration};

    fn sample_tools() -> Vec<ToolDeclaration> {
        vec![ToolDeclaration {
            name: "apply_seed".into(),
            description: "seed the cortex".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {"kind": {"type": "string"}, "n": {"type": "integer"}},
                "required": ["kind", "n"]
            }),
        }]
    }

    #[test]
    fn request_lifts_system_and_maps_roles_and_tools() {
        let req = ChatRequest::new(
            "gemini-2.0-flash",
            vec![
                ChatMessage::system("you build cortexes"),
                ChatMessage::user("seed 200 neurons"),
            ],
        )
        .with_tools(sample_tools())
        .with_config(GenerationConfig { temperature: Some(0.1), max_output_tokens: Some(256), top_p: None });

        let body = to_wire_request(&req);
        let json = serde_json::to_value(&body).unwrap();

        // system lifted out of contents
        assert_eq!(json["systemInstruction"]["parts"][0]["text"], "you build cortexes");
        assert_eq!(json["contents"].as_array().unwrap().len(), 1);
        assert_eq!(json["contents"][0]["role"], "user");
        assert_eq!(json["contents"][0]["parts"][0]["text"], "seed 200 neurons");
        // tool declaration present
        assert_eq!(json["tools"][0]["functionDeclarations"][0]["name"], "apply_seed");
        // generation config camelCased (f32 -> JSON, compare with tolerance)
        assert!((json["generationConfig"]["temperature"].as_f64().unwrap() - 0.1).abs() < 1e-6);
        assert_eq!(json["generationConfig"]["maxOutputTokens"], 256);
        assert!(json["generationConfig"].get("topP").is_none());
    }

    #[test]
    fn assistant_tool_call_and_tool_result_round_trip_to_wire() {
        let req = ChatRequest::new(
            "gemini-2.0-flash",
            vec![
                ChatMessage::user("seed it"),
                ChatMessage {
                    role: Role::Assistant,
                    content: String::new(),
                    tool_calls: vec![ToolCall {
                        id: "call_0".into(),
                        name: "apply_seed".into(),
                        arguments: serde_json::json!({"kind": "small_world", "n": 200}),
                    }],
                    tool_call_id: None,
                    name: None,
                },
                ChatMessage::tool_result("call_0", "apply_seed", r#"{"added_nodes":200}"#),
            ],
        );
        let json = serde_json::to_value(&to_wire_request(&req)).unwrap();
        let contents = json["contents"].as_array().unwrap();
        // user, model(functionCall), user(functionResponse)
        assert_eq!(contents.len(), 3);
        assert_eq!(contents[1]["role"], "model");
        assert_eq!(contents[1]["parts"][0]["functionCall"]["name"], "apply_seed");
        assert_eq!(contents[1]["parts"][0]["functionCall"]["args"]["n"], 200);
        assert_eq!(contents[2]["role"], "user");
        assert_eq!(contents[2]["parts"][0]["functionResponse"]["name"], "apply_seed");
        assert_eq!(contents[2]["parts"][0]["functionResponse"]["response"]["added_nodes"], 200);
    }

    #[test]
    fn tool_response_object_wraps_non_objects() {
        assert_eq!(tool_response_object(r#"{"a":1}"#), serde_json::json!({"a": 1}));
        assert_eq!(tool_response_object("plain text"), serde_json::json!({"result": "plain text"}));
        assert_eq!(tool_response_object("42"), serde_json::json!({"result": 42}));
    }

    #[test]
    fn parses_text_response_with_usage() {
        let wire = r#"{
            "candidates": [{
                "content": {"role": "model", "parts": [{"text": "200 neurons fired"}]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 120, "candidatesTokenCount": 18, "totalTokenCount": 138}
        }"#;
        let body: GeminiResponseBody = serde_json::from_str(wire).unwrap();
        let resp = from_wire_response(body).unwrap();
        assert_eq!(resp.text, "200 neurons fired");
        assert!(resp.tool_calls.is_empty());
        assert_eq!(resp.finish_reason, FinishReason::Stop);
        assert_eq!(resp.usage.prompt_tokens, 120);
        assert_eq!(resp.usage.completion_tokens, 18);
        assert_eq!(resp.usage.total_tokens, 138);
    }

    #[test]
    fn parses_function_call_response_as_tool_calls() {
        let wire = r#"{
            "candidates": [{
                "content": {"role": "model", "parts": [
                    {"functionCall": {"name": "apply_seed", "args": {"kind": "ring", "n": 64}}}
                ]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 50, "candidatesTokenCount": 12}
        }"#;
        let body: GeminiResponseBody = serde_json::from_str(wire).unwrap();
        let resp = from_wire_response(body).unwrap();
        assert_eq!(resp.finish_reason, FinishReason::ToolCalls);
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].name, "apply_seed");
        assert_eq!(resp.tool_calls[0].id, "call_0");
        assert_eq!(resp.tool_calls[0].arguments["n"], 64);
    }

    #[test]
    fn no_candidates_is_provider_error() {
        let wire = r#"{"promptFeedback": {"blockReason": "SAFETY"}}"#;
        let body: GeminiResponseBody = serde_json::from_str(wire).unwrap();
        let err = from_wire_response(body).unwrap_err();
        assert!(matches!(err, ProviderError::Provider(m) if m == "SAFETY"));
    }

    #[test]
    fn http_errors_are_classified() {
        let body = r#"{"error": {"code": 403, "message": "API key not valid", "status": "PERMISSION_DENIED"}}"#;
        assert!(matches!(
            classify_http_error(403, body, None),
            ProviderError::Auth(m) if m.contains("not valid")
        ));
        assert!(matches!(
            classify_http_error(400, body, None),
            ProviderError::InvalidRequest(_)
        ));
    }

    /// 5xx maps to `Transient` so the retry loop ([A6]) backs off and
    /// tries again; the message preserves the HTTP status for debugging.
    #[test]
    fn five_hundreds_classify_as_transient() {
        let body = r#"{"error": {"code": 503, "message": "backend overloaded"}}"#;
        for status in [500, 502, 503, 504] {
            let err = classify_http_error(status, body, None);
            assert!(
                matches!(&err, ProviderError::Transient(m) if m.contains(&status.to_string())),
                "expected Transient for {status}, got {err:?}"
            );
            assert!(err.is_retryable());
        }
    }

    /// 429 maps to `RateLimit`; a `Retry-After` header threads through
    /// to the variant so the retry helper honours it verbatim.
    #[test]
    fn rate_limited_passes_retry_after_through() {
        let body = r#"{"error": {"code": 429, "message": "quota exceeded"}}"#;
        let hint = std::time::Duration::from_secs(7);
        let err = classify_http_error(429, body, Some(hint));
        let ProviderError::RateLimit { retry_after, message } = err else {
            panic!("expected RateLimit");
        };
        assert_eq!(retry_after, Some(hint));
        assert_eq!(message, "quota exceeded");
    }

    /// 4xx codes we don't model explicitly stay in the unclassified
    /// `Provider` bucket (not retried).
    #[test]
    fn unmodeled_4xx_stays_in_provider_bucket() {
        let body = r#"{"error": {"code": 404, "message": "model not found"}}"#;
        let err = classify_http_error(404, body, None);
        assert!(matches!(&err, ProviderError::Provider(m) if m.contains("404")));
        assert!(!err.is_retryable());
    }

    #[test]
    fn parse_retry_after_handles_integer_seconds() {
        use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
        let mut h = HeaderMap::new();
        h.insert(RETRY_AFTER, HeaderValue::from_static("12"));
        assert_eq!(parse_retry_after(&h), Some(std::time::Duration::from_secs(12)));

        // Non-numeric (e.g. HTTP-date format) yields None — caller falls
        // back to exponential backoff.
        let mut h = HeaderMap::new();
        h.insert(RETRY_AFTER, HeaderValue::from_static("Wed, 21 Oct 2026 07:28:00 GMT"));
        assert!(parse_retry_after(&h).is_none());

        // Missing header.
        assert!(parse_retry_after(&HeaderMap::new()).is_none());
    }

    /// Live smoke test against the real Gemini API. Ignored by default; run with
    /// `GOOGLE_API_KEY=... cargo test -p core gemini_live -- --ignored --nocapture`.
    /// Optionally set `GEMINI_TEST_MODEL` (defaults to gemini-2.0-flash).
    #[tokio::test]
    #[ignore]
    async fn gemini_live_smoke() {
        let _ = dotenvy::dotenv();
        let key = match std::env::var("GOOGLE_API_KEY").or_else(|_| std::env::var("GEMINI_API_KEY")) {
            Ok(k) => k,
            Err(_) => {
                eprintln!("skipping: GOOGLE_API_KEY not set");
                return;
            }
        };
        // gemini-2.0-flash has no free-tier quota on some keys; 2.5-flash does.
        let model = std::env::var("GEMINI_TEST_MODEL").unwrap_or_else(|_| "gemini-2.5-flash".into());
        let provider = GeminiProvider::new();

        // 1) plain text
        let req = ChatRequest::new(
            &model,
            vec![ChatMessage::user("Reply with exactly the word: pong")],
        );
        let resp = provider.complete(&req, &key).await.expect("text completion");
        eprintln!("TEXT -> {:?} | usage={:?}", resp.text, resp.usage);
        assert!(!resp.text.is_empty());
        assert!(resp.usage.total_tokens > 0);

        // 2) tool calling
        let req = ChatRequest::new(
            &model,
            vec![ChatMessage::user("Seed a small-world cortex with 200 neurons.")],
        )
        .with_tools(sample_tools());
        let resp = provider.complete(&req, &key).await.expect("tool completion");
        eprintln!("TOOLCALLS -> {:?} | finish={:?}", resp.tool_calls, resp.finish_reason);
        assert_eq!(resp.finish_reason, FinishReason::ToolCalls);
        assert_eq!(resp.tool_calls[0].name, "apply_seed");
    }
}

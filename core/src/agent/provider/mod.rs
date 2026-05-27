//! Provider-neutral LLM abstraction (`#8`).
//!
//! The agent reasoning loop ([[ideas/agent-harness-llm-loop]]) talks to LLMs
//! through one trait — [`LlmProvider`] — and a small set of provider-neutral
//! request/response types. Concrete providers (Gemini `#9`, Anthropic `#11`,
//! …) live in sibling modules and translate these types to and from their
//! wire formats. Nothing here performs I/O or depends on `reqwest`; this
//! module is pure data + the trait, so it compiles and tests without a
//! network and stays the stable seam between Stream A (providers) and
//! Stream C (the loop).
//!
//! Design notes:
//! - **Function/tool calling is first-class.** A request carries
//!   [`ToolDeclaration`]s (built from the registry's `ToolDescriptor`s) and a
//!   response may carry [`ToolCall`]s. The loop dispatches those through the
//!   `Registry` and feeds results back as [`Role::Tool`] messages.
//! - **Token usage is always reported** so the session can meter cost (`#12`)
//!   and enforce a budget cap.
//! - The error type here is deliberately small; the full retry/backoff
//!   taxonomy is `#13` and will extend [`ProviderError`] additively.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod gemini;
pub use gemini::GeminiProvider;

/// Who authored a message in the conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// System / developer instructions. Providers that have no dedicated
    /// system slot fold this into the first user turn.
    System,
    /// End-user input.
    User,
    /// Model output (text and/or tool calls).
    Assistant,
    /// The result of a tool the model asked to call, fed back into context.
    /// Pairs with [`ChatMessage::tool_call_id`].
    Tool,
}

/// One message in the conversation history.
///
/// A single assistant turn can contain both `content` (natural-language
/// text) and `tool_calls` (structured calls the loop must execute). A
/// `Tool` message carries the result text plus the `tool_call_id` it answers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    /// Natural-language content. Empty string when an assistant turn is
    /// purely tool calls.
    #[serde(default)]
    pub content: String,
    /// Tool calls requested by an assistant turn. Empty for non-assistant
    /// turns (and for assistant turns that only produced text).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// For `Role::Tool` messages: the id of the [`ToolCall`] this answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// For `Role::Tool` messages: the tool's name. Providers route tool
    /// results differently — Gemini matches `functionResponse` by name,
    /// OpenAI carries a `name` field — so we keep both id and name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ChatMessage {
    /// A plain text message in the given role (no tool calls).
    pub fn text(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            name: None,
        }
    }

    /// A `system` instruction message.
    pub fn system(content: impl Into<String>) -> Self {
        Self::text(Role::System, content)
    }

    /// A `user` message.
    pub fn user(content: impl Into<String>) -> Self {
        Self::text(Role::User, content)
    }

    /// A `tool` result message answering a specific tool call, carrying the
    /// tool's name so name-routed providers (Gemini) can correlate it.
    pub fn tool_result(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
            name: Some(name.into()),
        }
    }
}

/// A tool the model is allowed to call this turn. Built from the registry's
/// `ToolDescriptor` (name + description + draft-07 input schema). Providers
/// translate `parameters` into their own function-declaration format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDeclaration {
    pub name: String,
    pub description: String,
    /// JSON Schema (draft-07) for the tool's arguments — the same schema the
    /// registry validates against before invoking (`#22`).
    pub parameters: serde_json::Value,
}

/// A structured tool call the model emitted. The loop validates `arguments`
/// against the tool schema, dispatches via the `Registry`, and replies with
/// a [`ChatMessage::tool_result`] keyed by `id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider-assigned id used to correlate the result. Synthesized
    /// (e.g. `call_<n>`) for providers that don't supply one.
    pub id: String,
    /// Tool name — matches a `ToolDescriptor::name`.
    pub name: String,
    /// Arguments as a JSON object. Already parsed from any provider-specific
    /// string encoding by the provider impl.
    pub arguments: serde_json::Value,
}

/// Knobs the loop sets per request. Provider-neutral; providers map these
/// onto their own field names and clamp to supported ranges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationConfig {
    /// Sampling temperature. `None` leaves the provider default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Cap on output tokens. `None` leaves the provider default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Nucleus sampling. `None` leaves the provider default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
}

impl Default for GenerationConfig {
    fn default() -> Self {
        Self { temperature: None, max_output_tokens: None, top_p: None }
    }
}

/// A complete request to a provider for one turn of the loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    /// Provider-specific model id (e.g. `gemini-flash-lite`). The provider
    /// validates it knows this model.
    pub model: String,
    /// Conversation so far, oldest first. May include `System` turns; a
    /// provider with a dedicated system slot lifts them out.
    pub messages: Vec<ChatMessage>,
    /// Tools the model may call this turn. Empty = no tool calling.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDeclaration>,
    #[serde(default)]
    pub config: GenerationConfig,
}

impl ChatRequest {
    /// Construct a request with the given model and messages; no tools, default config.
    pub fn new(model: impl Into<String>, messages: Vec<ChatMessage>) -> Self {
        Self { model: model.into(), messages, tools: Vec::new(), config: GenerationConfig::default() }
    }

    /// Builder: attach the tool declarations the model may call.
    pub fn with_tools(mut self, tools: Vec<ToolDeclaration>) -> Self {
        self.tools = tools;
        self
    }

    /// Builder: set generation config.
    pub fn with_config(mut self, config: GenerationConfig) -> Self {
        self.config = config;
        self
    }
}

/// Token accounting for one provider call. Fed into the session's running
/// total for the cost meter and budget cap (`#12`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl TokenUsage {
    /// Build from prompt + completion counts, computing the total.
    pub fn new(prompt_tokens: u32, completion_tokens: u32) -> Self {
        Self {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens.saturating_add(completion_tokens),
        }
    }
}

impl std::ops::Add for TokenUsage {
    type Output = TokenUsage;
    fn add(self, rhs: TokenUsage) -> TokenUsage {
        TokenUsage {
            prompt_tokens: self.prompt_tokens.saturating_add(rhs.prompt_tokens),
            completion_tokens: self.completion_tokens.saturating_add(rhs.completion_tokens),
            total_tokens: self.total_tokens.saturating_add(rhs.total_tokens),
        }
    }
}

/// Why the model stopped. The loop branches on this: `ToolCalls` means
/// dispatch and continue; `Stop` means the turn is complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Natural end of the assistant turn.
    Stop,
    /// The model emitted tool calls and expects results back.
    ToolCalls,
    /// Output hit the token cap.
    Length,
    /// Provider stopped for a content/safety reason.
    ContentFilter,
    /// Anything the provider reports that we don't model explicitly.
    Other,
}

/// One provider response. Carries any natural-language `text`, any
/// `tool_calls` the model wants executed, token `usage`, and the
/// `finish_reason` the loop branches on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub usage: TokenUsage,
    pub finish_reason: FinishReason,
}

/// Errors a provider can surface. Intentionally small for `#8`; `#13`
/// extends this with rate-limit/transient classification and backoff.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// Authentication failed (missing/invalid API key). Not retryable.
    #[error("provider auth failed: {0}")]
    Auth(String),
    /// The request was rejected as malformed (bad model id, unsupported field).
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// Transport / network failure talking to the provider.
    #[error("transport error: {0}")]
    Transport(String),
    /// The provider returned a response we couldn't parse into neutral types.
    #[error("could not parse provider response: {0}")]
    Decode(String),
    /// Provider-side error not otherwise classified (5xx, quota, etc.).
    #[error("provider error: {0}")]
    Provider(String),
}

/// The contract every LLM provider implements. One method: take a neutral
/// [`ChatRequest`], return a neutral [`ChatResponse`]. The API key is passed
/// per call (never stored on the provider) so it can come from the OS
/// keychain via the Tauri bridge (`#34`/`#32`) or an env var fallback (`#37`)
/// without the provider caring which.
///
/// `Send + Sync` so the loop can hold `Arc<dyn LlmProvider>` across awaits.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Stable provider id for config + logs, e.g. `"gemini"`.
    fn id(&self) -> &'static str;

    /// Run one completion. `api_key` is supplied per request and must not be
    /// retained or logged by the implementation.
    async fn complete(
        &self,
        request: &ChatRequest,
        api_key: &str,
    ) -> Result<ChatResponse, ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_constructors_set_roles() {
        assert_eq!(ChatMessage::system("hi").role, Role::System);
        assert_eq!(ChatMessage::user("hi").role, Role::User);
        let t = ChatMessage::tool_result("call_1", "graph_snapshot", "{}");
        assert_eq!(t.role, Role::Tool);
        assert_eq!(t.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(t.name.as_deref(), Some("graph_snapshot"));
    }

    #[test]
    fn request_builder_attaches_tools_and_config() {
        let tools = vec![ToolDeclaration {
            name: "graph_snapshot".into(),
            description: "read the graph".into(),
            parameters: serde_json::json!({"type": "object"}),
        }];
        let req = ChatRequest::new("gemini-flash-lite", vec![ChatMessage::user("seed it")])
            .with_tools(tools.clone())
            .with_config(GenerationConfig { temperature: Some(0.2), ..Default::default() });
        assert_eq!(req.model, "gemini-flash-lite");
        assert_eq!(req.tools.len(), 1);
        assert_eq!(req.tools[0].name, "graph_snapshot");
        assert_eq!(req.config.temperature, Some(0.2));
    }

    #[test]
    fn token_usage_totals_and_adds() {
        let a = TokenUsage::new(10, 5);
        assert_eq!(a.total_tokens, 15);
        let b = TokenUsage::new(3, 4);
        let sum = a + b;
        assert_eq!(sum.prompt_tokens, 13);
        assert_eq!(sum.completion_tokens, 9);
        assert_eq!(sum.total_tokens, 22);
    }

    /// The seam contract: a hand-built response round-trips through serde, so
    /// providers and the loop agree on the shape without a network.
    #[test]
    fn response_round_trips_through_serde() {
        let resp = ChatResponse {
            text: "seeding done".into(),
            tool_calls: vec![ToolCall {
                id: "call_1".into(),
                name: "apply_seed".into(),
                arguments: serde_json::json!({"kind": "small_world", "n": 200}),
            }],
            usage: TokenUsage::new(120, 30),
            finish_reason: FinishReason::ToolCalls,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: ChatResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.text, "seeding done");
        assert_eq!(back.tool_calls, resp.tool_calls);
        assert_eq!(back.usage, resp.usage);
        assert_eq!(back.finish_reason, FinishReason::ToolCalls);
    }

    #[test]
    fn finish_reason_serde_is_snake_case() {
        assert_eq!(
            serde_json::to_string(&FinishReason::ToolCalls).unwrap(),
            r#""tool_calls""#
        );
    }

    /// A trivial in-memory provider proves the trait is object-safe and
    /// usable behind `Arc<dyn LlmProvider>` — the shape the loop (`#25`) and
    /// the mock-provider test (`#44`) rely on.
    struct EchoProvider;

    #[async_trait]
    impl LlmProvider for EchoProvider {
        fn id(&self) -> &'static str {
            "echo"
        }
        async fn complete(
            &self,
            request: &ChatRequest,
            _api_key: &str,
        ) -> Result<ChatResponse, ProviderError> {
            let last = request.messages.last().map(|m| m.content.clone()).unwrap_or_default();
            Ok(ChatResponse {
                text: last,
                tool_calls: Vec::new(),
                usage: TokenUsage::new(1, 1),
                finish_reason: FinishReason::Stop,
            })
        }
    }

    #[tokio::test]
    async fn provider_trait_is_object_safe_and_callable() {
        let provider: std::sync::Arc<dyn LlmProvider> = std::sync::Arc::new(EchoProvider);
        assert_eq!(provider.id(), "echo");
        let req = ChatRequest::new("echo-1", vec![ChatMessage::user("hello loop")]);
        let resp = provider.complete(&req, "unused-key").await.unwrap();
        assert_eq!(resp.text, "hello loop");
        assert_eq!(resp.finish_reason, FinishReason::Stop);
    }
}

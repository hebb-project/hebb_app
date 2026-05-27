//! `AgentTool` trait and supporting types.
//!
//! Every tool that the agent harness exposes implements [`AgentTool`]. The
//! trait is intentionally minimal: a descriptor (name, description,
//! permission, input JSON Schema) plus an async invoke entry point that
//! takes a validated JSON payload and a [`ToolContext`] for substrate
//! access. Tools never touch the disk directly — they go through the
//! existing `Cortex` / engine handles in the context, which already audit
//! and persist correctly.
//!
//! See [[agent-harness-tool-surface]] in the vault for the catalog of
//! tools planned for v1. This module ships only the contract; tool impls
//! land in a follow-up.

use async_trait::async_trait;

use super::Permission;
use crate::engine::SimHandle;

/// Descriptor metadata returned by every tool. The harness reads this to
/// build the listing the LLM sees and to enforce permissions/input shape
/// before calling [`AgentTool::invoke`].
///
/// All fields are stable across invocations of the same tool — the
/// registry caches the descriptor at registration time.
#[derive(Debug, Clone)]
pub struct ToolDescriptor {
    /// Stable, machine-friendly name. The LLM uses this as the call key.
    /// Snake_case; matches the conventions used by the existing REST API.
    pub name: &'static str,
    /// Human-readable description shown to the model. Should be terse but
    /// complete — the LLM does not have access to source code.
    pub description: &'static str,
    /// Permission required to invoke. The registry refuses to register a
    /// tool whose declared permission is missing from this enum (impossible
    /// at the type level today, but kept as an invariant for future
    /// extensions).
    pub permission: Permission,
    /// JSON Schema (draft-07) describing the expected input. The registry
    /// validates the agent's payload against this **before** invoking, so
    /// `invoke()` implementations can assume well-formed input and focus
    /// on the semantic work.
    pub input_schema: serde_json::Value,
}

/// Substrate handles passed into every tool invocation. The skeleton
/// version is empty; concrete handles (engine, cortex folder, audit hook)
/// are wired in when the first tool lands. Keeping the type around now lets
/// the trait signature stabilize without churn.
#[derive(Default, Clone)]
pub struct ToolContext {
    pub sim: Option<SimHandle>,
}

impl ToolContext {
    pub fn new(sim: SimHandle) -> Self {
        Self { sim: Some(sim) }
    }

    pub fn sim(&self) -> Result<&SimHandle, ToolError> {
        self.sim
            .as_ref()
            .ok_or_else(|| ToolError::Substrate("tool context missing SimHandle".into()))
    }
}

/// Failure modes the harness can surface from a tool call. Translated to
/// the wire (MCP error code, Tauri error JSON) by the transport layer; the
/// tool itself never formats for the wire.
#[derive(Debug)]
pub enum ToolError {
    /// The agent named a tool that is not in the registry.
    UnknownTool(String),
    /// The session's permission level is below the tool's required level.
    PermissionDenied {
        tool: String,
        required: Permission,
        granted: Permission,
    },
    /// The input did not validate against the tool's schema. Carries the
    /// human-readable validator messages so the LLM can correct itself.
    BadInput { tool: String, errors: Vec<String> },
    /// The tool executed but the substrate returned an error (param out of
    /// range, dangling edge, folder not open, etc.).
    Substrate(String),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTool(name) => write!(f, "unknown tool '{name}'"),
            Self::PermissionDenied {
                tool,
                required,
                granted,
            } => write!(
                f,
                "permission denied for '{tool}': required {required}, granted {granted}"
            ),
            Self::BadInput { tool, errors } => {
                write!(f, "bad input for '{tool}': {}", errors.join("; "))
            }
            Self::Substrate(msg) => write!(f, "substrate error: {msg}"),
        }
    }
}

impl std::error::Error for ToolError {}

/// The contract every agent-facing tool implements.
///
/// `Send + Sync` is required so the registry can hold `Box<dyn AgentTool>`
/// across awaits and across the MCP server's task boundary.
#[async_trait]
pub trait AgentTool: Send + Sync {
    /// Static metadata. Called at registration and again whenever the
    /// harness rebuilds the tool list shown to the agent.
    fn descriptor(&self) -> ToolDescriptor;

    /// Run the tool. The registry has already verified the session has
    /// adequate permission and that `args` matches the descriptor's input
    /// schema, so implementations can `.unwrap()` on schema-guaranteed
    /// fields. Substrate errors must be returned as
    /// [`ToolError::Substrate`].
    async fn invoke(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<serde_json::Value, ToolError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal test tool to anchor the trait shape — the registry tests
    /// use this to verify dispatch + permission semantics without dragging
    /// substrate machinery into the skeleton PR.
    pub struct EchoTool;

    #[async_trait]
    impl AgentTool for EchoTool {
        fn descriptor(&self) -> ToolDescriptor {
            ToolDescriptor {
                name: "echo",
                description: "Echo the input back unchanged. Test scaffolding only.",
                permission: Permission::ReadOnly,
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": { "msg": { "type": "string" } },
                    "required": ["msg"],
                    "additionalProperties": false,
                }),
            }
        }

        async fn invoke(
            &self,
            args: serde_json::Value,
            _ctx: ToolContext,
        ) -> Result<serde_json::Value, ToolError> {
            Ok(args)
        }
    }

    #[tokio::test]
    async fn echo_tool_round_trips() {
        let tool = EchoTool;
        let out = tool
            .invoke(serde_json::json!({"msg": "hello"}), ToolContext::default())
            .await
            .unwrap();
        assert_eq!(out, serde_json::json!({"msg": "hello"}));
    }

    #[test]
    fn descriptor_carries_permission_and_schema() {
        let d = EchoTool.descriptor();
        assert_eq!(d.name, "echo");
        assert_eq!(d.permission, Permission::ReadOnly);
        assert!(d.input_schema.get("required").is_some());
    }

    #[test]
    fn tool_error_display_strings_are_actionable() {
        let e = ToolError::PermissionDenied {
            tool: "add_neuron".into(),
            required: Permission::TopologyWrite,
            granted: Permission::ReadOnly,
        };
        let s = format!("{e}");
        assert!(s.contains("'add_neuron'"));
        assert!(s.contains("topology_write"));
        assert!(s.contains("read_only"));
    }
}

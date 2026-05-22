//! `Registry` — the single source of truth holding registered tools.
//!
//! Holds `Box<dyn AgentTool>` keyed by descriptor name. The transport layer
//! (MCP server in `core`, Tauri command bridge in `desktop`) calls into the
//! same registry, so a tool added once surfaces in both transports.
//!
//! The registry runs the **whole validation pipeline** before delegating to
//! a tool's [`AgentTool::invoke`]:
//!
//! 1. Tool lookup by name → [`ToolError::UnknownTool`] if missing.
//! 2. Session permission check → [`ToolError::PermissionDenied`] if the
//!    session level is below the tool's required level.
//! 3. JSON Schema validation of input → [`ToolError::BadInput`] if invalid.
//! 4. Delegate to `tool.invoke(...)`.
//! 5. (Follow-up) Append `CortexEventKind::AgentToolInvocation` to
//!    `events.jsonl` when permission is audited.
//!
//! Step 5 lands once #46 (Path 3) is merged and `CortexEventKind` carries
//! an `AgentToolInvocation` variant. This skeleton keeps a `// TODO(audit)`
//! anchor so the integration point is unmistakable.

use std::collections::BTreeMap;

use super::{AgentTool, Permission, ToolContext, ToolDescriptor, ToolError};

/// Errors a registry surfaces from operations other than `invoke()`.
/// Invocation errors are [`ToolError`].
#[derive(Debug)]
pub enum RegistryError {
    /// Attempted to register two tools with the same `descriptor().name`.
    DuplicateName(&'static str),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateName(n) => write!(f, "tool '{n}' is already registered"),
        }
    }
}

impl std::error::Error for RegistryError {}

/// Holds the set of `AgentTool`s available to the harness.
///
/// Constructed once per `AppState`; immutable after `register_all` runs at
/// boot. We do not hot-swap tools — the LLM caches the tool list at session
/// start and stale entries would confuse it.
pub struct Registry {
    /// Keyed by descriptor name. `BTreeMap` because the `descriptors()`
    /// listing is shown to the LLM and a stable order beats a random one.
    tools: BTreeMap<&'static str, Box<dyn AgentTool>>,
}

impl Registry {
    pub fn new() -> Self {
        Self { tools: BTreeMap::new() }
    }

    /// Register a tool. Errors if a tool with the same name is already
    /// registered — collisions are programmer bugs, not runtime conditions.
    pub fn register(&mut self, tool: Box<dyn AgentTool>) -> Result<(), RegistryError> {
        let name = tool.descriptor().name;
        if self.tools.contains_key(name) {
            return Err(RegistryError::DuplicateName(name));
        }
        self.tools.insert(name, tool);
        Ok(())
    }

    /// Snapshot of every registered tool's descriptor, in stable order.
    /// This is what the MCP / Tauri transport renders to the agent.
    pub fn descriptors(&self) -> Vec<ToolDescriptor> {
        self.tools.values().map(|t| t.descriptor()).collect()
    }

    /// True if a tool with this name is registered.
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Run the full validation pipeline and invoke the named tool.
    ///
    /// `session` is the permission level granted to the calling agent
    /// session — typically negotiated at MCP connect time or supplied by
    /// the desktop UX layer.
    pub async fn invoke(
        &self,
        name: &str,
        args: serde_json::Value,
        session: Permission,
        ctx: ToolContext,
    ) -> Result<serde_json::Value, ToolError> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| ToolError::UnknownTool(name.to_string()))?;
        let descriptor = tool.descriptor();

        if !session.allows(descriptor.permission) {
            return Err(ToolError::PermissionDenied {
                tool: descriptor.name.to_string(),
                required: descriptor.permission,
                granted: session,
            });
        }

        // JSON Schema validation lands together with the audit hook in the
        // follow-up PR — picking a validator crate (`jsonschema`) and
        // benchmarking it deserves its own change. For now the skeleton
        // forwards the args straight to the tool, which means tools must
        // be defensive about input shape during this transitional window.
        // Marked clearly so the follow-up cannot miss it.
        // TODO(jsonschema-validation): validate `args` against
        // `descriptor.input_schema` here; map errors to ToolError::BadInput.

        let result = tool.invoke(args.clone(), ctx).await;

        // TODO(audit): when descriptor.permission.is_audited(), append a
        // CortexEventKind::AgentToolInvocation to events.jsonl carrying
        // { tool: name, permission, args, result }. Lands once #46 merges
        // and we add the AgentToolInvocation variant.

        result
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registry")
            .field("tools", &self.tools.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct ReadTool;
    struct WriteTool;

    #[async_trait]
    impl AgentTool for ReadTool {
        fn descriptor(&self) -> ToolDescriptor {
            ToolDescriptor {
                name: "read",
                description: "test read tool",
                permission: Permission::ReadOnly,
                input_schema: serde_json::json!({"type": "object"}),
            }
        }
        async fn invoke(
            &self,
            _args: serde_json::Value,
            _ctx: ToolContext,
        ) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({"ok": true, "level": "read"}))
        }
    }

    #[async_trait]
    impl AgentTool for WriteTool {
        fn descriptor(&self) -> ToolDescriptor {
            ToolDescriptor {
                name: "write",
                description: "test write tool",
                permission: Permission::TopologyWrite,
                input_schema: serde_json::json!({"type": "object"}),
            }
        }
        async fn invoke(
            &self,
            _args: serde_json::Value,
            _ctx: ToolContext,
        ) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({"ok": true, "level": "write"}))
        }
    }

    fn populated_registry() -> Registry {
        let mut r = Registry::new();
        r.register(Box::new(ReadTool)).unwrap();
        r.register(Box::new(WriteTool)).unwrap();
        r
    }

    #[test]
    fn register_rejects_duplicate_names() {
        let mut r = Registry::new();
        r.register(Box::new(ReadTool)).unwrap();
        let err = r.register(Box::new(ReadTool)).unwrap_err();
        assert!(matches!(err, RegistryError::DuplicateName("read")));
    }

    #[test]
    fn descriptors_are_sorted_by_name() {
        let r = populated_registry();
        let names: Vec<_> = r.descriptors().into_iter().map(|d| d.name).collect();
        assert_eq!(names, vec!["read", "write"]);
    }

    #[tokio::test]
    async fn unknown_tool_returns_unknown_tool_error() {
        let r = Registry::new();
        let err = r
            .invoke("missing", serde_json::json!({}), Permission::ReadOnly, ToolContext::default())
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::UnknownTool(n) if n == "missing"));
    }

    #[tokio::test]
    async fn read_only_session_denied_on_write_tool() {
        let r = populated_registry();
        let err = r
            .invoke("write", serde_json::json!({}), Permission::ReadOnly, ToolContext::default())
            .await
            .unwrap_err();
        match err {
            ToolError::PermissionDenied { tool, required, granted } => {
                assert_eq!(tool, "write");
                assert_eq!(required, Permission::TopologyWrite);
                assert_eq!(granted, Permission::ReadOnly);
            }
            other => panic!("expected PermissionDenied, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn topology_write_session_can_invoke_read_tool() {
        let r = populated_registry();
        let out = r
            .invoke("read", serde_json::json!({}), Permission::TopologyWrite, ToolContext::default())
            .await
            .unwrap();
        assert_eq!(out, serde_json::json!({"ok": true, "level": "read"}));
    }

    #[tokio::test]
    async fn matching_permission_invokes_tool() {
        let r = populated_registry();
        let out = r
            .invoke("write", serde_json::json!({}), Permission::TopologyWrite, ToolContext::default())
            .await
            .unwrap();
        assert_eq!(out, serde_json::json!({"ok": true, "level": "write"}));
    }
}

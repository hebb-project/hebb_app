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
use crate::agent::tools::read_only::read_only_tools;
use crate::agent::tools::stimulate::stimulate_tools;
use crate::agent::tools::topology_write::topology_write_tools;

/// Errors a registry surfaces from operations other than `invoke()`.
/// Invocation errors are [`ToolError`].
#[derive(Debug)]
pub enum RegistryError {
    /// Attempted to register two tools with the same `descriptor().name`.
    DuplicateName(&'static str),
    /// A tool declared an invalid JSON Schema.
    InvalidSchema { tool: &'static str, error: String },
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateName(n) => write!(f, "tool '{n}' is already registered"),
            Self::InvalidSchema { tool, error } => {
                write!(f, "tool '{tool}' has an invalid input schema: {error}")
            }
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
    tools: BTreeMap<&'static str, RegisteredTool>,
}

struct RegisteredTool {
    tool: Box<dyn AgentTool>,
    descriptor: ToolDescriptor,
    validator: jsonschema::Validator,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            tools: BTreeMap::new(),
        }
    }

    /// Register a tool. Errors if a tool with the same name is already
    /// registered — collisions are programmer bugs, not runtime conditions.
    pub fn register(&mut self, tool: Box<dyn AgentTool>) -> Result<(), RegistryError> {
        let descriptor = tool.descriptor();
        let name = descriptor.name;
        if self.tools.contains_key(name) {
            return Err(RegistryError::DuplicateName(name));
        }
        let validator = jsonschema::draft7::new(&descriptor.input_schema).map_err(|error| {
            RegistryError::InvalidSchema {
                tool: name,
                error: error.to_string(),
            }
        })?;
        self.tools.insert(
            name,
            RegisteredTool {
                tool,
                descriptor,
                validator,
            },
        );
        Ok(())
    }

    /// Register the built-in tool catalog owned by `core`.
    pub fn register_builtin_tools(&mut self) -> Result<(), RegistryError> {
        for tool in read_only_tools()
            .into_iter()
            .chain(stimulate_tools().into_iter())
            .chain(topology_write_tools().into_iter())
        {
            self.register(tool)?;
        }
        Ok(())
    }

    /// Snapshot of every registered tool's descriptor, in stable order.
    /// This is what the MCP / Tauri transport renders to the agent.
    pub fn descriptors(&self) -> Vec<ToolDescriptor> {
        self.tools.values().map(|t| t.descriptor.clone()).collect()
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
        let registered = self
            .tools
            .get(name)
            .ok_or_else(|| ToolError::UnknownTool(name.to_string()))?;
        let descriptor = &registered.descriptor;

        if !session.allows(descriptor.permission) {
            return Err(ToolError::PermissionDenied {
                tool: descriptor.name.to_string(),
                required: descriptor.permission,
                granted: session,
            });
        }

        let errors: Vec<String> = registered
            .validator
            .iter_errors(&args)
            .map(|error| {
                let path = error.instance_path().to_string();
                if path.is_empty() {
                    error.to_string()
                } else {
                    format!("{path}: {error}")
                }
            })
            .collect();
        if !errors.is_empty() {
            return Err(ToolError::BadInput {
                tool: descriptor.name.to_string(),
                errors,
            });
        }

        let result = registered.tool.invoke(args.clone(), ctx).await;

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
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct ReadTool;
    struct WriteTool;
    struct CountingTool {
        invocations: Arc<AtomicUsize>,
    }
    struct InvalidSchemaTool;

    #[async_trait]
    impl AgentTool for ReadTool {
        fn descriptor(&self) -> ToolDescriptor {
            ToolDescriptor {
                name: "read",
                description: "test read tool",
                permission: Permission::ReadOnly,
                input_schema: serde_json::json!({
                    "$schema": "http://json-schema.org/draft-07/schema#",
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false,
                }),
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
                input_schema: serde_json::json!({
                    "$schema": "http://json-schema.org/draft-07/schema#",
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false,
                }),
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

    #[async_trait]
    impl AgentTool for CountingTool {
        fn descriptor(&self) -> ToolDescriptor {
            ToolDescriptor {
                name: "counting",
                description: "test counting tool",
                permission: Permission::ReadOnly,
                input_schema: serde_json::json!({
                    "$schema": "http://json-schema.org/draft-07/schema#",
                    "type": "object",
                    "properties": {
                        "msg": { "type": "string" }
                    },
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
            self.invocations.fetch_add(1, Ordering::SeqCst);
            Ok(args)
        }
    }

    #[async_trait]
    impl AgentTool for InvalidSchemaTool {
        fn descriptor(&self) -> ToolDescriptor {
            ToolDescriptor {
                name: "invalid_schema",
                description: "test invalid schema tool",
                permission: Permission::ReadOnly,
                input_schema: serde_json::json!({
                    "$schema": "http://json-schema.org/draft-07/schema#",
                    "type": "definitely-not-a-json-schema-type",
                }),
            }
        }
        async fn invoke(
            &self,
            _args: serde_json::Value,
            _ctx: ToolContext,
        ) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({"ok": true}))
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

    #[test]
    fn descriptors_surface_input_schemas_for_provider_declarations() {
        let r = populated_registry();
        let descriptors = r.descriptors();
        let read = descriptors.iter().find(|d| d.name == "read").unwrap();
        assert_eq!(
            read.input_schema["$schema"],
            "http://json-schema.org/draft-07/schema#"
        );
        assert_eq!(read.input_schema["additionalProperties"], false);
    }

    #[test]
    fn register_rejects_invalid_schema() {
        let mut r = Registry::new();
        let err = r.register(Box::new(InvalidSchemaTool)).unwrap_err();
        match err {
            RegistryError::InvalidSchema { tool, error } => {
                assert_eq!(tool, "invalid_schema");
                assert!(error.contains("definitely-not-a-json-schema-type"));
            }
            other => panic!("expected InvalidSchema, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unknown_tool_returns_unknown_tool_error() {
        let r = Registry::new();
        let err = r
            .invoke(
                "missing",
                serde_json::json!({}),
                Permission::ReadOnly,
                ToolContext::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::UnknownTool(n) if n == "missing"));
    }

    #[tokio::test]
    async fn read_only_session_denied_on_write_tool() {
        let r = populated_registry();
        let err = r
            .invoke(
                "write",
                serde_json::json!({}),
                Permission::ReadOnly,
                ToolContext::default(),
            )
            .await
            .unwrap_err();
        match err {
            ToolError::PermissionDenied {
                tool,
                required,
                granted,
            } => {
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
            .invoke(
                "read",
                serde_json::json!({}),
                Permission::TopologyWrite,
                ToolContext::default(),
            )
            .await
            .unwrap();
        assert_eq!(out, serde_json::json!({"ok": true, "level": "read"}));
    }

    #[tokio::test]
    async fn matching_permission_invokes_tool() {
        let r = populated_registry();
        let out = r
            .invoke(
                "write",
                serde_json::json!({}),
                Permission::TopologyWrite,
                ToolContext::default(),
            )
            .await
            .unwrap();
        assert_eq!(out, serde_json::json!({"ok": true, "level": "write"}));
    }

    #[tokio::test]
    async fn valid_args_invoke_tool_after_schema_validation() {
        let invocations = Arc::new(AtomicUsize::new(0));
        let mut r = Registry::new();
        r.register(Box::new(CountingTool {
            invocations: invocations.clone(),
        }))
        .unwrap();

        let out = r
            .invoke(
                "counting",
                serde_json::json!({"msg": "hello"}),
                Permission::ReadOnly,
                ToolContext::default(),
            )
            .await
            .unwrap();

        assert_eq!(out, serde_json::json!({"msg": "hello"}));
        assert_eq!(invocations.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn malformed_args_are_rejected_before_tool_runs() {
        let invocations = Arc::new(AtomicUsize::new(0));
        let mut r = Registry::new();
        r.register(Box::new(CountingTool {
            invocations: invocations.clone(),
        }))
        .unwrap();

        let err = r
            .invoke(
                "counting",
                serde_json::json!({"msg": 42, "extra": true}),
                Permission::ReadOnly,
                ToolContext::default(),
            )
            .await
            .unwrap_err();

        match err {
            ToolError::BadInput { tool, errors } => {
                assert_eq!(tool, "counting");
                assert!(errors.iter().any(|e| e.contains("/msg")));
                assert!(errors.iter().any(|e| e.contains("extra")));
            }
            other => panic!("expected BadInput, got {other:?}"),
        }
        assert_eq!(invocations.load(Ordering::SeqCst), 0);
    }
}

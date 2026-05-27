//! Parameter-writing tools backed by the live engine.

use async_trait::async_trait;
use uuid::Uuid;

use crate::agent::{AgentTool, Permission, ToolContext, ToolDescriptor, ToolError};

pub struct PatchNodeParamTool;
pub struct PatchSynapseParamTool;

#[async_trait]
impl AgentTool for PatchNodeParamTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "patch_node_param",
            description:
                "Patch one parameter on one live neuron and return the updated parameter map.",
            permission: Permission::ParameterWrite,
            input_schema: patch_schema("node_id"),
        }
    }

    async fn invoke(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<serde_json::Value, ToolError> {
        let node_id = parse_uuid_arg(&args, "node_id")?;
        let (key, value) = parse_patch(&args)?;
        ctx.sim()?
            .set_node_param(node_id, key, value)
            .await
            .map_err(ToolError::Substrate)
    }
}

#[async_trait]
impl AgentTool for PatchSynapseParamTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "patch_synapse_param",
            description:
                "Patch one parameter on one live synapse and return the updated parameter map.",
            permission: Permission::ParameterWrite,
            input_schema: patch_schema("edge_id"),
        }
    }

    async fn invoke(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<serde_json::Value, ToolError> {
        let edge_id = parse_uuid_arg(&args, "edge_id")?;
        let (key, value) = parse_patch(&args)?;
        ctx.sim()?
            .set_synapse_param(edge_id, key, value)
            .await
            .map_err(ToolError::Substrate)
    }
}

pub fn parameter_write_tools() -> Vec<Box<dyn AgentTool>> {
    vec![
        Box::new(PatchNodeParamTool),
        Box::new(PatchSynapseParamTool),
    ]
}

fn patch_schema(id_property: &'static str) -> serde_json::Value {
    serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": {
            id_property: { "type": "string", "format": "uuid" },
            "key": { "type": "string", "minLength": 1 },
            "value": {}
        },
        "required": [id_property, "key", "value"],
        "additionalProperties": false,
    })
}

fn parse_uuid_arg(args: &serde_json::Value, property: &'static str) -> Result<Uuid, ToolError> {
    let raw = args
        .get(property)
        .and_then(|value| value.as_str())
        .ok_or_else(|| bad_input(property, format!("{property} must be a UUID string")))?;
    Uuid::parse_str(raw).map_err(|e| bad_input(property, format!("{property}: {e}")))
}

fn parse_patch(args: &serde_json::Value) -> Result<(String, serde_json::Value), ToolError> {
    let key = args
        .get("key")
        .and_then(|value| value.as_str())
        .ok_or_else(|| bad_input("patch_param", "key must be a non-empty string"))?
        .to_string();
    let value = args
        .get("value")
        .cloned()
        .ok_or_else(|| bad_input("patch_param", "value is required"))?;
    Ok((key, value))
}

fn bad_input(tool: &'static str, message: impl Into<String>) -> ToolError {
    ToolError::BadInput {
        tool: tool.into(),
        errors: vec![message.into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::tools::read_only::read_only_tools;
    use crate::agent::Registry;
    use crate::engine::spawn_engine;

    fn registry() -> Registry {
        let mut registry = Registry::new();
        for tool in read_only_tools()
            .into_iter()
            .chain(parameter_write_tools().into_iter())
        {
            registry.register(tool).unwrap();
        }
        registry
    }

    #[tokio::test]
    async fn parameter_tools_register_with_parameter_write_permission() {
        let mut registry = Registry::new();
        for tool in parameter_write_tools() {
            registry.register(tool).unwrap();
        }
        assert!(registry
            .descriptors()
            .iter()
            .all(|descriptor| descriptor.permission == Permission::ParameterWrite));
    }

    #[tokio::test]
    async fn patch_node_param_is_reflected_in_read_node_params() {
        let (sim, _join) = spawn_engine(10_000);
        let node_id = Uuid::new_v4();
        sim.add_node(node_id).await.unwrap();
        let ctx = ToolContext::new(sim);
        let registry = registry();

        registry
            .invoke(
                "patch_node_param",
                serde_json::json!({ "node_id": node_id, "key": "v_thresh", "value": -50.0 }),
                Permission::ParameterWrite,
                ctx.clone(),
            )
            .await
            .unwrap();
        let params = registry
            .invoke(
                "read_node_params",
                serde_json::json!({ "node_id": node_id }),
                Permission::ReadOnly,
                ctx,
            )
            .await
            .unwrap();
        assert_eq!(params["v_thresh"], -50.0);
    }

    #[tokio::test]
    async fn patch_synapse_param_is_reflected_in_read_synapse_params() {
        let (sim, _join) = spawn_engine(10_000);
        let pre = Uuid::new_v4();
        let post = Uuid::new_v4();
        let edge = Uuid::new_v4();
        sim.add_node(pre).await.unwrap();
        sim.add_node(post).await.unwrap();
        sim.add_edge(edge, pre, post, 0.4).await.unwrap();
        let ctx = ToolContext::new(sim);
        let registry = registry();

        registry
            .invoke(
                "patch_synapse_param",
                serde_json::json!({ "edge_id": edge, "key": "weight", "value": 0.8 }),
                Permission::ParameterWrite,
                ctx.clone(),
            )
            .await
            .unwrap();
        let params = registry
            .invoke(
                "read_synapse_params",
                serde_json::json!({ "edge_id": edge }),
                Permission::ReadOnly,
                ctx,
            )
            .await
            .unwrap();
        assert!((params["weight"].as_f64().unwrap() - 0.8).abs() < 1e-6);
    }

    #[tokio::test]
    async fn invalid_param_returns_clear_substrate_error() {
        let (sim, _join) = spawn_engine(10_000);
        let node_id = Uuid::new_v4();
        sim.add_node(node_id).await.unwrap();
        let err = registry()
            .invoke(
                "patch_node_param",
                serde_json::json!({ "node_id": node_id, "key": "not_a_param", "value": 1.0 }),
                Permission::ParameterWrite,
                ToolContext::new(sim),
            )
            .await
            .unwrap_err();
        match err {
            ToolError::Substrate(message) => assert!(message.contains("unknown parameter")),
            other => panic!("expected Substrate, got {other:?}"),
        }
    }
}

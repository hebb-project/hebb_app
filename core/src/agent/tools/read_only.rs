//! Read-only tools backed by the live simulation handle.

use async_trait::async_trait;
use uuid::Uuid;

use crate::agent::{AgentTool, Permission, ToolContext, ToolDescriptor, ToolError};

pub struct GraphSnapshotTool;
pub struct ReadNodeParamsTool;
pub struct ReadSynapseParamsTool;
pub struct ReadFolderStatusTool;

#[async_trait]
impl AgentTool for GraphSnapshotTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "graph_snapshot",
            description: "Return the live graph topology with node ids, synapse endpoints, weights, and engine time.",
            permission: Permission::ReadOnly,
            input_schema: empty_object_schema(),
        }
    }

    async fn invoke(
        &self,
        _args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<serde_json::Value, ToolError> {
        let snapshot = ctx
            .sim()?
            .graph_snapshot()
            .await
            .map_err(|msg| ToolError::Substrate(msg.into()))?;
        serde_json::to_value(snapshot).map_err(|e| ToolError::Substrate(e.to_string()))
    }
}

#[async_trait]
impl AgentTool for ReadNodeParamsTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "read_node_params",
            description: "Return introspectable parameters for one live neuron by node_id.",
            permission: Permission::ReadOnly,
            input_schema: id_schema("node_id"),
        }
    }

    async fn invoke(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<serde_json::Value, ToolError> {
        let node_id = parse_uuid_arg(&args, "node_id")?;
        match ctx
            .sim()?
            .get_node_params(node_id)
            .await
            .map_err(|msg| ToolError::Substrate(msg.into()))?
        {
            Some(params) => Ok(params),
            None => Err(ToolError::Substrate(format!(
                "node {node_id} not in engine"
            ))),
        }
    }
}

#[async_trait]
impl AgentTool for ReadSynapseParamsTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "read_synapse_params",
            description: "Return introspectable parameters for one live synapse by edge_id.",
            permission: Permission::ReadOnly,
            input_schema: id_schema("edge_id"),
        }
    }

    async fn invoke(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<serde_json::Value, ToolError> {
        let edge_id = parse_uuid_arg(&args, "edge_id")?;
        match ctx
            .sim()?
            .get_synapse_params(edge_id)
            .await
            .map_err(|msg| ToolError::Substrate(msg.into()))?
        {
            Some(params) => Ok(params),
            None => Err(ToolError::Substrate(format!(
                "synapse {edge_id} not in engine"
            ))),
        }
    }
}

#[async_trait]
impl AgentTool for ReadFolderStatusTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "read_folder_status",
            description: "Return the current cortex type and open .cortex folder path, if any.",
            permission: Permission::ReadOnly,
            input_schema: empty_object_schema(),
        }
    }

    async fn invoke(
        &self,
        _args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<serde_json::Value, ToolError> {
        let sim = ctx.sim()?;
        let cortex_type = sim
            .cortex_type()
            .await
            .map_err(|msg| ToolError::Substrate(msg.into()))?;
        let folder = sim
            .current_folder()
            .await
            .map_err(|msg| ToolError::Substrate(msg.into()))?;
        Ok(serde_json::json!({
            "cortex_type": cortex_type,
            "folder": folder.map(|p| p.display().to_string()),
        }))
    }
}

pub fn read_only_tools() -> Vec<Box<dyn AgentTool>> {
    vec![
        Box::new(GraphSnapshotTool),
        Box::new(ReadNodeParamsTool),
        Box::new(ReadSynapseParamsTool),
        Box::new(ReadFolderStatusTool),
    ]
}

fn empty_object_schema() -> serde_json::Value {
    serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": {},
        "additionalProperties": false,
    })
}

fn id_schema(property: &'static str) -> serde_json::Value {
    serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": {
            property: {
                "type": "string",
                "format": "uuid",
            }
        },
        "required": [property],
        "additionalProperties": false,
    })
}

fn parse_uuid_arg(args: &serde_json::Value, property: &'static str) -> Result<Uuid, ToolError> {
    let raw = args
        .get(property)
        .and_then(|value| value.as_str())
        .ok_or_else(|| ToolError::BadInput {
            tool: property.into(),
            errors: vec![format!("{property} must be a UUID string")],
        })?;
    Uuid::parse_str(raw).map_err(|e| ToolError::BadInput {
        tool: property.into(),
        errors: vec![format!("{property}: {e}")],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Registry;
    use crate::engine::spawn_engine;

    async fn live_context() -> (ToolContext, tokio::task::JoinHandle<()>, Uuid, Uuid, Uuid) {
        let (sim, join) = spawn_engine(10_000);
        let pre = Uuid::new_v4();
        let post = Uuid::new_v4();
        let edge = Uuid::new_v4();
        sim.add_node(pre).await.unwrap();
        sim.add_node(post).await.unwrap();
        sim.add_edge(edge, pre, post, 0.42).await.unwrap();
        (ToolContext::new(sim), join, pre, post, edge)
    }

    fn registry() -> Registry {
        let mut registry = Registry::new();
        for tool in read_only_tools() {
            registry.register(tool).unwrap();
        }
        registry
    }

    #[tokio::test]
    async fn read_only_tools_register_with_read_only_permission() {
        let registry = registry();
        let descriptors = registry.descriptors();
        let names: Vec<_> = descriptors.iter().map(|d| d.name).collect();
        assert_eq!(
            names,
            vec![
                "graph_snapshot",
                "read_folder_status",
                "read_node_params",
                "read_synapse_params"
            ]
        );
        assert!(descriptors
            .iter()
            .all(|descriptor| descriptor.permission == Permission::ReadOnly));
    }

    #[tokio::test]
    async fn graph_snapshot_returns_live_json() {
        let (ctx, _join, pre, post, edge) = live_context().await;
        let out = registry()
            .invoke(
                "graph_snapshot",
                serde_json::json!({}),
                Permission::ReadOnly,
                ctx,
            )
            .await
            .unwrap();

        assert_eq!(out["nodes"].as_array().unwrap().len(), 2);
        let edges = out["edges"].as_array().unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0]["id"], edge.to_string());
        assert_eq!(edges[0]["pre_id"], pre.to_string());
        assert_eq!(edges[0]["post_id"], post.to_string());
    }

    #[tokio::test]
    async fn read_node_and_synapse_params_return_live_json() {
        let (ctx, _join, pre, _post, edge) = live_context().await;
        let registry = registry();

        let node = registry
            .invoke(
                "read_node_params",
                serde_json::json!({ "node_id": pre }),
                Permission::ReadOnly,
                ctx.clone(),
            )
            .await
            .unwrap();
        assert!(node.get("v").is_some());

        let synapse = registry
            .invoke(
                "read_synapse_params",
                serde_json::json!({ "edge_id": edge }),
                Permission::ReadOnly,
                ctx,
            )
            .await
            .unwrap();
        assert!((synapse["weight"].as_f64().unwrap() - 0.42).abs() < 1e-6);
    }

    #[tokio::test]
    async fn read_folder_status_reports_transient_state() {
        let (ctx, _join, _pre, _post, _edge) = live_context().await;
        let out = registry()
            .invoke(
                "read_folder_status",
                serde_json::json!({}),
                Permission::ReadOnly,
                ctx,
            )
            .await
            .unwrap();

        assert_eq!(out["folder"], serde_json::Value::Null);
        assert_eq!(out["cortex_type"]["kind"], "lif");
    }
}

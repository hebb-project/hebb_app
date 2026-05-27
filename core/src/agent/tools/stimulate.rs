//! Stimulation tools backed by the live engine.

use async_trait::async_trait;
use uuid::Uuid;

use crate::agent::{AgentTool, Permission, ToolContext, ToolDescriptor, ToolError};

const MIN_CURRENT_PA: f32 = -100.0;
const MAX_CURRENT_PA: f32 = 100.0;
const MIN_DURATION_MS: f32 = 0.0;
const MAX_DURATION_MS: f32 = 1000.0;

pub struct InjectCurrentTool;
pub struct ForceSpikeTool;

#[async_trait]
impl AgentTool for InjectCurrentTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "inject_current",
            description: "Inject clamped current into one neuron for a clamped duration.",
            permission: Permission::Stimulate,
            input_schema: serde_json::json!({
                "$schema": "http://json-schema.org/draft-07/schema#",
                "type": "object",
                "properties": {
                    "node_id": { "type": "string", "format": "uuid" },
                    "current_pa": { "type": "number" },
                    "duration_ms": { "type": "number", "default": 20.0 }
                },
                "required": ["node_id", "current_pa"],
                "additionalProperties": false,
            }),
        }
    }

    async fn invoke(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<serde_json::Value, ToolError> {
        let node_id = parse_uuid_arg(&args, "node_id")?;
        let requested_current = parse_f32_arg(&args, "current_pa")?;
        let requested_duration = args
            .get("duration_ms")
            .map(|_| parse_f32_arg(&args, "duration_ms"))
            .transpose()?
            .unwrap_or(20.0);
        let current_pa = requested_current.clamp(MIN_CURRENT_PA, MAX_CURRENT_PA);
        let duration_ms = requested_duration.clamp(MIN_DURATION_MS, MAX_DURATION_MS);

        ctx.sim()?
            .stimulate(node_id, current_pa, duration_ms)
            .await
            .map_err(|msg| ToolError::Substrate(msg.into()))?;
        Ok(serde_json::json!({
            "node_id": node_id,
            "requested_current_pa": requested_current,
            "current_pa": current_pa,
            "requested_duration_ms": requested_duration,
            "duration_ms": duration_ms,
        }))
    }
}

#[async_trait]
impl AgentTool for ForceSpikeTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "force_spike",
            description: "Force one live neuron to emit an immediate spike frame.",
            permission: Permission::Stimulate,
            input_schema: serde_json::json!({
                "$schema": "http://json-schema.org/draft-07/schema#",
                "type": "object",
                "properties": {
                    "node_id": { "type": "string", "format": "uuid" }
                },
                "required": ["node_id"],
                "additionalProperties": false,
            }),
        }
    }

    async fn invoke(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<serde_json::Value, ToolError> {
        let node_id = parse_uuid_arg(&args, "node_id")?;
        ctx.sim()?
            .force_spike(node_id)
            .await
            .map_err(ToolError::Substrate)?;
        Ok(serde_json::json!({ "node_id": node_id, "forced": true }))
    }
}

pub fn stimulate_tools() -> Vec<Box<dyn AgentTool>> {
    vec![Box::new(InjectCurrentTool), Box::new(ForceSpikeTool)]
}

fn parse_uuid_arg(args: &serde_json::Value, property: &'static str) -> Result<Uuid, ToolError> {
    let raw = args
        .get(property)
        .and_then(|value| value.as_str())
        .ok_or_else(|| bad_input(property, format!("{property} must be a UUID string")))?;
    Uuid::parse_str(raw).map_err(|e| bad_input(property, format!("{property}: {e}")))
}

fn parse_f32_arg(args: &serde_json::Value, property: &'static str) -> Result<f32, ToolError> {
    let value = args
        .get(property)
        .and_then(|value| value.as_f64())
        .ok_or_else(|| bad_input(property, format!("{property} must be a finite number")))?;
    let value = value as f32;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(bad_input(property, format!("{property} must be finite")))
    }
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
    use crate::agent::Registry;
    use crate::engine::spawn_engine;
    use tokio::time::{timeout, Duration};

    fn registry() -> Registry {
        let mut registry = Registry::new();
        for tool in stimulate_tools() {
            registry.register(tool).unwrap();
        }
        registry
    }

    #[tokio::test]
    async fn stimulate_tools_register_with_stimulate_permission() {
        let registry = registry();
        let descriptors = registry.descriptors();
        assert_eq!(
            descriptors.iter().map(|d| d.name).collect::<Vec<_>>(),
            vec!["force_spike", "inject_current"]
        );
        assert!(descriptors
            .iter()
            .all(|descriptor| descriptor.permission == Permission::Stimulate));
    }

    #[tokio::test]
    async fn inject_current_clamps_current_and_duration() {
        let (sim, _join) = spawn_engine(10_000);
        let node_id = Uuid::new_v4();
        sim.add_node(node_id).await.unwrap();

        let out = registry()
            .invoke(
                "inject_current",
                serde_json::json!({
                    "node_id": node_id,
                    "current_pa": 250.0,
                    "duration_ms": 5000.0
                }),
                Permission::Stimulate,
                ToolContext::new(sim),
            )
            .await
            .unwrap();

        assert_eq!(out["requested_current_pa"], 250.0);
        assert_eq!(out["current_pa"], 100.0);
        assert_eq!(out["requested_duration_ms"], 5000.0);
        assert_eq!(out["duration_ms"], 1000.0);
    }

    #[tokio::test]
    async fn force_spike_emits_observable_spike_frame() {
        let (sim, _join) = spawn_engine(10_000);
        let node_id = Uuid::new_v4();
        sim.add_node(node_id).await.unwrap();
        let mut spikes = sim.spikes.subscribe();

        registry()
            .invoke(
                "force_spike",
                serde_json::json!({ "node_id": node_id }),
                Permission::Stimulate,
                ToolContext::new(sim),
            )
            .await
            .unwrap();

        let frame = timeout(Duration::from_millis(200), spikes.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(frame.events.len(), 1);
        assert_eq!(frame.events[0].node_id, node_id);
    }

    #[tokio::test]
    async fn stimulate_tools_require_stimulate_permission() {
        let (sim, _join) = spawn_engine(10_000);
        let node_id = Uuid::new_v4();
        sim.add_node(node_id).await.unwrap();

        let err = registry()
            .invoke(
                "force_spike",
                serde_json::json!({ "node_id": node_id }),
                Permission::TopologyWrite,
                ToolContext::new(sim),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::PermissionDenied { .. }));
    }
}

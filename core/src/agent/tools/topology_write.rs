//! Topology-writing tools backed by deterministic seed generators.

use async_trait::async_trait;
use hebb::seeds::{self, Seed, SeedParams};
use uuid::Uuid;

use crate::agent::{AgentTool, Permission, ToolContext, ToolDescriptor, ToolError};

pub struct ApplySeedTool;
pub struct AddNeuronTool;
pub struct AddSynapseTool;
pub struct RemoveNeuronTool;
pub struct RemoveSynapseTool;

#[async_trait]
impl AgentTool for ApplySeedTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "apply_seed",
            description: "Generate and apply a deterministic seed topology. Supports random, ring, small_world, and layered.",
            permission: Permission::TopologyWrite,
            input_schema: serde_json::json!({
                "$schema": "http://json-schema.org/draft-07/schema#",
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "enum": ["random", "ring", "small_world", "layered"]
                    },
                    "n": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 100000
                    },
                    "params": {
                        "type": "object",
                        "properties": {
                            "p": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
                            "k": { "type": "integer", "minimum": 1 },
                            "p_rewire": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
                            "layers": {
                                "type": "array",
                                "items": { "type": "integer", "minimum": 1 },
                                "minItems": 2
                            },
                            "seed": { "type": "integer", "minimum": 0 },
                            "weight_range": {
                                "type": "array",
                                "items": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
                                "minItems": 2,
                                "maxItems": 2
                            },
                            "delay_ms": { "type": "number", "minimum": 0.0 }
                        },
                        "additionalProperties": false
                    }
                },
                "required": ["kind"],
                "additionalProperties": false,
            }),
        }
    }

    async fn invoke(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<serde_json::Value, ToolError> {
        let spec = SeedToolSpec::from_args(&args)?;
        let kind = spec.kind.clone();
        let seed = spec.build_seed()?;
        let requested_nodes = seed.nodes.len();
        let requested_edges = seed.edges.len();
        let report = ctx
            .sim()?
            .apply_seed(seed)
            .await
            .map_err(ToolError::Substrate)?;
        Ok(serde_json::json!({
            "kind": kind,
            "requested_nodes": requested_nodes,
            "requested_edges": requested_edges,
            "added_nodes": report.added_nodes,
            "added_edges": report.added_edges,
        }))
    }
}

pub fn topology_write_tools() -> Vec<Box<dyn AgentTool>> {
    vec![
        Box::new(AddNeuronTool),
        Box::new(AddSynapseTool),
        Box::new(ApplySeedTool),
        Box::new(RemoveNeuronTool),
        Box::new(RemoveSynapseTool),
    ]
}

#[async_trait]
impl AgentTool for AddNeuronTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "add_neuron",
            description: "Add one neuron to the live topology.",
            permission: Permission::TopologyWrite,
            input_schema: serde_json::json!({
                "$schema": "http://json-schema.org/draft-07/schema#",
                "type": "object",
                "properties": {
                    "node_id": { "type": "string", "format": "uuid" },
                    "label": { "type": "string", "default": "" },
                    "metadata": { "type": "object", "default": {} }
                },
                "additionalProperties": false,
            }),
        }
    }

    async fn invoke(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<serde_json::Value, ToolError> {
        let sim = ctx.sim()?;
        let folder = sim
            .current_folder()
            .await
            .map_err(|msg| ToolError::Substrate(msg.into()))?;
        if folder.is_some() {
            let label = args
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let metadata = args
                .get("metadata")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let record = sim
                .add_neuron_to_folder(label, metadata)
                .await
                .map_err(ToolError::Substrate)?;
            return Ok(serde_json::json!({
                "id": record.id,
                "label": record.label,
                "node_type": record.node_type,
                "metadata": record.metadata,
            }));
        }

        let node_id = args
            .get("node_id")
            .map(|_| parse_uuid_arg(&args, "node_id"))
            .transpose()?
            .unwrap_or_else(Uuid::new_v4);
        sim.add_node(node_id)
            .await
            .map_err(|msg| ToolError::Substrate(msg.into()))?;
        Ok(serde_json::json!({ "id": node_id }))
    }
}

#[async_trait]
impl AgentTool for AddSynapseTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "add_synapse",
            description: "Add one synapse to the live topology.",
            permission: Permission::TopologyWrite,
            input_schema: serde_json::json!({
                "$schema": "http://json-schema.org/draft-07/schema#",
                "type": "object",
                "properties": {
                    "edge_id": { "type": "string", "format": "uuid" },
                    "pre_id": { "type": "string", "format": "uuid" },
                    "post_id": { "type": "string", "format": "uuid" },
                    "weight": { "type": "number", "minimum": 0.0, "maximum": 1.0, "default": 0.5 },
                    "metadata": { "type": "object", "default": {} }
                },
                "required": ["pre_id", "post_id"],
                "additionalProperties": false,
            }),
        }
    }

    async fn invoke(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<serde_json::Value, ToolError> {
        let pre = parse_uuid_arg(&args, "pre_id")?;
        let post = parse_uuid_arg(&args, "post_id")?;
        if pre == post {
            return Err(bad_input("add_synapse", "self-loops are not allowed"));
        }
        let weight = args
            .get("weight")
            .map(|v| value_to_f32(v, "weight"))
            .transpose()?
            .unwrap_or(0.5);
        let sim = ctx.sim()?;
        let folder = sim
            .current_folder()
            .await
            .map_err(|msg| ToolError::Substrate(msg.into()))?;
        if folder.is_some() {
            let metadata = args
                .get("metadata")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let record = sim
                .add_synapse_to_folder(pre, post, weight, metadata)
                .await
                .map_err(ToolError::Substrate)?;
            return Ok(serde_json::json!({
                "id": record.id,
                "pre_id": record.pre_id,
                "post_id": record.post_id,
                "weight": record.weight,
                "edge_type": record.edge_type,
            }));
        }

        let edge_id = args
            .get("edge_id")
            .map(|_| parse_uuid_arg(&args, "edge_id"))
            .transpose()?
            .unwrap_or_else(Uuid::new_v4);
        sim.add_edge(edge_id, pre, post, weight)
            .await
            .map_err(|msg| ToolError::Substrate(msg.into()))?;
        Ok(serde_json::json!({
            "id": edge_id,
            "pre_id": pre,
            "post_id": post,
            "weight": weight,
        }))
    }
}

#[async_trait]
impl AgentTool for RemoveNeuronTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "remove_neuron",
            description: "Remove one neuron from the live topology, cascading incident synapses.",
            permission: Permission::TopologyWrite,
            input_schema: id_schema("node_id"),
        }
    }

    async fn invoke(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<serde_json::Value, ToolError> {
        let node_id = parse_uuid_arg(&args, "node_id")?;
        let removed = ctx
            .sim()?
            .remove_neuron(node_id)
            .await
            .map_err(ToolError::Substrate)?;
        Ok(match removed {
            Some(summary) => serde_json::json!({
                "removed": true,
                "node_id": node_id,
                "cascaded_edges": summary.cascaded_edges,
            }),
            None => serde_json::json!({
                "removed": false,
                "node_id": node_id,
                "cascaded_edges": 0,
            }),
        })
    }
}

#[async_trait]
impl AgentTool for RemoveSynapseTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "remove_synapse",
            description: "Remove one synapse from the live topology.",
            permission: Permission::TopologyWrite,
            input_schema: id_schema("edge_id"),
        }
    }

    async fn invoke(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<serde_json::Value, ToolError> {
        let edge_id = parse_uuid_arg(&args, "edge_id")?;
        let removed = ctx
            .sim()?
            .remove_synapse(edge_id)
            .await
            .map_err(ToolError::Substrate)?;
        Ok(serde_json::json!({ "removed": removed, "edge_id": edge_id }))
    }
}

struct SeedToolSpec {
    kind: String,
    n: Option<usize>,
    params: serde_json::Value,
}

impl SeedToolSpec {
    fn from_args(args: &serde_json::Value) -> Result<Self, ToolError> {
        let kind = args
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                bad_input(
                    "apply_seed",
                    "kind must be one of random, ring, small_world, layered",
                )
            })?
            .to_string();
        let n = args
            .get("n")
            .map(|v| {
                v.as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| bad_input("apply_seed", "n must be a positive integer"))
            })
            .transpose()?;
        let params = args
            .get("params")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        Ok(Self { kind, n, params })
    }

    fn build_seed(&self) -> Result<Seed, ToolError> {
        let params = self.seed_params()?;
        let seed = self.seed_u64()?;
        match self.kind.as_str() {
            "random" => seeds::random(self.require_n()?, self.f32_param("p", 0.05)?, seed, params),
            "ring" => seeds::ring(self.require_n()?, self.usize_param("k", 2)?, seed, params),
            "small_world" => seeds::small_world(
                self.require_n()?,
                self.usize_param("k", 2)?,
                self.f32_param("p_rewire", 0.2)?,
                seed,
                params,
            ),
            "layered" => seeds::layered(&self.layers_param()?, seed, params),
            other => {
                return Err(bad_input(
                    "apply_seed",
                    format!("unsupported seed kind '{other}'"),
                ));
            }
        }
        .map_err(|e| ToolError::Substrate(e.to_string()))
    }

    fn seed_params(&self) -> Result<SeedParams, ToolError> {
        let mut params = SeedParams::default();
        if let Some(delay_ms) = self.optional_f32("delay_ms")? {
            params.delay_ms = delay_ms;
        }
        if let Some(range) = self.params.get("weight_range") {
            let values = range
                .as_array()
                .ok_or_else(|| bad_input("apply_seed", "weight_range must be [low, high]"))?;
            if values.len() != 2 {
                return Err(bad_input(
                    "apply_seed",
                    "weight_range must contain exactly 2 numbers",
                ));
            }
            params.weight_range = (
                value_to_f32(&values[0], "weight_range[0]")?,
                value_to_f32(&values[1], "weight_range[1]")?,
            );
        }
        params
            .validate()
            .map_err(|e| ToolError::Substrate(e.to_string()))?;
        Ok(params)
    }

    fn require_n(&self) -> Result<usize, ToolError> {
        self.n
            .ok_or_else(|| bad_input("apply_seed", "n is required for this seed kind"))
    }

    fn seed_u64(&self) -> Result<u64, ToolError> {
        self.params
            .get("seed")
            .map(|v| {
                v.as_u64()
                    .ok_or_else(|| bad_input("apply_seed", "params.seed must be an integer >= 0"))
            })
            .transpose()
            .map(|v| v.unwrap_or(0))
    }

    fn usize_param(&self, key: &'static str, default: usize) -> Result<usize, ToolError> {
        self.params
            .get(key)
            .map(|v| {
                v.as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| {
                        bad_input("apply_seed", format!("params.{key} must be an integer"))
                    })
            })
            .transpose()
            .map(|v| v.unwrap_or(default))
    }

    fn f32_param(&self, key: &'static str, default: f32) -> Result<f32, ToolError> {
        self.optional_f32(key).map(|v| v.unwrap_or(default))
    }

    fn optional_f32(&self, key: &'static str) -> Result<Option<f32>, ToolError> {
        self.params
            .get(key)
            .map(|v| value_to_f32(v, key))
            .transpose()
    }

    fn layers_param(&self) -> Result<Vec<usize>, ToolError> {
        let values = self
            .params
            .get("layers")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                bad_input("apply_seed", "params.layers is required for layered seeds")
            })?;
        values
            .iter()
            .map(|v| {
                v.as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| {
                        bad_input("apply_seed", "params.layers must contain positive integers")
                    })
            })
            .collect()
    }
}

fn value_to_f32(value: &serde_json::Value, label: &'static str) -> Result<f32, ToolError> {
    let n = value
        .as_f64()
        .ok_or_else(|| bad_input("apply_seed", format!("{label} must be a number")))?;
    let f = n as f32;
    if f.is_finite() {
        Ok(f)
    } else {
        Err(bad_input("apply_seed", format!("{label} must be finite")))
    }
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
        .ok_or_else(|| bad_input(property, format!("{property} must be a UUID string")))?;
    Uuid::parse_str(raw).map_err(|e| bad_input(property, format!("{property}: {e}")))
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
            .chain(topology_write_tools().into_iter())
        {
            registry.register(tool).unwrap();
        }
        registry
    }

    #[tokio::test]
    async fn apply_seed_registers_as_topology_write() {
        let mut registry = Registry::new();
        registry.register(Box::new(ApplySeedTool)).unwrap();
        let descriptor = registry.descriptors().pop().unwrap();
        assert_eq!(descriptor.name, "apply_seed");
        assert_eq!(descriptor.permission, Permission::TopologyWrite);
    }

    #[tokio::test]
    async fn apply_seed_supports_all_seed_kinds() {
        let cases = [
            serde_json::json!({"kind": "random", "n": 20, "params": {"p": 0.1, "seed": 1}}),
            serde_json::json!({"kind": "ring", "n": 20, "params": {"k": 2, "seed": 2}}),
            serde_json::json!({"kind": "small_world", "n": 20, "params": {"k": 2, "p_rewire": 0.3, "seed": 3}}),
            serde_json::json!({"kind": "layered", "params": {"layers": [4, 5, 3], "seed": 4}}),
        ];

        for args in cases {
            let (sim, _join) = spawn_engine(10_000);
            let out = registry()
                .invoke(
                    "apply_seed",
                    args,
                    Permission::TopologyWrite,
                    ToolContext::new(sim),
                )
                .await
                .unwrap();
            assert!(out["added_nodes"].as_u64().unwrap() > 0);
        }
    }

    #[tokio::test]
    async fn apply_seed_makes_200_node_small_world_visible_to_graph_snapshot() {
        let (sim, _join) = spawn_engine(10_000);
        let registry = registry();
        let ctx = ToolContext::new(sim);

        let seed = registry
            .invoke(
                "apply_seed",
                serde_json::json!({
                    "kind": "small_world",
                    "n": 200,
                    "params": { "k": 2, "p_rewire": 0.2, "seed": 42 }
                }),
                Permission::TopologyWrite,
                ctx.clone(),
            )
            .await
            .unwrap();
        assert_eq!(seed["added_nodes"], 200);

        let graph = registry
            .invoke(
                "graph_snapshot",
                serde_json::json!({}),
                Permission::ReadOnly,
                ctx,
            )
            .await
            .unwrap();
        assert_eq!(graph["nodes"].as_array().unwrap().len(), 200);
        let visible_edges = graph["edges"].as_array().unwrap().len() as u64;
        assert_eq!(visible_edges, seed["added_edges"].as_u64().unwrap());
        assert!(visible_edges > 0);
        assert!(visible_edges <= seed["requested_edges"].as_u64().unwrap());
    }

    #[tokio::test]
    async fn apply_seed_requires_topology_write_permission() {
        let (sim, _join) = spawn_engine(10_000);
        let err = registry()
            .invoke(
                "apply_seed",
                serde_json::json!({"kind": "ring", "n": 10, "params": {"k": 1}}),
                Permission::ReadOnly,
                ToolContext::new(sim),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::PermissionDenied { .. }));
    }

    #[tokio::test]
    async fn add_and_remove_topology_are_visible_in_graph_snapshot() {
        let (sim, _join) = spawn_engine(10_000);
        let registry = registry();
        let ctx = ToolContext::new(sim);

        let pre = registry
            .invoke(
                "add_neuron",
                serde_json::json!({}),
                Permission::TopologyWrite,
                ctx.clone(),
            )
            .await
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let post = registry
            .invoke(
                "add_neuron",
                serde_json::json!({}),
                Permission::TopologyWrite,
                ctx.clone(),
            )
            .await
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let edge = registry
            .invoke(
                "add_synapse",
                serde_json::json!({ "pre_id": pre, "post_id": post, "weight": 0.4 }),
                Permission::TopologyWrite,
                ctx.clone(),
            )
            .await
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();

        let graph = registry
            .invoke(
                "graph_snapshot",
                serde_json::json!({}),
                Permission::ReadOnly,
                ctx.clone(),
            )
            .await
            .unwrap();
        assert_eq!(graph["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(graph["edges"].as_array().unwrap().len(), 1);

        let removed_synapse = registry
            .invoke(
                "remove_synapse",
                serde_json::json!({ "edge_id": edge }),
                Permission::TopologyWrite,
                ctx.clone(),
            )
            .await
            .unwrap();
        assert_eq!(removed_synapse["removed"], true);

        let removed_neuron = registry
            .invoke(
                "remove_neuron",
                serde_json::json!({ "node_id": pre }),
                Permission::TopologyWrite,
                ctx.clone(),
            )
            .await
            .unwrap();
        assert_eq!(removed_neuron["removed"], true);

        let graph = registry
            .invoke(
                "graph_snapshot",
                serde_json::json!({}),
                Permission::ReadOnly,
                ctx,
            )
            .await
            .unwrap();
        assert_eq!(graph["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(graph["edges"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn remove_neuron_cascades_edges() {
        let (sim, _join) = spawn_engine(10_000);
        let registry = registry();
        let ctx = ToolContext::new(sim);
        let pre = Uuid::new_v4();
        let post = Uuid::new_v4();
        let edge = Uuid::new_v4();
        ctx.sim().unwrap().add_node(pre).await.unwrap();
        ctx.sim().unwrap().add_node(post).await.unwrap();
        ctx.sim()
            .unwrap()
            .add_edge(edge, pre, post, 0.4)
            .await
            .unwrap();

        let removed = registry
            .invoke(
                "remove_neuron",
                serde_json::json!({ "node_id": pre }),
                Permission::TopologyWrite,
                ctx,
            )
            .await
            .unwrap();
        assert_eq!(removed["removed"], true);
        assert_eq!(removed["cascaded_edges"], 1);
    }
}

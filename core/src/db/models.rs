//! Diesel ORM structs for the three M0 tables. Domain logic lives elsewhere;
//! these are row shapes only.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::schema::{edges, nodes, spike_log};

// ─── nodes ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Queryable, Selectable, Serialize)]
#[diesel(table_name = nodes)]
pub struct NodeRow {
    pub id: Uuid,
    pub label: String,
    pub node_type: String,
    pub source_file: Option<String>,
    pub model_blob_path: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable, Deserialize)]
#[diesel(table_name = nodes)]
pub struct NewNode {
    pub label: String,
    #[serde(default = "default_node_type")]
    pub node_type: String,
    #[serde(default)]
    pub source_file: Option<String>,
    #[serde(default)]
    pub model_blob_path: Option<String>,
    #[serde(default = "default_metadata")]
    pub metadata: serde_json::Value,
}

fn default_node_type() -> String {
    "concept".to_string()
}
fn default_metadata() -> serde_json::Value {
    serde_json::json!({})
}

// ─── edges ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Queryable, Selectable, Serialize)]
#[diesel(table_name = edges)]
pub struct EdgeRow {
    pub id: Uuid,
    pub pre_id: Uuid,
    pub post_id: Uuid,
    pub weight: f32,
    pub edge_type: String,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable, Deserialize)]
#[diesel(table_name = edges)]
pub struct NewEdge {
    pub pre_id: Uuid,
    pub post_id: Uuid,
    #[serde(default = "default_weight")]
    pub weight: f32,
    #[serde(default = "default_edge_type")]
    pub edge_type: String,
    #[serde(default = "default_metadata")]
    pub metadata: serde_json::Value,
}

fn default_weight() -> f32 {
    0.5
}
fn default_edge_type() -> String {
    "association".to_string()
}

// ─── spike_log ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Queryable, Selectable, Serialize)]
#[diesel(table_name = spike_log)]
pub struct SpikeRow {
    pub id: i64,
    pub node_id: Uuid,
    pub t_ms: f64,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = spike_log)]
pub struct NewSpike {
    pub node_id: Uuid,
    pub t_ms: f64,
}

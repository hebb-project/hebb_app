//! Graph CRUD: nodes, edges, full-graph snapshot for the frontend.

use axum::extract::{Path, Query, State};
use axum::Json;
use cortex_snn::Cortex;
use diesel::prelude::*;
use serde::Deserialize;
use uuid::Uuid;

use super::{ok, AppState};
use crate::db::models::{EdgeRow, NewEdge, NewNode, NodeRow};
use crate::db::run_blocking;
use crate::db::schema::{edges, nodes};
use crate::error::{CoreError, CoreResult};

#[derive(Debug, Deserialize)]
pub struct Pagination {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    500
}

pub async fn list_nodes(
    State(s): State<AppState>,
    Query(p): Query<Pagination>,
) -> CoreResult<Json<serde_json::Value>> {
    if let Some(folder) = s
        .engine
        .current_folder()
        .await
        .map_err(|m| CoreError::EngineOffline(m.into()))?
    {
        if let Some(snapshot) = graph_from_open_folder(&folder)? {
            let rows = snapshot
                .get("nodes")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            return Ok(ok(paginate_values(rows, &p)));
        }
    }

    let rows: Vec<NodeRow> = run_blocking(&s.pool, move |conn| {
        Ok(nodes::table
            .order(nodes::created_at.asc())
            .limit(p.limit)
            .offset(p.offset)
            .select(NodeRow::as_select())
            .load(conn)?)
    })
    .await?;
    Ok(ok(rows))
}

pub async fn get_node(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> CoreResult<Json<serde_json::Value>> {
    if let Some(folder) = s
        .engine
        .current_folder()
        .await
        .map_err(|m| CoreError::EngineOffline(m.into()))?
    {
        if let Some(snapshot) = graph_from_open_folder(&folder)? {
            if let Some(node) = snapshot
                .get("nodes")
                .and_then(|v| v.as_array())
                .and_then(|rows| {
                    rows.iter()
                        .find(|row| row.get("id").and_then(|v| v.as_str()) == Some(&id.to_string()))
                        .cloned()
                })
            {
                return Ok(ok(node));
            }
            return Err(CoreError::NotFound(format!("node {id}")));
        }
    }

    let row: NodeRow = run_blocking(&s.pool, move |conn| {
        nodes::table
            .find(id)
            .select(NodeRow::as_select())
            .first(conn)
            .map_err(|e| match e {
                diesel::result::Error::NotFound => CoreError::NotFound(format!("node {id}")),
                other => CoreError::from(other),
            })
    })
    .await?;
    Ok(ok(row))
}

pub async fn create_node(
    State(s): State<AppState>,
    Json(new): Json<NewNode>,
) -> CoreResult<Json<serde_json::Value>> {
    // When a `.cortex/` folder is open, structural edits must flow
    // through the Cortex handle so topology.json is the source of
    // truth — otherwise the desktop's build mode would mutate a single
    // shared Postgres pile that every folder-backed network sees.
    if let Some(_folder) = s
        .engine
        .current_folder()
        .await
        .map_err(|m| CoreError::EngineOffline(m.into()))?
    {
        let record = s
            .engine
            .add_neuron_to_folder(new.label.clone(), new.metadata.clone())
            .await
            .map_err(CoreError::BadRequest)?;
        return Ok(ok(serde_json::json!({
            "id": record.id,
            "label": record.label,
            "node_type": record.node_type,
            "source_file": serde_json::Value::Null,
            "metadata": record.metadata,
        })));
    }

    let engine = s.engine.clone();
    let row: NodeRow = run_blocking(&s.pool, move |conn| {
        Ok(diesel::insert_into(nodes::table)
            .values(&new)
            .returning(NodeRow::as_returning())
            .get_result(conn)?)
    })
    .await?;
    let _ = engine.add_node(row.id).await;
    Ok(ok(row))
}

pub async fn delete_node(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> CoreResult<Json<serde_json::Value>> {
    if let Some(_folder) = s
        .engine
        .current_folder()
        .await
        .map_err(|m| CoreError::EngineOffline(m.into()))?
    {
        match s
            .engine
            .remove_neuron_from_folder(id)
            .await
            .map_err(CoreError::BadRequest)?
        {
            Some(summary) => {
                return Ok(ok(serde_json::json!({
                    "deleted": id,
                    "cascaded_edges": summary.cascaded_edges,
                })));
            }
            None => return Err(CoreError::NotFound(format!("node {id}"))),
        }
    }

    let n: usize = run_blocking(&s.pool, move |conn| {
        Ok(diesel::delete(nodes::table.find(id)).execute(conn)?)
    })
    .await?;
    if n == 0 {
        return Err(CoreError::NotFound(format!("node {id}")));
    }
    Ok(ok(serde_json::json!({ "deleted": id })))
}

pub async fn list_edges(
    State(s): State<AppState>,
    Query(p): Query<Pagination>,
) -> CoreResult<Json<serde_json::Value>> {
    if let Some(folder) = s
        .engine
        .current_folder()
        .await
        .map_err(|m| CoreError::EngineOffline(m.into()))?
    {
        if let Some(snapshot) = graph_from_open_folder(&folder)? {
            let rows = snapshot
                .get("edges")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            return Ok(ok(paginate_values(rows, &p)));
        }
    }

    let rows: Vec<EdgeRow> = run_blocking(&s.pool, move |conn| {
        Ok(edges::table
            .order(edges::created_at.asc())
            .limit(p.limit)
            .offset(p.offset)
            .select(EdgeRow::as_select())
            .load(conn)?)
    })
    .await?;
    Ok(ok(rows))
}

pub async fn create_edge(
    State(s): State<AppState>,
    Json(new): Json<NewEdge>,
) -> CoreResult<Json<serde_json::Value>> {
    if new.pre_id == new.post_id {
        return Err(CoreError::BadRequest("self-loops are not allowed".into()));
    }

    if let Some(_folder) = s
        .engine
        .current_folder()
        .await
        .map_err(|m| CoreError::EngineOffline(m.into()))?
    {
        let record = s
            .engine
            .add_synapse_to_folder(new.pre_id, new.post_id, new.weight, new.metadata.clone())
            .await
            .map_err(CoreError::BadRequest)?;
        return Ok(ok(serde_json::json!({
            "id": record.id,
            "pre_id": record.pre_id,
            "post_id": record.post_id,
            "weight": record.weight,
            "edge_type": record.edge_type,
        })));
    }

    let engine = s.engine.clone();
    let row: EdgeRow = run_blocking(&s.pool, move |conn| {
        Ok(diesel::insert_into(edges::table)
            .values(&new)
            .returning(EdgeRow::as_returning())
            .get_result(conn)?)
    })
    .await?;
    let _ = engine
        .add_edge(row.id, row.pre_id, row.post_id, row.weight)
        .await;
    Ok(ok(row))
}

pub async fn delete_edge(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> CoreResult<Json<serde_json::Value>> {
    if let Some(_folder) = s
        .engine
        .current_folder()
        .await
        .map_err(|m| CoreError::EngineOffline(m.into()))?
    {
        let removed = s
            .engine
            .remove_synapse_from_folder(id)
            .await
            .map_err(CoreError::BadRequest)?;
        if !removed {
            return Err(CoreError::NotFound(format!("edge {id}")));
        }
        return Ok(ok(serde_json::json!({ "deleted": id })));
    }

    let n: usize = run_blocking(&s.pool, move |conn| {
        Ok(diesel::delete(edges::table.find(id)).execute(conn)?)
    })
    .await?;
    if n == 0 {
        return Err(CoreError::NotFound(format!("edge {id}")));
    }
    Ok(ok(serde_json::json!({ "deleted": id })))
}

/// Single-shot snapshot the frontend uses on mount. Returns the whole
/// graph; assumes the M0 scale of low-thousands of nodes is fine for one
/// JSON payload.
pub async fn get_full_graph(State(s): State<AppState>) -> CoreResult<Json<serde_json::Value>> {
    if let Some(folder) = s
        .engine
        .current_folder()
        .await
        .map_err(|m| CoreError::EngineOffline(m.into()))?
    {
        if let Some(snapshot) = graph_from_open_folder(&folder)? {
            return Ok(ok(snapshot));
        }
    }

    let (n, e): (Vec<NodeRow>, Vec<EdgeRow>) = run_blocking(&s.pool, |conn| {
        let n = nodes::table.select(NodeRow::as_select()).load(conn)?;
        let e = edges::table.select(EdgeRow::as_select()).load(conn)?;
        Ok((n, e))
    })
    .await?;

    Ok(ok(serde_json::json!({
        "nodes": n.into_iter().map(strip_search_body).collect::<Vec<_>>(),
        "edges": e,
    })))
}

fn strip_search_body(row: NodeRow) -> serde_json::Value {
    let mut value = serde_json::to_value(row).unwrap_or_else(|_| serde_json::json!({}));
    if let Some(metadata) = value.get_mut("metadata").and_then(|m| m.as_object_mut()) {
        metadata.remove("body_text");
    }
    value
}

fn paginate_values(
    rows: Vec<serde_json::Value>,
    pagination: &Pagination,
) -> Vec<serde_json::Value> {
    let offset = pagination.offset.max(0) as usize;
    let limit = pagination.limit.max(0) as usize;
    rows.into_iter().skip(offset).take(limit).collect()
}

fn graph_from_open_folder(folder: &std::path::Path) -> CoreResult<Option<serde_json::Value>> {
    let cortex = Cortex::open(folder)
        .map_err(|e| CoreError::BadRequest(format!("opening {}: {}", folder.display(), e)))?;

    // Knowledge-graph networks still render from Postgres today. The
    // folder-backed snapshot path is for fresh LIF/HH families where
    // topology now lives on disk.
    if cortex.cortex_type() == "knowledge-graph" {
        return Ok(None);
    }

    let topology = cortex.topology();
    let nodes = topology
        .nodes
        .iter()
        .map(|n| {
            serde_json::json!({
                "id": n.id,
                "label": n.label,
                "node_type": n
                    .kind
                    .as_ref()
                    .map(|k| k.kind.clone())
                    .unwrap_or_else(|| topology.defaults.neuron.kind.clone()),
                "source_file": serde_json::Value::Null,
                "metadata": n.metadata,
            })
        })
        .collect::<Vec<_>>();
    let edges = topology
        .edges
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "pre_id": e.pre,
                "post_id": e.post,
                "weight": e.init_weight,
                "edge_type": e
                    .kind
                    .as_ref()
                    .map(|k| k.kind.clone())
                    .unwrap_or_else(|| topology.defaults.synapse.kind.clone()),
            })
        })
        .collect::<Vec<_>>();

    Ok(Some(serde_json::json!({
        "nodes": nodes,
        "edges": edges,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_snn::format::topology::{NeuronSpec, SynapseSpec, TopologyDefaults};
    use cortex_snn::{AddNeuron, CreateOptions};
    use std::time::SystemTime;

    fn unique_root(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("core-graph-{label}-{nanos}"))
    }

    fn make_lif_folder(root: &std::path::Path, name: &str, labels: &[&str]) {
        let mut cx = cortex_snn::Cortex::create(
            root,
            CreateOptions {
                name: name.into(),
                cortex_type: "lif".into(),
                source_root: root.display().to_string(),
                now_rfc3339: "2026-05-22T12:00:00Z".into(),
                defaults: TopologyDefaults {
                    neuron: NeuronSpec::lif(),
                    synapse: SynapseSpec::stdp(),
                },
                hh_config: None,
            },
        )
        .unwrap();

        for label in labels {
            cx.add_neuron(AddNeuron {
                label: (*label).into(),
                ..Default::default()
            })
            .unwrap();
        }
    }

    #[test]
    fn graph_snapshot_reads_the_requested_folder_not_global_state() {
        let root_a = unique_root("a");
        let root_b = unique_root("b");
        make_lif_folder(&root_a, "Folder A", &["a-only"]);
        make_lif_folder(&root_b, "Folder B", &["b-one", "b-two"]);

        let graph_a = graph_from_open_folder(&root_a).unwrap().unwrap();
        let graph_b = graph_from_open_folder(&root_b).unwrap().unwrap();

        let labels_a: Vec<_> = graph_a["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["label"].as_str().unwrap())
            .collect();
        let labels_b: Vec<_> = graph_b["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["label"].as_str().unwrap())
            .collect();

        assert_eq!(labels_a, vec!["a-only"]);
        assert_eq!(labels_b, vec!["b-one", "b-two"]);

        std::fs::remove_dir_all(&root_a).ok();
        std::fs::remove_dir_all(&root_b).ok();
    }

    #[test]
    fn folder_snapshot_pagination_is_stable() {
        let rows = vec![
            serde_json::json!({ "label": "a" }),
            serde_json::json!({ "label": "b" }),
            serde_json::json!({ "label": "c" }),
        ];
        let page = paginate_values(
            rows,
            &Pagination {
                limit: 1,
                offset: 1,
            },
        );
        assert_eq!(page, vec![serde_json::json!({ "label": "b" })]);
    }
}

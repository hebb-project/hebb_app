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

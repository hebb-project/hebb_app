//! Graph CRUD: nodes, edges, full-graph snapshot for the frontend.

use axum::extract::{Path, Query, State};
use axum::Json;
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

fn default_limit() -> i64 { 500 }

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
    }).await?;
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
    }).await?;
    Ok(ok(row))
}

pub async fn create_node(
    State(s): State<AppState>,
    Json(new): Json<NewNode>,
) -> CoreResult<Json<serde_json::Value>> {
    let engine = s.engine.clone();
    let row: NodeRow = run_blocking(&s.pool, move |conn| {
        Ok(diesel::insert_into(nodes::table)
            .values(&new)
            .returning(NodeRow::as_returning())
            .get_result(conn)?)
    }).await?;
    let _ = engine.add_node(row.id).await;
    Ok(ok(row))
}

pub async fn delete_node(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> CoreResult<Json<serde_json::Value>> {
    let n: usize = run_blocking(&s.pool, move |conn| {
        Ok(diesel::delete(nodes::table.find(id)).execute(conn)?)
    }).await?;
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
    }).await?;
    Ok(ok(rows))
}

pub async fn create_edge(
    State(s): State<AppState>,
    Json(new): Json<NewEdge>,
) -> CoreResult<Json<serde_json::Value>> {
    if new.pre_id == new.post_id {
        return Err(CoreError::BadRequest("self-loops are not allowed".into()));
    }
    let engine = s.engine.clone();
    let row: EdgeRow = run_blocking(&s.pool, move |conn| {
        Ok(diesel::insert_into(edges::table)
            .values(&new)
            .returning(EdgeRow::as_returning())
            .get_result(conn)?)
    }).await?;
    let _ = engine.add_edge(row.id, row.pre_id, row.post_id, row.weight).await;
    Ok(ok(row))
}

pub async fn delete_edge(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> CoreResult<Json<serde_json::Value>> {
    let n: usize = run_blocking(&s.pool, move |conn| {
        Ok(diesel::delete(edges::table.find(id)).execute(conn)?)
    }).await?;
    if n == 0 {
        return Err(CoreError::NotFound(format!("edge {id}")));
    }
    Ok(ok(serde_json::json!({ "deleted": id })))
}

/// Single-shot snapshot the frontend uses on mount. Returns the whole
/// graph; assumes the M0 scale of low-thousands of nodes is fine for one
/// JSON payload.
pub async fn get_full_graph(
    State(s): State<AppState>,
) -> CoreResult<Json<serde_json::Value>> {
    let (n, e): (Vec<NodeRow>, Vec<EdgeRow>) = run_blocking(&s.pool, |conn| {
        let n = nodes::table.select(NodeRow::as_select()).load(conn)?;
        let e = edges::table.select(EdgeRow::as_select()).load(conn)?;
        Ok((n, e))
    }).await?;

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

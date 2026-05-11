//! `POST /api/vault/ingest` — parse a directory of Obsidian markdown and
//! upsert it into the graph (DB) + into the live engine.
//!
//! Upsert key is `source_file` for nodes. Edges are matched on
//! (pre, post, edge_type) and inserted on conflict-do-nothing so repeated
//! ingestion is idempotent.

use std::collections::HashMap;
use std::path::PathBuf;

use axum::extract::State;
use axum::Json;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde::Deserialize;
use uuid::Uuid;

use super::{ok, AppState};
use crate::db::models::{EdgeRow, NewEdge, NewNode, NodeRow};
use crate::db::run_blocking;
use crate::db::schema::{edges, nodes};
use crate::error::{CoreError, CoreResult};
use crate::vault::parse_vault;

#[derive(Debug, Deserialize, Default)]
pub struct IngestBody {
    /// Absolute or workspace-relative path. Falls back to `AppState.vault_path`.
    pub path: Option<String>,
}

pub async fn post_ingest(
    State(s): State<AppState>,
    Json(body): Json<IngestBody>,
) -> CoreResult<Json<serde_json::Value>> {
    let path: PathBuf = body.path
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| (*s.vault_path).clone());

    if !path.exists() {
        return Err(CoreError::BadRequest(format!("vault path not found: {}", path.display())));
    }

    let path_for_parse = path.clone();
    let parsed = tokio::task::spawn_blocking(move || parse_vault(&path_for_parse))
        .await
        .map_err(|e| CoreError::Other(anyhow::anyhow!("parse task panicked: {e}")))??;

    let engine = s.engine.clone();
    let (db_nodes, db_edges, summary): (Vec<NodeRow>, Vec<EdgeRow>, serde_json::Value) =
        run_blocking(&s.pool, move |conn| {
            ingest_into_db(conn, parsed)
        }).await?;

    // Push the new subgraph into the live engine.
    let node_ids: Vec<Uuid> = db_nodes.iter().map(|n| n.id).collect();
    let edge_tuples: Vec<(Uuid, Uuid, f32)> = db_edges.iter()
        .map(|e| (e.pre_id, e.post_id, e.weight))
        .collect();
    let _ = engine.ingest_batch(node_ids, edge_tuples).await;

    Ok(ok(summary))
}

fn ingest_into_db(
    conn: &mut PgConnection,
    parsed: crate::vault::VaultParseSummary,
) -> CoreResult<(Vec<NodeRow>, Vec<EdgeRow>, serde_json::Value)> {
    use diesel::result::Error as DieselErr;

    let mut nodes_created = 0usize;
    let mut nodes_existing = 0usize;
    let mut edges_created = 0usize;
    let mut edges_existing = 0usize;

    // Run inside a transaction so partial failures don't leave a half-ingested graph.
    conn.transaction::<_, DieselErr, _>(|conn| {
        // 1. Upsert nodes. Two paths because some nodes have source_file
        //    (real files) and others don't (wikilink stubs).
        for n in &parsed.nodes {
            let new = NewNode {
                label: n.label.clone(),
                node_type: n.node_type.clone(),
                source_file: n.source_file.clone(),
                model_blob_path: None,
                metadata: n.metadata.clone(),
            };

            let existing: Option<NodeRow> = match &n.source_file {
                Some(sf) => nodes::table
                    .filter(nodes::source_file.eq(sf))
                    .select(NodeRow::as_select())
                    .first(conn)
                    .optional()?,
                None => nodes::table
                    .filter(nodes::label.eq(&n.label))
                    .filter(nodes::source_file.is_null())
                    .select(NodeRow::as_select())
                    .first(conn)
                    .optional()?,
            };

            match existing {
                Some(row) => {
                    nodes_existing += 1;
                    // Refresh metadata / type from the latest parse.
                    diesel::update(nodes::table.find(row.id))
                        .set((
                            nodes::label.eq(&new.label),
                            nodes::node_type.eq(&new.node_type),
                            nodes::metadata.eq(&new.metadata),
                            nodes::updated_at.eq(chrono::Utc::now()),
                        ))
                        .execute(conn)?;
                }
                None => {
                    nodes_created += 1;
                    diesel::insert_into(nodes::table)
                        .values(&new)
                        .execute(conn)?;
                }
            }
        }

        // 2. Build label → id map after node upsert.
        let all: Vec<NodeRow> = nodes::table.select(NodeRow::as_select()).load(conn)?;
        let id_by_label: HashMap<String, Uuid> =
            all.iter().map(|r| (r.label.clone(), r.id)).collect();

        // 3. Upsert edges.
        for e in &parsed.edges {
            let pre = match id_by_label.get(&e.pre_label) { Some(id) => *id, None => continue };
            let post = match id_by_label.get(&e.post_label) { Some(id) => *id, None => continue };
            if pre == post { continue; }

            let existing: Option<EdgeRow> = edges::table
                .filter(edges::pre_id.eq(pre))
                .filter(edges::post_id.eq(post))
                .filter(edges::edge_type.eq(&e.edge_type))
                .select(EdgeRow::as_select())
                .first(conn)
                .optional()?;

            match existing {
                Some(_) => { edges_existing += 1; }
                None => {
                    edges_created += 1;
                    diesel::insert_into(edges::table)
                        .values(&NewEdge {
                            pre_id: pre,
                            post_id: post,
                            weight: e.weight,
                            edge_type: e.edge_type.clone(),
                            metadata: serde_json::json!({}),
                        })
                        .execute(conn)?;
                }
            }
        }

        Ok(())
    })?;

    // Return fresh rows so the caller can hydrate the engine.
    let nodes_now: Vec<NodeRow> = nodes::table.select(NodeRow::as_select()).load(conn)?;
    let edges_now: Vec<EdgeRow> = edges::table.select(EdgeRow::as_select()).load(conn)?;

    let summary = serde_json::json!({
        "files_seen": parsed.files_seen,
        "nodes_created": nodes_created,
        "nodes_existing": nodes_existing,
        "edges_created": edges_created,
        "edges_existing": edges_existing,
        "total_nodes": nodes_now.len(),
        "total_edges": edges_now.len(),
    });

    Ok((nodes_now, edges_now, summary))
}

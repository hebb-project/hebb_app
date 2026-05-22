//! Generic vault search over ingested Obsidian-compatible notes.
//!
//! This is intentionally corpus-agnostic. It searches node labels,
//! source paths, and body text captured during `/api/vault/ingest`.

use axum::extract::{Query, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{ok, AppState};
use crate::db::models::NodeRow;
use crate::db::run_blocking;
use crate::db::schema::nodes;
use crate::error::{CoreError, CoreResult};

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    10
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub node_id: Uuid,
    pub label: String,
    pub node_type: String,
    pub source_file: Option<String>,
    pub score: f32,
    pub snippet: Option<String>,
    pub highlights: Vec<String>,
}

pub async fn search_nodes(
    State(s): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> CoreResult<Json<serde_json::Value>> {
    let terms = query_terms(&q.q);
    if terms.is_empty() {
        return Err(CoreError::BadRequest("search query is empty".into()));
    }

    let limit = q.limit.clamp(1, 50);
    let rows: Vec<NodeRow> = run_blocking(&s.pool, move |conn| {
        Ok(nodes::table
            .order(nodes::updated_at.desc())
            .limit(10_000)
            .select(NodeRow::as_select())
            .load(conn)?)
    })
    .await?;

    let mut results: Vec<SearchResult> = rows
        .into_iter()
        .filter_map(|row| score_row(row, &terms))
        .collect();

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.label.cmp(&b.label))
    });
    results.truncate(limit);

    Ok(ok(serde_json::json!({
        "query": q.q,
        "results": results,
    })))
}

fn score_row(row: NodeRow, terms: &[String]) -> Option<SearchResult> {
    let label = row.label.to_lowercase();
    let source = row.source_file.clone().unwrap_or_default().to_lowercase();
    let body = row
        .metadata
        .get("body_text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    let metadata = row.metadata.to_string().to_lowercase();

    let mut score = 0.0f32;
    let mut highlights = Vec::new();
    for term in terms {
        if label.contains(term) {
            score += 12.0;
            highlights.push(term.clone());
        }
        if source.contains(term) {
            score += 4.0;
        }
        if body.contains(term) {
            score += 3.0;
            highlights.push(term.clone());
        } else if metadata.contains(term) {
            score += 1.0;
        }
    }

    if score <= 0.0 {
        return None;
    }

    highlights.sort();
    highlights.dedup();

    Some(SearchResult {
        node_id: row.id,
        label: row.label,
        node_type: row.node_type,
        source_file: row.source_file,
        score,
        snippet: snippet_for(&row.metadata, terms),
        highlights,
    })
}

fn query_terms(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .map(str::trim)
        .filter(|s| s.len() > 1 && !STOPWORDS.contains(s))
        .map(ToOwned::to_owned)
        .collect()
}

fn snippet_for(metadata: &serde_json::Value, terms: &[String]) -> Option<String> {
    let body = metadata
        .get("body_text")
        .and_then(|v| v.as_str())
        .or_else(|| metadata.get("body_excerpt").and_then(|v| v.as_str()))?;

    let body_lower = body.to_lowercase();
    let first_hit = terms
        .iter()
        .filter_map(|term| body_lower.find(term))
        .min()
        .unwrap_or(0);
    let start = first_hit.saturating_sub(90);
    let snippet: String = body.chars().skip(start).take(240).collect();
    Some(snippet.split_whitespace().collect::<Vec<_>>().join(" "))
}

const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "how", "in", "is", "it", "of",
    "on", "or", "the", "to", "was", "were", "what", "when", "where", "who", "why", "with",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_terms_drop_stopwords() {
        assert_eq!(
            query_terms("What is active inference?"),
            vec!["active", "inference"]
        );
    }
}

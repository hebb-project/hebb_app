//! Parse a directory of Obsidian markdown into nodes + edges.
//!
//! * One node per `.md` file. Label = filename stem.
//! * One edge per `[[wikilink]]` (directed: source → target).
//! * Frontmatter `tags:` → array on node metadata. Other frontmatter
//!   keys stored verbatim in metadata.
//! * `#hashtag` mentions are recorded as metadata, not nodes — keeps the
//!   initial graph clean.
//!
//! Targets that don't resolve to a known file are still emitted as nodes
//! with `node_type = "stub"` so the link survives. This is how Obsidian
//! itself behaves.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize)]
pub struct ParsedNode {
    pub label: String,
    pub node_type: String,
    pub source_file: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParsedEdge {
    pub pre_label: String,
    pub post_label: String,
    pub edge_type: String,
    pub weight: f32,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct VaultParseSummary {
    pub files_seen: usize,
    pub nodes: Vec<ParsedNode>,
    pub edges: Vec<ParsedEdge>,
}

pub fn parse_vault(root: &Path) -> std::io::Result<VaultParseSummary> {
    let mut summary = VaultParseSummary::default();
    let mut nodes_by_label: HashMap<String, ParsedNode> = HashMap::new();
    let mut edges: Vec<ParsedEdge> = Vec::new();

    for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !entry.file_type().is_file() {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }

        summary.files_seen += 1;
        let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        let label = label_from_path(&rel);
        // Tolerate per-file read failures (e.g. macOS Docker Desktop's
        // intermittent EDEADLK on bind-mounted files) — one bad file
        // shouldn't bork an entire vault ingest.
        let raw = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "skipping unreadable file");
                continue;
            }
        };
        let (frontmatter, body) = split_frontmatter(&raw);

        let mut metadata = serde_json::Map::new();
        if let Some(fm) = frontmatter {
            if let Ok(yaml) = serde_yaml::from_str::<serde_yaml::Value>(fm) {
                if let Ok(json) = serde_json::to_value(yaml) {
                    if let serde_json::Value::Object(map) = json {
                        metadata.extend(map);
                    }
                }
            }
        }

        // Hashtags in body — record but don't materialize as nodes.
        let tags = extract_hashtags(body);
        if !tags.is_empty() {
            metadata
                .entry("body_tags".to_string())
                .or_insert(serde_json::json!(tags));
        }
        metadata.insert(
            "body_excerpt".to_string(),
            serde_json::json!(excerpt(body, 280)),
        );
        metadata.insert(
            "body_text".to_string(),
            serde_json::json!(truncate_for_search(body, 20_000)),
        );

        let node_type = infer_node_type(&rel, &metadata);
        nodes_by_label.insert(
            label.clone(),
            ParsedNode {
                label: label.clone(),
                node_type,
                source_file: Some(rel.to_string_lossy().to_string()),
                metadata: serde_json::Value::Object(metadata),
            },
        );

        for target in extract_wikilinks(body) {
            edges.push(ParsedEdge {
                pre_label: label.clone(),
                post_label: target,
                edge_type: "wikilink".to_string(),
                weight: 0.5,
            });
        }
    }

    // Create stub nodes for wikilink targets that don't resolve to any file.
    for e in &edges {
        nodes_by_label
            .entry(e.post_label.clone())
            .or_insert_with(|| ParsedNode {
                label: e.post_label.clone(),
                node_type: "stub".to_string(),
                source_file: None,
                metadata: serde_json::json!({}),
            });
    }

    summary.nodes = nodes_by_label.into_values().collect();
    summary.edges = edges;
    Ok(summary)
}

fn label_from_path(rel: &Path) -> String {
    rel.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| rel.to_string_lossy().to_string())
}

fn infer_node_type(rel: &Path, metadata: &serde_json::Map<String, serde_json::Value>) -> String {
    if let Some(tags) = metadata.get("tags").and_then(|v| v.as_array()) {
        for t in tags {
            if let Some(s) = t.as_str() {
                if let Some(kind) = s.strip_prefix("type/") {
                    return kind.to_string();
                }
            }
        }
    }
    // Fall back to top-level folder name.
    let first_dir = rel
        .components()
        .next()
        .and_then(|c| c.as_os_str().to_str())
        .unwrap_or("");
    match first_dir {
        "concepts" => "concept",
        "ideas" => "idea",
        "papers" => "paper",
        "logs" => "log",
        "experiments" => "experiment",
        "entities" => "entity",
        _ => "concept",
    }
    .to_string()
}

fn split_frontmatter(raw: &str) -> (Option<&str>, &str) {
    // Frontmatter delimiter is `---` on its own line at file start.
    if let Some(rest) = raw.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---\n") {
            return (Some(&rest[..end]), &rest[end + 5..]);
        }
        if let Some(end) = rest.find("\n---\r\n") {
            return (Some(&rest[..end]), &rest[end + 6..]);
        }
    }
    if let Some(rest) = raw.strip_prefix("---\r\n") {
        if let Some(end) = rest.find("\r\n---\r\n") {
            return (Some(&rest[..end]), &rest[end + 7..]);
        }
    }
    (None, raw)
}

fn extract_wikilinks(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = body.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '[' {
            if let Some(&(_, '[')) = chars.peek() {
                // [[ ... ]]
                let start = i + 2;
                let end_rel = body[start..].find("]]");
                if let Some(off) = end_rel {
                    let raw = &body[start..start + off];
                    // Strip alias / heading qualifiers.
                    let target = raw.split('|').next().unwrap_or(raw);
                    let target = target.split('#').next().unwrap_or(target);
                    let target = target.split('^').next().unwrap_or(target);
                    let label = Path::new(target.trim())
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| target.trim().to_string());
                    if !label.is_empty() {
                        out.push(label);
                    }
                    // Skip past the consumed content.
                    while let Some(&(j, _)) = chars.peek() {
                        if j >= start + off + 2 {
                            break;
                        }
                        chars.next();
                    }
                }
            }
        }
    }
    out
}

fn extract_hashtags(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in body.split(|c: char| c.is_whitespace() || c == ',' || c == '.') {
        if let Some(tag) = word.strip_prefix('#') {
            let tag = tag
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '-' && c != '_');
            if !tag.is_empty() && tag.chars().next().map_or(false, |c| c.is_alphabetic()) {
                out.push(tag.to_string());
            }
        }
    }
    out
}

fn excerpt(body: &str, max_chars: usize) -> String {
    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_for_search(&compact, max_chars)
}

fn truncate_for_search(body: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in body.chars().take(max_chars) {
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_wikilinks_with_aliases_and_headings() {
        let body =
            "see [[memory]] and [[learning|the learning page]] and [[concepts/memory#section]].";
        let links = extract_wikilinks(body);
        assert_eq!(links, vec!["memory", "learning", "memory"]);
    }

    #[test]
    fn splits_frontmatter() {
        let raw = "---\ntags: [foo]\n---\nbody here\n";
        let (fm, body) = split_frontmatter(raw);
        assert_eq!(fm, Some("tags: [foo]"));
        assert_eq!(body, "body here\n");
    }

    #[test]
    fn excerpt_compacts_whitespace() {
        assert_eq!(excerpt("alpha\n\n beta   gamma", 80), "alpha beta gamma");
    }
}

//! `.cortex/` neural-metadata folder management.
//!
//! Every network the desktop opens is rooted in a user-chosen folder
//! (the knowledge source — e.g. an Obsidian vault, or `tests/classical-
//! knowledge-base`). The neural side of that network — its identity,
//! creation timestamp, and eventually weight checkpoints / embedding
//! caches — lives in a sibling `.cortex/` directory inside that root.
//!
//! Layout:
//!   <root>/
//!     .cortex/
//!       metadata.json   ← identity + provenance (this file)
//!       weights/        ← future: STDP weight snapshots
//!       embeddings/     ← future: vector encoder cache
//!
//! Only `metadata.json` exists today. The folder presence is the
//! discriminator between "this is a Cortex-initialized network" and
//! "this is just a folder of notes". The frontend uses
//! [`inspect_cortex_folder`] on every Open to decide whether to surface
//! "metadata detected" vs ask the user to initialize.
//!
//! Safety / design notes:
//! - Both commands canonicalize the user-supplied path first. The
//!   canonical form is what's persisted into `source_root`, so a
//!   metadata file moved between machines doesn't carry stale
//!   relative-path baggage.
//! - All I/O goes through `tokio::fs`, so a tauri command running on
//!   the IPC executor never blocks the runtime even if the user's
//!   storage stalls.
//! - We never accept a non-directory or non-existent root — callers
//!   pick the folder via the OS dialog, so a missing path is an
//!   error, not "create the parents too".

use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use tokio::fs;
use uuid::Uuid;

const CORTEX_DIR: &str = ".cortex";
const METADATA_FILE: &str = "metadata.json";
const METADATA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CortexMetadata {
    pub version: u32,
    pub id: Uuid,
    pub name: String,
    pub source_kind: String, // "knowledge-graph" | "fresh"
    pub source_root: String,
    pub created_at: String,  // RFC 3339
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CortexFolderInfo {
    pub root: String,
    pub cortex_path: String,
    pub has_cortex: bool,
    pub metadata: Option<CortexMetadata>,
}

fn cortex_dir(root: &Path) -> PathBuf {
    root.join(CORTEX_DIR)
}

fn metadata_path(root: &Path) -> PathBuf {
    cortex_dir(root).join(METADATA_FILE)
}

async fn load_metadata(root: &Path) -> Option<CortexMetadata> {
    let p = metadata_path(root);
    let raw = fs::read_to_string(&p).await.ok()?;
    match serde_json::from_str::<CortexMetadata>(&raw) {
        Ok(m) => Some(m),
        Err(e) => {
            tracing::warn!(path = %p.display(), error = %e, "corrupt cortex metadata");
            None
        }
    }
}

fn rfc3339_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Resolve a user-supplied path to a canonical, existing directory.
/// Centralizes the validation so both commands return identical errors
/// for the same input shape.
async fn resolve_root(path: &str) -> Result<PathBuf, String> {
    let raw = PathBuf::from(path);
    if !raw.is_absolute() {
        return Err(format!(
            "path must be absolute (got {:?}); the OS folder picker returns absolute paths",
            path
        ));
    }
    let canonical = fs::canonicalize(&raw)
        .await
        .map_err(|e| format!("resolving {}: {}", raw.display(), e))?;
    let meta = fs::metadata(&canonical)
        .await
        .map_err(|e| format!("stat {}: {}", canonical.display(), e))?;
    if !meta.is_dir() {
        return Err(format!("not a directory: {}", canonical.display()));
    }
    Ok(canonical)
}

#[tauri::command]
pub async fn inspect_cortex_folder(path: String) -> Result<CortexFolderInfo, String> {
    let root = resolve_root(&path).await?;
    let cortex = cortex_dir(&root);
    let has_cortex = matches!(fs::metadata(&cortex).await, Ok(m) if m.is_dir())
        && matches!(fs::metadata(metadata_path(&root)).await, Ok(m) if m.is_file());
    let metadata = if has_cortex { load_metadata(&root).await } else { None };
    Ok(CortexFolderInfo {
        root: root.to_string_lossy().to_string(),
        cortex_path: cortex.to_string_lossy().to_string(),
        has_cortex,
        metadata,
    })
}

#[tauri::command]
pub async fn init_cortex_folder(
    path: String,
    name: String,
    source_kind: String,
) -> Result<CortexFolderInfo, String> {
    let root = resolve_root(&path).await?;

    // Reject blank names early — the metadata file is human-readable and
    // an empty name makes the network unidentifiable in the UI.
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("network name must be non-empty".into());
    }

    // Whitelist the source_kind values rather than free-form so a future
    // typo on the JS side becomes a 400, not a silent metadata oddity.
    let source_kind = match source_kind.as_str() {
        "knowledge-graph" | "fresh" => source_kind,
        other => return Err(format!("unknown source_kind: {other:?}")),
    };

    let cortex = cortex_dir(&root);
    fs::create_dir_all(&cortex)
        .await
        .map_err(|e| format!("creating {}: {}", cortex.display(), e))?;

    // Subdirs reserved for future weight + embedding caches. Cheap to
    // create now so external tooling (e.g. python eval scripts) can
    // assume the layout. Failures here are non-fatal — metadata is the
    // only thing the desktop reads back.
    for sub in ["weights", "embeddings"] {
        let p = cortex.join(sub);
        if let Err(e) = fs::create_dir_all(&p).await {
            tracing::warn!(path = %p.display(), error = %e, "could not create cortex subdir");
        }
    }

    let meta_path = metadata_path(&root);
    let existing = load_metadata(&root).await;
    let now = rfc3339_now();
    let metadata = CortexMetadata {
        version: METADATA_VERSION,
        id: existing.as_ref().map(|m| m.id).unwrap_or_else(Uuid::new_v4),
        name,
        source_kind,
        source_root: root.to_string_lossy().to_string(),
        created_at: existing.map(|m| m.created_at).unwrap_or_else(|| now.clone()),
        updated_at: now,
    };
    let body = serde_json::to_string_pretty(&metadata)
        .map_err(|e| format!("serializing metadata: {e}"))?;

    // Write to a sibling temp file then rename — std::fs::rename is
    // atomic on the same filesystem, so a crash mid-write can never
    // leave a half-baked metadata.json. The metadata file is small
    // (~250 bytes) so cost is negligible.
    let tmp = meta_path.with_extension("json.tmp");
    fs::write(&tmp, body.as_bytes())
        .await
        .map_err(|e| format!("writing {}: {}", tmp.display(), e))?;
    fs::rename(&tmp, &meta_path)
        .await
        .map_err(|e| format!("renaming {} → {}: {}", tmp.display(), meta_path.display(), e))?;
    tracing::info!(path = %meta_path.display(), "wrote cortex metadata");

    Ok(CortexFolderInfo {
        root: root.to_string_lossy().to_string(),
        cortex_path: cortex.to_string_lossy().to_string(),
        has_cortex: true,
        metadata: Some(metadata),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> std::path::PathBuf {
        // Deliberately not pulling in `tempfile` for one test. Use a
        // unique path under the system temp dir; clean up at end.
        let id = uuid::Uuid::new_v4();
        let p = std::env::temp_dir().join(format!("cortex-test-{id}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[tokio::test]
    async fn inspect_reports_missing_when_no_dotcortex() {
        let dir = tempdir();
        let info = inspect_cortex_folder(dir.to_string_lossy().to_string())
            .await
            .expect("inspect");
        assert!(!info.has_cortex);
        assert!(info.metadata.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn init_then_inspect_round_trips() {
        let dir = tempdir();
        let p = dir.to_string_lossy().to_string();
        let after = init_cortex_folder(p.clone(), "Test".into(), "fresh".into())
            .await
            .expect("init");
        assert!(after.has_cortex);
        let m = after.metadata.expect("metadata");
        assert_eq!(m.name, "Test");
        assert_eq!(m.source_kind, "fresh");

        let again = inspect_cortex_folder(p).await.expect("inspect");
        assert!(again.has_cortex);
        assert_eq!(again.metadata.unwrap().id, m.id);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn init_rejects_unknown_source_kind() {
        let dir = tempdir();
        let p = dir.to_string_lossy().to_string();
        let err = init_cortex_folder(p, "Test".into(), "bogus".into()).await;
        assert!(err.is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

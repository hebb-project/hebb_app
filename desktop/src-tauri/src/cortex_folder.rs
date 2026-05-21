//! `.cortex/` neural-metadata folder management.
//!
//! Every network the desktop opens is rooted in a user-chosen folder
//! (the knowledge source — e.g. an Obsidian vault, or `tests/classical-
//! knowledge-base`). The neural side of that network — its identity,
//! creation timestamp, cortex type, and eventually weight checkpoints
//! / embedding caches — lives in a sibling `.cortex/` directory.
//!
//! Layout:
//!   <root>/
//!     .cortex/
//!       metadata.json   ← identity + cortex type + provenance
//!       weights/<type>/ ← future: per-type weight snapshots
//!       embeddings/     ← future: vector encoder cache
//!
//! ## Metadata versions
//!
//! - v1 (M0): `source_kind: "knowledge-graph" | "fresh"` was the
//!   provenance discriminator.
//! - v2 (current): `cortex_type` replaces `source_kind` with a richer
//!   taxonomy that lines up with `core::CortexType` (kebab-case strings
//!   `"knowledge-graph"`, `"lif"`, `"hh"`) plus an optional `hh_config`
//!   field carrying the HK1952 parameters for `"hh"` networks.
//!
//! `load_metadata` auto-migrates v1 → v2 on first read and rewrites
//! the file in place. Old networks just keep working.
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
use cortex_snn::format::topology::{NeuronSpec, SynapseSpec, TopologyDefaults};
use cortex_snn::{Cortex, CreateOptions};
use serde::{Deserialize, Serialize};
use tokio::fs;
use uuid::Uuid;

const CORTEX_DIR: &str = ".cortex";
const METADATA_FILE: &str = "metadata.json";
const METADATA_VERSION: u32 = 2;

/// Persisted-on-disk shape of `.cortex/metadata.json`. New fields are
/// added with `#[serde(default)]` so older files (v1) deserialize
/// without crashing, then [`migrate_to_current`] fills in the gaps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CortexMetadata {
    pub version: u32,
    pub id: Uuid,
    pub name: String,
    /// User-facing cortex type. Kebab-case: `"knowledge-graph"` /
    /// `"lif"` / `"hh"`. Matches `core::CortexType`'s `slug()`.
    /// Empty in v1 files on disk; the loader fills it in.
    #[serde(default)]
    pub cortex_type: String,
    /// HK1952 / integrator config when `cortex_type == "hh"`. Opaque
    /// JSON so the desktop doesn't need to depend on `cortex-snn`'s
    /// `HhConfig` struct; `core` parses this when it gets posted to
    /// `/api/cortex`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hh_config: Option<serde_json::Value>,
    /// v1 legacy field. Retained on read for migration; new writes
    /// omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
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

/// Whitelist of accepted cortex-type slugs. Mirrors the variants of
/// `core::CortexType`. Kept as a free function so the validation can
/// be called from both the init command and the v1→v2 migration.
fn is_known_cortex_type(s: &str) -> bool {
    matches!(s, "knowledge-graph" | "lif" | "hh")
}

/// Map a v1 `source_kind` value to its v2 `cortex_type` equivalent.
/// `"fresh"` was the M0 name for what's now `"lif"`; `"knowledge-graph"`
/// carried over unchanged.
fn source_kind_to_cortex_type(source_kind: &str) -> &'static str {
    match source_kind {
        "knowledge-graph" => "knowledge-graph",
        "fresh" => "lif",
        // Unknown / corrupted: fall back to LIF so the user still gets
        // a working network rather than a hard failure. The warning is
        // already emitted by the caller.
        _ => "lif",
    }
}

/// Bring a freshly-parsed `CortexMetadata` up to the current version.
/// Returns `true` if anything changed, signaling that the file on disk
/// should be rewritten.
fn migrate_to_current(m: &mut CortexMetadata) -> bool {
    let mut changed = false;

    // v1 → v2: derive cortex_type from legacy source_kind.
    if m.cortex_type.is_empty() {
        let derived = m
            .source_kind
            .as_deref()
            .map(source_kind_to_cortex_type)
            .unwrap_or("lif");
        m.cortex_type = derived.to_string();
        changed = true;
    }

    // Drop legacy field on rewrite — keeps new files clean.
    if m.source_kind.is_some() {
        m.source_kind = None;
        changed = true;
    }

    if m.version < METADATA_VERSION {
        m.version = METADATA_VERSION;
        changed = true;
    }

    changed
}

async fn load_metadata(root: &Path) -> Option<CortexMetadata> {
    let p = metadata_path(root);
    let raw = fs::read_to_string(&p).await.ok()?;
    let mut m: CortexMetadata = match serde_json::from_str(&raw) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(path = %p.display(), error = %e, "corrupt cortex metadata");
            return None;
        }
    };

    if migrate_to_current(&mut m) {
        tracing::info!(
            path = %p.display(),
            version = m.version,
            cortex_type = %m.cortex_type,
            "migrated cortex metadata to current version"
        );
        // Best-effort rewrite — if the disk is read-only or the user
        // moved the file mid-migration, log and continue with the
        // in-memory v2. Inspect must not fail just because rewrite did.
        if let Err(e) = write_metadata(&p, &m).await {
            tracing::warn!(path = %p.display(), error = %e, "failed to persist migrated metadata");
        }
    }
    Some(m)
}

/// Atomic-write helper: serialize → temp file → rename. Same shape as
/// the original inline `init_cortex_folder` code; pulled out so the
/// migration path can reuse it.
async fn write_metadata(meta_path: &Path, m: &CortexMetadata) -> Result<(), String> {
    let body = serde_json::to_string_pretty(m)
        .map_err(|e| format!("serializing metadata: {e}"))?;
    let tmp = meta_path.with_extension("json.tmp");
    fs::write(&tmp, body.as_bytes())
        .await
        .map_err(|e| format!("writing {}: {}", tmp.display(), e))?;
    fs::rename(&tmp, meta_path)
        .await
        .map_err(|e| format!("renaming {} → {}: {}", tmp.display(), meta_path.display(), e))?;
    Ok(())
}

fn rfc3339_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn defaults_for_cortex_type(
    cortex_type: &str,
    hh_config: Option<serde_json::Value>,
) -> Result<TopologyDefaults, String> {
    let neuron = match cortex_type {
        "knowledge-graph" | "lif" => NeuronSpec::lif(),
        "hh" => NeuronSpec::hh(hh_config),
        other => return Err(format!("unknown cortex_type: {other:?}")),
    };
    Ok(TopologyDefaults {
        neuron,
        synapse: SynapseSpec::stdp(),
    })
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
    cortex_type: String,
    hh_config: Option<serde_json::Value>,
) -> Result<CortexFolderInfo, String> {
    let root = resolve_root(&path).await?;

    // Reject blank names early — the metadata file is human-readable and
    // an empty name makes the network unidentifiable in the UI.
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("network name must be non-empty".into());
    }

    if !is_known_cortex_type(&cortex_type) {
        return Err(format!("unknown cortex_type: {cortex_type:?}"));
    }
    // hh_config only makes sense for HH cortexes. Reject if it was
    // attached to a non-HH type so the file stays self-consistent.
    if hh_config.is_some() && cortex_type != "hh" {
        return Err("hh_config is only valid when cortex_type == \"hh\"".into());
    }

    let cortex = cortex_dir(&root);
    let meta_path = metadata_path(&root);
    let now = rfc3339_now();

    if !matches!(fs::metadata(&meta_path).await, Ok(m) if m.is_file()) {
        let defaults = defaults_for_cortex_type(&cortex_type, hh_config.clone())?;
        let create = CreateOptions {
            name: name.clone(),
            cortex_type: cortex_type.clone(),
            source_root: root.to_string_lossy().to_string(),
            now_rfc3339: now.clone(),
            defaults,
            hh_config: hh_config.clone(),
        };
        Cortex::create(&cortex, create)
            .map_err(|e| format!("creating cortex folder at {}: {}", cortex.display(), e))?;
    } else {
        fs::create_dir_all(&cortex)
            .await
            .map_err(|e| format!("creating {}: {}", cortex.display(), e))?;

        let existing = load_metadata(&root).await;
        let metadata = CortexMetadata {
            version: METADATA_VERSION,
            id: existing.as_ref().map(|m| m.id).unwrap_or_else(Uuid::new_v4),
            name,
            cortex_type: cortex_type.clone(),
            hh_config: hh_config.clone(),
            source_kind: None,
            source_root: root.to_string_lossy().to_string(),
            created_at: existing.map(|m| m.created_at).unwrap_or_else(|| now.clone()),
            updated_at: now.clone(),
        };
        write_metadata(&meta_path, &metadata).await?;
        tracing::info!(path = %meta_path.display(), cortex_type = %metadata.cortex_type, "wrote cortex metadata");
    }

    // Subdirs reserved for future weight + embedding caches. The
    // per-type weights/{slug}/ subdir is created eagerly so external
    // tooling can assume the layout exists from day one.
    for sub in ["weights", "embeddings"] {
        let p = cortex.join(sub);
        if let Err(e) = fs::create_dir_all(&p).await {
            tracing::warn!(path = %p.display(), error = %e, "could not create cortex subdir");
        }
    }
    let typed_weights = cortex.join("weights").join(&cortex_type);
    if let Err(e) = fs::create_dir_all(&typed_weights).await {
        tracing::warn!(path = %typed_weights.display(), error = %e, "could not create typed weights subdir");
    }

    Ok(CortexFolderInfo {
        root: root.to_string_lossy().to_string(),
        cortex_path: cortex.to_string_lossy().to_string(),
        has_cortex: true,
        metadata: load_metadata(&root).await,
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
    async fn init_then_inspect_round_trips_lif() {
        let dir = tempdir();
        let p = dir.to_string_lossy().to_string();
        let after = init_cortex_folder(p.clone(), "Test".into(), "lif".into(), None)
            .await
            .expect("init");
        assert!(after.has_cortex);
        let m = after.metadata.expect("metadata");
        assert_eq!(m.name, "Test");
        assert_eq!(m.cortex_type, "lif");
        assert_eq!(m.version, 2);
        assert!(m.source_kind.is_none());
        assert!(
            dir.join(".cortex").join("topology.json").is_file(),
            "fresh init should create topology.json"
        );

        let again = inspect_cortex_folder(p).await.expect("inspect");
        assert!(again.has_cortex);
        assert_eq!(again.metadata.unwrap().id, m.id);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn init_persists_hh_config() {
        let dir = tempdir();
        let p = dir.to_string_lossy().to_string();
        let cfg = serde_json::json!({ "integrator": "rk4" });
        let after = init_cortex_folder(p, "HH".into(), "hh".into(), Some(cfg.clone()))
            .await
            .expect("init");
        let m = after.metadata.expect("metadata");
        assert_eq!(m.cortex_type, "hh");
        assert_eq!(m.hh_config.as_ref().unwrap()["integrator"], "rk4");

        // weights/hh/ should have been created so downstream tooling
        // can drop checkpoints into a predictable path.
        let weights_hh = std::path::Path::new(&after.cortex_path).join("weights").join("hh");
        assert!(weights_hh.is_dir(), "weights/hh/ should exist");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn init_rejects_unknown_cortex_type() {
        let dir = tempdir();
        let p = dir.to_string_lossy().to_string();
        let err = init_cortex_folder(p, "Test".into(), "bogus".into(), None).await;
        assert!(err.is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn init_rejects_hh_config_on_non_hh_type() {
        let dir = tempdir();
        let p = dir.to_string_lossy().to_string();
        let cfg = Some(serde_json::json!({ "integrator": "rk4" }));
        let err = init_cortex_folder(p, "Test".into(), "lif".into(), cfg).await;
        assert!(err.is_err(), "hh_config on LIF should be rejected");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Hand-write a v1 metadata file (`source_kind: "fresh"`, version: 1)
    /// and prove `inspect` reads it as v2 with `cortex_type: "lif"`, and
    /// that the file on disk gets rewritten in place.
    #[tokio::test]
    async fn inspect_auto_migrates_v1_metadata() {
        let dir = tempdir();
        let cortex = dir.join(".cortex");
        std::fs::create_dir_all(&cortex).unwrap();
        let path = cortex.join("metadata.json");
        let v1 = serde_json::json!({
            "version": 1,
            "id": uuid::Uuid::new_v4(),
            "name": "Legacy",
            "source_kind": "knowledge-graph",
            "source_root": dir.to_string_lossy(),
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
        });
        std::fs::write(&path, serde_json::to_string_pretty(&v1).unwrap()).unwrap();

        let info = inspect_cortex_folder(dir.to_string_lossy().to_string())
            .await
            .expect("inspect");
        let m = info.metadata.expect("metadata");
        assert_eq!(m.version, 2);
        assert_eq!(m.cortex_type, "knowledge-graph");
        assert!(m.source_kind.is_none(), "legacy field should be dropped on rewrite");

        // File on disk should now be v2 too — re-read and re-parse.
        let raw = std::fs::read_to_string(&path).unwrap();
        let on_disk: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(on_disk["version"], 2);
        assert_eq!(on_disk["cortex_type"], "knowledge-graph");
        assert!(on_disk.get("source_kind").is_none());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn source_kind_mapping_covers_known_cases() {
        assert_eq!(source_kind_to_cortex_type("fresh"), "lif");
        assert_eq!(source_kind_to_cortex_type("knowledge-graph"), "knowledge-graph");
        assert_eq!(source_kind_to_cortex_type("garbage"), "lif");
    }
}

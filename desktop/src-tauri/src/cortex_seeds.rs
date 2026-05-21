//! Seed-network Tauri command.
//!
//! `seed_cortex_folder` populates a freshly-initialized `.cortex/`
//! folder's `topology.json` with a generated network so the user
//! lands in the visualizer with something to look at. Runs locally in
//! the desktop process — we don't need core to be up yet, which keeps
//! the start-screen flow free of ordering hazards.
//!
//! Flow:
//!   1. Resolve + canonicalize the root (shared with `cortex_folder`).
//!   2. `Cortex::open(root)` — succeeds even if `topology.json` is
//!      absent (`open` synthesizes an empty one from metadata).
//!   3. Reject KG cortexes — their topology comes from the source
//!      folder's ingestion, not a seed generator.
//!   4. Dispatch on `SeedSpec` to the matching `cortex_snn::seeds`
//!      generator, then `cortex.apply_seed(seed)`, which atomically
//!      writes `topology.json` (write-temp + fsync + rename).
//!   5. Return `SeedReport { added_nodes, added_edges }`.
//!
//! Errors are returned as `String` (the Tauri convention) — the
//! substrate's `DiskError` / `SeedError` `Display` impls already
//! produce stable, machine-parseable text.

use std::path::PathBuf;

use cortex_snn::seeds::{self, SeedParams};
use cortex_snn::{Cortex, SeedReport};
use serde::{Deserialize, Serialize};
use tokio::fs;

/// Mirrors `cortex_snn::seeds::SeedParams` minus the kind overrides
/// — the UI doesn't need to pick a heterogeneous neuron mix yet, so
/// `neuron_kind` / `synapse_kind` are left at `None` and the cortex
/// type's default applies. `seed` is exposed so the same UI selection
/// reproduces bit-for-bit on retry.
#[derive(Debug, Clone, Deserialize)]
pub struct SeedConfig {
    /// `[low, high]` for uniformly-sampled init weights. Both in
    /// `[0.0, 1.0]`; `low <= high`. Defaults to `[0.4, 0.6]` — same
    /// default the substrate uses.
    #[serde(default = "default_weight_range")]
    pub weight_range: (f32, f32),
    /// Synaptic conduction delay in ms. Default 1.0.
    #[serde(default = "default_delay_ms")]
    pub delay_ms: f32,
    /// PRNG seed. Determinism is the point — same value → same network.
    #[serde(default)]
    pub seed: u64,
}

fn default_weight_range() -> (f32, f32) {
    (0.4, 0.6)
}
fn default_delay_ms() -> f32 {
    1.0
}

impl Default for SeedConfig {
    fn default() -> Self {
        Self {
            weight_range: default_weight_range(),
            delay_ms: default_delay_ms(),
            seed: 0,
        }
    }
}

impl From<SeedConfig> for SeedParams {
    fn from(c: SeedConfig) -> Self {
        SeedParams {
            weight_range: c.weight_range,
            delay_ms: c.delay_ms,
            neuron_kind: None,
            synapse_kind: None,
        }
    }
}

/// Tagged enum matching the four generators in `cortex_snn::seeds`.
/// JSON shape from the web side:
///   `{ "kind": "random", "n": 32, "p": 0.05, "config": { ... } }`
///   `{ "kind": "ring", "n": 32, "k": 3, "config": { ... } }`
///   `{ "kind": "small_world", "n": 32, "k": 3, "p_rewire": 0.2, "config": { ... } }`
///   `{ "kind": "layered", "layers": [8, 16, 8], "config": { ... } }`
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SeedSpec {
    Random {
        n: usize,
        p: f32,
        #[serde(default)]
        config: SeedConfig,
    },
    Ring {
        n: usize,
        k: usize,
        #[serde(default)]
        config: SeedConfig,
    },
    SmallWorld {
        n: usize,
        k: usize,
        p_rewire: f32,
        #[serde(default)]
        config: SeedConfig,
    },
    Layered {
        layers: Vec<usize>,
        #[serde(default)]
        config: SeedConfig,
    },
}

/// Tauri-facing summary returned to the web layer. Mirrors
/// `cortex_snn::SeedReport` but is a local type so the surface stays
/// stable if the substrate adds fields later.
#[derive(Debug, Clone, Serialize)]
pub struct SeedSummary {
    pub added_nodes: usize,
    pub added_edges: usize,
    pub cortex_type: String,
}

async fn resolve_root(path: &str) -> Result<PathBuf, String> {
    let raw = PathBuf::from(path);
    if !raw.is_absolute() {
        return Err(format!("path must be absolute (got {path:?})"));
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
pub async fn seed_cortex_folder(path: String, spec: SeedSpec) -> Result<SeedSummary, String> {
    let root = resolve_root(&path).await?;

    // The web layer passes the *source* folder (the one the user
    // picked in the OS dialog). The cortex metadata lives in the
    // `.cortex/` subdirectory; `cortex_snn::Cortex` treats *that*
    // subdir as its root.
    let cortex_root = root.join(".cortex");
    if !cortex_root.is_dir() {
        return Err(format!(
            ".cortex/ subdir missing under {} — call init_cortex_folder first",
            root.display()
        ));
    }

    // `Cortex::open` does its own validating reads (size guard, JSON
    // depth check, referential integrity). If `topology.json` doesn't
    // exist yet — which is the common case for a folder that's only
    // had `init_cortex_folder` called against it — `open` synthesizes
    // an empty one in memory so `apply_seed` can layer on top.
    //
    // `apply_seed` rolls back the in-memory topology on validation
    // failure, so a bad seed leaves disk untouched.
    let mut cortex = tokio::task::spawn_blocking(move || Cortex::open(&cortex_root))
        .await
        .map_err(|e| format!("join error: {e}"))?
        .map_err(|e| format!("opening cortex folder: {e}"))?;

    let cortex_type = cortex.cortex_type().to_string();
    if cortex_type == "knowledge-graph" {
        return Err("knowledge-graph cortexes ingest their topology from the source folder; seeding is not supported".into());
    }

    let report: SeedReport = tokio::task::spawn_blocking(move || -> Result<SeedReport, String> {
        let seed = build_seed(&spec).map_err(|e| format!("seed generation: {e}"))?;
        cortex
            .apply_seed(seed)
            .map_err(|e| format!("apply seed: {e}"))
    })
    .await
    .map_err(|e| format!("join error: {e}"))??;

    tracing::info!(
        added_nodes = report.added_nodes,
        added_edges = report.added_edges,
        cortex_type = %cortex_type,
        "applied seed to cortex folder"
    );

    Ok(SeedSummary {
        added_nodes: report.added_nodes,
        added_edges: report.added_edges,
        cortex_type,
    })
}

fn build_seed(spec: &SeedSpec) -> Result<seeds::Seed, seeds::SeedError> {
    match spec.clone() {
        SeedSpec::Random { n, p, config } => seeds::random(n, p, config.seed, config.into()),
        SeedSpec::Ring { n, k, config } => seeds::ring(n, k, config.seed, config.into()),
        SeedSpec::SmallWorld { n, k, p_rewire, config } => {
            seeds::small_world(n, k, p_rewire, config.seed, config.into())
        }
        SeedSpec::Layered { layers, config } => seeds::layered(&layers, config.seed, config.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cortex_folder::{init_cortex_folder, inspect_cortex_folder};

    fn tempdir() -> std::path::PathBuf {
        let id = uuid::Uuid::new_v4();
        let p = std::env::temp_dir().join(format!("cortex-seed-test-{id}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    async fn init_lif(p: &str) {
        init_cortex_folder(p.to_string(), "Test".into(), "lif".into(), None)
            .await
            .expect("init");
    }

    #[tokio::test]
    async fn seed_random_round_trips() {
        let dir = tempdir();
        let p = dir.to_string_lossy().to_string();
        init_lif(&p).await;

        let summary = seed_cortex_folder(
            p.clone(),
            SeedSpec::Random {
                n: 8,
                p: 0.5,
                config: SeedConfig { seed: 7, ..Default::default() },
            },
        )
        .await
        .expect("seed");
        assert_eq!(summary.added_nodes, 8);
        assert!(summary.added_edges > 0, "p=0.5 on 8 nodes should yield edges");
        assert_eq!(summary.cortex_type, "lif");

        // topology.json must now exist and round-trip the node count.
        // Re-canonicalize since `init_cortex_folder` resolves to /private/... on macOS.
        let canonical = std::fs::canonicalize(&dir).unwrap();
        let topology_path = canonical.join(".cortex").join("topology.json");
        let raw = std::fs::read_to_string(&topology_path).expect("topology written");
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["nodes"].as_array().unwrap().len(), 8);

        // inspect_cortex_folder still works (no metadata corruption).
        let info = inspect_cortex_folder(p).await.expect("inspect");
        assert!(info.has_cortex);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn seed_ring_deterministic() {
        let dir1 = tempdir();
        let dir2 = tempdir();
        let p1 = dir1.to_string_lossy().to_string();
        let p2 = dir2.to_string_lossy().to_string();
        init_lif(&p1).await;
        init_lif(&p2).await;

        let spec = || SeedSpec::Ring {
            n: 12,
            k: 2,
            config: SeedConfig { seed: 42, ..Default::default() },
        };
        let a = seed_cortex_folder(p1, spec()).await.unwrap();
        let b = seed_cortex_folder(p2, spec()).await.unwrap();
        // Ring emits exactly 2k edges per neuron, both directions.
        assert_eq!(a.added_edges, 12 * 2 * 2);
        assert_eq!(a.added_nodes, b.added_nodes);
        assert_eq!(a.added_edges, b.added_edges);

        std::fs::remove_dir_all(&dir1).unwrap();
        std::fs::remove_dir_all(&dir2).unwrap();
    }

    #[tokio::test]
    async fn seed_layered_dimensions() {
        let dir = tempdir();
        let p = dir.to_string_lossy().to_string();
        init_lif(&p).await;

        let summary = seed_cortex_folder(
            p,
            SeedSpec::Layered {
                layers: vec![2, 3, 1],
                config: SeedConfig::default(),
            },
        )
        .await
        .expect("seed");
        assert_eq!(summary.added_nodes, 6);
        // 2*3 + 3*1 = 9
        assert_eq!(summary.added_edges, 9);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn seed_rejects_knowledge_graph() {
        let dir = tempdir();
        let p = dir.to_string_lossy().to_string();
        init_cortex_folder(p.clone(), "KG".into(), "knowledge-graph".into(), None)
            .await
            .expect("init");

        let err = seed_cortex_folder(
            p,
            SeedSpec::Random {
                n: 4,
                p: 0.5,
                config: SeedConfig::default(),
            },
        )
        .await
        .expect_err("kg should be rejected");
        assert!(err.contains("knowledge-graph"), "unexpected error: {err}");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn seed_rejects_bad_params() {
        let dir = tempdir();
        let p = dir.to_string_lossy().to_string();
        init_lif(&p).await;

        let err = seed_cortex_folder(
            p.clone(),
            SeedSpec::Random {
                n: 4,
                p: 1.5, // out of [0,1]
                config: SeedConfig::default(),
            },
        )
        .await
        .expect_err("bad p must be rejected");
        assert!(err.contains("seed") || err.contains("probability"));

        // topology.json must NOT have been written (apply_seed rolls back
        // on validation failure, and a generator-level rejection happens
        // before apply_seed at all).
        let canonical = std::fs::canonicalize(&dir).unwrap();
        let topology_path = canonical.join(".cortex").join("topology.json");
        assert!(!topology_path.exists(), "topology.json should not exist after rejected seed");

        std::fs::remove_dir_all(&dir).unwrap();
    }
}

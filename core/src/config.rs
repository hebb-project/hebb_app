//! Process-wide configuration loaded once from environment.
//!
//! Loading order: `.env` (best-effort) → process env → defaults baked here.

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct CoreConfig {
    pub database_url: String,

    #[serde(default = "default_model_store_path")]
    pub model_store_path: PathBuf,

    #[serde(default = "default_ws_host")]
    pub ws_host: String,

    #[serde(default = "default_ws_port")]
    pub ws_port: u16,

    #[serde(default = "default_vault_path")]
    pub vault_path: PathBuf,

    #[serde(default = "default_cors_allow_origin")]
    pub cors_allow_origin: String,

    /// Simulation tick rate in Hz. 200Hz → dt = 5ms.
    #[serde(default = "default_tick_hz")]
    pub tick_hz: u32,

    /// Spike persister timer-driven flush cadence in milliseconds.
    #[serde(default = "default_spike_persist_interval_ms")]
    pub spike_persist_interval_ms: u64,

    /// Spike persister buffer size that forces an early flush before
    /// the timer fires (whichever hits first). Bounds buffer growth
    /// during high-spike-rate stimulation.
    #[serde(default = "default_spike_persist_max_batch")]
    pub spike_persist_max_batch: usize,

    /// Weight persister flush cadence in milliseconds. Trades
    /// fresh-on-disk for write volume.
    #[serde(default = "default_weight_persist_interval_ms")]
    pub weight_persist_interval_ms: u64,

    /// Minimum absolute weight change since the last persisted value
    /// required to issue an UPDATE. Filters STDP trace noise on stable
    /// edges so we don't write every tick.
    #[serde(default = "default_weight_persist_epsilon")]
    pub weight_persist_epsilon: f32,
}

fn default_model_store_path() -> PathBuf { PathBuf::from("./data/models") }
fn default_ws_host() -> String { "127.0.0.1".to_string() }
// Must match DEFAULT_BIND in desktop/src-tauri/src/supervisor/core.rs and DEFAULT_HTTP in web/lib/cortex-api.ts.
fn default_ws_port() -> u16 { 7654 }
fn default_vault_path() -> PathBuf { PathBuf::from("./tests/mock-knowledge-base") }
fn default_cors_allow_origin() -> String { "*".to_string() }
fn default_tick_hz() -> u32 { 200 }
fn default_spike_persist_interval_ms() -> u64 { 250 }
fn default_spike_persist_max_batch() -> usize { 2_000 }
fn default_weight_persist_interval_ms() -> u64 { 7_000 }
fn default_weight_persist_epsilon() -> f32 { 0.001 }

impl CoreConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let _ = dotenvy::dotenv();
        let cfg = envy::from_env::<Self>()?;
        Ok(cfg)
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.ws_host, self.ws_port)
    }

    pub fn dt_ms(&self) -> f32 {
        1000.0 / self.tick_hz as f32
    }
}

//! Embedded Postgres provider (stub).
//!
//! The follow-up commit on the COR-33 PR fills this in using the
//! `postgresql_embedded` crate. The crate downloads PG 17 binaries to
//! a cache dir on first run and exposes start/stop + a `settings()`
//! struct that yields the connection URL.
//!
//! Why a stub here: the trait shape + enum dispatch is the contract
//! the supervisor depends on. Wiring the crate involves a Cargo deps
//! decision (default features pull in archives for the host arch
//! only, which matters for Tauri's per-platform CI) — better to ship
//! the contract separately so reviewers can ratify the shape before
//! the implementation choice is auditable.

use super::PostgresHandle;

#[derive(Debug, Clone, Default)]
pub struct EmbeddedPostgres {
    /// Where the cached PG binaries + data dir live. `None` =
    /// platform-specific app-data dir, resolved at `start()` time.
    pub data_dir: Option<std::path::PathBuf>,
}

impl EmbeddedPostgres {
    pub async fn start(&self) -> anyhow::Result<PostgresHandle> {
        anyhow::bail!(
            "EmbeddedPostgres::start not implemented yet — see ADR-002 + follow-up commit on COR-33 PR"
        )
    }

    pub async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

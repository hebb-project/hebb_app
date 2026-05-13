//! Embedded Postgres provider.
//!
//! Backed by the `postgresql_embedded` crate. On first run the crate
//! downloads PG binaries into `~/.theseus/postgresql/<version>/` (or
//! the OS-equivalent cache dir) and unpacks them; subsequent runs are
//! near-instant. We don't override the cache location yet — the crate's
//! default is appropriate for a single-user desktop install. The data
//! directory is colocated with the binary cache by default.
//!
//! State note: the `postgresql_embedded::PostgreSQL` value must stay
//! alive for the server to remain up — drop = stop. We store it
//! behind an `Arc<tokio::sync::Mutex<Option<...>>>` so the provider's
//! `start`/`stop` can take `&self` (matches the enum dispatch in
//! `PostgresProvider`) while preserving exclusive access for the
//! lifecycle calls themselves.
//!
//! The database name is fixed at `cortex_dev` for now. A future
//! commit will read this (and the version pin, user/password) from
//! the desktop config so power users can override.

use std::sync::Arc;

use postgresql_embedded::{PostgreSQL, Settings};
use tokio::sync::Mutex;

use super::PostgresHandle;

const DATABASE_NAME: &str = "cortex_dev";

#[derive(Default)]
pub struct EmbeddedPostgres {
    /// Lazily-initialized. `None` until `start()` first runs.
    inner: Arc<Mutex<Option<PostgreSQL>>>,
}

impl EmbeddedPostgres {
    pub async fn start(&self) -> anyhow::Result<PostgresHandle> {
        let mut guard = self.inner.lock().await;

        if guard.is_none() {
            tracing::info!("embedded postgres: setting up (may download on first run)");
            // Default settings: latest stable PG, ephemeral data dir
            // under the crate's cache, random ephemeral port. We pin
            // user/password so the URL we hand the core is stable
            // across restarts even if the port shifts.
            let settings = Settings {
                username: "postgres".into(),
                password: "postgres".into(),
                ..Default::default()
            };
            let mut pg = PostgreSQL::new(settings);
            pg.setup()
                .await
                .map_err(|e| anyhow::anyhow!("embedded postgres setup: {e}"))?;
            pg.start()
                .await
                .map_err(|e| anyhow::anyhow!("embedded postgres start: {e}"))?;

            // Idempotent: create_database errors if the DB already
            // exists, so check first. Postgres-embedded preserves the
            // data dir across restarts, so on the second boot the DB
            // is already there.
            let exists = pg
                .database_exists(DATABASE_NAME)
                .await
                .map_err(|e| anyhow::anyhow!("embedded postgres database_exists: {e}"))?;
            if !exists {
                pg.create_database(DATABASE_NAME)
                    .await
                    .map_err(|e| anyhow::anyhow!("embedded postgres create_database: {e}"))?;
                tracing::info!(database = DATABASE_NAME, "embedded postgres: database created");
            }

            tracing::info!(
                port = pg.settings().port,
                "embedded postgres: started"
            );
            *guard = Some(pg);
        }

        let pg = guard.as_ref().expect("just set");
        let url = pg.settings().url(DATABASE_NAME);
        Ok(PostgresHandle {
            url,
            provider: "embedded",
        })
    }

    pub async fn stop(&self) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().await;
        if let Some(pg) = guard.as_mut() {
            pg.stop()
                .await
                .map_err(|e| anyhow::anyhow!("embedded postgres stop: {e}"))?;
            tracing::info!("embedded postgres: stopped");
        }
        // Drop the PostgreSQL value so Drop's process kill runs if the
        // remote stop call didn't fully tear down.
        *guard = None;
        Ok(())
    }
}

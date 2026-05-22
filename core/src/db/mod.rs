//! Database layer: r2d2 connection pool over sync diesel, plus a
//! `run_blocking` helper so axum handlers can await DB work without
//! manually shuttling closures into `spawn_blocking` everywhere.

pub mod models;
pub mod schema;

use diesel::pg::PgConnection;
use diesel::r2d2::{ConnectionManager, Pool, PooledConnection};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};

use crate::error::{CoreError, CoreResult};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("./migrations");

pub type PgPool = Pool<ConnectionManager<PgConnection>>;
pub type PgPooled = PooledConnection<ConnectionManager<PgConnection>>;

pub fn build_pool(database_url: &str) -> anyhow::Result<PgPool> {
    let mgr = ConnectionManager::<PgConnection>::new(database_url);
    let pool = Pool::builder().max_size(16).build(mgr)?;
    Ok(pool)
}

pub fn run_migrations(pool: &PgPool) -> anyhow::Result<()> {
    let mut conn = pool.get()?;
    conn.run_pending_migrations(MIGRATIONS)
        .map_err(|e| anyhow::anyhow!("migration error: {e}"))?;
    Ok(())
}

/// Run a synchronous diesel closure off the async runtime.
///
/// The closure receives an already-checked-out pooled connection. Any
/// `CoreError` it returns is propagated; panics are surfaced as
/// `CoreError::Other`.
pub async fn run_blocking<F, R>(pool: &PgPool, f: F) -> CoreResult<R>
where
    F: FnOnce(&mut PgPooled) -> CoreResult<R> + Send + 'static,
    R: Send + 'static,
{
    let pool = pool.clone();
    tokio::task::spawn_blocking(move || {
        let mut conn = pool.get().map_err(CoreError::from)?;
        f(&mut conn)
    })
    .await
    .map_err(|e| CoreError::Other(anyhow::anyhow!("blocking task panicked: {e}")))?
}

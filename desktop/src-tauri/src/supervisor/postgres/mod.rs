//! Pluggable Postgres provider.
//!
//! The supervisor depends on this enum, not a concrete embedded type.
//! See `vault/ideas/adr-002-postgres-provider.md` for rationale and
//! the production-path argument.
//!
//! Today two variants exist; both implementations are scaffold stubs.
//! The concrete logic lands in follow-up commits on the COR-33 PR.

pub mod embedded;
pub mod external;

use serde::Serialize;

pub use embedded::EmbeddedPostgres;
pub use external::ExternalPostgres;

/// What every provider yields once Postgres is ready. The URL is what
/// the [`core`] launcher receives as `DATABASE_URL`; the `provider`
/// tag flows through to the diagnostics pane verbatim.
#[derive(Debug, Clone, Serialize)]
pub struct PostgresHandle {
    pub url: String,
    pub provider: &'static str,
}

/// Pluggable provider. The set is closed and selected at startup;
/// adding a variant (e.g. `ManagedCloud` for multi-tenant prod) forces
/// every match arm to be updated, which is the right property for a
/// production-path component that must not be left half-wired.
pub enum PostgresProvider {
    Embedded(EmbeddedPostgres),
    External(ExternalPostgres),
}

impl PostgresProvider {
    /// Resolve the provider from config. Today: `CORTEX_PG_URL` env
    /// switches to External; absence selects Embedded. The full config
    /// schema lands when the launchers do.
    pub fn from_env() -> Self {
        match std::env::var("CORTEX_PG_URL") {
            Ok(url) if !url.trim().is_empty() => Self::External(ExternalPostgres::new(url)),
            _ => Self::Embedded(EmbeddedPostgres::default()),
        }
    }

    pub async fn start(&self) -> anyhow::Result<PostgresHandle> {
        match self {
            Self::Embedded(p) => p.start().await,
            Self::External(p) => p.start().await,
        }
    }

    pub async fn stop(&self) -> anyhow::Result<()> {
        match self {
            Self::Embedded(p) => p.stop().await,
            Self::External(p) => p.stop().await,
        }
    }
}

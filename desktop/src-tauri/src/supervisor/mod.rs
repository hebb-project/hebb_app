//! Managed-subprocess supervisor.
//!
//! Owns the lifecycle of the auxiliary processes the desktop app needs
//! to run: Postgres, the Rust `core` server, and (optionally) the
//! Python bridge. Each is wrapped in a [`ManagedProcess`] with its own
//! restart policy; the [`Supervisor`] orchestrates startup order,
//! status reporting, and graceful shutdown.
//!
//! ## Status
//!
//! This module currently lands the **abstraction** only (COR-33
//! scaffold). The concrete launchers (`postgres.rs`, `core.rs`,
//! `bridge.rs`) are intentionally deferred to follow-up commits on
//! the same PR so that the abstraction can be reviewed before any
//! decision about the Postgres bundling strategy (embedded vs sidecar
//! binary vs system-installed) lands.
//!
//! ## Threading
//!
//! The supervisor lives in Tauri's `State<>` so IPC commands can
//! query/restart processes. All internal mutation happens behind
//! `tokio::sync::Mutex` / `RwLock`; nothing in here is blocking.

pub mod core;
pub mod postgres;
pub mod process;

use std::sync::Arc;
use tokio::sync::RwLock;

pub use core::{wait_for_health, CoreLauncher};
pub use postgres::{EmbeddedPostgres, ExternalPostgres, PostgresHandle, PostgresProvider};
pub use process::{ManagedProcess, ProcessConfig, ProcessStatus, RestartPolicy, State};

/// Top-level supervisor: a registry of managed subprocesses + an
/// optional Postgres provider.
///
/// Postgres is special: it's a resource provider, not a child the
/// supervisor spawned, so it doesn't fit the `ManagedProcess` shape.
/// It's tracked separately and shut down last (last-stopped invariant
/// — dependents go down before the DB they depend on).
///
/// Construct once at app startup, register subprocesses via
/// [`Supervisor::register`], install the Postgres provider via
/// [`Supervisor::start_postgres`], then call
/// [`Supervisor::shutdown_all`] on the Tauri window-close hook.
#[derive(Default)]
pub struct Supervisor {
    processes: RwLock<Vec<Arc<ManagedProcess>>>,
    postgres: RwLock<Option<PostgresEntry>>,
}

struct PostgresEntry {
    provider: PostgresProvider,
    handle: PostgresHandle,
}

impl Supervisor {
    pub fn new() -> Self {
        Self {
            processes: RwLock::new(Vec::new()),
            postgres: RwLock::new(None),
        }
    }

    /// Register a managed process. The supervisor takes a strong
    /// reference; the caller may keep its own for direct interaction.
    pub async fn register(&self, p: ManagedProcess) -> Arc<ManagedProcess> {
        let arc = Arc::new(p);
        self.processes.write().await.push(arc.clone());
        arc
    }

    /// Start the Postgres provider, register it, and return the
    /// resolved connection handle (typically passed to the core
    /// launcher as `DATABASE_URL`).
    pub async fn start_postgres(
        &self,
        provider: PostgresProvider,
    ) -> anyhow::Result<PostgresHandle> {
        let handle = provider.start().await?;
        *self.postgres.write().await = Some(PostgresEntry {
            provider,
            handle: handle.clone(),
        });
        Ok(handle)
    }

    /// Snapshot every service's status. The Postgres entry, when
    /// present, surfaces as a synthetic `Running { pid: 0 }` — pid 0
    /// signals "no local OS handle" (embedded crate owns it, or it's
    /// external). The frontend can distinguish by name suffix
    /// (`postgres (embedded)` vs `postgres (external)`).
    pub async fn status_all(&self) -> Vec<ProcessStatus> {
        let mut out = Vec::new();
        if let Some(entry) = self.postgres.read().await.as_ref() {
            out.push(ProcessStatus {
                name: format!("postgres ({})", entry.handle.provider),
                state: State::Running { pid: 0 },
            });
        }
        let procs = self.processes.read().await;
        for p in procs.iter() {
            out.push(p.status().await);
        }
        out
    }

    /// Best-effort graceful shutdown. Managed processes stop first in
    /// reverse-registration order (so core stops before Postgres),
    /// then the Postgres provider last.
    pub async fn shutdown_all(&self) {
        let procs = self.processes.read().await;
        for p in procs.iter().rev() {
            if let Err(e) = p.stop().await {
                tracing::warn!(name = %p.name, error = %e, "supervisor: stop failed");
            }
        }
        drop(procs);

        if let Some(entry) = self.postgres.write().await.take() {
            if let Err(e) = entry.provider.stop().await {
                tracing::warn!(error = %e, "supervisor: postgres stop failed");
            }
        }
    }
}

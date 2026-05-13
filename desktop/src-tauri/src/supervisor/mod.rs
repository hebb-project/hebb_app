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

pub mod postgres;
pub mod process;

use std::sync::Arc;
use tokio::sync::RwLock;

pub use postgres::{EmbeddedPostgres, ExternalPostgres, PostgresHandle, PostgresProvider};
pub use process::{ManagedProcess, ProcessConfig, ProcessStatus, RestartPolicy, State};

/// Top-level supervisor: a registry of managed subprocesses.
///
/// Construct once at app startup, register each subprocess via
/// [`Supervisor::register`], then call [`Supervisor::shutdown_all`] on
/// the Tauri window-close hook. Concrete launcher modules (Postgres,
/// core, bridge) build their `ProcessConfig` and hand it off here;
/// they do not own state directly.
#[derive(Default)]
pub struct Supervisor {
    processes: RwLock<Vec<Arc<ManagedProcess>>>,
}

impl Supervisor {
    pub fn new() -> Self {
        Self {
            processes: RwLock::new(Vec::new()),
        }
    }

    /// Register a managed process. The supervisor takes a strong
    /// reference; the caller may also keep one for direct interaction
    /// (e.g. the core launcher needs the Postgres process's resolved
    /// connection string before starting).
    pub async fn register(&self, p: ManagedProcess) -> Arc<ManagedProcess> {
        let arc = Arc::new(p);
        self.processes.write().await.push(arc.clone());
        arc
    }

    /// Snapshot all registered processes' status. Stable shape for IPC.
    pub async fn status_all(&self) -> Vec<ProcessStatus> {
        let lock = self.processes.read().await;
        let mut out = Vec::with_capacity(lock.len());
        for p in lock.iter() {
            out.push(p.status().await);
        }
        out
    }

    /// Best-effort graceful shutdown of every registered process, in
    /// reverse-registration order (so dependents stop before their
    /// dependencies — core before Postgres).
    pub async fn shutdown_all(&self) {
        let lock = self.processes.read().await;
        for p in lock.iter().rev() {
            if let Err(e) = p.stop().await {
                tracing::warn!(name = %p.name, error = %e, "supervisor: stop failed");
            }
        }
    }
}

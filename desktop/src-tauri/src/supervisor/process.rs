//! `ManagedProcess` — a single supervised child process.
//!
//! Wraps `tokio::process::Command`, forwards stdout/stderr through
//! `tracing` (so the existing log infrastructure handles aggregation),
//! tracks lifecycle state behind a mutex, and exposes a stable
//! `ProcessStatus` shape for IPC consumption.
//!
//! Auto-restart on crash is **not** implemented in this scaffold — the
//! state machine has a `Failed` terminal variant but the supervisor
//! currently relies on the user manually restarting via the (future)
//! diagnostics pane. Once we have real telemetry on which subprocess
//! crashes look like, the restart policy will gain backoff + jitter.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use serde::Serialize;
use tokio::process::Command;
use tokio::sync::{oneshot, Mutex};

/// What to do when the child exits.
///
/// Today only `Never` is honored by the supervisor — `OnFailure` and
/// `Always` are reserved for the follow-up that adds the restart loop.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartPolicy {
    Never,
    OnFailure,
    Always,
}

/// Public lifecycle state. Serialized to the frontend as `{ "state":
/// "running", "pid": 1234 }` etc., so the diagnostics UI can render
/// without an additional mapping layer.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum State {
    Pending,
    Running { pid: u32 },
    Stopped { code: Option<i32> },
    Failed { reason: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessStatus {
    pub name: String,
    #[serde(flatten)]
    pub state: State,
}

/// Static spawn configuration for a managed process.
#[derive(Debug, Clone)]
pub struct ProcessConfig {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
    pub restart: RestartPolicy,
}

pub struct ManagedProcess {
    pub name: String,
    cfg: ProcessConfig,
    state: Arc<Mutex<State>>,
    /// One-shot used to ask the exit-waiter task to kill the child.
    /// `None` after the first `stop()` call or after the child exits.
    /// The exit-waiter owns the `Child` for the duration of `wait()`,
    /// so killing it from another task needs an out-of-band signal —
    /// we can't share `&mut Child` across tasks.
    kill_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

impl ManagedProcess {
    pub fn new(cfg: ProcessConfig) -> Self {
        Self {
            name: cfg.name.clone(),
            cfg,
            state: Arc::new(Mutex::new(State::Pending)),
            kill_tx: Arc::new(Mutex::new(None)),
        }
    }

    /// Spawn the child process and start log forwarding. Returns once
    /// the OS handle is acquired; the child runs detached in the
    /// background. A separate task watches for exit and updates state.
    pub async fn spawn(&self) -> anyhow::Result<()> {
        let mut cmd = Command::new(&self.cfg.program);
        cmd.args(&self.cfg.args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (k, v) in &self.cfg.env {
            cmd.env(k, v);
        }
        if let Some(cwd) = &self.cfg.cwd {
            cmd.current_dir(cwd);
        }

        let mut child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!(
                "spawn {}: {e} (program: {})",
                self.cfg.name,
                self.cfg.program
            )
        })?;
        let pid = child.id().unwrap_or(0);

        if let Some(stdout) = child.stdout.take() {
            Self::forward("stdout", self.name.clone(), stdout);
        }
        if let Some(stderr) = child.stderr.take() {
            Self::forward("stderr", self.name.clone(), stderr);
        }

        let (kill_tx, kill_rx) = oneshot::channel::<()>();
        *self.kill_tx.lock().await = Some(kill_tx);
        *self.state.lock().await = State::Running { pid };

        // Exit waiter — owns the Child for the lifetime of the
        // process. Either the child exits on its own, or stop() sends
        // through the oneshot and we kill it.
        let state = self.state.clone();
        let kill_slot = self.kill_tx.clone();
        let name = self.name.clone();
        tokio::spawn(async move {
            let result = tokio::select! {
                biased;
                _ = kill_rx => {
                    let _ = child.kill().await;
                    child.wait().await
                }
                r = child.wait() => r,
            };
            // Drop the sender slot so further stop() calls are no-ops.
            *kill_slot.lock().await = None;
            match result {
                Ok(status) => {
                    let code = status.code();
                    tracing::info!(name = %name, ?code, "managed process exited");
                    *state.lock().await = State::Stopped { code };
                }
                Err(e) => {
                    tracing::error!(name = %name, error = %e, "wait() failed");
                    *state.lock().await = State::Failed {
                        reason: e.to_string(),
                    };
                }
            }
        });

        Ok(())
    }

    /// Best-effort terminate. Signals the exit-waiter to kill the
    /// child; the waiter then transitions state to `Stopped` once
    /// `wait()` returns. Idempotent: a second call after exit is a
    /// no-op because the sender slot is cleared by the waiter.
    pub async fn stop(&self) -> anyhow::Result<()> {
        if let Some(tx) = self.kill_tx.lock().await.take() {
            let _ = tx.send(());
        }
        Ok(())
    }

    pub async fn status(&self) -> ProcessStatus {
        ProcessStatus {
            name: self.name.clone(),
            state: self.state.lock().await.clone(),
        }
    }

    /// Spawn a task that forwards a child output stream into `tracing`,
    /// tagged with the process name + which channel the line came from.
    /// One `info!` per line; the existing `tracing-subscriber` setup
    /// handles formatting and routing.
    fn forward<R>(channel: &'static str, name: String, reader: R)
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
    {
        use tokio::io::{AsyncBufReadExt, BufReader};
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::info!(target: "managed_process", name = %name, channel, "{line}");
            }
        });
    }
}

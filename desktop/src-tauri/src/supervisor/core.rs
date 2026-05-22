//! `core` launcher.
//!
//! Spawns the `cortex-core` axum server with the resolved
//! `DATABASE_URL`, then probes `/api/health` until 200 or timeout.
//! The launcher returns once the server is responding, so callers
//! can treat "Ok(())" as "the WS endpoints are accepting connections."
//!
//! Binary resolution today is dev-mode: look at the env var
//! `CORTEX_CORE_BIN`, else walk to `../../core/target/{release,debug}/core`
//! from the desktop crate. Production sidecar resolution
//! (`tauri.conf.json::bundle.externalBin`) lands with COR-87.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::process::{ManagedProcess, ProcessConfig};

// Must match DEFAULT_HTTP in web/lib/cortex-api.ts and default_ws_port in core/src/config.rs.
const DEFAULT_BIND: &str = "127.0.0.1:7654";
const HEALTH_TIMEOUT: Duration = Duration::from_secs(30);
const HEALTH_INTERVAL: Duration = Duration::from_millis(500);

pub struct CoreLauncher {
    pub binary: PathBuf,
    pub database_url: String,
    pub bind_addr: String,
}

impl CoreLauncher {
    /// Resolve the launcher from the current environment + a provided
    /// `DATABASE_URL` (typically from [`crate::supervisor::postgres::PostgresHandle::url`]).
    pub fn from_env(database_url: String) -> anyhow::Result<Self> {
        let binary = resolve_binary()?;
        let bind_addr = std::env::var("CORTEX_CORE_BIND").unwrap_or_else(|_| DEFAULT_BIND.into());
        Ok(Self {
            binary,
            database_url,
            bind_addr,
        })
    }

    /// Build a `ManagedProcess` ready to register with the supervisor.
    /// The process is not spawned until `process.spawn()` is called.
    pub fn into_process(self) -> (ManagedProcess, String /* bind_addr */) {
        let bind = self.bind_addr.clone();
        let (host, port) = parse_bind(&bind);
        let cfg = ProcessConfig {
            name: "core".into(),
            program: self.binary.to_string_lossy().into_owned(),
            args: Vec::new(),
            env: vec![
                ("DATABASE_URL".into(), self.database_url),
                ("WS_HOST".into(), host),
                ("WS_PORT".into(), port),
                // RUST_LOG can be overridden by the desktop's own env.
                (
                    "RUST_LOG".into(),
                    std::env::var("CORTEX_CORE_LOG").unwrap_or_else(|_| "info,core=debug".into()),
                ),
            ],
            cwd: None,
        };
        (ManagedProcess::new(cfg), bind)
    }
}

/// Probe `http://<bind>/api/health` until 200 OK or the deadline expires.
/// Uses raw HTTP/1.0 over `TcpStream` so we don't pull in a full HTTP
/// client just for one health probe.
pub async fn wait_for_health(bind: &str) -> anyhow::Result<()> {
    let deadline = Instant::now() + HEALTH_TIMEOUT;
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        if Instant::now() >= deadline {
            anyhow::bail!(
                "core /api/health probe timed out after {:?} ({} attempts)",
                HEALTH_TIMEOUT,
                attempt
            );
        }
        if probe_once(bind).await {
            tracing::info!(bind, attempts = attempt, "core: /health responded 200");
            return Ok(());
        }
        tokio::time::sleep(HEALTH_INTERVAL).await;
    }
}

async fn probe_once(bind: &str) -> bool {
    let Ok(mut stream) = TcpStream::connect(bind).await else {
        return false;
    };
    let req = "GET /api/health HTTP/1.0\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    if stream.write_all(req.as_bytes()).await.is_err() {
        return false;
    }
    let mut buf = [0u8; 64];
    let n = match stream.read(&mut buf).await {
        Ok(n) => n,
        Err(_) => return false,
    };
    let head = std::str::from_utf8(&buf[..n]).unwrap_or("");
    head.starts_with("HTTP/1.") && head.contains(" 200 ")
}

fn parse_bind(bind: &str) -> (String, String) {
    if let Some((h, p)) = bind.rsplit_once(':') {
        (h.into(), p.into())
    } else {
        (
            DEFAULT_BIND.split(':').next().unwrap().into(),
            DEFAULT_BIND.rsplit(':').next().unwrap().into(),
        )
    }
}

/// Dev-mode binary resolution. `CORTEX_CORE_BIN` overrides; otherwise
/// look at `../../core/target/release/core` then `debug/core` relative
/// to the desktop crate. Production (bundled sidecar) is COR-87.
fn resolve_binary() -> anyhow::Result<PathBuf> {
    if let Ok(p) = std::env::var("CORTEX_CORE_BIN") {
        let path = PathBuf::from(p);
        if !path.exists() {
            anyhow::bail!(
                "CORTEX_CORE_BIN set but path does not exist: {}",
                path.display()
            );
        }
        return Ok(path);
    }

    // CARGO_MANIFEST_DIR is set at compile time to `desktop/src-tauri`.
    // The repo is a Cargo workspace, so `cargo build -p core` lands the
    // binary under the workspace `target/`, not `core/target/`. Check
    // both so per-crate builds (legacy) and workspace builds both work.
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("could not locate repo root from CARGO_MANIFEST_DIR"))?;

    let bin_name = if cfg!(windows) { "core.exe" } else { "core" };
    let target_roots = [repo_root.join("target"), repo_root.join("core/target")];
    for target_root in &target_roots {
        for profile in &["release", "debug"] {
            let candidate = target_root.join(profile).join(bin_name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }
    anyhow::bail!(
        "core binary not found under {} or {}; run `task core:build` first, or set CORTEX_CORE_BIN",
        target_roots[0].display(),
        target_roots[1].display(),
    );
}

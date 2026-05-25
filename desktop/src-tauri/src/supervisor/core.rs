//! `core` launcher.
//!
//! Spawns the `cortex-core` axum server with the resolved
//! `DATABASE_URL`, then probes `/api/health` until 200 or timeout.
//! The launcher returns once the server is responding, so callers
//! can treat "Ok(())" as "the WS endpoints are accepting connections."
//!
//! Binary resolution today is dev-mode: look at the env var
//! `CORTEX_CORE_BIN`, else walk to the workspace `target/{debug,release}/core`
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

/// Build SHA the desktop was compiled from. Compared against
/// `/api/version` to detect a stale `core` squatting on the port (see
/// [`verify_core_matches_supervisor`]).
pub const SUPERVISOR_GIT_SHA: &str = env!("BUILD_GIT_SHA");

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

/// Fetch `/api/version` from the core that just responded to the health
/// probe and compare its SHA against the supervisor's own build SHA.
/// A mismatch means a foreign `core` (typically a stale binary from an
/// earlier desktop run that never exited cleanly) is squatting on the
/// port — the May 25 stale-release-binary report was exactly this.
///
/// Returns `Ok(())` if the SHAs match. Returns an error message naming
/// both SHAs and pointing at the likely fix ("a foreign core is running
/// on <bind>; kill it and retry") otherwise. We deliberately do NOT
/// attempt to kill the foreign process from here — that requires a PID
/// the supervisor doesn't own, and a launcher should never SIGKILL
/// processes it didn't spawn.
///
/// `"unknown"` SHAs on either side (non-git build, vendored release)
/// are treated as "skip the check, log a warning" rather than fail —
/// otherwise a release tarball would refuse to start.
pub async fn verify_core_matches_supervisor(bind: &str) -> anyhow::Result<()> {
    let response = fetch_version(bind).await?;
    let remote_sha = extract_sha(&response).unwrap_or("unknown");
    let local_sha = SUPERVISOR_GIT_SHA;

    if remote_sha == "unknown" || local_sha == "unknown" {
        tracing::warn!(
            bind,
            remote_sha,
            local_sha,
            "skipping stale-core check: at least one SHA is 'unknown' \
             (non-git build); the supervisor cannot prove core is fresh"
        );
        return Ok(());
    }

    if remote_sha != local_sha {
        anyhow::bail!(
            "stale core detected on {bind}: \
             /api/version reports git_sha={remote_sha}, supervisor was built from git_sha={local_sha}. \
             A foreign `core` process is squatting on the port — find and kill it \
             (`lsof -nP -iTCP:{port} -sTCP:LISTEN`), then restart the desktop.",
            port = bind.rsplit(':').next().unwrap_or("7654"),
        );
    }

    tracing::info!(bind, git_sha = remote_sha, "core: /api/version matches supervisor build");
    Ok(())
}

/// Tiny HTTP/1.0 fetch for `/api/version`. Same shape as `probe_once`
/// but reads the whole body so we can extract the JSON `git_sha`.
async fn fetch_version(bind: &str) -> anyhow::Result<String> {
    let mut stream = TcpStream::connect(bind)
        .await
        .map_err(|e| anyhow::anyhow!("connect to {bind} for /api/version failed: {e}"))?;
    let req = "GET /api/version HTTP/1.0\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    stream
        .write_all(req.as_bytes())
        .await
        .map_err(|e| anyhow::anyhow!("write /api/version request: {e}"))?;
    let mut buf = Vec::with_capacity(1024);
    stream
        .read_to_end(&mut buf)
        .await
        .map_err(|e| anyhow::anyhow!("read /api/version response: {e}"))?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Pull the `git_sha` value out of the JSON body of a `/api/version`
/// HTTP/1.0 response. Hand-parsed to avoid pulling in a JSON crate just
/// for one field on one supervisor hot path.
fn extract_sha(http_response: &str) -> Option<&str> {
    let body_start = http_response.find("\r\n\r\n").map(|i| i + 4)?;
    let body = &http_response[body_start..];
    let key = "\"git_sha\":\"";
    let start = body.find(key)? + key.len();
    let end = body[start..].find('"')?;
    Some(&body[start..start + end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_sha_pulls_value_from_envelope() {
        let r = "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\r\n\
                 {\"data\":{\"version\":\"0.1.0\",\"git_sha\":\"abc123def\"},\"error\":null}";
        assert_eq!(extract_sha(r), Some("abc123def"));
    }

    #[test]
    fn extract_sha_returns_none_on_missing_key() {
        let r = "HTTP/1.0 200 OK\r\n\r\n{\"data\":{\"version\":\"0.1.0\"}}";
        assert_eq!(extract_sha(r), None);
    }

    #[test]
    fn extract_sha_returns_none_on_no_body() {
        assert_eq!(extract_sha("HTTP/1.0 500 oops"), None);
    }
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
/// look at workspace `target/debug/core` before `target/release/core`.
/// Debug wins in dev because `tauri dev` rebuilds the shell in debug
/// mode and a stale release core can otherwise shadow fresh source.
/// Production (bundled sidecar) is COR-87.
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
        for profile in &["debug", "release"] {
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

//! External Postgres provider.
//!
//! User-supplied `DATABASE_URL`. The supervisor doesn't spawn or own
//! anything; it just verifies reachability (TCP connect with timeout)
//! and hands the URL through to the core launcher. The point of
//! failing fast here is so the user sees "external Postgres at
//! <host>:<port> unreachable" instead of letting `core` panic on its
//! first diesel query several seconds into startup.
//!
//! We deliberately do NOT do a libpq-level handshake — TCP connect is
//! enough to distinguish "wrong host / no route / firewall" from
//! "core's diesel migration failed." Auth + schema errors belong with
//! whoever runs the query, not the reachability check.

use std::time::Duration;

use tokio::net::TcpStream;
use url::Url;

use super::PostgresHandle;

const REACHABILITY_TIMEOUT: Duration = Duration::from_secs(2);
const DEFAULT_PORT: u16 = 5432;

#[derive(Debug, Clone)]
pub struct ExternalPostgres {
    url: String,
}

impl ExternalPostgres {
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }

    pub async fn start(&self) -> anyhow::Result<PostgresHandle> {
        let parsed = Url::parse(&self.url)
            .map_err(|e| anyhow::anyhow!("CORTEX_PG_URL is not a valid URL: {e}"))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("CORTEX_PG_URL is missing a host component"))?;
        let port = parsed.port().unwrap_or(DEFAULT_PORT);

        // tokio's TcpStream::connect resolves DNS internally; the
        // timeout bound is what saves us from a 75s connect on a
        // typo'd hostname.
        let addr = format!("{host}:{port}");
        tracing::info!(addr = %addr, "external postgres: probing reachability");
        match tokio::time::timeout(REACHABILITY_TIMEOUT, TcpStream::connect(&addr)).await {
            Ok(Ok(_stream)) => {
                tracing::info!(addr = %addr, "external postgres: reachable");
            }
            Ok(Err(e)) => {
                anyhow::bail!("external postgres at {addr} unreachable: {e}");
            }
            Err(_) => {
                anyhow::bail!(
                    "external postgres at {addr} unreachable: connect timed out after {:?}",
                    REACHABILITY_TIMEOUT
                );
            }
        }

        Ok(PostgresHandle {
            url: self.url.clone(),
            provider: "external",
        })
    }

    pub async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

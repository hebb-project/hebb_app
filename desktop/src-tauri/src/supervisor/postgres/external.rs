//! External Postgres provider.
//!
//! User-supplied `DATABASE_URL`. The supervisor doesn't spawn or own
//! anything; it just hands the URL through to the core launcher and
//! reports the provider tag for diagnostics. The follow-up commit
//! adds a reachability check (TCP connect with timeout) so the
//! supervisor can fail fast when the external DB is unreachable
//! instead of letting `core` discover that on its first query.

use super::PostgresHandle;

#[derive(Debug, Clone)]
pub struct ExternalPostgres {
    url: String,
}

impl ExternalPostgres {
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }

    pub async fn start(&self) -> anyhow::Result<PostgresHandle> {
        // TODO: reachability check (tokio::net::TcpStream::connect with
        // a 2s timeout against the parsed host:port). Lands on the
        // same follow-up commit as the embedded provider, since both
        // are needed for the "core gets a verified URL" contract.
        Ok(PostgresHandle {
            url: self.url.clone(),
            provider: "external",
        })
    }

    pub async fn stop(&self) -> anyhow::Result<()> {
        // Nothing to stop — we didn't start anything.
        Ok(())
    }
}

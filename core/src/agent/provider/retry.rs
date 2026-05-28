//! Exponential-backoff retry around a `LlmProvider::complete` call ([A6] /
//! hebb_app#13).
//!
//! The agent reasoning loop should not give up the first time a provider
//! returns 429 or 500 — those are usually transient. This module's
//! `retry_with_backoff` wraps an arbitrary async closure that returns
//! `Result<T, ProviderError>` and retries on errors the classifier marks
//! retryable, applying:
//!
//! - exponential backoff with a configurable initial delay and multiplier;
//! - a cap on the per-attempt delay so the loop doesn't sleep for minutes
//!   in a Cargo test by accident;
//! - random jitter so concurrent sessions don't synchronize a thundering
//!   herd on the same provider;
//! - the server's `Retry-After` hint (via [`ProviderError::retry_after`])
//!   overrides the computed delay so a 429 with `Retry-After: 30` waits
//!   exactly that long.
//!
//! Pure async; no tokio task spawns. The retry loop is just `tokio::time::sleep`
//! between calls.

use std::future::Future;
use std::time::Duration;

use rand::Rng;

use super::ProviderError;

/// Caller-facing tuning for [`retry_with_backoff`]. Defaults are picked
/// for an interactive agent turn:
///
/// - 3 retries (4 total attempts).
/// - 250 ms initial delay, doubling each attempt, capped at 8 s.
/// - ±25% jitter on the computed delay.
///
/// These are conservative — a 4-attempt turn budget keeps a stuck session
/// honest while still surviving a hiccup. The reasoning loop ([C2]) can
/// pass `RetryConfig::aggressive()` for background work where waiting
/// longer is cheaper than failing.
#[derive(Debug, Clone, Copy)]
pub struct RetryConfig {
    /// Maximum number of *retries* (i.e. total attempts = `max_retries + 1`).
    pub max_retries: u32,
    /// Delay before the first retry. Each subsequent retry doubles it,
    /// capped at `max_delay`.
    pub initial_delay: Duration,
    /// Upper bound on a single sleep. Without this, attempt 5 would sleep
    /// for 16 × initial — surprising in a unit test.
    pub max_delay: Duration,
    /// Random jitter applied to the computed delay, in `[0.0, 1.0]`. A
    /// jitter of `0.25` means the realised delay is uniformly sampled
    /// from `[delay * 0.75, delay * 1.25]`.
    pub jitter: f64,
}

impl RetryConfig {
    /// Conservative defaults — see [`RetryConfig`] doc.
    pub fn interactive() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(250),
            max_delay: Duration::from_secs(8),
            jitter: 0.25,
        }
    }

    /// More patience: 6 retries, 500 ms initial, capped at 30 s. Useful
    /// for background / batch turns where finishing is cheaper than
    /// failing.
    pub fn aggressive() -> Self {
        Self {
            max_retries: 6,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            jitter: 0.25,
        }
    }

    /// Tests/CI: no waiting between retries. Lets a test exercise the
    /// retry loop without `tokio::time::pause()` boilerplate.
    pub fn no_wait(max_retries: u32) -> Self {
        Self {
            max_retries,
            initial_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
            jitter: 0.0,
        }
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self::interactive()
    }
}

/// Run `op` with retries on [`ProviderError::is_retryable`] failures.
///
/// The closure is re-invoked from scratch each attempt — callers pass it
/// as `|| async { provider.complete(&req, key).await }` so the same
/// request goes out. Non-retryable errors are returned immediately.
///
/// The realised delay between attempts is:
///
/// 1. If the error carries a `retry_after` hint, use that (capped at
///    `cfg.max_delay`).
/// 2. Otherwise: `initial_delay * 2^(attempt - 1)`, capped at
///    `cfg.max_delay`, then jittered by `± cfg.jitter`.
///
/// `tokio::time::sleep` is used between attempts. Tests can use
/// `RetryConfig::no_wait` to skip sleeping.
pub async fn retry_with_backoff<T, F, Fut>(cfg: RetryConfig, mut op: F) -> Result<T, ProviderError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, ProviderError>>,
{
    let mut attempt: u32 = 0;
    loop {
        match op().await {
            Ok(value) => return Ok(value),
            Err(err) => {
                if !err.is_retryable() || attempt >= cfg.max_retries {
                    return Err(err);
                }
                let delay = next_delay(&cfg, attempt, err.retry_after());
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                attempt += 1;
            }
        }
    }
}

fn next_delay(cfg: &RetryConfig, attempt: u32, retry_after: Option<Duration>) -> Duration {
    // Respect the server's hint when present. Cap it so a misbehaving
    // provider can't pin the loop on a 10-minute wait.
    if let Some(hint) = retry_after {
        return hint.min(cfg.max_delay);
    }
    if cfg.initial_delay.is_zero() {
        return Duration::ZERO;
    }
    let base = cfg
        .initial_delay
        .saturating_mul(1u32.checked_shl(attempt).unwrap_or(u32::MAX));
    let capped = base.min(cfg.max_delay);
    apply_jitter(capped, cfg.jitter)
}

fn apply_jitter(d: Duration, jitter: f64) -> Duration {
    if jitter <= 0.0 || d.is_zero() {
        return d;
    }
    let j = jitter.clamp(0.0, 1.0);
    let factor = 1.0 + rand::thread_rng().gen_range(-j..=j);
    let ms = (d.as_millis() as f64 * factor).max(0.0) as u64;
    Duration::from_millis(ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::time::Duration;

    /// A non-retryable error returns immediately; the closure is called
    /// exactly once.
    #[tokio::test]
    async fn auth_error_is_not_retried() {
        let calls = Cell::new(0u32);
        let result: Result<(), _> = retry_with_backoff(RetryConfig::no_wait(5), || {
            calls.set(calls.get() + 1);
            async { Err(ProviderError::Auth("bad key".into())) }
        })
        .await;
        assert!(matches!(result, Err(ProviderError::Auth(_))));
        assert_eq!(calls.get(), 1);
    }

    /// A retryable error retries up to `max_retries`, then surfaces the
    /// last error.
    #[tokio::test]
    async fn transient_retries_until_budget_exhausted() {
        let calls = Cell::new(0u32);
        let result: Result<(), _> = retry_with_backoff(RetryConfig::no_wait(2), || {
            calls.set(calls.get() + 1);
            async { Err(ProviderError::Transient("5xx".into())) }
        })
        .await;
        assert!(matches!(result, Err(ProviderError::Transient(_))));
        // 1 initial + 2 retries = 3 total attempts.
        assert_eq!(calls.get(), 3);
    }

    /// Retries stop as soon as the closure succeeds.
    #[tokio::test]
    async fn retry_stops_on_first_success() {
        let calls = Cell::new(0u32);
        let result = retry_with_backoff(RetryConfig::no_wait(5), || {
            let n = calls.get() + 1;
            calls.set(n);
            async move {
                if n < 3 {
                    Err(ProviderError::Transient("flaky".into()))
                } else {
                    Ok::<i32, ProviderError>(42)
                }
            }
        })
        .await;
        assert_eq!(result.unwrap(), 42);
        assert_eq!(calls.get(), 3);
    }

    /// RateLimit with no `retry_after` is retryable just like Transient.
    #[tokio::test]
    async fn rate_limit_without_hint_is_retried() {
        let calls = Cell::new(0u32);
        let _result: Result<(), _> = retry_with_backoff(RetryConfig::no_wait(1), || {
            calls.set(calls.get() + 1);
            async {
                Err(ProviderError::RateLimit {
                    message: "slow down".into(),
                    retry_after: None,
                })
            }
        })
        .await;
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn next_delay_uses_retry_after_when_present() {
        let cfg = RetryConfig::interactive();
        let hint = Duration::from_secs(2);
        // The Retry-After hint should win over the computed base.
        assert_eq!(next_delay(&cfg, 0, Some(hint)), hint);
    }

    #[test]
    fn next_delay_caps_retry_after_at_max_delay() {
        let cfg = RetryConfig {
            max_delay: Duration::from_secs(5),
            ..RetryConfig::interactive()
        };
        let too_long = Duration::from_secs(600);
        assert_eq!(next_delay(&cfg, 0, Some(too_long)), Duration::from_secs(5));
    }

    #[test]
    fn next_delay_doubles_then_caps() {
        // No jitter so the math is exact.
        let cfg = RetryConfig {
            max_retries: 5,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(1_000),
            jitter: 0.0,
        };
        assert_eq!(next_delay(&cfg, 0, None), Duration::from_millis(100));
        assert_eq!(next_delay(&cfg, 1, None), Duration::from_millis(200));
        assert_eq!(next_delay(&cfg, 2, None), Duration::from_millis(400));
        assert_eq!(next_delay(&cfg, 3, None), Duration::from_millis(800));
        // Capped at max_delay.
        assert_eq!(next_delay(&cfg, 4, None), Duration::from_millis(1_000));
        assert_eq!(next_delay(&cfg, 100, None), Duration::from_millis(1_000));
    }

    #[test]
    fn jitter_zero_is_identity() {
        let d = Duration::from_millis(500);
        assert_eq!(apply_jitter(d, 0.0), d);
    }

    #[test]
    fn jitter_stays_within_window() {
        let d = Duration::from_millis(1_000);
        for _ in 0..1_000 {
            let j = apply_jitter(d, 0.25);
            assert!(j >= Duration::from_millis(750));
            assert!(j <= Duration::from_millis(1_250));
        }
    }
}

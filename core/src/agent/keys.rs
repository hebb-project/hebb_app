//! Provider API key resolution — env-var fallback path ([E4] / hebb_app#37).
//!
//! The agent harness's primary key source is the OS keychain (Tauri plugin,
//! [E1] / hebb_app#34): keys never reach the JS layer and core receives
//! them per request through the Tauri bridge. For headless contexts — the
//! CLI, MCP server, dev runs, CI — there is no keychain available, so
//! providers fall back to environment variables.
//!
//! The precedence the agent session is expected to apply is:
//!
//! 1. Keychain entry for the provider (when [E1] is wired in).
//! 2. `provider_key_from_env(provider_id)`.
//! 3. Unauthenticated → provider returns [`crate::agent::ProviderError::Auth`].
//!
//! Centralising the env-var → provider mapping in one place keeps the names
//! agreed across the loop, the test live-smoke paths, and any future docs.
//! It also gives [E2] / hebb_app#35 a single function whose call-sites need
//! to never log the returned key — easier to grep than scattered
//! `std::env::var` reads.

/// Lookup the API key for `provider_id` from the process environment.
///
/// Returns `None` when no relevant env var is set. Empty values are
/// treated as unset — a blank `GEMINI_API_KEY=` from a misconfigured shell
/// should not pass to a provider only to be reported as an auth failure.
///
/// Recognised provider ids match `LlmProvider::id()`:
/// - `"gemini"` → `GEMINI_API_KEY`, falling back to `GOOGLE_API_KEY`
///   (the same precedence the Gemini live-smoke test honours, so a
///   single `GOOGLE_API_KEY` works across both surfaces).
/// - `"anthropic"` → `ANTHROPIC_API_KEY`.
///
/// Unknown ids return `None` so a misspelled provider doesn't silently
/// pick up an unrelated env var.
pub fn provider_key_from_env(provider_id: &str) -> Option<String> {
    match provider_id {
        "gemini" => env_nonempty("GEMINI_API_KEY").or_else(|| env_nonempty("GOOGLE_API_KEY")),
        "anthropic" => env_nonempty("ANTHROPIC_API_KEY"),
        _ => None,
    }
}

fn env_nonempty(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Single guard for the whole module: env mutation is process-global, so
    /// the tests in this module run serially regardless of cargo's
    /// per-test-binary parallelism.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Save + clear the vars the tests touch; restore on drop so the test
    /// process's env never leaks state between cases (or out to other
    /// tests in the binary that read these names).
    struct EnvGuard {
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn new(vars: &[&'static str]) -> Self {
            let saved = vars
                .iter()
                .map(|v| (*v, std::env::var(v).ok()))
                .collect();
            for v in vars {
                // SAFETY: tests in this module are serialized by `env_lock`.
                unsafe { std::env::remove_var(v) };
            }
            Self { saved }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (k, v) in &self.saved {
                // SAFETY: tests in this module are serialized by `env_lock`.
                unsafe {
                    match v {
                        Some(val) => std::env::set_var(k, val),
                        None => std::env::remove_var(k),
                    }
                }
            }
        }
    }

    #[test]
    fn unknown_provider_returns_none() {
        let _l = env_lock();
        let _g = EnvGuard::new(&["GEMINI_API_KEY", "GOOGLE_API_KEY", "ANTHROPIC_API_KEY"]);
        assert!(provider_key_from_env("openai").is_none());
        assert!(provider_key_from_env("").is_none());
    }

    #[test]
    fn gemini_prefers_gemini_api_key_over_google_api_key() {
        let _l = env_lock();
        let _g = EnvGuard::new(&["GEMINI_API_KEY", "GOOGLE_API_KEY"]);
        // SAFETY: serialised by `env_lock`.
        unsafe {
            std::env::set_var("GEMINI_API_KEY", "from-gemini");
            std::env::set_var("GOOGLE_API_KEY", "from-google");
        }
        assert_eq!(provider_key_from_env("gemini").as_deref(), Some("from-gemini"));
    }

    #[test]
    fn gemini_falls_back_to_google_api_key() {
        let _l = env_lock();
        let _g = EnvGuard::new(&["GEMINI_API_KEY", "GOOGLE_API_KEY"]);
        // SAFETY: serialised by `env_lock`.
        unsafe { std::env::set_var("GOOGLE_API_KEY", "from-google") };
        assert_eq!(provider_key_from_env("gemini").as_deref(), Some("from-google"));
    }

    #[test]
    fn anthropic_reads_anthropic_api_key() {
        let _l = env_lock();
        let _g = EnvGuard::new(&["ANTHROPIC_API_KEY"]);
        // SAFETY: serialised by `env_lock`.
        unsafe { std::env::set_var("ANTHROPIC_API_KEY", "ant-key") };
        assert_eq!(provider_key_from_env("anthropic").as_deref(), Some("ant-key"));
    }

    #[test]
    fn empty_env_value_is_treated_as_unset() {
        let _l = env_lock();
        let _g = EnvGuard::new(&["GEMINI_API_KEY", "GOOGLE_API_KEY"]);
        // SAFETY: serialised by `env_lock`.
        unsafe { std::env::set_var("GEMINI_API_KEY", "") };
        // An empty Gemini key should not pass through; with no Google
        // fallback we expect None.
        assert!(provider_key_from_env("gemini").is_none());
    }

    #[test]
    fn missing_env_returns_none() {
        let _l = env_lock();
        let _g = EnvGuard::new(&["GEMINI_API_KEY", "GOOGLE_API_KEY", "ANTHROPIC_API_KEY"]);
        assert!(provider_key_from_env("gemini").is_none());
        assert!(provider_key_from_env("anthropic").is_none());
    }
}

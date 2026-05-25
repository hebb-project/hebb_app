//! `/api/version` — what source this `core` binary was compiled from.
//!
//! The supervisor calls this immediately after `/api/health` returns OK
//! and compares the SHA against its own build SHA. A mismatch means a
//! stale `core` is squatting on the port — the new desktop instance
//! must refuse to talk to it rather than silently piggy-back (which is
//! how the May 21 release binary served pre-#43 errors to a fresh
//! desktop after every later fix had merged).
//!
//! The endpoint is unauthenticated and read-only by design. Cheap
//! enough to call on every supervisor boot.

use serde_json::json;

use crate::error::CoreResult;

/// Compile-time crate version from `Cargo.toml`.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Compile-time git SHA captured by `build.rs`. `"unknown"` if the
/// build wasn't run from a git checkout.
pub const BUILD_GIT_SHA: &str = env!("BUILD_GIT_SHA");

pub async fn get_version() -> CoreResult<axum::Json<serde_json::Value>> {
    Ok(super::ok(json!({
        "version": CRATE_VERSION,
        "git_sha": BUILD_GIT_SHA,
    })))
}

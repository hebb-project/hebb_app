//! Build-time capture of the git revision so the binary can report what
//! source it was compiled from. The supervisor (and `/api/version`) use
//! this to detect a stale `core` that's silently squatting on the port.
//!
//! Falls back to "unknown" if the source tree isn't a git checkout
//! (release tarball, vendored build, etc.). Re-runs on every HEAD move
//! so the SHA stays in lockstep with what's actually compiled.

use std::process::Command;

fn main() {
    let sha = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_GIT_SHA={sha}");

    // Re-run if HEAD moves or the working tree's git state changes.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/index");
}

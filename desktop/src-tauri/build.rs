//! Build-time wiring for the Tauri shell.
//!
//! Captures the git revision so the supervisor's stale-core check can
//! compare its own build SHA against `/api/version`. The check is the
//! one thing standing between a fresh desktop and silently piggy-
//! backing on a pre-fix `core` already squatting on the port.

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
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");

    tauri_build::build()
}

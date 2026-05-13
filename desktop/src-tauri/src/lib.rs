//! Long Horizon Cortex — desktop shell.
//!
//! The Tauri "main process": owns the OS interface, the window, and
//! (soon, COR-33) the managed subprocesses for Postgres, the Rust
//! core, and the Python bridge. Today it only opens a webview onto
//! the existing `web/` frontend so we can verify the architectural
//! decision before adding subprocess management.
//!
//! UI lives in `../../web` and is loaded as a Next.js static export
//! (`output: "export"`) — see `tauri.conf.json::build.frontendDist`.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(
            "info,long_horizon_cortex_desktop_lib=debug,tauri=info",
        )
    });
    tracing_subscriber::fmt().with_env_filter(filter).init();

    tauri::Builder::default()
        .setup(|_app| {
            tracing::info!(
                version = env!("CARGO_PKG_VERSION"),
                "cortex desktop shell starting"
            );
            // COR-33 will spawn Postgres + core + bridge here. The
            // existing core HTTP+WS server is currently expected to
            // be running on http://127.0.0.1:8080 (see web/lib/cortex-api.ts).
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

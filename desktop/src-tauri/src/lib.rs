//! Long Horizon Cortex — desktop shell.
//!
//! The Tauri "main process": owns the OS interface, the window, and
//! the managed subprocesses for Postgres, the Rust core, and (later)
//! the Python bridge. Subprocess management lives in [`supervisor`].
//!
//! UI lives in `../../web` and is loaded as a Next.js static export
//! (`output: "export"`) — see `tauri.conf.json::build.frontendDist`.

mod supervisor;

use std::sync::Arc;

use supervisor::{ProcessStatus, Supervisor};

/// IPC: snapshot every managed subprocess's status. Frontend reads this
/// to render the (future) diagnostics pane.
#[tauri::command]
async fn supervisor_status(
    supervisor: tauri::State<'_, Arc<Supervisor>>,
) -> Result<Vec<ProcessStatus>, String> {
    Ok(supervisor.status_all().await)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(
            "info,long_horizon_cortex_desktop_lib=debug,tauri=info,managed_process=info",
        )
    });
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let supervisor = Arc::new(Supervisor::new());

    tauri::Builder::default()
        .manage(supervisor.clone())
        .invoke_handler(tauri::generate_handler![supervisor_status])
        .setup(move |_app| {
            tracing::info!(
                version = env!("CARGO_PKG_VERSION"),
                "cortex desktop shell starting"
            );
            // Concrete launchers (Postgres, core, bridge) attach here
            // in follow-up commits on this PR. The supervisor itself is
            // already registered in Tauri's state so the frontend can
            // call `supervisor_status` even before any process is wired.
            Ok(())
        })
        .on_window_event(move |window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let supervisor = supervisor.clone();
                let window = window.clone();
                tauri::async_runtime::spawn(async move {
                    supervisor.shutdown_all().await;
                    tracing::info!("supervisor: shutdown_all complete");
                    let _ = window;
                });
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

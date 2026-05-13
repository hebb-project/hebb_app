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

use supervisor::{
    wait_for_health, CoreLauncher, PostgresProvider, ProcessStatus, Supervisor,
};

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
    let supervisor_for_setup = supervisor.clone();
    let supervisor_for_close = supervisor.clone();

    tauri::Builder::default()
        .manage(supervisor)
        .invoke_handler(tauri::generate_handler![supervisor_status])
        .setup(move |_app| {
            tracing::info!(
                version = env!("CARGO_PKG_VERSION"),
                "cortex desktop shell starting"
            );
            // Bootstrap runs in the tokio runtime so the window can
            // show immediately while Postgres + core come up in the
            // background. Frontend polls `supervisor_status` for live
            // state.
            let supervisor = supervisor_for_setup.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = bootstrap(supervisor).await {
                    tracing::error!(error = %e, "supervisor bootstrap failed");
                }
            });
            Ok(())
        })
        .on_window_event(move |window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let supervisor = supervisor_for_close.clone();
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

/// Start Postgres → spawn core → wait for `/health`. Errors surface
/// via tracing; the frontend learns about failures by seeing
/// `Stopped`/`Failed` rows in `supervisor_status`.
async fn bootstrap(supervisor: Arc<Supervisor>) -> anyhow::Result<()> {
    tracing::info!("supervisor: starting postgres");
    let pg = PostgresProvider::from_env();
    let pg_handle = supervisor.start_postgres(pg).await?;
    tracing::info!(provider = pg_handle.provider, "supervisor: postgres ready");

    tracing::info!("supervisor: spawning core");
    let launcher = CoreLauncher::from_env(pg_handle.url.clone())?;
    let (process, bind) = launcher.into_process();
    let process = supervisor.register(process).await;
    process.spawn().await?;
    wait_for_health(&bind).await?;
    tracing::info!(bind, "supervisor: core ready");

    Ok(())
}

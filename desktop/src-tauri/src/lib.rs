//! Cortex — desktop shell.
//!
//! The Tauri "main process": owns the OS interface, the window, and
//! the managed subprocesses for Postgres, the Rust core, and (later)
//! the Python bridge. Subprocess management lives in [`supervisor`].
//!
//! UI lives in `../../web` and is loaded as a Next.js static export
//! (`output: "export"`) — see `tauri.conf.json::build.frontendDist`.

mod cortex_folder;
mod cortex_seeds;
mod supervisor;

use std::sync::Arc;

use supervisor::{
    wait_for_health, BootstrapState, CoreLauncher, PostgresProvider, Supervisor, SupervisorOverview,
};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    Emitter,
};

/// IPC: full supervisor snapshot (bootstrap flow + per-process). One
/// IPC call so the diagnostics pane (COR-86) renders from a single
/// fetch.
#[tauri::command]
async fn supervisor_status(
    supervisor: tauri::State<'_, Arc<Supervisor>>,
) -> Result<SupervisorOverview, String> {
    Ok(supervisor.overview().await)
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
        .plugin(tauri_plugin_dialog::init())
        .manage(supervisor)
        .menu(|app| {
            let new_window = MenuItem::with_id(
                app,
                "file:new-window",
                "New Window",
                true,
                Some("CmdOrCtrl+N"),
            )?;
            let open_network = MenuItem::with_id(
                app,
                "file:open-network",
                "Open Network...",
                true,
                Some("CmdOrCtrl+O"),
            )?;
            let file = Submenu::with_items(
                app,
                "File",
                true,
                &[
                    &new_window,
                    &open_network,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::close_window(app, None::<&str>)?,
                ],
            )?;
            Menu::with_items(app, &[&file])
        })
        .on_menu_event(|app, event| {
            let payload = match event.id().as_ref() {
                "file:new-window" => Some("new-window"),
                "file:open-network" => Some("open-network"),
                _ => None,
            };
            if let Some(payload) = payload {
                if let Err(e) = app.emit("cortex://file-menu", payload) {
                    tracing::warn!(error = %e, "failed to emit file menu action");
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            supervisor_status,
            cortex_folder::inspect_cortex_folder,
            cortex_folder::init_cortex_folder,
            cortex_seeds::seed_cortex_folder,
        ])
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
                if let Err(e) = bootstrap(&supervisor).await {
                    let reason = format!("{e:#}");
                    tracing::error!(error = %reason, "supervisor bootstrap failed");
                    supervisor
                        .set_bootstrap(BootstrapState::Failed { reason })
                        .await;
                    // CLEANUP: a failure between start_postgres and
                    // wait_for_health would leak embedded Postgres
                    // until window-close. Tear it down (and any
                    // already-spawned child) now.
                    supervisor.shutdown_all().await;
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

/// Start Postgres → spawn core → wait for `/health`. Each step
/// updates `BootstrapState::Starting { step }` before running, so the
/// frontend can render progress. On Err, the caller (`setup` hook)
/// transitions to `Failed { reason }` and runs `shutdown_all` to
/// avoid leaking the embedded PG process when a downstream step
/// fails.
async fn bootstrap(supervisor: &Arc<Supervisor>) -> anyhow::Result<()> {
    supervisor
        .set_bootstrap(BootstrapState::Starting { step: "postgres" })
        .await;
    tracing::info!("supervisor: starting postgres");
    let pg = PostgresProvider::from_env();
    let pg_handle = supervisor.start_postgres(pg).await?;
    tracing::info!(provider = pg_handle.provider, "supervisor: postgres ready");

    supervisor
        .set_bootstrap(BootstrapState::Starting { step: "core" })
        .await;
    tracing::info!("supervisor: spawning core");
    let launcher = CoreLauncher::from_env(pg_handle.url.clone())?;
    let (process, bind) = launcher.into_process();
    let process = supervisor.register(process).await;
    process.spawn().await?;

    supervisor
        .set_bootstrap(BootstrapState::Starting { step: "health" })
        .await;
    wait_for_health(&bind).await?;
    tracing::info!(bind, "supervisor: core ready");

    supervisor.set_bootstrap(BootstrapState::Ready).await;
    Ok(())
}

// Hide the console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod errors;
mod interrupt;
mod pipeline;
mod refresh;
mod scheduler;

use std::path::Path;
use std::sync::Mutex;

use anyhow::Context;
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tolon_store::Store;
use tolon_sync::IcsFetcher;

use commands::AppState;

/// `tolon.db` in the app data dir, migrated. Any failure here means Tolon must not run.
fn open_store(app: &tauri::AppHandle) -> anyhow::Result<Store> {
    let dir = app
        .path()
        .app_data_dir()
        .context("Tolon could not find its data folder.")?;
    open_store_in(&dir)
}

/// Creates `dir` (and parents) first: SQLite does not make folders. The dialog text stays a short
/// sentence; the log line carries the path and the full cause chain for diagnosis.
fn open_store_in(dir: &Path) -> anyhow::Result<Store> {
    let db = dir.join("tolon.db");
    let opened = std::fs::create_dir_all(dir)
        .context("Tolon could not create its data folder.")
        .and_then(|()| Ok(Store::open(&db)?));
    opened.inspect_err(
        |e| tracing::error!(path = %db.display(), error = %format!("{e:#}"), "could not open the database"),
    )
}

fn start(app: &tauri::AppHandle) -> anyhow::Result<()> {
    let store = open_store(app)?;
    let fetcher = IcsFetcher::new().context("Tolon could not start its network client.")?;
    app.manage(AppState {
        db: Mutex::new(store),
        fetcher,
    });
    scheduler::setup_fallback_actions(app)?;
    app.manage(commands::AppScheduler::new(scheduler::TauriHost::new(
        app.clone(),
    )));
    tauri::async_runtime::spawn(refresh::run(app.clone()));
    tauri::async_runtime::spawn(scheduler::run(app.clone()));
    Ok(())
}

fn main() -> anyhow::Result<()> {
    // Info and below, stderr only; fetch failures log a redacted host, never the URL.
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(std::io::stderr)
        .try_init();

    tauri::Builder::default()
        .manage(interrupt::InterruptState::default())
        .plugin(tauri_plugin_dialog::init())
        // Shows the F4.5 fallback; no webview permission grants it (capabilities/*.json).
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            commands::calendars_list,
            commands::calendars_add_ics,
            commands::calendars_remove,
            commands::calendars_set_enabled,
            commands::calendars_sync_now,
            commands::occurrences_today,
            commands::alarms_snooze,
            commands::alarms_dismiss,
            commands::alarms_join,
            commands::alarms_already_there,
            commands::alarms_current,
            commands::alarms_test,
            #[cfg(debug_assertions)]
            commands::alarms_e2e_state,
        ])
        .setup(|app| {
            // A store that cannot open or migrate stops the app; it never runs on a half-applied
            // schema. The dialog needs the running event loop (a dialog shown before it never
            // appears), so a thread shows it and exits the app once it is dismissed.
            if let Err(e) = start(app.handle()) {
                tracing::error!(error = %format!("{e:#}"), "startup failed");
                let handle = app.handle().clone();
                let text = e.to_string();
                std::thread::spawn(move || {
                    handle
                        .dialog()
                        .message(text)
                        .title("Tolon cannot start")
                        .kind(MessageDialogKind::Error)
                        .blocking_show();
                    handle.exit(1);
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = anyhow::Result<()>;

    #[test]
    fn open_store_in_creates_missing_nested_dirs() -> TestResult {
        let root = tempfile::tempdir()?;
        let dir = root.path().join("no").join("such").join("dir");
        assert!(!dir.exists());
        let store = open_store_in(&dir)?;
        assert!(dir.join("tolon.db").is_file());
        assert!(store.migration_status()?.is_current());
        Ok(())
    }

    #[test]
    fn open_store_in_refuses_a_file_that_is_not_a_database() -> TestResult {
        let root = tempfile::tempdir()?;
        std::fs::write(root.path().join("tolon.db"), "not a database, just text")?;
        let Err(e) = open_store_in(root.path()) else {
            return Err(anyhow::anyhow!("a corrupt file must not open"));
        };
        assert_eq!(e.to_string(), "Tolon could not read or write its database.");
        Ok(())
    }
}

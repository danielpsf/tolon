// Hide the console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod errors;
mod interrupt;
mod pipeline;
mod refresh;
mod scheduler;
mod settings;
mod tray;

use std::path::Path;
use std::sync::Mutex;

use anyhow::Context;
use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tolon_platform::Platform;
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
    // Before the scheduler: its first tick sets the paused icon after a restart during a pause,
    // and needs the icon to exist. Without a tray (a Linux session with no status notifier host)
    // Tolon still runs and alarms still fire; the settings window stays the way in.
    if let Err(e) = tray::setup(app) {
        tracing::warn!(error = %e, "could not create the tray icon");
    }
    tauri::async_runtime::spawn(refresh::run(app.clone()));
    tauri::async_runtime::spawn(scheduler::run(app.clone()));
    Ok(())
}

/// The last thing before the process ends (Quit, Cmd+Q, logout): waits for the store lock, so a
/// tick or a command that is writing finishes its transaction first. Every write commits on its
/// own, so nothing else needs flushing.
fn settle_store(app: &tauri::AppHandle) {
    if let Ok(state) = commands::state(app) {
        drop(pipeline::lock(&state.db));
    }
}

/// S7.2: closing the settings window hides it, and the panel hides when it loses focus.
fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    match event {
        WindowEvent::CloseRequested { api, .. } if tray::hides_on_close(window.label()) => {
            api.prevent_close();
            if let Err(e) = window.hide() {
                tracing::warn!(error = %e, "could not hide the settings window");
            }
        }
        WindowEvent::Focused(false) if window.label() == tray::PANEL => {
            tray::hide_panel(window.app_handle());
        }
        _ => {}
    }
}

/// Only Quit ends Tolon (AC7.4): an exit the runtime asks for on its own, such as after the
/// last window closed, is refused; `AppHandle::exit` carries a code and goes through.
fn keeps_running(event: &RunEvent) -> bool {
    matches!(event, RunEvent::ExitRequested { code: None, .. })
}

fn main() -> anyhow::Result<()> {
    // Info and below, stderr only; fetch failures log a redacted host, never the URL.
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(std::io::stderr)
        .try_init();

    // Tauri's default menu stays (no menu of our own, `enable_macos_default_menu` left on): an
    // Accessory app shows no menu bar, but the menu still carries the Edit key equivalents
    // (Cmd+C, Cmd+V, Cmd+A) that the settings window's text fields need on macOS (L-042).
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
            commands::occurrences_join,
            commands::occurrences_snooze,
            commands::occurrences_dismiss,
            commands::alarms_snooze,
            commands::alarms_dismiss,
            commands::alarms_join,
            commands::alarms_already_there,
            commands::alarms_current,
            commands::alarms_test,
            commands::settings_get,
            commands::settings_set,
            commands::sound_preview,
            commands::app_status,
            commands::app_pause,
            commands::app_resume,
            commands::app_show_settings,
            commands::app_hide_panel,
            commands::app_quit,
            commands::tray_tooltip,
            #[cfg(feature = "e2e-loopback")]
            commands::alarms_e2e_state,
        ])
        .setup(|app| {
            // Before any window opens: on macOS the Accessory activation policy lets the interrupt
            // show over another app's full-screen Space (L-042). Nothing elsewhere. A failure costs
            // only that, so Tolon starts anyway.
            if let Err(e) = tolon_platform::NATIVE.prepare_app() {
                tracing::warn!(error = %e, "could not set the Accessory activation policy");
            }
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
        .on_window_event(on_window_event)
        .build(tauri::generate_context!())?
        .run(|app, event| {
            if keeps_running(&event) {
                if let RunEvent::ExitRequested { api, .. } = event {
                    api.prevent_exit();
                }
            } else if matches!(event, RunEvent::Exit) {
                settle_store(app);
            }
        });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = anyhow::Result<()>;

    /// L-042: the activation policy is set before anything opens a window, and the bundle hides
    /// the Dock icon from launch and needs macOS 13 (SMAppService).
    #[test]
    fn the_app_is_an_accessory_app_on_macos() -> TestResult {
        let source = include_str!("main.rs");
        let setup = source.find(".setup(|app| {").unwrap_or(usize::MAX);
        let prepare = source.find("NATIVE.prepare_app()").unwrap_or(usize::MAX);
        let start = source.find("start(app.handle())").unwrap_or(0);
        assert!(
            setup < prepare && prepare < start,
            "prepare_app runs first in setup"
        );
        let shipped = source.split("#[cfg(test)]").next().unwrap_or_default();
        assert!(!shipped.contains(".menu("), "keep Tauri's default menu");
        // B2: the tray exists before the scheduler's first tick, which may set the paused icon.
        let tray = shipped.find("tray::setup(app)").unwrap_or(usize::MAX);
        let scheduler = shipped
            .find("spawn(scheduler::run(app.clone()))")
            .unwrap_or(0);
        assert!(
            tray < scheduler,
            "tray::setup runs before the scheduler spawns"
        );
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))?;
        assert_eq!(
            conf["bundle"]["macOS"]["minimumSystemVersion"],
            serde_json::json!("13.0")
        );
        let plist = include_str!("../Info.plist");
        assert!(plist.contains("<key>LSUIElement</key>\n\t<true/>"));
        Ok(())
    }

    /// S7.2 and AC7.4: closing the settings window hides it (`tray::hides_on_close`), and an
    /// exit nobody asked for keeps Tolon in the tray. Only the panel's Quit exits.
    #[test]
    fn s7_2_close_is_not_quit() {
        let source = include_str!("main.rs");
        let shipped = source.split("#[cfg(test)]").next().unwrap_or_default();
        for needed in [
            ".on_window_event(on_window_event)",
            "api.prevent_close();",
            "api.prevent_exit();",
            "if keeps_running(&event)",
        ] {
            assert!(shipped.contains(needed), "main.rs must contain {needed}");
        }
        let commands = include_str!("commands.rs");
        let quit = commands
            .split("pub async fn app_quit(")
            .nth(1)
            .unwrap_or_default();
        let quit = quit.split("#[tauri::command]").next().unwrap_or_default();
        let stop = quit.find("scheduler.shutdown(").unwrap_or(usize::MAX);
        let exit = quit.find("app.exit(0)").unwrap_or(0);
        assert!(stop < exit, "Quit stops the scheduler before it exits");
    }

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

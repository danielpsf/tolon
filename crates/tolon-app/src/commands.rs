//! Tauri glue: shared state, the commands and the change events. The work lives in `pipeline`.
//!
//! Tauri command names cannot contain dots, so `calendars.list` is `calendars_list`, and so on
//! (names in docs/architecture.md, "Tauri commands"). A failed command returns
//! `{ "code": "..." }` (see `errors`); the UI turns the code into localised copy.

use chrono::Utc;
use tauri::{AppHandle, Emitter, Manager, State};
use tolon_sync::IcsFetcher;

use crate::errors::{command_error, AppError, CommandError};
use crate::interrupt::{AlarmShow, InterruptState, LABEL_PREFIX};
use crate::pipeline::{self, CalendarDto, Db, ListedDto};
use crate::scheduler::{Act, Scheduler, TauriHost};
use crate::settings::{self, SettingsDto, SoundChoice};

/// The scheduler of the running app, managed next to `AppState`.
pub type AppScheduler = Scheduler<TauriHost>;

/// The settings window: the only one that may start the test alarm or change settings.
const MAIN_WINDOW: &str = "main";

/// Every command of the settings window starts with this. Any other window, an interrupt or a page
/// a future change loads, gets `not_found`: the capability files cannot scope app commands.
fn main_only(label: &str) -> Result<(), CommandError> {
    if label == MAIN_WINDOW {
        Ok(())
    } else {
        Err(command_error(&AppError::NOT_FOUND.into()))
    }
}

/// `settings_get` only: the interrupt windows read the settings too, to render in the person's
/// appearance and language (AC9.3). Reading is all they get.
fn main_or_interrupt(label: &str) -> Result<(), CommandError> {
    if label.starts_with(LABEL_PREFIX) {
        Ok(())
    } else {
        main_only(label)
    }
}

pub struct AppState {
    pub db: Db,
    pub fetcher: IcsFetcher,
}

/// The state, or `not_ready` when startup failed and it was never set up.
pub fn state(app: &AppHandle) -> anyhow::Result<State<'_, AppState>> {
    app.try_state::<AppState>()
        .ok_or_else(|| AppError::NOT_READY.into())
}

/// Tells the UI to reload the calendar list and today's occurrences, and the scheduler to re-plan
/// at once (AC8.3). The payload is empty. `settings_set` calls this too.
pub fn notify(app: &AppHandle) {
    if let Some(scheduler) = app.try_state::<AppScheduler>() {
        scheduler.wake();
    }
    for event in ["calendars:changed", "occurrences:changed"] {
        if let Err(e) = app.emit(event, ()) {
            tracing::warn!(event, error = %e, "could not emit event");
        }
    }
}

/// One calendar, or every enabled one when `id` is `None`; the UI is told afterwards even on failure
/// because `last_error` may have changed.
pub async fn sync_and_notify(app: &AppHandle, id: Option<&str>) -> anyhow::Result<()> {
    let state = state(app)?;
    let now = Utc::now();
    let result = match id {
        Some(id) => pipeline::sync_one(&state.db, &state.fetcher, id, now).await,
        None => pipeline::sync_all(&state.db, &state.fetcher, now).await,
    };
    notify(app);
    result
}

#[tauri::command]
pub async fn calendars_list(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Vec<CalendarDto>, CommandError> {
    main_only(window.label())?;
    let list = state(&app).and_then(|s| pipeline::list(&s.db));
    list.map_err(|e| command_error(&e))
}

#[tauri::command]
pub async fn calendars_add_ics(
    app: AppHandle,
    window: tauri::WebviewWindow,
    url: String,
) -> Result<CalendarDto, CommandError> {
    main_only(window.label())?;
    let added = async {
        let state = state(&app)?;
        pipeline::add_ics(&state.db, &state.fetcher, &url, Utc::now()).await
    }
    .await;
    let dto = added.map_err(|e| command_error(&e))?;
    notify(&app);
    Ok(dto)
}

#[tauri::command]
pub async fn calendars_remove(
    app: AppHandle,
    window: tauri::WebviewWindow,
    id: String,
) -> Result<(), CommandError> {
    main_only(window.label())?;
    state(&app)
        .and_then(|s| pipeline::remove(&s.db, &id))
        .map_err(|e| command_error(&e))?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn calendars_set_enabled(
    app: AppHandle,
    window: tauri::WebviewWindow,
    id: String,
    enabled: bool,
) -> Result<(), CommandError> {
    main_only(window.label())?;
    state(&app)
        .and_then(|s| pipeline::set_enabled(&s.db, &id, enabled))
        .map_err(|e| command_error(&e))?;
    notify(&app);
    Ok(())
}

/// With an id the calendar syncs even when disabled (the person asked); without one, only enabled ones.
#[tauri::command]
pub async fn calendars_sync_now(
    app: AppHandle,
    window: tauri::WebviewWindow,
    id: Option<String>,
) -> Result<(), CommandError> {
    main_only(window.label())?;
    sync_and_notify(&app, id.as_deref())
        .await
        .map_err(|e| command_error(&e))
}

/// Today's meetings, the ignored ones included with their reason in `ignored` (S8.1). No URLs.
#[tauri::command]
pub async fn occurrences_today(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Vec<ListedDto>, CommandError> {
    main_only(window.label())?;
    let today =
        state(&app).and_then(|s| pipeline::today(&s.db, Utc::now(), pipeline::local_zone()));
    today.map_err(|e| command_error(&e))
}

/// The panel's Join for one of today's meetings: `key` is `ListedDto::key`. Rust opens the link.
#[tauri::command]
pub async fn occurrences_join(
    app: AppHandle,
    window: tauri::WebviewWindow,
    key: String,
) -> Result<(), CommandError> {
    main_only(window.label())?;
    let done = state(&app).and_then(|s| {
        let scheduler = app.try_state::<AppScheduler>().ok_or(AppError::NOT_READY)?;
        scheduler.join_listed(&s.db, &key)
    });
    done.map_err(|e| command_error(&e))
}

// ---- Alarm actions (F3, F4, F5) ---------------------------------------------------------------
// `key` is `MeetingDto::key` (an `OccurrenceKey`) and must belong to the interrupt on screen, else
// `not_found`. `alarms_snooze` and `alarms_dismiss` act on every meeting of the interrupt, because
// the window offers one Snooze and one Dismiss for the whole group (S3.6). `alarms_join` and
// `alarms_already_there` act on that one meeting; `alarms_join` opens its link or event page
// itself (the URL never reaches the webview). The work is in `scheduler::Scheduler::act`.

async fn alarm_action(app: &AppHandle, key: &str, act: Act) -> Result<(), CommandError> {
    let done = async {
        let state = state(app)?;
        let scheduler = app.try_state::<AppScheduler>().ok_or(AppError::NOT_READY)?;
        scheduler.act(&state.db, key, act).await
    }
    .await;
    done.map_err(|e| command_error(&e))
}

/// The minutes are clamped to 1..=60.
#[tauri::command]
pub async fn alarms_snooze(app: AppHandle, key: String, minutes: u32) -> Result<(), CommandError> {
    alarm_action(&app, &key, Act::Snooze(minutes)).await
}

#[tauri::command]
pub async fn alarms_dismiss(app: AppHandle, key: String) -> Result<(), CommandError> {
    alarm_action(&app, &key, Act::Dismiss).await
}

#[tauri::command]
pub async fn alarms_join(app: AppHandle, key: String) -> Result<(), CommandError> {
    alarm_action(&app, &key, Act::Join).await
}

#[tauri::command]
pub async fn alarms_already_there(app: AppHandle, key: String) -> Result<(), CommandError> {
    alarm_action(&app, &key, Act::AlreadyThere).await
}

/// The interrupt on screen, for a window that loaded after `alarm:show` was emitted.
#[tauri::command]
pub async fn alarms_current(app: AppHandle) -> Option<AlarmShow> {
    app.try_state::<InterruptState>().and_then(|s| s.get())
}

/// F10's test alarm: a sample interrupt on every screen (also the WebDriver tests' way to open one).
/// Only the settings window may ask: an interrupt window, or any page a future change loads, may not.
#[tauri::command]
pub async fn alarms_test(app: AppHandle, window: tauri::WebviewWindow) -> Result<(), CommandError> {
    main_only(window.label())?;
    let done = async {
        let scheduler = app.try_state::<AppScheduler>().ok_or(AppError::NOT_READY)?;
        scheduler.show_test().await
    }
    .await;
    done.map_err(|e| command_error(&e))
}

// ---- Settings (F8, F9; the contract in docs/decisions.md, 2026-10-21) -------------------------

#[tauri::command]
pub async fn settings_get(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<SettingsDto, CommandError> {
    main_or_interrupt(window.label())?;
    state(&app)
        .and_then(|s| settings::get(&s.db))
        .map_err(|e| command_error(&e))
}

/// Saves the patch at once (AC9.1), tells the windows (`settings:changed`, the full new
/// `SettingsDto`) and the panel (`occurrences:changed`), and wakes the scheduler so the plan
/// follows within 15 s (AC8.3). A refused field is `invalid:<field>` and nothing is saved.
#[tauri::command]
pub async fn settings_set(
    app: AppHandle,
    window: tauri::WebviewWindow,
    patch: serde_json::Value,
) -> Result<SettingsDto, CommandError> {
    main_only(window.label())?;
    // The event goes to the main window and the interrupts, the windows that may read settings.
    let announce = |dto: &SettingsDto| {
        if let Err(e) = app.emit("settings:changed", dto) {
            tracing::warn!(error = %e, "could not emit settings:changed");
        }
    };
    state(&app)
        .and_then(|s| settings::change(&s.db, &patch, announce, || notify(&app)))
        .map_err(|e| command_error(&e))
}

/// One shot of `sound` (`bell`, `chime` or `none`) for the Play button (AC6.3).
#[tauri::command]
pub async fn sound_preview(
    app: AppHandle,
    window: tauri::WebviewWindow,
    sound: serde_json::Value,
) -> Result<(), CommandError> {
    main_only(window.label())?;
    let choice: SoundChoice = serde_json::from_value(sound)
        .map_err(|_| command_error(&AppError("invalid:sound").into()))?;
    let scheduler = app
        .try_state::<AppScheduler>()
        .ok_or_else(|| command_error(&AppError::NOT_READY.into()))?;
    scheduler.preview(choice.into());
    Ok(())
}

/// The alarm rows, for the WebDriver tests. It exists only in the end-to-end build, whose feature no
/// release build can have (decision "End-to-end fetch seam"). Keys and states, never titles.
#[cfg(feature = "e2e-loopback")]
#[derive(serde::Serialize)]
pub struct AlarmStateDto {
    pub key: String,
    pub kind: String,
    pub state: String,
    pub until: Option<String>,
}

#[cfg(feature = "e2e-loopback")]
#[tauri::command]
pub async fn alarms_e2e_state(app: AppHandle) -> Result<Vec<AlarmStateDto>, CommandError> {
    let rows = state(&app)
        .and_then(|s| {
            let store = crate::pipeline::lock(&s.db);
            Ok(store.alarms().list()?)
        })
        .map_err(|e| command_error(&e))?;
    Ok(rows
        .into_iter()
        .map(|r| AlarmStateDto {
            key: r.occurrence_key,
            kind: r.kind,
            state: r.state,
            until: r.until,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_set_and_sound_preview_answer_only_the_main_window() {
        assert_eq!(main_only("main"), Ok(()));
        for label in ["interrupt-0", "interrupt-main", "Main", "main ", ""] {
            assert_eq!(
                main_only(label),
                Err(CommandError { code: "not_found" }),
                "{label:?}"
            );
        }
    }

    #[test]
    fn settings_get_answers_the_main_window_and_the_interrupts_only() {
        for label in ["main", "interrupt-0", "interrupt-screen-2"] {
            assert_eq!(main_or_interrupt(label), Ok(()), "{label:?}");
        }
        for label in ["interrupt", "Interrupt-0", "panel", "main-2", ""] {
            assert_eq!(
                main_or_interrupt(label),
                Err(CommandError { code: "not_found" }),
                "{label:?}"
            );
        }
    }

    /// Every command that reads or changes settings, and the test alarm, checks the window first,
    /// with the check its access calls for.
    #[test]
    fn every_settings_command_checks_the_window_first() {
        let source = include_str!("commands.rs");
        for (name, check) in [
            ("calendars_list", "main_only"),
            ("calendars_add_ics", "main_only"),
            ("calendars_remove", "main_only"),
            ("calendars_set_enabled", "main_only"),
            ("calendars_sync_now", "main_only"),
            ("occurrences_today", "main_only"),
            ("occurrences_join", "main_only"),
            ("alarms_test", "main_only"),
            ("settings_get", "main_or_interrupt"),
            ("settings_set", "main_only"),
            ("sound_preview", "main_only"),
        ] {
            let start = source
                .find(&format!("pub async fn {name}("))
                .unwrap_or(usize::MAX);
            let body = source.get(start..).unwrap_or_default();
            let first = body
                .split_once(") -> ")
                .and_then(|(_, rest)| rest.split_once('{'))
                .map(|(_, rest)| rest.trim_start())
                .unwrap_or_default();
            assert!(
                first.starts_with(&format!("{check}(window.label())?;")),
                "{name} must start with {check}"
            );
        }
    }

    /// `settings_set` saves, announces and wakes through `settings::change` (tested on the rig),
    /// and hands it `notify`, which wakes the scheduler.
    #[test]
    fn settings_set_goes_through_change_and_notify() {
        let source = include_str!("commands.rs");
        let start = source
            .find("pub async fn settings_set(")
            .unwrap_or(usize::MAX);
        let body = source.get(start..).unwrap_or_default();
        let body = body.split("#[tauri::command]").next().unwrap_or_default();
        for needed in [
            "settings::change(&s.db, &patch, announce, || notify(&app))",
            "app.emit(\"settings:changed\", dto)",
        ] {
            assert!(body.contains(needed), "settings_set must call {needed}");
        }
    }
}

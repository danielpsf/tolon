//! Tauri glue: shared state, the commands and the change events. The work lives in `pipeline`.
//!
//! Tauri command names cannot contain dots, so `calendars.list` is `calendars_list`, and so on
//! (names in docs/architecture.md, "Tauri commands"). A failed command returns
//! `{ "code": "..." }` (see `errors`); the UI turns the code into localised copy.

use chrono::Utc;
use tauri::{AppHandle, Emitter, Manager, State};
use tolon_core::Occurrence;
use tolon_sync::IcsFetcher;

use crate::errors::{command_error, AppError, CommandError};
use crate::interrupt::{AlarmShow, InterruptState};
use crate::pipeline::{self, CalendarDto, Db};
use crate::scheduler::{Act, Scheduler, TauriHost};

/// The scheduler of the running app, managed next to `AppState`.
pub type AppScheduler = Scheduler<TauriHost>;

/// The settings window: the only one that may start the test alarm.
const MAIN_WINDOW: &str = "main";

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
/// at once (AC8.3). The payload is empty. A settings command (L-016) must call this too.
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
pub async fn calendars_list(app: AppHandle) -> Result<Vec<CalendarDto>, CommandError> {
    let list = state(&app).and_then(|s| pipeline::list(&s.db));
    list.map_err(|e| command_error(&e))
}

#[tauri::command]
pub async fn calendars_add_ics(app: AppHandle, url: String) -> Result<CalendarDto, CommandError> {
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
pub async fn calendars_remove(app: AppHandle, id: String) -> Result<(), CommandError> {
    state(&app)
        .and_then(|s| pipeline::remove(&s.db, &id))
        .map_err(|e| command_error(&e))?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn calendars_set_enabled(
    app: AppHandle,
    id: String,
    enabled: bool,
) -> Result<(), CommandError> {
    state(&app)
        .and_then(|s| pipeline::set_enabled(&s.db, &id, enabled))
        .map_err(|e| command_error(&e))?;
    notify(&app);
    Ok(())
}

/// With an id the calendar syncs even when disabled (the person asked); without one, only enabled ones.
#[tauri::command]
pub async fn calendars_sync_now(app: AppHandle, id: Option<String>) -> Result<(), CommandError> {
    sync_and_notify(&app, id.as_deref())
        .await
        .map_err(|e| command_error(&e))
}

#[tauri::command]
pub async fn occurrences_today(app: AppHandle) -> Result<Vec<Occurrence>, CommandError> {
    let today =
        state(&app).and_then(|s| pipeline::today(&s.db, Utc::now(), pipeline::local_zone()));
    today.map_err(|e| command_error(&e))
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
    if window.label() != MAIN_WINDOW {
        return Err(command_error(&AppError::NOT_FOUND.into()));
    }
    let done = async {
        let scheduler = app.try_state::<AppScheduler>().ok_or(AppError::NOT_READY)?;
        scheduler.show_test().await
    }
    .await;
    done.map_err(|e| command_error(&e))
}

/// The alarm rows, for the WebDriver tests. It exists only with the `e2e-loopback` feature, which no
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

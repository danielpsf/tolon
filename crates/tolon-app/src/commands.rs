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
use crate::interrupt::{self, AlarmShow, InterruptState};
use crate::pipeline::{self, CalendarDto, Db};

pub struct AppState {
    pub db: Db,
    pub fetcher: IcsFetcher,
}

/// The state, or `not_ready` when startup failed and it was never set up.
pub fn state(app: &AppHandle) -> anyhow::Result<State<'_, AppState>> {
    app.try_state::<AppState>()
        .ok_or_else(|| AppError::NOT_READY.into())
}

/// Tells the UI to reload the calendar list and today's occurrences. The payload is empty.
pub fn notify(app: &AppHandle) {
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

// ---- Alarm actions (L-010) --------------------------------------------------------------------
// Stubs: each checks the key, closes every interrupt window (AC4.6) and returns. L-032 replaces the
// marked line with the real state change.
//
// Contract for L-032. `key` is `MeetingDto::key` (an `OccurrenceKey`). `alarms_snooze` and
// `alarms_dismiss` act on every meeting of the interrupt on screen (`InterruptState`), because the
// window offers one Snooze and one Dismiss for the whole group (S3.6). `alarms_join` and
// `alarms_already_there` act on that one meeting; `alarms_join` opens its link or event page
// itself (the URL never reaches the webview).

/// `action` is remembered for `alarms_last_action` before the windows close.
fn act_on(app: &AppHandle, key: &str, action: String) -> Result<(), CommandError> {
    interrupt::parse_key(key).map_err(|e| command_error(&e.into()))?;
    if let Some(state) = app.try_state::<InterruptState>() {
        state.record(action);
    }
    interrupt::close_all(app);
    Ok(())
}

#[tauri::command]
pub async fn alarms_snooze(app: AppHandle, key: String, minutes: u32) -> Result<(), CommandError> {
    // L-032: update alarm state (Snoozed until now + minutes)
    act_on(&app, &key, format!("snooze:{minutes}"))
}

#[tauri::command]
pub async fn alarms_dismiss(app: AppHandle, key: String) -> Result<(), CommandError> {
    // L-032: update alarm state (Dismissed)
    act_on(&app, &key, "dismiss".to_owned())
}

#[tauri::command]
pub async fn alarms_join(app: AppHandle, key: String) -> Result<(), CommandError> {
    // L-032: update alarm state (Joined) and open the join link or the event page
    act_on(&app, &key, "join".to_owned())
}

#[tauri::command]
pub async fn alarms_already_there(app: AppHandle, key: String) -> Result<(), CommandError> {
    // L-032: update alarm state (Joined, no link opened)
    act_on(&app, &key, "already_there".to_owned())
}

/// The last alarm action, so the WebDriver test can tell Esc from D and Space from Enter.
/// L-032: drop it once the alarm state itself can be read.
#[tauri::command]
pub async fn alarms_last_action(app: AppHandle) -> Option<String> {
    app.try_state::<InterruptState>()
        .and_then(|s| s.last_action())
}

/// The interrupt on screen, for a window that loaded after `alarm:show` was emitted.
#[tauri::command]
pub async fn alarms_current(app: AppHandle) -> Option<AlarmShow> {
    app.try_state::<InterruptState>().and_then(|s| s.get())
}

/// Opens a sample interrupt on every screen (F10's test alarm, and the WebDriver test for the keys).
#[tauri::command]
pub async fn alarms_test(app: AppHandle) -> Result<(), CommandError> {
    let platform = interrupt::native();
    let screens = interrupt::screens(&app, platform).await;
    interrupt::open_interrupts(&app, platform, &screens, &AlarmShow::sample(Utc::now()))
        .map_err(|e| command_error(&e))
}

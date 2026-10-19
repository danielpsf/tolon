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

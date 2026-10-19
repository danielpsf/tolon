//! Tauri glue: shared state, the commands and the change events. The work lives in `pipeline`.
//!
//! Tauri command names cannot contain dots, so `calendars.list` is `calendars_list`, and so on
//! (names in docs/architecture.md, "Tauri commands"). Errors reach the UI as one human sentence.

use chrono::Utc;
use tauri::{AppHandle, Emitter, Manager, State};
use tolon_core::Occurrence;
use tolon_sync::IcsFetcher;

use crate::pipeline::{self, CalendarDto, Db};

pub struct AppState {
    pub db: Db,
    pub fetcher: IcsFetcher,
}

fn ui_err(e: anyhow::Error) -> String {
    e.to_string()
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
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| anyhow::anyhow!("Tolon is not ready yet."))?;
    let now = Utc::now();
    let result = match id {
        Some(id) => pipeline::sync_one(&state.db, &state.fetcher, id, now).await,
        None => pipeline::sync_all(&state.db, &state.fetcher, now).await,
    };
    notify(app);
    result
}

#[tauri::command]
pub async fn calendars_list(state: State<'_, AppState>) -> Result<Vec<CalendarDto>, String> {
    pipeline::list(&state.db).map_err(ui_err)
}

#[tauri::command]
pub async fn calendars_add_ics(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
) -> Result<CalendarDto, String> {
    let dto = pipeline::add_ics(&state.db, &state.fetcher, &url, Utc::now())
        .await
        .map_err(ui_err)?;
    notify(&app);
    Ok(dto)
}

#[tauri::command]
pub async fn calendars_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    pipeline::remove(&state.db, &id).map_err(ui_err)?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn calendars_set_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    pipeline::set_enabled(&state.db, &id, enabled).map_err(ui_err)?;
    notify(&app);
    Ok(())
}

/// With an id the calendar syncs even when disabled (the person asked); without one, only enabled ones.
#[tauri::command]
pub async fn calendars_sync_now(app: AppHandle, id: Option<String>) -> Result<(), String> {
    sync_and_notify(&app, id.as_deref()).await.map_err(ui_err)
}

#[tauri::command]
pub async fn occurrences_today(state: State<'_, AppState>) -> Result<Vec<Occurrence>, String> {
    pipeline::today(&state.db, Utc::now(), pipeline::local_zone()).map_err(ui_err)
}

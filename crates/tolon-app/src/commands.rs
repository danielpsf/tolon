//! Tauri glue: shared state, the commands and the change events. The work lives in `pipeline`.
//!
//! Tauri command names cannot contain dots, so `calendars.list` is `calendars_list`, and so on
//! (names in docs/architecture.md, "Tauri commands"). A failed command returns
//! `{ "code": "..." }` (see `errors`); the UI turns the code into localised copy.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tolon_platform::PlatformError;
use tolon_sync::IcsFetcher;

use crate::errors::{command_error, AppError, CommandError};
use crate::interrupt::{AlarmShow, InterruptState, LABEL_PREFIX};
use crate::pipeline::{self, CalendarDto, Db, ListedDto};
use crate::scheduler::{Act, Scheduler, TauriHost};
use crate::settings::{self, SettingsDto, SoundChoice};
use crate::tray::{self, PANEL};

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

/// The tray panel's own commands: pause, resume, Snooze and Dismiss ahead, Settings, Quit.
fn panel_only(label: &str) -> Result<(), CommandError> {
    if label == PANEL {
        Ok(())
    } else {
        Err(command_error(&AppError::NOT_FOUND.into()))
    }
}

/// Today's meetings, Join from the list and the pause state: the settings window (for the tray
/// tooltip) and the panel. Never an interrupt.
fn main_or_panel(label: &str) -> Result<(), CommandError> {
    panel_only(label).or_else(|_| main_only(label))
}

/// `settings_get` only: the interrupt windows and the panel read the settings too, to render in
/// the person's appearance and language (AC9.3). Reading is all they get.
fn main_panel_or_interrupt(label: &str) -> Result<(), CommandError> {
    if label.starts_with(LABEL_PREFIX) {
        Ok(())
    } else {
        main_or_panel(label)
    }
}

/// Runs `f` on the main thread, where every `Platform` call belongs, and waits for it. `None`
/// when the main thread cannot be reached.
async fn on_main<T: Send + 'static>(
    app: &AppHandle,
    f: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let sent = app.run_on_main_thread(move || {
        // The receiver only goes away when the caller does.
        let _ = tx.send(f());
    });
    match sent {
        Ok(()) => rx.await.ok(),
        Err(e) => {
            tracing::warn!(error = %e, "could not reach the main thread");
            None
        }
    }
}

/// The login item as the OS reports it now (AC9.4).
async fn login_state(app: &AppHandle) -> Result<tolon_platform::StartAtLogin, PlatformError> {
    on_main(app, || crate::interrupt::native().start_at_login())
        .await
        .unwrap_or(Err(PlatformError::Unsupported))
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
    main_or_panel(window.label())?;
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
    main_or_panel(window.label())?;
    let done = state(&app).and_then(|s| {
        let scheduler = app.try_state::<AppScheduler>().ok_or(AppError::NOT_READY)?;
        scheduler.join_listed(&s.db, &key)
    });
    if done.is_ok() {
        tray::hide_panel(&app);
    }
    done.map_err(|e| command_error(&e))
}

async fn listed_action(app: &AppHandle, key: &str, act: Act) -> Result<bool, CommandError> {
    let done = async {
        let state = state(app)?;
        let scheduler = app.try_state::<AppScheduler>().ok_or(AppError::NOT_READY)?;
        scheduler.act_listed(&state.db, key, act).await
    }
    .await;
    let ended = done.map_err(|e| command_error(&e))?;
    notify(app);
    Ok(ended)
}

/// What the panel's Snooze did. `ended`: the snooze would have come after the meeting's window
/// (start + 10 min), so its reminders ended instead and the panel says so (L-015 N7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SnoozeDto {
    pub ended: bool,
}

/// The panel's Snooze for one of today's meetings (AC7.2): the interrupt's Snooze when that
/// meeting is on screen, else its alarms come `minutes` (1 to 60) later than they were due.
#[tauri::command]
pub async fn occurrences_snooze(
    app: AppHandle,
    window: tauri::WebviewWindow,
    key: String,
    minutes: u32,
) -> Result<SnoozeDto, CommandError> {
    panel_only(window.label())?;
    let ended = listed_action(&app, &key, Act::Snooze(minutes)).await?;
    Ok(SnoozeDto { ended })
}

/// The panel's Dismiss for one of today's meetings (AC7.2): the interrupt's Dismiss when that
/// meeting is on screen, else none of its alarms takes over the screen.
#[tauri::command]
pub async fn occurrences_dismiss(
    app: AppHandle,
    window: tauri::WebviewWindow,
    key: String,
) -> Result<(), CommandError> {
    panel_only(window.label())?;
    listed_action(&app, &key, Act::Dismiss).await.map(|_| ())
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
    main_panel_or_interrupt(window.label())?;
    let dto = state(&app)
        .and_then(|s| settings::get(&s.db))
        .map_err(|e| command_error(&e))?;
    // Only the settings window shows the login item; the others need no trip to the OS.
    if window.label() == MAIN_WINDOW {
        return Ok(settings::with_login(dto, login_state(&app).await));
    }
    Ok(dto)
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
    let dto = state(&app)
        .and_then(|s| settings::change(&s.db, &patch, announce, || notify(&app)))
        .map_err(|e| command_error(&e))?;
    // Start at login (AC9.4, L-041): the OS login item follows the switch, and the answer is what
    // the OS reports afterwards. A refusal (on macOS, a build outside an app bundle) is
    // `login_item`; the preference stays saved, the switch shows the real state.
    if let Some(enabled) = patch
        .get("start_at_login")
        .and_then(serde_json::Value::as_bool)
    {
        let set = on_main(&app, move || {
            settings::set_login(crate::interrupt::native(), enabled)
        })
        .await;
        if !matches!(set, Some(Ok(_))) {
            return Err(command_error(&AppError::LOGIN_ITEM.into()));
        }
    }
    Ok(settings::with_login(dto, login_state(&app).await))
}

/// One shot of `sound` (`bell`, `chime` or `none`) for the Play button (AC6.3), unless
/// `Scheduler::preview` drops it.
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
    // A dropped preview (another one playing, or an interrupt open) is not an error.
    scheduler.preview(choice.into()).await;
    Ok(())
}

// ---- Tray and panel (F7) ---------------------------------------------------------------------

/// Whether alarms are paused, and until when (RFC 3339 UTC). Also the `app:paused` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PauseDto {
    pub paused_until: Option<String>,
}

impl PauseDto {
    pub fn of(until: Option<DateTime<Utc>>) -> Self {
        Self {
            paused_until: until.map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true)),
        }
    }
}

#[tauri::command]
pub async fn app_status(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<PauseDto, CommandError> {
    main_or_panel(window.label())?;
    state(&app)
        .and_then(|s| settings::paused_until(&pipeline::lock(&s.db), Utc::now()))
        .map(PauseDto::of)
        .map_err(|e| command_error(&e))
}

/// Pause for `minutes` (1 to 60; the panel sends 60), kept across a restart (S7.1, AC7.3). The
/// scheduler wakes, holds every alarm back until then and changes the tray icon.
#[tauri::command]
pub async fn app_pause(
    app: AppHandle,
    window: tauri::WebviewWindow,
    minutes: u32,
) -> Result<PauseDto, CommandError> {
    panel_only(window.label())?;
    let until = state(&app)
        .and_then(|s| settings::pause(&s.db, minutes, Utc::now()))
        .map_err(|e| command_error(&e))?;
    notify(&app);
    Ok(PauseDto::of(Some(until)))
}

#[tauri::command]
pub async fn app_resume(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<PauseDto, CommandError> {
    panel_only(window.label())?;
    state(&app)
        .and_then(|s| settings::resume(&s.db))
        .map_err(|e| command_error(&e))?;
    notify(&app);
    Ok(PauseDto::of(None))
}

/// The panel's Settings: the settings window shows and takes focus.
#[tauri::command]
pub async fn app_show_settings(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<(), CommandError> {
    panel_only(window.label())?;
    tray::show_settings(&app);
    Ok(())
}

/// The panel's Quit (AC7.4): the scheduler stops with the store settled, then the process exits.
#[tauri::command]
pub async fn app_quit(app: AppHandle, window: tauri::WebviewWindow) -> Result<(), CommandError> {
    panel_only(window.label())?;
    if let (Some(scheduler), Ok(state)) = (app.try_state::<AppScheduler>(), state(&app)) {
        scheduler.shutdown(&state.db).await;
    }
    app.exit(0);
    Ok(())
}

/// The tray tooltip, written by the settings window in the person's language (AC7.1). The text
/// can hold a meeting title: it goes to the OS tooltip only, never to a log.
#[tauri::command]
pub async fn tray_tooltip(
    app: AppHandle,
    window: tauri::WebviewWindow,
    text: String,
) -> Result<(), CommandError> {
    main_only(window.label())?;
    tray::set_tooltip(&app, &text);
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
    fn settings_get_answers_the_main_window_the_panel_and_the_interrupts_only() {
        for label in ["main", "panel", "interrupt-0", "interrupt-screen-2"] {
            assert_eq!(main_panel_or_interrupt(label), Ok(()), "{label:?}");
        }
        for label in ["interrupt", "Interrupt-0", "Panel", "panel-2", "main-2", ""] {
            assert_eq!(
                main_panel_or_interrupt(label),
                Err(CommandError { code: "not_found" }),
                "{label:?}"
            );
        }
    }

    #[test]
    fn ac7_2_panel_commands_answer_the_panel_and_listing_the_main_window_too() {
        assert_eq!(panel_only("panel"), Ok(()));
        for label in ["main", "interrupt-0", "Panel", "panel ", ""] {
            assert_eq!(
                panel_only(label),
                Err(CommandError { code: "not_found" }),
                "{label:?}"
            );
        }
        for label in ["main", "panel"] {
            assert_eq!(main_or_panel(label), Ok(()), "{label:?}");
        }
        for label in ["interrupt-0", "interrupt-panel", "Main", ""] {
            assert_eq!(
                main_or_panel(label),
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
            ("occurrences_today", "main_or_panel"),
            ("occurrences_join", "main_or_panel"),
            ("occurrences_snooze", "panel_only"),
            ("occurrences_dismiss", "panel_only"),
            ("alarms_test", "main_only"),
            ("settings_get", "main_panel_or_interrupt"),
            ("app_status", "main_or_panel"),
            ("app_pause", "panel_only"),
            ("app_resume", "panel_only"),
            ("app_show_settings", "panel_only"),
            ("app_quit", "panel_only"),
            ("tray_tooltip", "main_only"),
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

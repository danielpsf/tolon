//! The interrupt windows (F4): one borderless, full-screen, always-on-top window per screen, all
//! loading `interrupt.html`, and the payload they show.
//!
//! A window is built the first time its screen needs one and then kept: closing the interrupt
//! hides it, the next alarm shows it again. Destroying a window leaks its native window on macOS
//! (decision 2026-10-22, "Interrupt windows are reused"), so only a window whose screen is gone is
//! destroyed. "Open" means shown: a hidden window never counts.
//!
//! The scheduler (`scheduler.rs`) calls [`open_interrupts`] when an alarm fires. The windows ask for the
//! payload with `alarms_current` when they load and also listen for `alarm:show` / `alarm:hide`,
//! because an event emitted before a new window has registered its listener is lost.
//!
//! The payload never carries a join URL or a finished sentence: the UI gets the link's host (to
//! show where it goes, `docs/decisions.md` "Join links are treated as untrusted") and the
//! occurrence key it sends back with `alarms_join`; the app opens the URL itself.

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use anyhow::Context;
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::Serialize;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize,
    Position, Size, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use tolon_core::{EventId, InterruptGroup, JoinTarget, Occurrence, OccurrenceKey};
use tolon_platform::{Focus, Placement, Platform, PlatformError, Screen, Takeover, NATIVE};

pub const SHOW_EVENT: &str = "alarm:show";
pub const HIDE_EVENT: &str = "alarm:hide";
/// Every interrupt window label starts with this; `capabilities/interrupt.json` matches it.
pub const LABEL_PREFIX: &str = "interrupt-";

/// Where the join button leads, without the URL. The URL stays in Rust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JoinDto {
    /// `host` is lower-case, punycode as `xn--`; `recognised` is false for a link from the
    /// location field whose host is not a known call provider (the UI then shows the host).
    Link {
        host: String,
        recognised: bool,
    },
    EventPage,
    None,
}

/// One meeting in an interrupt. `key` is `OccurrenceKey`'s text form, sent back by the commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MeetingDto {
    pub key: String,
    pub title: String,
    /// RFC 3339 with the machine's offset.
    pub start: String,
    pub end: String,
    pub join: JoinDto,
    /// Attendee count; names are not sent.
    pub people: u32,
}

/// The `alarm:show` payload: every meeting that starts in the same minute (AC3.5, S3.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AlarmShow {
    /// True for the test alarm: the UI shows its own "Test alarm" title, not `title`.
    pub test: bool,
    pub meetings: Vec<MeetingDto>,
}

impl MeetingDto {
    fn from_occurrence(o: &Occurrence) -> Self {
        Self {
            key: o.key.to_string(),
            title: o.title.clone(),
            start: o.start.to_rfc3339_opts(SecondsFormat::Secs, false),
            end: o.end.to_rfc3339_opts(SecondsFormat::Secs, false),
            join: JoinDto::of(&o.join),
            people: o.people,
        }
    }
}

impl JoinDto {
    /// The join target as a page may see it: never the URL.
    pub(crate) fn of(target: &JoinTarget) -> Self {
        match target {
            JoinTarget::Link {
                host, recognised, ..
            } => Self::Link {
                host: host.clone(),
                recognised: *recognised,
            },
            JoinTarget::EventPage(_) => Self::EventPage,
            JoinTarget::None => Self::None,
        }
    }
}

impl AlarmShow {
    /// The scheduler calls this with each `group_firing` result.
    pub fn from_group(group: &InterruptGroup) -> Self {
        Self {
            test: false,
            meetings: group
                .occurrences
                .iter()
                .map(MeetingDto::from_occurrence)
                .collect(),
        }
    }

    /// What `alarms_test` shows: one meeting in 2 minutes whose link is not a known provider, so
    /// the host line is visible. The key parses like a real one; no command acts on it.
    pub fn sample(now: DateTime<Utc>) -> Self {
        let start = now + Duration::minutes(2);
        let key = OccurrenceKey {
            event: EventId("tolon-test".to_owned()),
            original_start: start,
        };
        Self {
            test: true,
            meetings: vec![MeetingDto {
                key: key.to_string(),
                title: String::new(),
                start: start.to_rfc3339_opts(SecondsFormat::Secs, true),
                end: (start + Duration::minutes(45)).to_rfc3339_opts(SecondsFormat::Secs, true),
                join: JoinDto::Link {
                    host: "example.com".to_owned(),
                    recognised: false,
                },
                people: 8,
            }],
        }
    }
}

/// The payload of the interrupt on screen, for windows that load after `alarm:show` was sent.
#[derive(Default)]
pub struct InterruptState {
    current: Mutex<Option<AlarmShow>>,
    /// The platform refused keyboard focus (`Focus::No`) when it raised a window of this interrupt.
    /// Written on the main thread, read by the scheduler's 500 ms focus re-check; cleared on close.
    focus_refused: AtomicBool,
}

impl InterruptState {
    fn set(&self, payload: Option<AlarmShow>) {
        // A poisoned lock only means another thread panicked; the value is still a plain Option.
        if payload.is_none() {
            self.focus_refused.store(false, Ordering::SeqCst);
        }
        *self.current.lock().unwrap_or_else(|p| p.into_inner()) = payload;
    }

    fn note_focus(&self, focus: Focus) {
        if focus == Focus::No {
            self.focus_refused.store(true, Ordering::SeqCst);
        }
    }

    /// True once a raise of this interrupt reported `Focus::No` (decision "Platform contract after L-012").
    pub fn focus_refused(&self) -> bool {
        self.focus_refused.load(Ordering::SeqCst)
    }

    pub fn get(&self) -> Option<AlarmShow> {
        self.current
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
}

/// `interrupt-<screen id>`.
pub fn window_label(screen_id: u32) -> String {
    format!("{LABEL_PREFIX}{screen_id}")
}

/// The single page, told which screen it is on.
pub fn window_path(screen_id: u32) -> String {
    format!("interrupt.html?screen={screen_id}")
}

/// What to build for one screen. Pure, so "one window per screen" is testable without a display.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowSpec {
    pub label: String,
    pub path: String,
    pub placement: Placement,
}

/// One spec per screen, placed with `Screen::placement()` (the platform contract after L-012).
pub fn plan_windows(screens: &[Screen]) -> Vec<WindowSpec> {
    screens
        .iter()
        .map(|s| WindowSpec {
            label: window_label(s.id),
            path: window_path(s.id),
            placement: s.placement(),
        })
        .collect()
}

/// A Tauri monitor as a [`Screen`], for platforms where `Platform::screens` is unsupported (Linux).
fn screen_of_monitor(index: u32, monitor: &tauri::Monitor, is_main: bool) -> Screen {
    let scale = monitor.scale_factor();
    let (position, size) = (monitor.position(), monitor.size());
    Screen {
        id: index,
        x: f64::from(position.x) / scale,
        y: f64::from(position.y) / scale,
        width: f64::from(size.width) / scale,
        height: f64::from(size.height) / scale,
        physical_x: position.x,
        physical_y: position.y,
        physical_width: size.width,
        physical_height: size.height,
        scale_factor: scale,
        is_main,
    }
}

/// The platform of this build, as the `'static` reference the main-thread calls need.
pub fn native() -> &'static dyn Platform {
    &NATIVE
}

/// Tauri's monitors as screens, for platforms where `Platform::screens` is unsupported (Linux).
pub(crate) fn monitor_screens(app: &AppHandle) -> Vec<Screen> {
    let primary = app.primary_monitor().ok().flatten();
    let monitors = app.available_monitors().unwrap_or_default();
    monitors
        .iter()
        .zip(0u32..)
        .map(|(m, i)| {
            let is_main = primary
                .as_ref()
                .is_some_and(|p| p.position() == m.position() && p.size() == m.size());
            screen_of_monitor(i, m, is_main)
        })
        .collect()
}

/// The platform's answer, or `fallback()` when it has none: an empty list, `Unsupported`, or a failure.
pub fn screens_or(
    answer: Result<Vec<Screen>, PlatformError>,
    fallback: impl FnOnce() -> Vec<Screen>,
) -> Vec<Screen> {
    match answer {
        Ok(list) if !list.is_empty() => list,
        Ok(_) | Err(PlatformError::Unsupported) => fallback(),
        Err(e) => {
            tracing::warn!(error = %e, "platform screens failed; using Tauri monitors");
            fallback()
        }
    }
}

/// Every connected screen. `Platform::screens` runs on the main thread (macOS refuses anywhere
/// else, and that is the call the L-012 enumeration needs); a command runs on a worker thread,
/// so the call is sent over and awaited. Where the platform has no list, Tauri's monitors stand in.
pub async fn screens(app: &AppHandle, platform: &'static dyn Platform) -> Vec<Screen> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let sent = app.run_on_main_thread(move || {
        // The receiver only goes away when the caller does.
        let _ = tx.send(platform.screens());
    });
    let answer = match sent {
        Ok(()) => rx.await.unwrap_or(Err(PlatformError::Unsupported)),
        Err(e) => {
            tracing::warn!(error = %e, "could not reach the main thread for the screen list");
            Err(PlatformError::Unsupported)
        }
    };
    screens_or(answer, || monitor_screens(app))
}

/// `Platform::takeover_support`, asked on the main thread like every platform call. A main thread
/// that cannot be reached counts as degraded: the fallback is the safe side.
pub async fn takeover(app: &AppHandle, platform: &'static dyn Platform) -> Takeover {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let sent = app.run_on_main_thread(move || {
        let _ = tx.send(platform.takeover_support());
    });
    match (sent, rx.await) {
        (Ok(()), Ok(support)) => support,
        _ => Takeover::Degraded(tolon_platform::Reason::Unsupported),
    }
}

pub(crate) fn place(window: &WebviewWindow, placement: Placement) -> tauri::Result<()> {
    match placement {
        Placement::Logical {
            x,
            y,
            width,
            height,
        } => {
            window.set_size(Size::Logical(LogicalSize::new(width, height)))?;
            window.set_position(Position::Logical(LogicalPosition::new(x, y)))
        }
        Placement::Physical {
            x,
            y,
            width,
            height,
        } => {
            window.set_size(Size::Physical(PhysicalSize::new(width, height)))?;
            window.set_position(Position::Physical(PhysicalPosition::new(x, y)))
        }
    }
}

/// One interrupt window as the windowing system reports it. A hidden window exists but is not
/// open: it takes no keys, clicks or focus, and nothing that asks "is an interrupt up" counts it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowState {
    pub label: String,
    pub visible: bool,
    pub focused: bool,
}

/// What opening the interrupt needs from the windowing system. Tauri implements it; tests use a fake.
pub trait Windows {
    /// Every interrupt window that exists, shown or hidden.
    fn states(&self) -> Vec<WindowState>;
    /// Shows the window for `spec` on its screen: builds it the first time and reuses it after that.
    fn open(&self, spec: &WindowSpec) -> anyhow::Result<()>;
    /// Puts a shown window above everything and asks for focus.
    fn raise(&self, label: &str) -> anyhow::Result<()>;
    /// Takes a window off the screen and out of the key and focus chain, and keeps it for the
    /// next alarm. A window that does not exist is ignored.
    fn hide(&self, label: &str);
    /// Closes a window for good. Only for a window whose screen is gone: closing leaks the
    /// native window on macOS (decision 2026-10-22, "Interrupt windows are reused").
    fn destroy(&self, label: &str);
    /// Notes the app that is in front, before anything of Tolon's comes forward.
    fn remember_frontmost(&self);
    /// Gives the front back to the app noted by `remember_frontmost`, once no window is left.
    fn restore_previous(&self);
    /// Drops the app noted by `remember_frontmost` without bringing it forward.
    fn forget_frontmost(&self);
    /// Hands the front on once no window is left after a Join: to the app noted for the link, else
    /// Tolon gives up its activation (L-060, L-061). Not while the settings window or the panel is on
    /// screen: Tolon keeps the front then and drops the app noted for the link (L-063). That is
    /// read when the call runs, on the main thread, so a window opened since it was queued counts.
    fn yield_unless_own_window_shown(&self);
    /// Takes Tolon out of Cmd-Tab and the Dock (the Accessory policy) before a reminder is raised,
    /// so it covers another app's full-screen Space (L-042, L-068). Queued ahead of the raise.
    fn leave_regular(&self);
    /// Puts Tolon back in Cmd-Tab and the Dock once the reminder is gone and the settings window is
    /// still shown (L-068). Whether it is, and whether a reminder window is up again, is read when
    /// the call runs, on the main thread. Queued after the focus hand-over.
    fn resume_regular(&self);
}

struct TauriWindows<'a> {
    app: &'a AppHandle,
    platform: &'static dyn Platform,
}

impl Windows for TauriWindows<'_> {
    fn states(&self) -> Vec<WindowState> {
        let mut states: Vec<WindowState> = self
            .app
            .webview_windows()
            .into_iter()
            .filter(|(label, _)| label.starts_with(LABEL_PREFIX))
            .map(|(label, window)| WindowState {
                label,
                visible: window.is_visible().unwrap_or(false),
                focused: window.is_focused().unwrap_or(false),
            })
            .collect();
        states.sort_by(|a, b| a.label.cmp(&b.label));
        states
    }

    fn open(&self, spec: &WindowSpec) -> anyhow::Result<()> {
        let physical = matches!(spec.placement, Placement::Physical { .. });
        let existing = self.app.get_webview_window(&spec.label);
        let reused = existing.is_some();
        let window = match existing {
            Some(window) => {
                // Full screen pins a window to its monitor; leave it so the window can move to
                // the screen it belongs to now (a monitor may have come and gone).
                if physical {
                    window.set_fullscreen(false)?;
                }
                window
            }
            None => WebviewWindowBuilder::new(
                self.app,
                &spec.label,
                WebviewUrl::App(spec.path.clone().into()),
            )
            .title("Tolon")
            .decorations(false)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .skip_taskbar(true)
            .resizable(false)
            .closable(false)
            .focused(true)
            .visible(false)
            .build()
            .context("could not create an interrupt window")?,
        };
        place(&window, spec.placement).context("could not place an interrupt window")?;
        window
            .show()
            .context("could not show an interrupt window")?;
        // Physical placement is every OS but macOS. There, full screen on the window's own monitor
        // is what covers panels and the taskbar (GTK keep-above plus full screen on X11,
        // docs/architecture.md); on macOS native full screen would make a Space of its own, so the
        // window covers its screen by size and window level instead.
        if physical {
            window.set_fullscreen(true)?;
        }
        if reused {
            // A window built just now asks for focus itself; one that was hidden has to be asked.
            // A refusal costs focus, not the takeover: the raise and the F4.5 re-check follow.
            if let Err(e) = window.set_focus() {
                tracing::warn!(error = %e, "could not focus a reused interrupt window");
            }
        }
        Ok(())
    }

    fn raise(&self, label: &str) -> anyhow::Result<()> {
        let window = self
            .app
            .get_webview_window(label)
            .with_context(|| format!("interrupt window {label} is not open"))?;
        let platform = self.platform;
        let app = self.app.clone();
        // Platform calls run on the main thread on every OS; the focus answer comes back through `InterruptState`.
        self.app
            .run_on_main_thread(move || match platform.raise_above_everything(&window) {
                Ok(raised) => {
                    tracing::debug!(focus = ?raised.focus, "interrupt window raised");
                    if let Some(state) = app.try_state::<InterruptState>() {
                        state.note_focus(raised.focus);
                    }
                }
                Err(e) => tracing::warn!(error = %e, "could not raise the interrupt window"),
            })?;
        Ok(())
    }

    fn hide(&self, label: &str) {
        if let Some(window) = self.app.get_webview_window(label) {
            // Tauri's `hide` is `orderOut` on macOS, `SW_HIDE` on Windows and an unmap on X11: the
            // window leaves the screen and the key chain, so the app behind it gets the keys back.
            if let Err(e) = window.hide() {
                tracing::warn!(label, error = %e, "could not hide an interrupt window");
            }
        }
    }

    fn destroy(&self, label: &str) {
        if let Some(window) = self.app.get_webview_window(label) {
            // `destroy`, not `close`: the windows are not closable by the person.
            if let Err(e) = window.destroy() {
                tracing::warn!(label, error = %e, "could not destroy an interrupt window");
            }
        }
    }

    fn remember_frontmost(&self) {
        self.on_main_thread("remember the frontmost app", |p| p.remember_frontmost());
    }

    fn restore_previous(&self) {
        self.on_main_thread("restore the previous app", |p| p.restore_previous());
    }

    fn forget_frontmost(&self) {
        self.on_main_thread("forget the frontmost app", |p| p.forget_frontmost());
    }

    fn yield_unless_own_window_shown(&self) {
        let (app, platform) = (self.app.clone(), self.platform);
        let sent = self.app.run_on_main_thread(move || {
            if let Err(e) = yield_or_keep(own_window_shown(&app), platform) {
                tracing::warn!(error = %e, "could not give up the active app");
            }
        });
        if let Err(e) = sent {
            tracing::warn!(error = %e, "could not reach the main thread to give up the active app");
        }
    }

    fn leave_regular(&self) {
        self.on_main_thread("leave the Regular activation policy", |p| {
            p.set_regular(false)
        });
    }

    fn resume_regular(&self) {
        let (app, platform) = (self.app.clone(), self.platform);
        let sent = self.app.run_on_main_thread(move || {
            let settings = app
                .get_webview_window(crate::tray::MAIN)
                .is_some_and(|w| w.is_visible().unwrap_or(false));
            let reminder = window_states(&app).iter().any(|s| s.visible);
            if let Err(e) = regular_or_keep(settings, reminder, platform) {
                tracing::warn!(error = %e, "could not return to the Regular activation policy");
            }
        });
        if let Err(e) = sent {
            tracing::warn!(error = %e, "could not reach the main thread to return to Regular");
        }
    }
}

/// True while the settings window or the panel is on screen: Tolon keeps the front then.
fn own_window_shown(app: &AppHandle) -> bool {
    [crate::tray::MAIN, crate::tray::PANEL]
        .into_iter()
        .filter_map(|label| app.get_webview_window(label))
        .any(|w| w.is_visible().unwrap_or(false))
}

/// The main-thread end of a joined close. `shown`: settings or the panel is on screen, so nothing
/// is handed over and the app noted for the link is dropped, or a later close would activate it
/// (L-063).
fn yield_or_keep(shown: bool, platform: &dyn Platform) -> Result<(), PlatformError> {
    if shown {
        tracing::debug!("settle: skipped, the settings window or the panel is shown");
        platform.forget_join_target()
    } else {
        tracing::debug!("settle: yield the front after a join");
        platform.yield_focus()
    }
}

/// The main-thread end of a reminder's close (L-068): Tolon is a Regular app again only while the
/// settings window is shown and no reminder window is, because a Regular app's reminder stays off
/// another app's full-screen Space (L-042).
fn regular_or_keep(
    settings_shown: bool,
    reminder_shown: bool,
    platform: &dyn Platform,
) -> Result<(), PlatformError> {
    if settings_shown && !reminder_shown {
        tracing::debug!("settle: the settings window is shown, Regular again");
        platform.set_regular(true)
    } else {
        Ok(())
    }
}

impl TauriWindows<'_> {
    /// Runs a platform call on the main thread. Calls queue in order, so a remember sent before the
    /// windows are shown runs before they are raised, and a restore sent after the hides and after
    /// any hand-over to a waiting reminder runs last.
    fn on_main_thread(
        &self,
        what: &'static str,
        call: fn(&dyn Platform) -> Result<(), PlatformError>,
    ) {
        let platform = self.platform;
        let sent = self.app.run_on_main_thread(move || {
            if let Err(e) = call(platform) {
                tracing::warn!(error = %e, "could not {what}");
            }
        });
        if let Err(e) = sent {
            tracing::warn!(error = %e, "could not reach the main thread to {what}");
        }
    }
}

fn is_visible(states: &[WindowState], label: &str) -> bool {
    states.iter().any(|s| s.label == label && s.visible)
}

/// One window per screen: each screen's window is shown (built the first time, reused after) and
/// raised, and a window that is already shown is raised again. A window whose screen is gone is
/// destroyed; no other window ever is. If a window cannot be shown, the ones this call showed are
/// hidden again and the error returned; windows that were already shown stay.
///
/// An open that fails with no window left on screen forgets the app noted for the front: nothing
/// is up to give it back after, and a later notification action must not pull up a stale app.
pub fn open_windows(windows: &dyn Windows, screens: &[Screen]) -> anyhow::Result<()> {
    let opened = show_windows(windows, screens);
    if opened.is_err() && !windows.states().iter().any(|s| s.visible) {
        windows.forget_frontmost();
        windows.resume_regular();
    }
    opened
}

fn show_windows(windows: &dyn Windows, screens: &[Screen]) -> anyhow::Result<()> {
    anyhow::ensure!(!screens.is_empty(), "no screen to show the interrupt on");
    let specs = plan_windows(screens);
    let before = windows.states();
    // A window that is already up means Tolon is in front: the app to give the front back to was
    // noted when it first opened, and this call only adds a meeting to it.
    if !before.iter().any(|s| s.visible) {
        windows.remember_frontmost();
    }
    // Before any open or raise: with the settings window shown Tolon is a Regular app, and a
    // Regular app's reminder stays off a full-screen Space (L-042, L-068).
    windows.leave_regular();
    for gone in before
        .iter()
        .filter(|s| !specs.iter().any(|spec| spec.label == s.label))
    {
        windows.destroy(&gone.label);
    }
    let mut shown: Vec<String> = Vec::new();
    for spec in specs {
        let done = if is_visible(&before, &spec.label) {
            windows.raise(&spec.label)
        } else {
            shown.push(spec.label.clone());
            windows
                .open(&spec)
                .and_then(|()| windows.raise(&spec.label))
        };
        if let Err(e) = done {
            for label in &shown {
                windows.hide(label);
            }
            return Err(e);
        }
    }
    Ok(())
}

/// Hides every interrupt window (AC4.6: one action closes all screens) and keeps them.
pub fn hide_windows(windows: &dyn Windows) {
    for state in windows.states() {
        windows.hide(&state.label);
    }
}

/// True while a shown interrupt window has keyboard focus. A hidden window never counts, whatever
/// its toolkit still says about focus.
fn any_focused(states: &[WindowState]) -> bool {
    states.iter().any(|s| s.visible && s.focused)
}

/// The interrupt windows that exist and whether each is shown and focused.
pub fn window_states(app: &AppHandle) -> Vec<WindowState> {
    TauriWindows {
        app,
        platform: native(),
    }
    .states()
}

/// True while any interrupt window is shown: a reminder is up (L-068).
pub fn reminder_shown(app: &AppHandle) -> bool {
    window_states(app).iter().any(|s| s.visible)
}

/// True while a shown interrupt window has keyboard focus.
pub fn focused(app: &AppHandle) -> bool {
    any_focused(&window_states(app))
}

/// Shows `payload` on every screen. The payload is kept for windows that load after the event;
/// when the windows cannot be shown it is dropped again and nothing stays on screen.
/// The alarm is emitted before the windows are shown, so a hidden window that is being reused is
/// told what to render ahead of its show. The emit is asynchronous and gives no guarantee that the
/// page has rendered it by the time the window is on screen; the page keeps the last alarm it was
/// told, and a window built now has no listener yet and asks for the payload when it loads.
/// The scheduler re-checks focus about 500 ms later and runs the F4.5 fallback
/// (`docs/decisions.md`, "Platform contract after L-012").
pub fn open_interrupts(
    app: &AppHandle,
    platform: &'static dyn Platform,
    screens: &[Screen],
    payload: &AlarmShow,
) -> anyhow::Result<()> {
    let state = app.try_state::<InterruptState>();
    if let Some(state) = &state {
        state.set(Some(payload.clone()));
    }
    let shown = app
        .emit(SHOW_EVENT, payload)
        .map_err(anyhow::Error::from)
        .and_then(|()| open_windows(&TauriWindows { app, platform }, screens));
    if let Err(e) = shown {
        if let Some(state) = &state {
            state.set(None);
        }
        return Err(e);
    }
    Ok(())
}

/// Hides every interrupt window and tells any listener (AC4.6: one action closes all screens).
/// The windows stay for the next alarm. The front stays with Tolon: the scheduler settles it with
/// [`settle_focus`] once it knows no waiting reminder takes the screen.
pub fn close_all(app: &AppHandle) {
    if let Some(state) = app.try_state::<InterruptState>() {
        state.set(None);
    }
    hide_windows(&TauriWindows {
        app,
        platform: native(),
    });
    if let Err(e) = app.emit(HIDE_EVENT, ()) {
        tracing::warn!(event = HIDE_EVENT, error = %e, "could not emit event");
    }
}

/// Ends a reminder's claim on the front (decision 2026-10-22, "Closing a reminder gives focus
/// back", L-060 and L-061). `restore`: give the front back to the app that was in front when it
/// opened. Otherwise (Join: the browser has focus) drop the note and hand the front on, to the app
/// that opened the link or by giving up Tolon's activation; not while settings or the panel is
/// shown, which is read on the main thread when the hand-over runs. Hiding alone leaves Tolon the
/// active app. Every call is queued after the hides. Each path logs at debug, so a run on screen
/// shows which one was taken.
pub fn settle_focus(app: &AppHandle, restore: bool) {
    settle(
        &TauriWindows {
            app,
            platform: native(),
        },
        restore,
    );
}

fn settle(windows: &dyn Windows, restore: bool) {
    if restore {
        tracing::debug!("settle: restore the app that was in front");
        windows.restore_previous();
    } else {
        windows.forget_frontmost();
        windows.yield_unless_own_window_shown();
    }
    windows.resume_regular();
}

/// Notes the app that opens `url` (decision 2026-10-22, L-061), on the main thread and in order
/// with the other focus calls. The URL is not logged: it can carry a meeting password.
pub fn remember_join_target(app: &AppHandle, url: &str) {
    let platform = native();
    let url = url.to_owned();
    let sent = app.run_on_main_thread(move || {
        if let Err(e) = platform.remember_join_target(&url) {
            tracing::warn!(error = %e, "could not note the app that opens the join link");
        }
    });
    if let Err(e) = sent {
        tracing::warn!(error = %e, "could not reach the main thread to note the join link's app");
    }
}

/// Drops the app noted by [`remember_join_target`], on the main thread and in order with it: the
/// link did not open, so no close should hand the front to that app (L-063).
pub fn forget_join_target(app: &AppHandle) {
    TauriWindows {
        app,
        platform: native(),
    }
    .on_main_thread("drop the app noted for the join link", |p| {
        p.forget_join_target()
    });
}

/// The key a command was called with, checked against the text form the payload carries.
pub fn parse_key(key: &str) -> Result<OccurrenceKey, crate::errors::AppError> {
    OccurrenceKey::from_str(key).map_err(|_| crate::errors::AppError::NOT_FOUND)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Tz;

    type TestResult = anyhow::Result<()>;

    fn at(h: u32, m: u32) -> anyhow::Result<DateTime<Utc>> {
        Ok(
            DateTime::parse_from_rfc3339(&format!("2026-10-20T{h:02}:{m:02}:00Z"))?
                .with_timezone(&Utc),
        )
    }

    fn screen(id: u32, x: f64, scale: f64) -> Screen {
        Screen {
            id,
            x,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
            physical_x: (x * scale) as i32,
            physical_y: 0,
            physical_width: (1920.0 * scale) as u32,
            physical_height: (1080.0 * scale) as u32,
            scale_factor: scale,
            is_main: id == 1,
        }
    }

    fn occurrence(title: &str, join: JoinTarget) -> anyhow::Result<Occurrence> {
        let zone: Tz = "Europe/Berlin".parse()?;
        let noon = at(12, 0)?;
        let start = noon.with_timezone(&zone);
        Ok(Occurrence {
            key: OccurrenceKey {
                event: EventId(format!("cal:{title}")),
                original_start: noon,
            },
            event: EventId(format!("cal:{title}")),
            start,
            end: start + Duration::minutes(45),
            title: title.to_owned(),
            all_day: false,
            join,
            people: 3,
        })
    }

    #[test]
    fn s4_2_one_window_per_screen_with_unique_labels_and_the_screen_placement() {
        let screens = [screen(1, 0.0, 2.0), screen(2, 1920.0, 1.0)];
        let specs = plan_windows(&screens);
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].label, "interrupt-1");
        assert_eq!(specs[1].label, "interrupt-2");
        assert_eq!(specs[1].path, "interrupt.html?screen=2");
        for (spec, screen) in specs.iter().zip(&screens) {
            assert_eq!(spec.placement, screen.placement());
        }
    }

    #[test]
    fn s4_2_every_label_matches_the_capability_glob() {
        for id in [0, 1, u32::MAX] {
            assert!(window_label(id).starts_with(LABEL_PREFIX));
        }
    }

    #[test]
    fn s3_6_a_group_lists_every_meeting_with_its_own_key_and_join() -> TestResult {
        let group = InterruptGroup {
            minute: at(12, 0)?,
            occurrences: vec![
                occurrence(
                    "a",
                    JoinTarget::Link {
                        url: "https://meet.google.com/abc-defg-hij".to_owned(),
                        host: "meet.google.com".to_owned(),
                        recognised: true,
                    },
                )?,
                occurrence(
                    "b",
                    JoinTarget::EventPage("https://example.com/e".to_owned()),
                )?,
                occurrence("c", JoinTarget::None)?,
            ],
        };
        let show = AlarmShow::from_group(&group);
        assert!(!show.test);
        assert_eq!(show.meetings.len(), 3);
        assert_eq!(
            show.meetings[0].join,
            JoinDto::Link {
                host: "meet.google.com".to_owned(),
                recognised: true
            }
        );
        assert_eq!(show.meetings[1].join, JoinDto::EventPage);
        assert_eq!(show.meetings[2].join, JoinDto::None);
        assert_eq!(show.meetings[0].start, "2026-10-20T14:00:00+02:00");
        // Each key parses back to the occurrence it came from.
        for (dto, occ) in show.meetings.iter().zip(&group.occurrences) {
            assert_eq!(parse_key(&dto.key).ok(), Some(occ.key.clone()));
        }
        Ok(())
    }

    #[test]
    fn ac1_3_the_payload_never_carries_a_join_or_event_url() -> TestResult {
        let group = InterruptGroup {
            minute: at(12, 0)?,
            occurrences: vec![
                occurrence(
                    "a",
                    JoinTarget::Link {
                        url: "https://zoom.us/j/123?pwd=SECRET-TOKEN".to_owned(),
                        host: "zoom.us".to_owned(),
                        recognised: true,
                    },
                )?,
                occurrence(
                    "b",
                    JoinTarget::EventPage("https://example.com/SECRET-PAGE".to_owned()),
                )?,
            ],
        };
        let json = serde_json::to_string(&AlarmShow::from_group(&group))?;
        assert!(!json.contains("SECRET"), "{json}");
        assert!(json.contains(r#"{"kind":"link","host":"zoom.us","recognised":true}"#));
        Ok(())
    }

    #[test]
    fn s4_3_the_sample_shows_an_unrecognised_host_and_a_parseable_key() -> TestResult {
        let show = AlarmShow::sample(at(13, 58)?);
        assert!(show.test);
        let [meeting] = show.meetings.as_slice() else {
            anyhow::bail!("the sample has exactly one meeting");
        };
        assert_eq!(
            meeting.join,
            JoinDto::Link {
                host: "example.com".to_owned(),
                recognised: false
            }
        );
        assert_eq!(meeting.start, "2026-10-20T14:00:00Z");
        assert!(parse_key(&meeting.key).is_ok());
        assert!(parse_key("no at sign").is_err());
        Ok(())
    }

    #[test]
    fn a_refused_focus_is_remembered_until_the_interrupt_closes() {
        let state = InterruptState::default();
        state.note_focus(Focus::Unknown);
        state.note_focus(Focus::Yes);
        assert!(!state.focus_refused());
        state.note_focus(Focus::No);
        assert!(state.focus_refused());
        state.set(None);
        assert!(!state.focus_refused());
    }

    #[test]
    fn the_state_holds_the_current_payload_until_cleared() -> TestResult {
        let state = InterruptState::default();
        assert_eq!(state.get(), None);
        let show = AlarmShow::sample(at(13, 58)?);
        state.set(Some(show.clone()));
        assert_eq!(state.get(), Some(show));
        state.set(None);
        assert_eq!(state.get(), None);
        Ok(())
    }

    use std::cell::RefCell;
    use tolon_platform::StubPlatform;

    /// A windowing system that records what it was asked and can be told to fail an open. A window
    /// is in `windows` from its first open until it is destroyed; `visible` says if it is shown.
    #[derive(Default)]
    struct FakeWindows {
        windows: RefCell<Vec<WindowState>>,
        calls: RefCell<Vec<String>>,
        /// How many times each label was built, as opposed to reused.
        built: RefCell<Vec<String>>,
        fail_open_of: Option<String>,
        /// Settings or the panel is on screen.
        own_window: std::cell::Cell<bool>,
        /// Hand-overs queued for the main thread and not run yet: the check is read by `run_queued`.
        queued: std::cell::Cell<usize>,
    }

    impl FakeWindows {
        fn with(shown: &[&str], hidden: &[&str]) -> Self {
            let state = |label: &&str, visible| WindowState {
                label: (*label).to_owned(),
                visible,
                focused: visible,
            };
            let windows = shown
                .iter()
                .map(|l| state(l, true))
                .chain(hidden.iter().map(|l| state(l, false)))
                .collect();
            Self {
                windows: RefCell::new(windows),
                ..Self::default()
            }
        }
        /// What was asked, without the activation policy switches (L-068), which have tests of
        /// their own through `all_calls`.
        fn calls(&self) -> Vec<String> {
            let mut calls = self.all_calls();
            calls.retain(|c| c != "accessory" && c != "regular");
            calls
        }
        fn all_calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
        /// The main thread gets to the queued hand-overs, and reads the windows as they are now.
        fn run_queued(&self) {
            for _ in 0..self.queued.replace(0) {
                let call = if self.own_window.get() {
                    "keep"
                } else {
                    "yield"
                };
                self.calls.borrow_mut().push(call.to_owned());
            }
        }
        fn labels(&self) -> Vec<String> {
            let mut labels: Vec<String> = self
                .windows
                .borrow()
                .iter()
                .map(|w| w.label.clone())
                .collect();
            labels.sort();
            labels
        }
        fn shown(&self) -> Vec<String> {
            let mut shown: Vec<String> = self
                .windows
                .borrow()
                .iter()
                .filter(|w| w.visible)
                .map(|w| w.label.clone())
                .collect();
            shown.sort();
            shown
        }
    }

    /// Windows whose yield goes to the stub platform, to see what it was handed; `.0` is whether
    /// settings or the panel is shown.
    struct StubWindows(bool);

    impl Windows for StubWindows {
        fn states(&self) -> Vec<WindowState> {
            Vec::new()
        }
        fn open(&self, _spec: &WindowSpec) -> anyhow::Result<()> {
            Ok(())
        }
        fn raise(&self, _label: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn hide(&self, _label: &str) {}
        fn destroy(&self, _label: &str) {}
        fn remember_frontmost(&self) {}
        fn restore_previous(&self) {}
        fn forget_frontmost(&self) {}
        fn yield_unless_own_window_shown(&self) {
            let _ = yield_or_keep(self.0, &StubPlatform);
        }
        fn leave_regular(&self) {}
        fn resume_regular(&self) {}
    }

    impl Windows for FakeWindows {
        fn states(&self) -> Vec<WindowState> {
            self.windows.borrow().clone()
        }
        fn open(&self, spec: &WindowSpec) -> anyhow::Result<()> {
            self.calls.borrow_mut().push(format!("open {}", spec.label));
            if self.fail_open_of.as_deref() == Some(spec.label.as_str()) {
                anyhow::bail!("no window for {}", spec.label);
            }
            let mut windows = self.windows.borrow_mut();
            if let Some(w) = windows.iter_mut().find(|w| w.label == spec.label) {
                w.visible = true;
                w.focused = true;
            } else {
                self.built.borrow_mut().push(spec.label.clone());
                windows.push(WindowState {
                    label: spec.label.clone(),
                    visible: true,
                    focused: true,
                });
            }
            Ok(())
        }
        fn raise(&self, label: &str) -> anyhow::Result<()> {
            self.calls.borrow_mut().push(format!("raise {label}"));
            Ok(())
        }
        fn hide(&self, label: &str) {
            self.calls.borrow_mut().push(format!("hide {label}"));
            // A toolkit can keep reporting focus on a window it has hidden; `focused` stays.
            if let Some(w) = self
                .windows
                .borrow_mut()
                .iter_mut()
                .find(|w| w.label == label)
            {
                w.visible = false;
            }
        }
        fn destroy(&self, label: &str) {
            self.calls.borrow_mut().push(format!("destroy {label}"));
            self.windows.borrow_mut().retain(|w| w.label != label);
        }
        fn remember_frontmost(&self) {
            self.calls.borrow_mut().push("remember".to_owned());
        }
        fn restore_previous(&self) {
            self.calls.borrow_mut().push("restore".to_owned());
        }
        fn forget_frontmost(&self) {
            self.calls.borrow_mut().push("forget".to_owned());
        }
        fn yield_unless_own_window_shown(&self) {
            self.calls.borrow_mut().push("queue yield".to_owned());
            self.queued.set(self.queued.get() + 1);
        }
        fn leave_regular(&self) {
            self.calls.borrow_mut().push("accessory".to_owned());
        }
        fn resume_regular(&self) {
            self.calls.borrow_mut().push("regular".to_owned());
        }
    }

    #[test]
    fn l060_a_joined_close_forgets_then_yields_once() {
        let windows = FakeWindows::with(&[], &["interrupt-1"]);
        settle(&windows, false);
        windows.run_queued();
        assert_eq!(windows.calls(), ["forget", "queue yield", "yield"]);
    }

    #[test]
    fn l060_a_close_without_a_join_restores_and_does_not_yield() {
        let windows = FakeWindows::with(&[], &["interrupt-1"]);
        settle(&windows, true);
        windows.run_queued();
        assert_eq!(windows.calls(), ["restore"]);
    }

    #[test]
    fn l060_settings_or_the_panel_on_screen_keeps_the_front_after_a_join() {
        let windows = FakeWindows::default();
        windows.own_window.set(true);
        settle(&windows, false);
        windows.run_queued();
        assert_eq!(
            windows.calls(),
            ["forget", "queue yield", "keep"],
            "no yield, no restore"
        );
    }

    #[test]
    fn l061_settings_shown_after_a_close_without_a_join_still_restores() {
        let windows = FakeWindows::default();
        windows.own_window.set(true);
        settle(&windows, true);
        windows.run_queued();
        assert_eq!(
            windows.calls(),
            ["restore"],
            "the guard is for the join path only"
        );
    }

    /// L-056: the front goes back to the app that was in front when settings is open and nothing
    /// was joined. Settings was in front only because the person opened it; the app noted before
    /// the reminder still gets the front back, and nothing is queued that could read the guard.
    #[test]
    fn l063_restore_still_runs_with_settings_visible_and_nothing_joined() {
        let windows = FakeWindows::with(&["interrupt-1"], &[]);
        windows.own_window.set(true);
        hide_windows(&windows);
        settle(&windows, true);
        windows.run_queued();
        assert_eq!(windows.calls(), ["hide interrupt-1", "restore"]);
        assert_eq!(windows.queued.get(), 0);
    }

    #[test]
    fn l061_settings_hidden_again_after_a_join_yields() {
        // Open, then hidden: the check reads what is shown when it runs.
        let windows = FakeWindows::default();
        windows.own_window.set(true);
        settle(&windows, false);
        windows.own_window.set(false);
        windows.run_queued();
        assert_eq!(windows.calls(), ["forget", "queue yield", "yield"]);
    }

    /// L-063: the hand-over is queued for the main thread, and settings can open in the gap before
    /// it runs. The check is made when it runs, so that window keeps the front.
    #[test]
    fn l063_a_window_opened_after_the_hand_over_was_queued_keeps_the_front() {
        let windows = FakeWindows::default();
        settle(&windows, false);
        windows.own_window.set(true);
        windows.run_queued();
        assert_eq!(windows.calls(), ["forget", "queue yield", "keep"]);
    }

    /// L-068: a reminder opens with settings shown. The Accessory switch is queued after the app is
    /// noted and before anything is opened or raised, so L-042 holds.
    #[test]
    fn l068_a_reminder_switches_to_accessory_before_the_first_open_and_raise() -> TestResult {
        let windows = FakeWindows::default();
        open_windows(&windows, &[screen(1, 0.0, 2.0), screen(2, 1920.0, 1.0)])?;
        assert_eq!(
            windows.all_calls(),
            [
                "remember",
                "accessory",
                "open interrupt-1",
                "raise interrupt-1",
                "open interrupt-2",
                "raise interrupt-2"
            ]
        );
        Ok(())
    }

    /// A second meeting joins a reminder that is up: the switch repeats ahead of the raise, harmless.
    #[test]
    fn l068_a_reminder_that_is_up_is_raised_after_the_switch() -> TestResult {
        let windows = FakeWindows::with(&["interrupt-1"], &[]);
        open_windows(&windows, &[screen(1, 0.0, 2.0)])?;
        assert_eq!(windows.all_calls(), ["accessory", "raise interrupt-1"]);
        Ok(())
    }

    /// L-068: the close asks for Regular after the focus hand-over, on both paths.
    #[test]
    fn l068_a_close_goes_back_to_regular_after_the_hand_over() {
        let windows = FakeWindows::with(&[], &["interrupt-1"]);
        settle(&windows, true);
        assert_eq!(windows.all_calls(), ["restore", "regular"]);
        let windows = FakeWindows::with(&[], &["interrupt-1"]);
        settle(&windows, false);
        windows.run_queued();
        assert_eq!(
            windows.all_calls(),
            ["forget", "queue yield", "regular", "yield"],
            "the Regular request is queued after the yield request"
        );
    }

    /// L-068: a reminder that fails to open with none on screen still hands Regular back.
    #[test]
    fn l068_a_failed_open_with_nothing_on_screen_goes_back_to_regular() {
        let windows = FakeWindows {
            fail_open_of: Some("interrupt-1".to_owned()),
            ..FakeWindows::default()
        };
        assert!(open_windows(&windows, &[screen(1, 0.0, 2.0)]).is_err());
        assert_eq!(
            windows.all_calls().last().map(String::as_str),
            Some("regular")
        );
    }

    /// L-068: Regular only with the settings window shown and no reminder window shown.
    #[test]
    fn l068_regular_only_while_settings_is_shown_and_no_reminder_is() {
        assert_eq!(regular_or_keep(true, false, &StubPlatform), Ok(()));
        assert_eq!(StubPlatform.policy_calls(), [true]);
        assert_eq!(regular_or_keep(true, true, &StubPlatform), Ok(()));
        assert_eq!(regular_or_keep(false, false, &StubPlatform), Ok(()));
        assert_eq!(StubPlatform.policy_calls(), [true], "no other call");
    }

    /// A source check: the Regular request reads both windows inside the main-thread closure, when
    /// it runs, and the Accessory request goes through the same queue as the raise.
    #[test]
    fn l068_the_switches_go_through_the_main_thread_queue() {
        let shipped = include_str!("interrupt.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap_or_default();
        let resume = shipped
            .split("fn resume_regular(&self) {")
            .nth(1)
            .and_then(|rest| rest.split("\n    }\n").next())
            .unwrap_or_default();
        assert!(resume.contains("run_on_main_thread(move ||"), "{resume}");
        assert!(resume.contains("regular_or_keep(settings, reminder, platform)"));
        let leave = shipped
            .split("fn leave_regular(&self) {")
            .nth(1)
            .and_then(|rest| rest.split("\n    }\n").next())
            .unwrap_or_default();
        assert!(leave.contains("on_main_thread("), "{leave}");
        assert!(leave.contains("set_regular(false)"));
    }

    /// The stub platform keeps one join target for the whole process.
    static STUB_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// The target noted at Join is used once at the close; a second close has none left and falls
    /// back to deactivation (the stub counts it as a yield without a hand-over).
    #[test]
    fn l061_the_stub_hands_over_to_the_noted_target_once() {
        let _one_at_a_time = STUB_TESTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let yields = StubPlatform.yields();
        let handed = StubPlatform.handed_over().len();
        assert_eq!(
            StubPlatform.remember_join_target("https://meet.example.com/x"),
            Ok(())
        );
        settle(&StubWindows(false), false);
        assert_eq!(
            StubPlatform.handed_over()[handed..],
            ["https://meet.example.com/x"]
        );
        settle(&StubWindows(false), false);
        assert_eq!(StubPlatform.yields(), yields + 2);
        assert_eq!(
            StubPlatform.handed_over().len(),
            handed + 1,
            "no target left"
        );
    }

    /// L-063: the guard skipped the yield, so the target is dropped. A later close, after the
    /// settings window is gone, must not activate an app that no join led to.
    #[test]
    fn l063_a_skipped_yield_drops_the_join_target() {
        let _one_at_a_time = STUB_TESTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (yields, handed) = (StubPlatform.yields(), StubPlatform.handed_over().len());
        assert_eq!(
            StubPlatform.remember_join_target("https://meet.example.com/x"),
            Ok(())
        );
        settle(&StubWindows(true), false);
        assert_eq!(StubPlatform.yields(), yields, "the yield was skipped");
        settle(&StubWindows(false), false);
        assert_eq!(StubPlatform.yields(), yields + 1);
        assert_eq!(
            StubPlatform.handed_over().len(),
            handed,
            "nothing is left to hand the front to"
        );
    }

    /// A source check: the yield is queued through the main-thread queue, and the settings and
    /// panel visibility is read inside its closure (L-063), not before it is queued.
    #[test]
    fn l060_the_yield_goes_through_the_main_thread_queue() {
        let source = include_str!("interrupt.rs");
        let shipped = source.split("#[cfg(test)]").next().unwrap_or_default();
        let queued = shipped
            .split("fn yield_unless_own_window_shown(&self) {")
            .nth(1)
            .and_then(|rest| rest.split("\n    }\n").next())
            .unwrap_or_default();
        assert!(queued.contains("run_on_main_thread(move ||"));
        assert!(queued.contains("yield_or_keep(own_window_shown(&app), platform)"));
        assert!(!shipped.contains("fn own_window_shown(&self)"));
    }

    #[test]
    fn s4_2_every_screen_gets_a_window_that_is_raised() -> TestResult {
        let windows = FakeWindows::default();
        open_windows(&windows, &[screen(1, 0.0, 2.0), screen(2, 1920.0, 1.0)])?;
        assert_eq!(
            windows.calls(),
            [
                "remember",
                "open interrupt-1",
                "raise interrupt-1",
                "open interrupt-2",
                "raise interrupt-2"
            ]
        );
        Ok(())
    }

    #[test]
    fn s4_2_a_second_call_raises_the_shown_windows_and_opens_none() -> TestResult {
        let windows = FakeWindows::with(&["interrupt-1", "interrupt-2"], &[]);
        open_windows(&windows, &[screen(1, 0.0, 2.0), screen(2, 1920.0, 1.0)])?;
        assert_eq!(windows.calls(), ["raise interrupt-1", "raise interrupt-2"]);
        Ok(())
    }

    #[test]
    fn s4_2_open_close_open_again_reuses_the_window_and_destroys_none() -> TestResult {
        let windows = FakeWindows::default();
        let screens = [screen(1, 0.0, 2.0), screen(2, 1920.0, 1.0)];
        for _ in 0..3 {
            open_windows(&windows, &screens)?;
            assert_eq!(windows.shown(), ["interrupt-1", "interrupt-2"]);
            hide_windows(&windows);
            assert!(windows.shown().is_empty());
            assert_eq!(windows.labels(), ["interrupt-1", "interrupt-2"]);
        }
        // Built once per screen, never again, never destroyed.
        assert_eq!(*windows.built.borrow(), ["interrupt-1", "interrupt-2"]);
        assert!(windows.calls().iter().all(|c| !c.starts_with("destroy")));
        assert_eq!(windows.labels().len(), 2, "at most one window per screen");
        Ok(())
    }

    #[test]
    fn s4_2_a_hidden_window_is_not_open_so_the_next_alarm_shows_and_raises_it() -> TestResult {
        let windows = FakeWindows::with(&[], &["interrupt-1"]);
        assert!(!is_visible(&windows.states(), "interrupt-1"));
        open_windows(&windows, &[screen(1, 0.0, 1.0)])?;
        // Not just raised: a hidden window has to be shown first.
        assert_eq!(
            windows.calls(),
            ["remember", "open interrupt-1", "raise interrupt-1"]
        );
        assert_eq!(*windows.built.borrow(), Vec::<String>::new());
        assert_eq!(windows.shown(), ["interrupt-1"]);
        Ok(())
    }

    #[test]
    fn s4_2_a_hidden_window_does_not_count_as_focused_even_if_the_toolkit_says_so() {
        let windows = FakeWindows::with(&["interrupt-1"], &[]);
        assert!(any_focused(&windows.states()));
        hide_windows(&windows);
        // The fake keeps `focused` set on the hidden window, as a toolkit can.
        assert!(windows.states().iter().all(|w| w.focused));
        assert!(!any_focused(&windows.states()));
    }

    #[test]
    fn s4_2_closing_hides_every_window_and_keeps_them() {
        let windows = FakeWindows::with(&["interrupt-1"], &["interrupt-2"]);
        hide_windows(&windows);
        assert_eq!(windows.shown(), Vec::<String>::new());
        assert_eq!(windows.labels(), ["interrupt-1", "interrupt-2"]);
        assert!(windows.calls().iter().all(|c| c.starts_with("hide ")));
    }

    #[test]
    fn s4_2_a_window_whose_screen_is_gone_is_destroyed_and_the_others_are_kept() -> TestResult {
        // Screens 1 and 2 had windows (2 hidden); only screen 1 is left.
        let windows = FakeWindows::with(&[], &["interrupt-1", "interrupt-2"]);
        open_windows(&windows, &[screen(1, 0.0, 1.0)])?;
        assert_eq!(
            windows.calls(),
            [
                "remember",
                "destroy interrupt-2",
                "open interrupt-1",
                "raise interrupt-1"
            ]
        );
        assert_eq!(windows.labels(), ["interrupt-1"]);
        // A shown window of a gone screen goes too.
        let windows = FakeWindows::with(&["interrupt-1", "interrupt-3"], &[]);
        open_windows(&windows, &[screen(1, 0.0, 1.0)])?;
        assert_eq!(windows.labels(), ["interrupt-1"]);
        Ok(())
    }

    #[test]
    fn s4_2_a_failed_open_hides_what_this_call_showed_and_keeps_older_windows() -> TestResult {
        // 1 was already shown, 2 is hidden and shows again, 3 fails. 2 is hidden again, 1 stays
        // shown, and nothing is destroyed.
        let windows = FakeWindows {
            fail_open_of: Some("interrupt-3".to_owned()),
            ..FakeWindows::with(&["interrupt-1"], &["interrupt-2"])
        };
        let screens = [
            screen(1, 0.0, 1.0),
            screen(2, 1920.0, 1.0),
            screen(3, 3840.0, 1.0),
        ];
        let Err(e) = open_windows(&windows, &screens) else {
            anyhow::bail!("the third window cannot open, so the call must fail");
        };
        assert!(e.to_string().contains("interrupt-3"), "{e}");
        assert_eq!(windows.shown(), ["interrupt-1"]);
        assert_eq!(windows.labels(), ["interrupt-1", "interrupt-2"]);
        assert_eq!(
            windows.calls(),
            [
                "raise interrupt-1",
                "open interrupt-2",
                "raise interrupt-2",
                "open interrupt-3",
                "hide interrupt-2",
                "hide interrupt-3"
            ]
        );
        Ok(())
    }

    /// A source check for the real windows, which no test can build: closing hides, and the only
    /// call that destroys is the one for a gone screen (the leak of L-047).
    #[test]
    fn s4_2_closing_hides_and_only_a_gone_screen_destroys() {
        let source = include_str!("interrupt.rs");
        let shipped = source.split("#[cfg(test)]").next().unwrap_or_default();
        let close = shipped
            .split("pub fn close_all(")
            .nth(1)
            .and_then(|rest| rest.split("\n}\n").next())
            .unwrap_or_default();
        assert!(close.contains("hide_windows("), "{close}");
        assert!(!close.contains("restore_previous"), "{close}");
        assert!(!close.contains("destroy"), "{close}");
        assert_eq!(shipped.matches("windows.destroy(").count(), 1);
        assert!(shipped.contains("window.hide()"));
    }

    /// A source check too: the alarm is emitted before the windows are shown, so a reused window
    /// is told what to render ahead of its show.
    #[test]
    fn s4_2_open_interrupts_emits_the_alarm_before_it_shows_the_windows() {
        let source = include_str!("interrupt.rs");
        let shipped = source.split("#[cfg(test)]").next().unwrap_or_default();
        let body = shipped
            .split("pub fn open_interrupts(")
            .nth(1)
            .and_then(|rest| rest.split("\n}\n").next())
            .unwrap_or_default();
        let emit = body.find(".emit(SHOW_EVENT").unwrap_or(usize::MAX);
        let open = body.find("open_windows(").unwrap_or(0);
        assert!(emit < open, "emit SHOW_EVENT, then open_windows: {body}");
    }

    #[test]
    fn l056_a_fresh_open_remembers_the_front_app_before_it_shows_anything() -> TestResult {
        let windows = FakeWindows::default();
        open_windows(&windows, &[screen(1, 0.0, 2.0), screen(2, 1920.0, 1.0)])?;
        assert_eq!(
            windows.calls(),
            [
                "remember",
                "open interrupt-1",
                "raise interrupt-1",
                "open interrupt-2",
                "raise interrupt-2"
            ]
        );
        // A reused (hidden) window is a fresh open too.
        let windows = FakeWindows::with(&[], &["interrupt-1"]);
        open_windows(&windows, &[screen(1, 0.0, 1.0)])?;
        assert_eq!(windows.calls()[0], "remember");
        Ok(())
    }

    #[test]
    fn l056_a_meeting_added_to_a_shown_interrupt_does_not_remember_again() -> TestResult {
        // Tolon is in front now; remembering it would replace the app to give the front back to.
        let windows = FakeWindows::with(&["interrupt-1", "interrupt-2"], &[]);
        open_windows(&windows, &[screen(1, 0.0, 2.0), screen(2, 1920.0, 1.0)])?;
        assert!(!windows.calls().iter().any(|c| c == "remember"));
        Ok(())
    }

    #[test]
    fn l056_an_open_that_fails_with_nothing_on_screen_forgets_the_front_app() -> TestResult {
        let windows = FakeWindows {
            fail_open_of: Some("interrupt-1".to_owned()),
            ..FakeWindows::default()
        };
        assert!(open_windows(&windows, &[screen(1, 0.0, 1.0)]).is_err());
        assert_eq!(windows.calls().last().map(String::as_str), Some("forget"));
        // No screen is a failed open too.
        let windows = FakeWindows::default();
        assert!(open_windows(&windows, &[]).is_err());
        assert_eq!(windows.calls(), ["forget"]);
        Ok(())
    }

    #[test]
    fn l056_an_open_that_fails_while_a_window_is_still_up_keeps_the_front_app() -> TestResult {
        // Interrupt 1 is still shown: its close will give the front back.
        let windows = FakeWindows {
            fail_open_of: Some("interrupt-2".to_owned()),
            ..FakeWindows::with(&["interrupt-1"], &[])
        };
        assert!(open_windows(&windows, &[screen(1, 0.0, 1.0), screen(2, 1920.0, 1.0)]).is_err());
        assert!(!windows.calls().iter().any(|c| c == "forget"));
        Ok(())
    }

    #[test]
    fn s4_2_no_screens_is_an_error_and_opens_nothing() {
        let windows = FakeWindows::default();
        assert!(open_windows(&windows, &[]).is_err());
        // Nothing was opened or raised; only the noted front app is dropped.
        assert_eq!(windows.calls(), ["forget"]);
    }

    #[test]
    fn s4_2_the_stub_platform_screens_are_used_when_it_has_them() -> TestResult {
        let from_stub = screens_or(tolon_platform::StubPlatform.screens(), Vec::new);
        assert_eq!(from_stub.len(), 1);
        assert!(from_stub[0].is_main);
        // The stub's screen is not replaced by the fallback.
        let fallback = vec![screen(9, 0.0, 1.0)];
        let kept = screens_or(tolon_platform::StubPlatform.screens(), || fallback.clone());
        assert_eq!(kept, from_stub);
        Ok(())
    }

    #[test]
    fn s4_2_screens_fall_back_to_monitors_when_the_platform_has_none() {
        let monitors = || vec![screen(7, 0.0, 1.0)];
        for answer in [
            Err(PlatformError::Unsupported),
            Err(PlatformError::NotMainThread),
            Ok(Vec::new()),
        ] {
            assert_eq!(screens_or(answer, monitors)[0].id, 7);
        }
    }

    #[test]
    fn native_is_a_platform_that_covers_the_stub_contract() {
        // Exactly one platform for this build, usable as `&'static dyn Platform`.
        assert!(!native().name().is_empty());
    }
}

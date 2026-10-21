//! The interrupt windows (F4): one borderless, full-screen, always-on-top window per screen, all
//! loading `interrupt.html`, and the payload they show.
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

/// What opening the interrupt needs from the windowing system. Tauri implements it; tests use a fake.
pub trait Windows {
    fn is_open(&self, label: &str) -> bool;
    /// Creates the window on its screen and shows it.
    fn open(&self, spec: &WindowSpec) -> anyhow::Result<()>;
    /// Puts an open window above everything and asks for focus.
    fn raise(&self, label: &str) -> anyhow::Result<()>;
    fn close(&self, label: &str);
}

struct TauriWindows<'a> {
    app: &'a AppHandle,
    platform: &'static dyn Platform,
}

impl Windows for TauriWindows<'_> {
    fn is_open(&self, label: &str) -> bool {
        self.app.get_webview_window(label).is_some()
    }

    fn open(&self, spec: &WindowSpec) -> anyhow::Result<()> {
        let window = WebviewWindowBuilder::new(
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
        .context("could not create an interrupt window")?;
        place(&window, spec.placement).context("could not place an interrupt window")?;
        window
            .show()
            .context("could not show an interrupt window")?;
        // Physical placement is every OS but macOS. There, full screen on the window's own monitor
        // is what covers panels and the taskbar (GTK keep-above plus full screen on X11,
        // docs/architecture.md); on macOS native full screen would make a Space of its own, so the
        // window covers its screen by size and window level instead.
        if matches!(spec.placement, Placement::Physical { .. }) {
            window.set_fullscreen(true)?;
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

    fn close(&self, label: &str) {
        if let Some(window) = self.app.get_webview_window(label) {
            // `destroy`, not `close`: the windows are not closable by the person.
            if let Err(e) = window.destroy() {
                tracing::warn!(label, error = %e, "could not close an interrupt window");
            }
        }
    }
}

/// One window per screen: new ones are opened and raised, windows that are already open are raised
/// again. If a window cannot be opened, the ones this call opened are closed and the error returned;
/// windows that were already open stay.
pub fn open_windows(windows: &dyn Windows, screens: &[Screen]) -> anyhow::Result<()> {
    anyhow::ensure!(!screens.is_empty(), "no screen to show the interrupt on");
    let mut opened: Vec<String> = Vec::new();
    for spec in plan_windows(screens) {
        let done = if windows.is_open(&spec.label) {
            windows.raise(&spec.label)
        } else {
            windows.open(&spec).and_then(|()| {
                opened.push(spec.label.clone());
                windows.raise(&spec.label)
            })
        };
        if let Err(e) = done {
            for label in &opened {
                windows.close(label);
            }
            return Err(e);
        }
    }
    Ok(())
}

/// Shows `payload` on every screen. The payload is kept for windows that load after the event;
/// when the windows cannot be opened it is dropped again and nothing stays on screen.
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
    if let Err(e) = open_windows(&TauriWindows { app, platform }, screens) {
        if let Some(state) = &state {
            state.set(None);
        }
        return Err(e);
    }
    app.emit(SHOW_EVENT, payload)?;
    Ok(())
}

/// Closes every interrupt window and tells any listener (AC4.6: one action closes all screens).
pub fn close_all(app: &AppHandle) {
    if let Some(state) = app.try_state::<InterruptState>() {
        state.set(None);
    }
    let windows = TauriWindows {
        app,
        platform: native(),
    };
    for (label, _) in app.webview_windows() {
        if label.starts_with(LABEL_PREFIX) {
            windows.close(&label);
        }
    }
    if let Err(e) = app.emit(HIDE_EVENT, ()) {
        tracing::warn!(event = HIDE_EVENT, error = %e, "could not emit event");
    }
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

    /// A windowing system that records what it was asked and can be told to fail an open.
    #[derive(Default)]
    struct FakeWindows {
        open: RefCell<Vec<String>>,
        calls: RefCell<Vec<String>>,
        fail_open_of: Option<String>,
    }

    impl FakeWindows {
        fn with_open(labels: &[&str]) -> Self {
            Self {
                open: RefCell::new(labels.iter().map(|l| (*l).to_owned()).collect()),
                ..Self::default()
            }
        }
        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl Windows for FakeWindows {
        fn is_open(&self, label: &str) -> bool {
            self.open.borrow().iter().any(|l| l == label)
        }
        fn open(&self, spec: &WindowSpec) -> anyhow::Result<()> {
            self.calls.borrow_mut().push(format!("open {}", spec.label));
            if self.fail_open_of.as_deref() == Some(spec.label.as_str()) {
                anyhow::bail!("no window for {}", spec.label);
            }
            self.open.borrow_mut().push(spec.label.clone());
            Ok(())
        }
        fn raise(&self, label: &str) -> anyhow::Result<()> {
            self.calls.borrow_mut().push(format!("raise {label}"));
            Ok(())
        }
        fn close(&self, label: &str) {
            self.calls.borrow_mut().push(format!("close {label}"));
            self.open.borrow_mut().retain(|l| l != label);
        }
    }

    #[test]
    fn s4_2_every_screen_gets_a_window_that_is_raised() -> TestResult {
        let windows = FakeWindows::default();
        open_windows(&windows, &[screen(1, 0.0, 2.0), screen(2, 1920.0, 1.0)])?;
        assert_eq!(
            windows.calls(),
            [
                "open interrupt-1",
                "raise interrupt-1",
                "open interrupt-2",
                "raise interrupt-2"
            ]
        );
        Ok(())
    }

    #[test]
    fn s4_2_a_second_call_raises_the_open_windows_and_opens_none() -> TestResult {
        let windows = FakeWindows::with_open(&["interrupt-1", "interrupt-2"]);
        open_windows(&windows, &[screen(1, 0.0, 2.0), screen(2, 1920.0, 1.0)])?;
        assert_eq!(windows.calls(), ["raise interrupt-1", "raise interrupt-2"]);
        Ok(())
    }

    #[test]
    fn s4_2_a_failed_open_closes_what_this_call_opened_and_keeps_older_windows() -> TestResult {
        // Screen 1 was already open; 2 opens; 3 fails. 2 is closed again, 1 stays.
        let windows = FakeWindows {
            fail_open_of: Some("interrupt-3".to_owned()),
            ..FakeWindows::with_open(&["interrupt-1"])
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
        assert_eq!(*windows.open.borrow(), ["interrupt-1"]);
        assert_eq!(
            windows.calls(),
            [
                "raise interrupt-1",
                "open interrupt-2",
                "raise interrupt-2",
                "open interrupt-3",
                "close interrupt-2"
            ]
        );
        Ok(())
    }

    #[test]
    fn s4_2_no_screens_is_an_error_and_opens_nothing() {
        let windows = FakeWindows::default();
        assert!(open_windows(&windows, &[]).is_err());
        assert!(windows.calls().is_empty());
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

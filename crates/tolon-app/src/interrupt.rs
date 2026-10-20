//! The interrupt windows (F4): one borderless, full-screen, always-on-top window per screen, all
//! loading `interrupt.html`, and the payload they show.
//!
//! The scheduler (L-032) calls [`open_interrupts`] when an alarm fires. The windows ask for the
//! payload with `alarms_current` when they load and also listen for `alarm:show` / `alarm:hide`,
//! because an event emitted before a new window has registered its listener is lost.
//!
//! The payload never carries a join URL or a finished sentence: the UI gets the link's host (to
//! show where it goes, `docs/decisions.md` "Join links are treated as untrusted") and the
//! occurrence key it sends back with `alarms_join`; the app opens the URL itself.

use std::str::FromStr;
use std::sync::Mutex;

use anyhow::Context;
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::Serialize;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize,
    Position, Size, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use tolon_core::{EventId, InterruptGroup, JoinTarget, Occurrence, OccurrenceKey};
use tolon_platform::{Placement, Platform, PlatformError, Screen, NATIVE};

pub const SHOW_EVENT: &str = "alarm:show";
pub const HIDE_EVENT: &str = "alarm:hide";
/// Every interrupt window label starts with this; `capabilities/interrupt.json` matches it.
pub const LABEL_PREFIX: &str = "interrupt-";

/// Where the join button leads, without the URL. The URL stays in Rust.
#[allow(dead_code)] // L-032 builds every variant through `AlarmShow::from_group`; until then only tests do.
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
    #[allow(dead_code)] // L-032: see `AlarmShow::from_group`
    fn from_occurrence(o: &Occurrence) -> Self {
        Self {
            key: o.key.to_string(),
            title: o.title.clone(),
            start: o.start.to_rfc3339_opts(SecondsFormat::Secs, false),
            end: o.end.to_rfc3339_opts(SecondsFormat::Secs, false),
            join: match &o.join {
                JoinTarget::Link {
                    host, recognised, ..
                } => JoinDto::Link {
                    host: host.clone(),
                    recognised: *recognised,
                },
                JoinTarget::EventPage(_) => JoinDto::EventPage,
                JoinTarget::None => JoinDto::None,
            },
            people: o.people,
        }
    }
}

impl AlarmShow {
    /// L-032 calls this with each `group_firing` result.
    #[allow(dead_code)]
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
}

impl InterruptState {
    fn set(&self, payload: Option<AlarmShow>) {
        // A poisoned lock only means another thread panicked; the value is still a plain Option.
        *self.current.lock().unwrap_or_else(|p| p.into_inner()) = payload;
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

/// Every connected screen: the platform's list, or Tauri's monitors where the platform has none.
pub fn screens(app: &AppHandle) -> Vec<Screen> {
    match NATIVE.screens() {
        Ok(list) if !list.is_empty() => return list,
        Ok(_) | Err(PlatformError::Unsupported) => {}
        Err(e) => tracing::warn!(error = %e, "platform screens failed; using Tauri monitors"),
    }
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

fn place(window: &WebviewWindow, placement: Placement) -> tauri::Result<()> {
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

/// Builds one window, hidden, places it on its screen, shows it, then raises it.
fn open_one(app: &AppHandle, spec: &WindowSpec) -> anyhow::Result<()> {
    let window =
        WebviewWindowBuilder::new(app, &spec.label, WebviewUrl::App(spec.path.clone().into()))
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
    let target = window.clone();
    // Platform calls run on the main thread on every OS.
    app.run_on_main_thread(move || match NATIVE.raise_above_everything(&target) {
        Ok(raised) => tracing::debug!(focus = ?raised.focus, "interrupt window raised"),
        Err(e) => tracing::warn!(error = %e, "could not raise the interrupt window"),
    })?;
    Ok(())
}

/// Shows `payload` on every screen: one window per screen, left alone when it is already open.
/// L-032: after this returns, re-check focus about 500 ms later and run the F4.5 fallback
/// (`docs/decisions.md`, "Platform contract after L-012").
pub fn open_interrupts(
    app: &AppHandle,
    screens: &[Screen],
    payload: &AlarmShow,
) -> anyhow::Result<()> {
    anyhow::ensure!(!screens.is_empty(), "no screen to show the interrupt on");
    if let Some(state) = app.try_state::<InterruptState>() {
        state.set(Some(payload.clone()));
    }
    for spec in plan_windows(screens) {
        if app.get_webview_window(&spec.label).is_none() {
            open_one(app, &spec)?;
        }
    }
    app.emit(SHOW_EVENT, payload)?;
    Ok(())
}

/// Closes every interrupt window and tells any listener (AC4.6: one action closes all screens).
pub fn close_all(app: &AppHandle) {
    if let Some(state) = app.try_state::<InterruptState>() {
        state.set(None);
    }
    for (label, window) in app.webview_windows() {
        if label.starts_with(LABEL_PREFIX) {
            // `destroy`, not `close`: the windows are not closable by the person.
            if let Err(e) = window.destroy() {
                tracing::warn!(label, error = %e, "could not close an interrupt window");
            }
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
}

//! The Tauri side of [`Host`]: windows through `interrupt`, sound through `SoundPlayer`, the
//! default browser through `tauri-plugin-opener`'s `open_url` (called from Rust only: the plugin is
//! not registered, so the webview has no opener command at all), and the F4.5 notification.
//!
//! The fallback notification's wording comes from `notify_copy`: the settings window hands over a
//! bundle from its catalogue, Rust keeps it, and the built-in English is used until one arrives.
//!
//! Action support of the notification plugin (tauri-plugin-notification 2.7, from its source):
//! - Linux: buttons when the notification server offers actions (GNOME, KDE and dunst do). Where it does
//!   not, the notification still shows, without them.
//! - macOS: two or more actions become an "Options" menu, and a click on the menu button itself reports
//!   the first action, so Snooze is listed first on every OS: the accidental one is harmless.
//! - Windows: buttons, but a click is reported only while the toast is on screen, not once it sits in
//!   the Action Center. Installed builds only: a dev build shows PowerShell's name and icon.

use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::{Action, ActionType, NotificationExt};
use tolon_platform::{Playback, Sound, SoundPlayer, Takeover};

use super::{Act, Host, Opened, SNOOZE_MINUTES};
use crate::commands::{self, AppScheduler};
use crate::interrupt::{self, AlarmShow, InterruptState, LABEL_PREFIX};
use crate::notify_copy::{NotifyCopy, NotifyCopyState};
use crate::pipeline;

const ACTION_TYPE: &str = "alarm";
const JOIN: &str = "join";
const SNOOZE: &str = "snooze";

pub struct TauriHost {
    app: AppHandle,
    /// `None` when the audio thread could not start: alarms then show without sound.
    sound: Option<Arc<SoundPlayer>>,
    playing: Arc<Mutex<Sounds<Playback>>>,
}

impl TauriHost {
    pub fn new(app: AppHandle) -> Self {
        let sound = match SoundPlayer::new() {
            Ok(player) => Some(Arc::new(player)),
            Err(e) => {
                tracing::warn!(error = %e, "the alarm sound is off");
                None
            }
        };
        Self {
            app,
            sound,
            playing: Arc::default(),
        }
    }
}

/// The sounds that are playing, and which generation of "wanted" they belong to. Both sit under one
/// lock: a `play` that is still starting when `stop` runs must find out in the same step that keeps
/// its handle, or its sound would outlive the interrupt.
struct Sounds<T> {
    generation: u64,
    held: Vec<T>,
}

impl<T> Default for Sounds<T> {
    fn default() -> Self {
        Self {
            generation: 0,
            held: Vec::new(),
        }
    }
}

impl<T> Sounds<T> {
    /// The generation a new play starts in.
    fn generation(&self) -> u64 {
        self.generation
    }

    /// The sounds still held, to be cut before a new one starts.
    fn take(&mut self) -> Vec<T> {
        std::mem::take(&mut self.held)
    }

    /// Keeps `started` unless a stop happened since `wanted`; then it comes back to be cut.
    fn keep(&mut self, wanted: u64, started: T) -> Option<T> {
        if self.generation == wanted {
            self.held.push(started);
            None
        } else {
            Some(started)
        }
    }

    /// Everything held. Nothing that started before this call can be kept afterwards.
    fn stop(&mut self) -> Vec<T> {
        self.generation = self.generation.wrapping_add(1);
        self.take()
    }
}

fn held(playing: &Mutex<Sounds<Playback>>) -> std::sync::MutexGuard<'_, Sounds<Playback>> {
    playing.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Host for TauriHost {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn zone(&self) -> Tz {
        pipeline::local_zone()
    }

    fn open(&self, show: &AlarmShow) -> impl Future<Output = anyhow::Result<Opened>> + Send {
        let app = self.app.clone();
        let show = show.clone();
        async move {
            let platform = interrupt::native();
            let screens = interrupt::screens(&app, platform).await;
            let takeover = interrupt::takeover(&app, platform).await;
            interrupt::open_interrupts(&app, platform, &screens, &show)?;
            Ok(Opened {
                degraded: match takeover {
                    Takeover::Full => None,
                    Takeover::Degraded(reason) => Some(reason.code()),
                },
            })
        }
    }

    fn focused(&self) -> impl Future<Output = bool> + Send {
        let app = self.app.clone();
        async move {
            if app
                .try_state::<InterruptState>()
                .is_some_and(|s| s.focus_refused())
            {
                return false;
            }
            app.webview_windows()
                .iter()
                .filter(|(label, _)| label.starts_with(LABEL_PREFIX))
                .any(|(_, window)| window.is_focused().unwrap_or(false))
        }
    }

    fn close(&self) {
        interrupt::close_all(&self.app);
    }

    fn play(&self, sound: Sound) {
        let Some(player) = self.sound.clone() else {
            return;
        };
        let playing = Arc::clone(&self.playing);
        let wanted = held(&playing).generation();
        // `play` waits for the audio thread (up to 3 s): it runs off the scheduler, which must stay
        // free for a key press. A stop while it waits changes the generation, so the late sound is cut.
        tauri::async_runtime::spawn_blocking(move || {
            let previous = held(&playing).take();
            for p in &previous {
                let _ = player.stop(p);
            }
            match player.play(sound) {
                Ok(started) => {
                    let late = held(&playing).keep(wanted, started);
                    if let Some(late) = late {
                        let _ = player.stop(&late);
                    }
                }
                // No output device is normal on a server or a CI runner; the alarm still shows.
                Err(e) => tracing::warn!(error = %e, "the alarm sound did not play"),
            }
        });
    }

    fn stop_sound(&self) {
        let Some(player) = &self.sound else { return };
        let stopped = held(&self.playing).stop();
        for p in &stopped {
            let _ = player.stop(p);
        }
    }

    fn open_url(&self, url: &str) -> anyhow::Result<()> {
        // The error text can quote the URL, which can carry a meeting password: log the host only.
        tauri_plugin_opener::open_url(url, None::<&str>).map_err(|_| {
            tracing::warn!(host = %tolon_sync::redact_url(url), "could not open the link in the browser");
            anyhow::anyhow!("could not open the link")
        })
    }

    fn notify(&self, show: &AlarmShow, id: i32) {
        let copy = self
            .app
            .try_state::<NotifyCopyState>()
            .map_or_else(|| Arc::new(NotifyCopy::english()), |s| s.current());
        let (title, body) = copy.text(show, self.zone());
        // The buttons are read when the notification shows, so the current language reaches them.
        if let Err(e) = register_actions(&self.app, &copy) {
            tracing::warn!(error = %e, "could not register the notification buttons");
        }
        let shown = self
            .app
            .notification()
            .builder()
            .title(title)
            .body(body)
            .id(id)
            .action_type_id(ACTION_TYPE)
            .show();
        if let Err(e) = shown {
            tracing::warn!(error = %e, "could not show the fallback notification");
        }
    }

    /// The tray icon and every page that shows the pause (`app:paused`, a `PauseDto`).
    fn paused(&self, until: Option<DateTime<Utc>>) {
        crate::tray::set_paused(&self.app, until.is_some());
        if let Err(e) = self.app.emit("app:paused", commands::PauseDto::of(until)) {
            tracing::warn!(error = %e, "could not emit app:paused");
        }
    }
}

/// The notification's two buttons with the bundle's labels; a type registered again replaces the old one.
fn register_actions(app: &AppHandle, copy: &NotifyCopy) -> tauri_plugin_notification::Result<()> {
    app.notification()
        .register_action_types(vec![ActionType::builder(ACTION_TYPE)
            .actions(vec![
                Action::builder(SNOOZE, copy.snooze.as_str()).build(),
                Action::builder(JOIN, copy.join.as_str()).build(),
            ])
            .build()])
}

/// Registers what the notification's Join and Snooze buttons do, and the English buttons for a
/// notification before the first bundle. Call once at start, before the first notification: the
/// plugin shows actions only when a handler exists.
pub fn setup_fallback_actions(app: &AppHandle) -> anyhow::Result<()> {
    register_actions(app, &NotifyCopy::english())?;
    let notification = app.notification();
    let handle = app.clone();
    notification.on_action(move |performed| {
        let act = match performed.action_id() {
            JOIN => Act::Join,
            SNOOZE => Act::Snooze(SNOOZE_MINUTES),
            // A click on the notification itself, or an action this build does not know.
            _ => return,
        };
        // The id says which notification this was; a platform that does not report it gets no action.
        let Some(notice) = performed.notification().map(|n| n.id()) else {
            tracing::debug!("a notification action without a notification id was ignored");
            return;
        };
        // The handler runs on a plain thread of the notification library.
        let handle = handle.clone();
        tauri::async_runtime::spawn(async move { fallback_action(&handle, notice, act).await });
    })?;
    Ok(())
}

async fn fallback_action(app: &AppHandle, notice: i32, act: Act) {
    let Some(scheduler) = app.try_state::<AppScheduler>() else {
        return;
    };
    let result = async {
        let state = commands::state(app)?;
        scheduler.act_on_notice(&state.db, notice, act).await
    }
    .await;
    // The interrupt may have been handled on screen already: that is not an error worth a warning.
    if let Err(e) = result {
        tracing::debug!(error = %e, "notification action had nothing to act on");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sound_that_starts_after_a_stop_is_cut_not_kept() {
        let mut sounds = Sounds::<u32>::default();
        let wanted = sounds.generation();
        // The interrupt closes while the audio thread is still starting sound 1.
        assert!(sounds.stop().is_empty());
        assert_eq!(sounds.keep(wanted, 1), Some(1), "late: cut");
        assert!(sounds.stop().is_empty(), "nothing is left playing");
    }

    #[test]
    fn a_sound_that_starts_in_time_is_kept_and_stopped_by_the_next_stop() {
        let mut sounds = Sounds::<u32>::default();
        let wanted = sounds.generation();
        assert_eq!(sounds.keep(wanted, 1), None);
        // The repeat cuts the previous sound before the next one starts.
        assert_eq!(sounds.take(), [1]);
        let wanted = sounds.generation();
        assert_eq!(sounds.keep(wanted, 2), None);
        assert_eq!(sounds.stop(), [2]);
    }

    #[test]
    fn a_play_that_began_before_two_stops_is_still_late() {
        let mut sounds = Sounds::<u32>::default();
        let wanted = sounds.generation();
        sounds.stop();
        sounds.stop();
        assert_eq!(sounds.keep(wanted, 7), Some(7));
    }
}

//! The Tauri side of [`Host`]: windows through `interrupt`, sound through `SoundPlayer`, the
//! default browser through `tauri-plugin-opener`'s `open_url` (called from Rust only: the plugin is
//! not registered, so the webview has no opener command at all), and the F4.5 notification.
//!
//! The fallback notification's wording is English for now: the core and the app send codes, not
//! sentences (decision "Tolon is translated"), and the notification is built here. Follow-up: let
//! the UI hand its catalogue strings over once, at start.
//!
//! Action support of the notification plugin (tauri-plugin-notification 2.7, from its source):
//! - Linux: buttons when the notification server offers actions (GNOME, KDE and dunst do). Where it does
//!   not, the notification still shows, without them.
//! - macOS: two or more actions become an "Options" menu, and a click on the menu button itself reports
//!   the first action, so Snooze is listed first on every OS: the accidental one is harmless.
//! - Windows: buttons, but a click is reported only while the toast is on screen, not once it sits in
//!   the Action Center. Installed builds only: a dev build shows PowerShell's name and icon.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::{Action, ActionType, NotificationExt};
use tolon_platform::{Playback, Sound, SoundPlayer, Takeover};

use super::{Act, Host, Opened, SNOOZE_MINUTES};
use crate::commands::{self, AppScheduler};
use crate::interrupt::{self, AlarmShow, InterruptState, LABEL_PREFIX};
use crate::pipeline;

const ACTION_TYPE: &str = "alarm";
const JOIN: &str = "join";
const SNOOZE: &str = "snooze";

pub struct TauriHost {
    app: AppHandle,
    /// `None` when the audio thread could not start: alarms then show without sound.
    sound: Option<Arc<SoundPlayer>>,
    playing: Arc<Mutex<Vec<Playback>>>,
    /// Bumped by every `stop_sound`: a play that was still starting then stops itself.
    generation: Arc<AtomicU64>,
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
            generation: Arc::default(),
        }
    }
}

fn held(playing: &Mutex<Vec<Playback>>) -> std::sync::MutexGuard<'_, Vec<Playback>> {
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
        let generation = Arc::clone(&self.generation);
        let wanted = generation.load(Ordering::SeqCst);
        // `play` waits for the audio thread (up to 3 s): it runs off the scheduler, which must stay
        // free for a key press. A close while it waits bumps `generation`, so the late sound is cut.
        tauri::async_runtime::spawn_blocking(move || {
            let previous: Vec<Playback> = held(&playing).drain(..).collect();
            for p in &previous {
                let _ = player.stop(p);
            }
            match player.play(sound) {
                Ok(started) if generation.load(Ordering::SeqCst) == wanted => {
                    held(&playing).push(started);
                }
                Ok(late) => {
                    let _ = player.stop(&late);
                }
                // No output device is normal on a server or a CI runner; the alarm still shows.
                Err(e) => tracing::warn!(error = %e, "the alarm sound did not play"),
            }
        });
    }

    fn stop_sound(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let Some(player) = &self.sound else { return };
        let stopped: Vec<Playback> = held(&self.playing).drain(..).collect();
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

    fn notify(&self, show: &AlarmShow) {
        let (title, body) = fallback_text(show);
        let shown = self
            .app
            .notification()
            .builder()
            .title(title)
            .body(body)
            .action_type_id(ACTION_TYPE)
            .show();
        if let Err(e) = shown {
            tracing::warn!(error = %e, "could not show the fallback notification");
        }
    }
}

/// Title and body of the F4.5 notification: the meeting, or "N meetings", and when it starts.
fn fallback_text(show: &AlarmShow) -> (String, String) {
    let first = show.meetings.first();
    let title = match (show.test, show.meetings.len()) {
        (true, _) => "Test alarm".to_owned(),
        (false, 0 | 1) => first.map(|m| m.title.clone()).unwrap_or_default(),
        (false, n) => format!("{n} meetings are about to start"),
    };
    // "2026-10-20T14:00:00+02:00": the clock part, in the machine's offset.
    let clock = first.and_then(|m| m.start.get(11..16)).unwrap_or_default();
    (
        title,
        format!("Starts at {clock}. Tolon could not cover the screen."),
    )
}

/// Registers the notification's Join and Snooze buttons and what they do. Call once at start,
/// before the first notification: the plugin shows actions only when a handler exists.
pub fn setup_fallback_actions(app: &AppHandle) -> anyhow::Result<()> {
    let notification = app.notification();
    notification.register_action_types(vec![ActionType::builder(ACTION_TYPE)
        .actions(vec![
            Action::builder(SNOOZE, format!("Snooze {SNOOZE_MINUTES} min")).build(),
            Action::builder(JOIN, "Join the call").build(),
        ])
        .build()])?;
    let handle = app.clone();
    notification.on_action(move |performed| {
        let act = match performed.action_id() {
            JOIN => Act::Join,
            SNOOZE => Act::Snooze(SNOOZE_MINUTES),
            // A click on the notification itself, or an action this build does not know.
            _ => return,
        };
        // The handler runs on a plain thread of the notification library.
        let handle = handle.clone();
        tauri::async_runtime::spawn(async move { fallback_action(&handle, act).await });
    })?;
    Ok(())
}

async fn fallback_action(app: &AppHandle, act: Act) {
    let Some(scheduler) = app.try_state::<AppScheduler>() else {
        return;
    };
    let result = async {
        let state = commands::state(app)?;
        let key = scheduler
            .first_key()
            .await
            .ok_or(crate::errors::AppError::NOT_FOUND)?;
        scheduler.act(&state.db, &key, act).await
    }
    .await;
    // The interrupt may have been handled on screen already: that is not an error worth a warning.
    if let Err(e) = result {
        tracing::debug!(error = %e, "notification action had nothing to act on");
    }
}

//! Linux implementation: session detection and the degraded-mode signal, no native calls.
//!
//! X11: Tauri's `always_on_top` (GTK keep-above, `_NET_WM_STATE_ABOVE`) plus `fullscreen`
//! already cover other windows, and the interrupt window is created with both, so
//! `raise_above_everything` has nothing to add. Re-implementing it with Xlib would need a new
//! X11 dependency and a second display connection for a window we cannot test in CI.
//!
//! Wayland: a compositor does not let an ordinary window stay above others or steal focus, so
//! [`Platform::takeover_support`] reports [`Takeover::Degraded`] and the app runs the F4.5
//! fallback (notification with Join and Snooze, alarm sound).
//!
//! `screens` returns an empty list: GDK and XRandR are the only sources, both need GTK or a new
//! X11 dependency, and Tauri's `available_monitors` already reads them.

use crate::session::{detect_session, session_of_handle, takeover_for};
use crate::{Platform, PlatformError, Raised, Screen, Takeover};
use raw_window_handle::HasWindowHandle;

pub struct LinuxPlatform;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

impl Platform for LinuxPlatform {
    fn name(&self) -> &'static str {
        "linux"
    }

    fn screens(&self) -> Result<Vec<Screen>, PlatformError> {
        Ok(Vec::new())
    }

    fn raise_above_everything(
        &self,
        window: &dyn HasWindowHandle,
    ) -> Result<Raised, PlatformError> {
        let handle = window
            .window_handle()
            .map_err(|e| PlatformError::HandleUnavailable(e.to_string()))?;
        session_of_handle(handle.as_raw())
            .map(|_| Raised { focused: false })
            .ok_or(PlatformError::WrongHandleKind("not X11 or Wayland"))
    }

    fn takeover_support(&self) -> Takeover {
        takeover_for(detect_session(
            env("XDG_SESSION_TYPE").as_deref(),
            env("WAYLAND_DISPLAY").as_deref(),
            env("DISPLAY").as_deref(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::{Closed, Fake};
    use raw_window_handle::{AppKitWindowHandle, RawWindowHandle, XlibWindowHandle};
    use std::ptr::NonNull;

    #[test]
    fn screens_are_left_to_tauri() {
        assert_eq!(LinuxPlatform.screens(), Ok(Vec::new()));
    }

    #[test]
    fn raise_accepts_an_x11_window() {
        let w = Fake(RawWindowHandle::Xlib(XlibWindowHandle::new(1)));
        assert_eq!(
            LinuxPlatform.raise_above_everything(&w),
            Ok(Raised { focused: false })
        );
    }

    #[test]
    fn raise_rejects_a_foreign_handle() {
        let w = Fake(RawWindowHandle::AppKit(AppKitWindowHandle::new(
            NonNull::dangling(),
        )));
        assert!(matches!(
            LinuxPlatform.raise_above_everything(&w),
            Err(PlatformError::WrongHandleKind(_))
        ));
    }

    #[test]
    fn raise_on_a_closed_window_is_an_error_not_a_panic() {
        assert!(matches!(
            LinuxPlatform.raise_above_everything(&Closed),
            Err(PlatformError::HandleUnavailable(_))
        ));
    }
}

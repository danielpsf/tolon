//! Which display server a Linux session runs, from environment variables. Free of OS calls so
//! it is testable on every OS; `linux.rs` feeds it the real environment.

use crate::{Reason, Takeover};
use raw_window_handle::RawWindowHandle;

/// The display server the session talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    X11,
    Wayland,
}

/// Decide the session from `XDG_SESSION_TYPE` and `WAYLAND_DISPLAY`.
///
/// `XDG_SESSION_TYPE` (`x11` or `wayland`, any case) wins because the login manager sets it
/// for the whole session. When it is missing, empty, `tty` or `unspecified`, a set
/// `WAYLAND_DISPLAY` means Wayland; a Wayland session with XWayland also sets `DISPLAY`, so
/// `DISPLAY` cannot tell the two apart and is not read. When nothing says Wayland the answer
/// is X11: Tolon is a GTK app and cannot start without a display, so a process that runs at all
/// has one, and X11 is the one that can take over the screen.
pub fn detect_session(xdg_session_type: Option<&str>, wayland_display: Option<&str>) -> Session {
    let wayland_display_set = wayland_display.is_some_and(|s| !s.trim().is_empty());
    match xdg_session_type
        .map(|s| s.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("wayland") => Session::Wayland,
        Some("x11") => Session::X11,
        _ if wayland_display_set => Session::Wayland,
        _ => Session::X11,
    }
}

/// The display server a window handle belongs to, or `None` for a handle from another OS.
pub fn session_of_handle(raw: RawWindowHandle) -> Option<Session> {
    match raw {
        RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_) => Some(Session::X11),
        RawWindowHandle::Wayland(_) => Some(Session::Wayland),
        _ => None,
    }
}

/// What a session allows (AC4.1, AC4.5): X11 takes over, Wayland does not.
pub fn takeover_for(session: Session) -> Takeover {
    match session {
        Session::X11 => Takeover::Full,
        Session::Wayland => Takeover::Degraded(Reason::WaylandNoTakeover),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use detect_session as d;

    #[test]
    fn xdg_session_type_decides_first() {
        assert_eq!(d(Some("wayland"), None), Session::Wayland);
        assert_eq!(d(Some("x11"), Some("wayland-0")), Session::X11);
        assert_eq!(d(Some("Wayland"), None), Session::Wayland);
        assert_eq!(d(Some(" X11 "), None), Session::X11);
    }

    #[test]
    fn falls_back_to_wayland_display() {
        assert_eq!(d(None, Some("wayland-0")), Session::Wayland);
        assert_eq!(d(Some("tty"), Some("wayland-1")), Session::Wayland);
        assert_eq!(d(Some("unspecified"), Some("wayland-1")), Session::Wayland);
    }

    #[test]
    fn nothing_detected_is_x11() {
        assert_eq!(d(None, None), Session::X11);
        assert_eq!(d(Some(""), Some("  ")), Session::X11);
        assert_eq!(d(Some("tty"), None), Session::X11);
        assert_eq!(d(Some("mir"), None), Session::X11);
    }

    #[test]
    fn handles_map_to_their_display_server() {
        use raw_window_handle::{
            AppKitWindowHandle, WaylandWindowHandle, XcbWindowHandle, XlibWindowHandle,
        };
        use std::{num::NonZeroU32, ptr::NonNull};
        let xlib = RawWindowHandle::Xlib(XlibWindowHandle::new(7));
        let xcb = RawWindowHandle::Xcb(XcbWindowHandle::new(NonZeroU32::MIN));
        let wl = RawWindowHandle::Wayland(WaylandWindowHandle::new(NonNull::dangling()));
        let mac = RawWindowHandle::AppKit(AppKitWindowHandle::new(NonNull::dangling()));
        assert_eq!(session_of_handle(xlib), Some(Session::X11));
        assert_eq!(session_of_handle(xcb), Some(Session::X11));
        assert_eq!(session_of_handle(wl), Some(Session::Wayland));
        assert_eq!(session_of_handle(mac), None);
    }

    #[test]
    fn s4_4_wayland_degrades_and_x11_does_not() {
        assert_eq!(
            takeover_for(Session::Wayland),
            Takeover::Degraded(Reason::WaylandNoTakeover)
        );
        assert_eq!(takeover_for(Session::X11), Takeover::Full);
    }
}

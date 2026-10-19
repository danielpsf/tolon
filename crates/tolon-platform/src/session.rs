//! Which display server a Linux session runs, from environment variables. Free of OS calls so
//! it is testable on every OS; `linux.rs` feeds it the real environment.

use crate::{Reason, Takeover};
use raw_window_handle::RawWindowHandle;

/// The display server the session talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    X11,
    Wayland,
    /// No display variable says anything: a console login or an SSH session.
    None,
}

/// Decide the session from `XDG_SESSION_TYPE`, `WAYLAND_DISPLAY` and `DISPLAY`.
///
/// `XDG_SESSION_TYPE` (`x11` or `wayland`, any case) wins because the login manager sets it
/// for the whole session. When it is missing, empty, `tty` or `unspecified`, a set
/// `WAYLAND_DISPLAY` means Wayland and a set `DISPLAY` means X11. Wayland is checked first
/// because a Wayland session with XWayland sets both, and the compositor decides stacking.
pub fn detect_session(
    xdg_session_type: Option<&str>,
    wayland_display: Option<&str>,
    display: Option<&str>,
) -> Session {
    let set = |v: Option<&str>| v.is_some_and(|s| !s.trim().is_empty());
    match xdg_session_type
        .map(|s| s.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("wayland") => Session::Wayland,
        Some("x11") => Session::X11,
        _ if set(wayland_display) => Session::Wayland,
        _ if set(display) => Session::X11,
        _ => Session::None,
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

/// What a session allows (AC4.1, AC4.5): X11 takes over, Wayland does not, no display cannot show anything.
pub fn takeover_for(session: Session) -> Takeover {
    match session {
        Session::X11 => Takeover::Full,
        Session::Wayland => Takeover::Degraded(Reason::WaylandNoTakeover),
        Session::None => Takeover::Degraded(Reason::NoDisplayServer),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(xdg: Option<&str>, wl: Option<&str>, x: Option<&str>) -> Session {
        detect_session(xdg, wl, x)
    }

    #[test]
    fn xdg_session_type_decides_first() {
        assert_eq!(d(Some("wayland"), None, Some(":0")), Session::Wayland);
        assert_eq!(d(Some("x11"), Some("wayland-0"), None), Session::X11);
        assert_eq!(d(Some("Wayland"), None, None), Session::Wayland);
        assert_eq!(d(Some(" X11 "), None, None), Session::X11);
    }

    #[test]
    fn falls_back_to_display_variables() {
        assert_eq!(d(None, Some("wayland-0"), None), Session::Wayland);
        assert_eq!(d(None, None, Some(":0")), Session::X11);
        assert_eq!(d(Some("tty"), None, Some(":1")), Session::X11);
        assert_eq!(
            d(Some("unspecified"), Some("wayland-1"), None),
            Session::Wayland
        );
    }

    #[test]
    fn wayland_with_xwayland_is_still_wayland() {
        assert_eq!(d(None, Some("wayland-0"), Some(":0")), Session::Wayland);
    }

    #[test]
    fn empty_values_count_as_unset() {
        assert_eq!(d(Some(""), Some(""), Some("  ")), Session::None);
        assert_eq!(d(None, None, None), Session::None);
        assert_eq!(d(Some("mir"), None, None), Session::None);
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
        assert_eq!(
            takeover_for(Session::None),
            Takeover::Degraded(Reason::NoDisplayServer)
        );
    }
}

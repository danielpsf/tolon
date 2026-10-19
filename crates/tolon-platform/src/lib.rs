//! OS-specific behaviour behind the [`Platform`] trait: window levels, screen
//! enumeration and sound. Platform code lives only in this crate; CI uses [`StubPlatform`].

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
pub mod macos_level;
pub mod monitors;
pub mod session;
#[cfg(target_os = "windows")]
mod windows;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::c_void;
use std::num::NonZeroIsize;
use std::ptr::NonNull;

#[cfg(target_os = "linux")]
pub use linux::LinuxPlatform;
#[cfg(target_os = "macos")]
pub use macos::MacPlatform;
#[cfg(target_os = "windows")]
pub use windows::WindowsPlatform;

/// The implementation for the OS this build targets. Other OSes fall back to [`StubPlatform`].
#[cfg(target_os = "linux")]
pub type NativePlatform = LinuxPlatform;
#[cfg(target_os = "macos")]
pub type NativePlatform = MacPlatform;
#[cfg(target_os = "windows")]
pub type NativePlatform = WindowsPlatform;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub type NativePlatform = StubPlatform;

/// One connected display.
///
/// Both coordinate sets put the primary screen's top-left corner at `(0, 0)` with `y` growing
/// downward.
///
/// **Place windows with the `physical_*` fields**, through Tauri's `PhysicalPosition` and
/// `PhysicalSize`. They are exact on every OS. `x`, `y`, `width` and `height` are logical
/// points, for display and size maths. Only macOS has one logical space shared by all screens;
/// on Windows each screen is divided by its own `scale_factor`, so the logical `x` and `y` of
/// two screens with different scales cannot be compared with each other.
#[derive(Debug, Clone, PartialEq)]
pub struct Screen {
    /// Stable while the display stays connected (macOS: `CGDirectDisplayID`).
    pub id: u32,
    /// Top-left corner and size in logical points.
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Top-left corner in physical pixels of the virtual screen (Windows: the native rectangle;
    /// macOS: logical points times `scale_factor`, rounded).
    pub physical_x: i32,
    pub physical_y: i32,
    /// Size in physical pixels.
    pub physical_width: u32,
    pub physical_height: u32,
    /// Physical pixels per logical point.
    pub scale_factor: f64,
    /// The primary screen (macOS: the one with the menu bar). Exactly one screen has it set.
    pub is_main: bool,
}

/// A platform call failed. The app logs it and uses the F4.5 fallback; it never crashes.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PlatformError {
    /// The call needs the main thread (AppKit). Run it through `AppHandle::run_on_main_thread`.
    #[error("must be called on the main thread")]
    NotMainThread,
    /// The window cannot give out a native handle, for example because it is already closed.
    #[error("window handle unavailable: {0}")]
    HandleUnavailable(String),
    /// The handle belongs to another windowing system than the one this implementation drives.
    #[error("unsupported window handle: {0}")]
    WrongHandleKind(&'static str),
    /// The native view is not attached to a native window.
    #[error("the view has no window")]
    NoWindow,
    /// An OS call failed. `code` is the Win32 error or `HRESULT` it reported.
    #[error("{call} failed with OS error {code:#010x}")]
    Os { call: &'static str, code: u32 },
}

/// Whether the interrupt window can take over the screen on this system (AC4.1, AC4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Takeover {
    /// Always-on-top and full screen work. Open the interrupt window.
    Full,
    /// The system will not let a window cover everything. The app runs the F4.5 fallback
    /// (notification with Join and Snooze, alarm sound) and logs the reason once.
    Degraded(Reason),
}

/// Why a takeover is [`Takeover::Degraded`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// A Wayland session: compositors ignore always-on-top requests from ordinary windows.
    WaylandNoTakeover,
    /// No implementation for this OS ([`StubPlatform`]): nothing can cover the screen.
    Unsupported,
}

impl Reason {
    /// Stable code for logs and the UI; the UI turns it into copy (decisions.md, "Scenario copy").
    pub fn code(self) -> &'static str {
        match self {
            Reason::WaylandNoTakeover => "wayland_no_takeover",
            Reason::Unsupported => "unsupported",
        }
    }
}

/// What [`Platform::raise_above_everything`] achieved. The window is above other windows
/// whenever the call returns `Ok`; focus is separate and the OS may refuse it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Raised {
    /// The window had keyboard focus when the call returned (AC4.4). macOS: the window is the key
    /// window at that moment, read right after `makeKeyAndOrderFront`; an app that is not yet
    /// active can report `false` and gain focus a moment later. Windows: `SetForegroundWindow`
    /// succeeded. Linux: always `false`, because Tauri and the window manager handle focus
    /// there; the app must check focus itself.
    pub focused: bool,
}

/// What the app needs from the operating system.
///
/// Every call returns `Result`; a failure is reported, never fatal. On macOS every call must run
/// on the main thread and returns [`PlatformError::NotMainThread`] anywhere else.
pub trait Platform: Send + Sync {
    /// Short implementation name, for logs and diagnostics.
    fn name(&self) -> &'static str;

    /// Connected screens, primary first. An empty list means this platform cannot enumerate
    /// screens (Linux today); the app then uses Tauri's `available_monitors`.
    fn screens(&self) -> Result<Vec<Screen>, PlatformError>;

    /// Put `window` above every other window, including full-screen apps, on all Spaces, and
    /// try to focus it. Call it after Tauri has created the window; Tauri's `WebviewWindow` and
    /// `Window` implement [`HasWindowHandle`]. [`Raised::focused`] says whether the window got
    /// keyboard focus; when it did not, the app runs the F4.5 notification as well.
    fn raise_above_everything(&self, window: &dyn HasWindowHandle)
        -> Result<Raised, PlatformError>;

    /// Whether [`Platform::raise_above_everything`] can really cover other windows here. The app
    /// asks once before an alarm and runs the F4.5 fallback on [`Takeover::Degraded`]. Every
    /// implementation answers for itself; there is no default.
    fn takeover_support(&self) -> Takeover;
}

/// Implementation for CI and tests: one fixed 1440x900 primary screen at 2x; raising does
/// nothing and reports no focus. It is also what unknown OSes get, so it reports
/// `Degraded(Unsupported)`: nothing here can cover the screen.
pub struct StubPlatform;

impl Platform for StubPlatform {
    fn name(&self) -> &'static str {
        "stub"
    }

    fn screens(&self) -> Result<Vec<Screen>, PlatformError> {
        Ok(vec![Screen {
            id: 1,
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 900.0,
            physical_x: 0,
            physical_y: 0,
            physical_width: 2880,
            physical_height: 1800,
            scale_factor: 2.0,
            is_main: true,
        }])
    }

    fn raise_above_everything(
        &self,
        _window: &dyn HasWindowHandle,
    ) -> Result<Raised, PlatformError> {
        Ok(Raised { focused: false })
    }

    fn takeover_support(&self) -> Takeover {
        Takeover::Degraded(Reason::Unsupported)
    }
}

/// The `NSView` behind an AppKit window handle, or why there is none.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn ns_view_of(window: &dyn HasWindowHandle) -> Result<NonNull<c_void>, PlatformError> {
    let handle = window
        .window_handle()
        .map_err(|e| PlatformError::HandleUnavailable(e.to_string()))?;
    ns_view_from_raw(handle.as_raw())
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn ns_view_from_raw(raw: RawWindowHandle) -> Result<NonNull<c_void>, PlatformError> {
    match raw {
        RawWindowHandle::AppKit(h) => Ok(h.ns_view),
        RawWindowHandle::Win32(_) => Err(PlatformError::WrongHandleKind("Win32")),
        RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_) => {
            Err(PlatformError::WrongHandleKind("X11"))
        }
        RawWindowHandle::Wayland(_) => Err(PlatformError::WrongHandleKind("Wayland")),
        _ => Err(PlatformError::WrongHandleKind("other")),
    }
}

/// The `HWND` behind a Win32 window handle, or why there is none.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn hwnd_of(window: &dyn HasWindowHandle) -> Result<NonZeroIsize, PlatformError> {
    let handle = window
        .window_handle()
        .map_err(|e| PlatformError::HandleUnavailable(e.to_string()))?;
    hwnd_from_raw(handle.as_raw())
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn hwnd_from_raw(raw: RawWindowHandle) -> Result<NonZeroIsize, PlatformError> {
    match raw {
        RawWindowHandle::Win32(h) => Ok(h.hwnd),
        RawWindowHandle::AppKit(_) => Err(PlatformError::WrongHandleKind("AppKit")),
        RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_) => {
            Err(PlatformError::WrongHandleKind("X11"))
        }
        RawWindowHandle::Wayland(_) => Err(PlatformError::WrongHandleKind("Wayland")),
        _ => Err(PlatformError::WrongHandleKind("other")),
    }
}

/// Window handles for tests: a fixed raw handle, or a window that is already closed.
#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod fake {
    use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};

    pub struct Fake(pub RawWindowHandle);
    impl HasWindowHandle for Fake {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            // SAFETY: tests never dereference the handle; it only has to be well formed.
            Ok(unsafe { WindowHandle::borrow_raw(self.0) })
        }
    }

    pub struct Closed;
    impl HasWindowHandle for Closed {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            Err(HandleError::Unavailable)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use raw_window_handle::{AppKitWindowHandle, Win32WindowHandle, XlibWindowHandle};

    #[test]
    fn stub_platform_reports_its_name() {
        assert_eq!(StubPlatform.name(), "stub");
    }

    #[test]
    fn stub_has_exactly_one_main_screen() {
        let screens = StubPlatform.screens().unwrap_or_default();
        assert_eq!(screens.len(), 1);
        assert_eq!(screens.iter().filter(|s| s.is_main).count(), 1);
    }

    #[test]
    fn appkit_handle_yields_the_view() {
        let view = NonNull::<c_void>::dangling();
        let raw = RawWindowHandle::AppKit(AppKitWindowHandle::new(view));
        assert_eq!(ns_view_from_raw(raw), Ok(view));
    }

    #[test]
    fn foreign_handle_maps_to_wrong_kind() {
        let raw = RawWindowHandle::Xlib(XlibWindowHandle::new(1));
        assert_eq!(
            ns_view_from_raw(raw),
            Err(PlatformError::WrongHandleKind("X11"))
        );
    }

    #[test]
    fn win32_handle_yields_the_hwnd() {
        let hwnd = NonZeroIsize::MIN;
        let raw = RawWindowHandle::Win32(Win32WindowHandle::new(hwnd));
        assert_eq!(hwnd_from_raw(raw), Ok(hwnd));
    }

    #[test]
    fn non_win32_handle_is_not_an_hwnd() {
        let raw = RawWindowHandle::Xlib(XlibWindowHandle::new(1));
        assert_eq!(
            hwnd_from_raw(raw),
            Err(PlatformError::WrongHandleKind("X11"))
        );
    }

    #[test]
    fn closed_window_has_no_hwnd() {
        assert!(matches!(
            hwnd_of(&fake::Closed),
            Err(PlatformError::HandleUnavailable(_))
        ));
    }

    #[test]
    fn os_errors_print_the_code() {
        let e = PlatformError::Os {
            call: "SetWindowPos",
            code: 0x8007_0005,
        };
        assert_eq!(
            e.to_string(),
            "SetWindowPos failed with OS error 0x80070005"
        );
    }

    #[test]
    fn stub_raise_reports_no_focus() {
        let w = fake::Fake(RawWindowHandle::Xlib(XlibWindowHandle::new(1)));
        assert_eq!(
            StubPlatform.raise_above_everything(&w),
            Ok(Raised { focused: false })
        );
    }

    #[test]
    fn stub_takeover_is_degraded_as_unsupported() {
        assert_eq!(
            StubPlatform.takeover_support(),
            Takeover::Degraded(Reason::Unsupported)
        );
    }

    #[test]
    fn reason_codes_are_stable() {
        assert_eq!(Reason::WaylandNoTakeover.code(), "wayland_no_takeover");
        assert_eq!(Reason::Unsupported.code(), "unsupported");
    }

    #[test]
    fn errors_have_readable_messages() {
        assert_eq!(
            PlatformError::NotMainThread.to_string(),
            "must be called on the main thread"
        );
        assert_eq!(
            PlatformError::HandleUnavailable("closed".into()).to_string(),
            "window handle unavailable: closed"
        );
    }
}

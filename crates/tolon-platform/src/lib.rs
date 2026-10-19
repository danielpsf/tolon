//! OS-specific behaviour behind the [`Platform`] trait: window levels, screen
//! enumeration and sound. Platform code lives only in this crate; CI uses [`StubPlatform`].

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
pub mod macos_level;
#[cfg(target_os = "windows")]
mod windows;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::c_void;
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
/// `x` and `y` are the top-left corner in logical points, in the global coordinate space with
/// the primary screen at `(0, 0)` and `y` growing downward (the convention Tauri and winit use
/// for monitor positions). Multiply by `scale_factor` for physical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Screen {
    /// Stable while the display stays connected (macOS: `CGDirectDisplayID`).
    pub id: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
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
}

/// What the app needs from the operating system.
///
/// Every call returns `Result`; a failure is reported, never fatal. On macOS every call must run
/// on the main thread and returns [`PlatformError::NotMainThread`] anywhere else.
pub trait Platform: Send + Sync {
    /// Short implementation name, for logs and diagnostics.
    fn name(&self) -> &'static str;

    /// Connected screens, primary first.
    fn screens(&self) -> Result<Vec<Screen>, PlatformError>;

    /// Put `window` above every other window, including full-screen apps, on all Spaces, and
    /// focus it. Call it after Tauri has created the window; Tauri's `WebviewWindow` and
    /// `Window` implement [`HasWindowHandle`].
    fn raise_above_everything(&self, window: &dyn HasWindowHandle) -> Result<(), PlatformError>;
}

/// Implementation for CI and tests: one fixed 1440x900 primary screen; raising does nothing.
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
            scale_factor: 2.0,
            is_main: true,
        }])
    }

    fn raise_above_everything(&self, _window: &dyn HasWindowHandle) -> Result<(), PlatformError> {
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use raw_window_handle::{AppKitWindowHandle, XlibWindowHandle};

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

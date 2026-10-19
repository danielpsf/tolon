//! OS-specific behaviour behind the [`Platform`] trait: window levels, screen
//! enumeration and sound. Platform code lives only in this crate; CI uses [`StubPlatform`].

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
pub mod macos_level;
pub mod monitors;
pub mod session;
pub mod sound;
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
/// **Do not pick coordinates yourself: call [`Screen::placement`]**, which returns the right
/// kind for the OS. The rule differs per OS:
/// - macOS: one logical space shared by all screens. Place with `LogicalPosition(x, y)` and
///   `LogicalSize(width, height)`. A physical position would be converted with the scale of the
///   window's *current* screen (tao does this), so a window on a 2x primary placed at the
///   physical x of a 1x screen to its right would land at half that x, still on the primary.
/// - Windows: no shared logical space, each screen is divided by its own `scale_factor`, so the
///   logical `x` and `y` of screens with different scales cannot be compared. Place with the
///   `physical_*` fields through `PhysicalPosition` and `PhysicalSize`.
/// - Linux: [`Platform::screens`] is unsupported there; the app uses Tauri's monitors.
///
/// Both coordinate sets put the primary screen's top-left corner at `(0, 0)` with `y` growing
/// downward. The physical fields are exact data on every OS even where they are not used for
/// placement.
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

/// Where and how large to put a window so it covers a [`Screen`]; see [`Screen::placement`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    /// Logical points: Tauri `LogicalPosition(x, y)` and `LogicalSize(width, height)`.
    Logical {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    /// Physical pixels: Tauri `PhysicalPosition(x, y)` and `PhysicalSize(width, height)`.
    Physical {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
}

impl Screen {
    /// The placement that is right for the OS this build targets: logical on macOS, physical
    /// everywhere else (Windows; the other OSes only see the stub).
    pub fn placement(&self) -> Placement {
        if cfg!(target_os = "macos") {
            self.logical_placement()
        } else {
            self.physical_placement()
        }
    }

    /// The logical rectangle, whatever the OS.
    pub fn logical_placement(&self) -> Placement {
        Placement::Logical {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
    }

    /// The physical rectangle, whatever the OS.
    pub fn physical_placement(&self) -> Placement {
        Placement::Physical {
            x: self.physical_x,
            y: self.physical_y,
            width: self.physical_width,
            height: self.physical_height,
        }
    }
}

/// A platform call failed. The app logs it and uses the F4.5 fallback; it never crashes.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PlatformError {
    /// The call needs the main thread (macOS checks). Run it through `AppHandle::run_on_main_thread`.
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
    /// This implementation cannot do the call (Linux `screens`); the app asks Tauri instead.
    #[error("not supported on this platform")]
    Unsupported,
    /// An OS call failed. `code` is the Win32 error or `HRESULT` it reported.
    #[error("{call} failed with OS error {code:#010x}")]
    Os { call: &'static str, code: u32 },
    /// Sound could not play: no output device, a decode error or a stopped audio thread. The app
    /// ignores it and shows the alarm without sound.
    #[error("audio: {0}")]
    Audio(String),
}

pub use sound::{Playback, Sound, SoundPlayer};

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
    /// Keyboard focus (AC4.4) as far as the call could tell.
    pub focus: Focus,
}

/// Whether a raised window has keyboard focus, as known when the call returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// The window has focus. macOS: it is the key window right after `makeKeyAndOrderFront`.
    /// Windows: `SetForegroundWindow` succeeded.
    Yes,
    /// The OS refused focus. Windows: `SetForegroundWindow` failed.
    No,
    /// The call cannot tell yet. macOS: not the key window now, but a tray app often becomes key
    /// a moment later. Linux and the stub: Tauri and the window manager handle focus, and the
    /// platform layer does not look.
    Unknown,
}

/// What the app needs from the operating system.
///
/// Every call returns `Result`; a failure is reported, never fatal.
///
/// **Threading:** on every OS, call every method from the main (UI) thread; in Tauri, through
/// `AppHandle::run_on_main_thread`. macOS checks it and returns [`PlatformError::NotMainThread`]
/// anywhere else. Windows and Linux do not check, but the windows belong to the UI thread there
/// too, so a call from another thread can block or race with it.
pub trait Platform: Send + Sync {
    /// Short implementation name, for logs and diagnostics.
    fn name(&self) -> &'static str;

    /// Connected screens, primary first; on success exactly one has `is_main`. Linux returns
    /// [`PlatformError::Unsupported`]: the app then asks Tauri (`available_monitors`).
    fn screens(&self) -> Result<Vec<Screen>, PlatformError>;

    /// Put `window` above every other window, including full-screen apps, on all Spaces, and
    /// try to focus it. Call it after Tauri has created the window; Tauri's `WebviewWindow` and
    /// `Window` implement [`HasWindowHandle`]. [`Raised::focus`] reports focus at the moment of
    /// the call. The app re-checks focus after about 500 ms and runs the F4.5 fallback only on a
    /// confirmed [`Focus::No`], or when focus is still missing at that check; [`Focus::Unknown`]
    /// alone is not a reason to fall back.
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
        Ok(Raised {
            focus: Focus::Unknown,
        })
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

    /// A 1x external screen to the right of a 2x Retina primary, as macOS reports them.
    fn external_right_of_retina() -> Screen {
        Screen {
            id: 2,
            x: 1440.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
            physical_x: 1440,
            physical_y: 0,
            physical_width: 1920,
            physical_height: 1080,
            scale_factor: 1.0,
            is_main: false,
        }
    }

    #[test]
    fn s4_2_logical_placement_keeps_the_shared_logical_origin() {
        assert_eq!(
            external_right_of_retina().logical_placement(),
            Placement::Logical {
                x: 1440.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0
            }
        );
    }

    #[test]
    fn s4_2_physical_placement_uses_the_physical_fields() {
        let mut s = external_right_of_retina();
        s.physical_x = -1920;
        s.physical_y = 40;
        assert_eq!(
            s.physical_placement(),
            Placement::Physical {
                x: -1920,
                y: 40,
                width: 1920,
                height: 1080
            }
        );
    }

    #[test]
    fn placement_picks_logical_on_macos_and_physical_elsewhere() {
        let s = external_right_of_retina();
        let expected = if cfg!(target_os = "macos") {
            s.logical_placement()
        } else {
            s.physical_placement()
        };
        assert_eq!(s.placement(), expected);
        // The stub is what CI and unknown OSes place with.
        let stub = StubPlatform.screens().unwrap_or_default();
        assert!(stub.iter().all(|s| matches!(
            (s.placement(), cfg!(target_os = "macos")),
            (Placement::Logical { .. }, true) | (Placement::Physical { .. }, false)
        )));
    }

    #[test]
    fn stub_raise_does_not_know_about_focus() {
        let w = fake::Fake(RawWindowHandle::Xlib(XlibWindowHandle::new(1)));
        assert_eq!(
            StubPlatform.raise_above_everything(&w),
            Ok(Raised {
                focus: Focus::Unknown
            })
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
    fn unsupported_has_a_readable_message() {
        assert_eq!(
            PlatformError::Unsupported.to_string(),
            "not supported on this platform"
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

//! OS-specific behaviour behind the [`Platform`] trait: window levels, screen
//! enumeration and sound. Platform code lives only in this crate; CI uses [`StubPlatform`].

#[cfg(target_os = "linux")]
mod linux;
pub mod login;
#[cfg(target_os = "macos")]
mod macos;
pub mod macos_level;
pub mod monitors;
pub mod secrets;
pub mod session;
pub mod sound;
#[cfg(target_os = "windows")]
mod windows;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::c_void;
use std::num::NonZeroIsize;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

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

/// [`NativePlatform`] as a value: an alias for a unit struct cannot be used as one.
#[cfg(target_os = "linux")]
pub const NATIVE: NativePlatform = LinuxPlatform;
#[cfg(target_os = "macos")]
pub const NATIVE: NativePlatform = MacPlatform;
#[cfg(target_os = "windows")]
pub const NATIVE: NativePlatform = WindowsPlatform;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub const NATIVE: NativePlatform = StubPlatform;

/// Keeps its statements only in macOS builds, so other crates can use the Tauri items that exist
/// only there (`RunEvent::Reopen`) while `target_os` stays in this crate (AGENTS.md rule 9).
#[macro_export]
macro_rules! macos_only {
    ($($statements:tt)*) => {
        #[cfg(target_os = "macos")]
        {
            $($statements)*
        }
    };
}

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

impl Screen {
    /// Whether `(x, y)`, in the unit [`Screen::placement`] uses on this OS, lies on this screen:
    /// logical points on macOS, physical pixels elsewhere. Match [`Platform::pointer`] with it.
    pub fn contains_placement_point(&self, x: f64, y: f64) -> bool {
        if cfg!(target_os = "macos") {
            self.contains_logical(x, y)
        } else {
            self.contains_physical(x, y)
        }
    }

    /// `(x, y)` in logical points is on this screen. Right and bottom edges are outside.
    pub fn contains_logical(&self, x: f64, y: f64) -> bool {
        self.x <= x && x < self.x + self.width && self.y <= y && y < self.y + self.height
    }

    /// `(x, y)` in physical pixels is on this screen. Right and bottom edges are outside.
    pub fn contains_physical(&self, x: f64, y: f64) -> bool {
        let (left, top) = (f64::from(self.physical_x), f64::from(self.physical_y));
        left <= x
            && x < left + f64::from(self.physical_width)
            && top <= y
            && y < top + f64::from(self.physical_height)
    }
}

/// How the tray icon behaves on this OS (F7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayStyle {
    /// The icon is a template image the system tints (macOS menu bar): black and alpha only.
    pub template_icon: bool,
    /// The tray reports clicks on the icon. Linux AppIndicator trays report none, only menus.
    pub clicks: bool,
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
    /// A file could not be read or written (Linux autostart entry).
    #[error("file access failed: {0}")]
    Io(String),
    /// Sound could not play: no output device, a decode error or a stopped audio thread. The app
    /// ignores it and shows the alarm without sound.
    #[error("audio: {0}")]
    Audio(String),
}

pub use login::StartAtLogin;
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

    /// Process-wide setup, once at startup on the main thread, before the first interrupt
    /// window opens. macOS: the Accessory activation policy (no Dock icon, no app menu), without
    /// which macOS keeps the interrupt off another app's full-screen Space (L-042). Nothing on
    /// other OSes.
    fn prepare_app(&self) -> Result<(), PlatformError>;

    /// Bring Tolon to the front so the window shown next takes focus. Call it after `show()` and
    /// before `set_focus()` when the settings window comes up (decision 2026-10-22). macOS: the
    /// call the interrupt uses, `activateIgnoringOtherApps`; an Accessory app's window does not
    /// come forward on `show()` alone. Windows and Linux: nothing, Tauri's `set_focus` brings the
    /// window forward there.
    fn activate_app(&self) -> Result<(), PlatformError>;

    /// Note which app is in front, before an interrupt opens and before [`Platform::activate_app`],
    /// so [`Platform::restore_previous`] can hand the front back (decision 2026-10-22, "Closing a
    /// reminder gives focus back"). An app that is not Tolon replaces what was noted. When Tolon
    /// itself is in front the call changes nothing: that is the settings window, or a reminder that
    /// opens right after another, and the app noted for the first one carries over. Every reminder
    /// ends in [`Platform::restore_previous`] or [`Platform::forget_frontmost`], so nothing stale
    /// is left. macOS: `NSWorkspace.frontmostApplication`, kept as a pid. Windows: the foreground
    /// window. Linux: nothing.
    fn remember_frontmost(&self) -> Result<(), PlatformError>;

    /// Make the app noted by [`Platform::remember_frontmost`] the front one again, once no
    /// interrupt window is left on screen, and forget it. Does nothing when no app was noted or it
    /// has quit. The OS may refuse (Windows' foreground lock); that is not an error.
    fn restore_previous(&self) -> Result<(), PlatformError>;

    /// Forget the app noted by [`Platform::remember_frontmost`] without bringing it forward: after
    /// Join (the browser has focus), or when the interrupt could not open.
    fn forget_frontmost(&self) -> Result<(), PlatformError>;

    /// How much of the inner size Tauri reports for `window` the title bar covers, in logical
    /// points. macOS lays the content view under the title bar, so Tauri's inner size there is
    /// the whole frame: a 760-point inner size shows 728 points of page below a 32-point title
    /// bar. Add this to a wanted visible height. Windows, Linux and the stub: 0, the inner size
    /// is already below the title bar.
    fn title_bar_overlap(&self, window: &dyn HasWindowHandle) -> Result<f64, PlatformError>;

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

    /// Whether the OS starts Tolon at login, read from the OS on every call, never cached
    /// (AC9.4). [`StartAtLogin::RequiresApproval`] (macOS) means registered but not yet allowed
    /// in System Settings, Login Items; the switch shows off and the UI says so.
    fn start_at_login(&self) -> Result<StartAtLogin, PlatformError>;

    /// Register or remove the login item for the running program. Call
    /// [`Platform::start_at_login`] afterwards for the real state: macOS may register and still
    /// wait for approval. Removing an item that is not there succeeds.
    fn set_start_at_login(&self, enabled: bool) -> Result<(), PlatformError>;

    /// How the tray icon looks and reacts here (F7). No OS call; any thread.
    fn tray_style(&self) -> TrayStyle;

    /// Where the pointer is now, in the unit [`Screen::placement`] uses: logical points on macOS
    /// (top-left origin of the primary screen, y down), physical pixels on Windows. Read it when a
    /// tray click arrives to find the icon's screen with [`Screen::contains_placement_point`].
    /// Linux and the stub return [`PlatformError::Unsupported`]: the app asks Tauri there.
    fn pointer(&self) -> Result<(f64, f64), PlatformError>;
}

/// The app [`Platform::remember_frontmost`] noted, until [`Platform::restore_previous`] takes it or
/// [`Platform::forget_frontmost`] drops it. `T` is whatever names an app on the OS (macOS pid,
/// Windows window handle and process id).
pub(crate) struct Previous<T>(Mutex<Option<T>>);

#[cfg_attr(target_os = "linux", allow(dead_code))]
impl<T> Previous<T> {
    pub(crate) const fn new() -> Self {
        Self(Mutex::new(None))
    }

    /// Replaces what was noted. Not called while Tolon is in front: what was noted then stays.
    pub(crate) fn remember(&self, app: T) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(app);
    }

    /// Drops what was noted.
    pub(crate) fn forget(&self) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
    }

    /// The noted app, once: a second call gets `None`.
    pub(crate) fn take(&self) -> Option<T> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

/// Implementation for CI and tests: one fixed 1440x900 primary screen at 2x; raising does
/// nothing and reports no focus; the login item is one flag in memory, shared by the process
/// like the one login item of a real machine, so tests that set it must not run in parallel. It
/// is also what unknown OSes get, so it reports `Degraded(Unsupported)`: nothing here can cover
/// the screen.
pub struct StubPlatform;

static STUB_START_AT_LOGIN: AtomicBool = AtomicBool::new(false);
static STUB_ACTIVATIONS: AtomicUsize = AtomicUsize::new(0);
/// The id of the app a test puts in front; 0 is Tolon itself or none.
static STUB_FRONT: AtomicU32 = AtomicU32::new(0);
static STUB_PREVIOUS: Previous<u32> = Previous::new();
static STUB_RESTORES: Mutex<Vec<u32>> = Mutex::new(Vec::new());

impl StubPlatform {
    /// How many times [`Platform::activate_app`] was called in this process, so tests can check
    /// it ran, and in which order against other steps. Shared by the process like the login flag.
    pub fn activations(&self) -> usize {
        STUB_ACTIVATIONS.load(Ordering::SeqCst)
    }

    /// Puts app `id` in front for the next [`Platform::remember_frontmost`]; 0 is Tolon itself.
    pub fn set_frontmost(&self, id: u32) {
        STUB_FRONT.store(id, Ordering::SeqCst);
    }

    /// The apps [`Platform::restore_previous`] brought back, oldest first.
    pub fn restored(&self) -> Vec<u32> {
        STUB_RESTORES
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Platform for StubPlatform {
    fn name(&self) -> &'static str {
        "stub"
    }

    fn prepare_app(&self) -> Result<(), PlatformError> {
        Ok(())
    }

    fn activate_app(&self) -> Result<(), PlatformError> {
        STUB_ACTIVATIONS.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn remember_frontmost(&self) -> Result<(), PlatformError> {
        let front = STUB_FRONT.load(Ordering::SeqCst);
        if front != 0 {
            STUB_PREVIOUS.remember(front);
        }
        Ok(())
    }

    fn forget_frontmost(&self) -> Result<(), PlatformError> {
        STUB_PREVIOUS.forget();
        Ok(())
    }

    fn restore_previous(&self) -> Result<(), PlatformError> {
        if let Some(id) = STUB_PREVIOUS.take() {
            STUB_RESTORES
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(id);
        }
        Ok(())
    }

    fn title_bar_overlap(&self, _window: &dyn HasWindowHandle) -> Result<f64, PlatformError> {
        Ok(0.0)
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

    fn start_at_login(&self) -> Result<StartAtLogin, PlatformError> {
        Ok(if STUB_START_AT_LOGIN.load(Ordering::SeqCst) {
            StartAtLogin::Enabled
        } else {
            StartAtLogin::Disabled
        })
    }

    fn set_start_at_login(&self, enabled: bool) -> Result<(), PlatformError> {
        STUB_START_AT_LOGIN.store(enabled, Ordering::SeqCst);
        Ok(())
    }

    /// A colour icon that reports clicks, like Windows.
    fn tray_style(&self) -> TrayStyle {
        TrayStyle {
            template_icon: false,
            clicks: true,
        }
    }

    fn pointer(&self) -> Result<(f64, f64), PlatformError> {
        Err(PlatformError::Unsupported)
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
    fn ac7_1_stub_tray_is_a_colour_icon_with_clicks_and_no_pointer() {
        assert_eq!(
            StubPlatform.tray_style(),
            TrayStyle {
                template_icon: false,
                clicks: true
            }
        );
        assert_eq!(StubPlatform.pointer(), Err(PlatformError::Unsupported));
    }

    /// AC7.2 on a Retina primary (1440x900 points at 2x) with a 1x screen to its right
    /// (1920x1080 at x = 1440). macOS gives each screen physical values of its own points times
    /// its own scale, so physical rectangles overlap: x = 2000 px is on both. Logical points,
    /// what `pointer` returns on macOS, put a click at x = 2000 on the 1x screen only.
    #[test]
    fn ac7_2_a_click_on_a_1x_screen_beside_a_retina_one_finds_the_1x_screen() {
        let retina = Screen {
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
        };
        let plain = Screen {
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
        };
        let hit = |x, y| {
            [&retina, &plain]
                .into_iter()
                .find(|s| s.contains_logical(x, y))
                .map(|s| s.id)
        };
        assert_eq!(hit(2000.0, 10.0), Some(2));
        assert_eq!(hit(1439.0, 10.0), Some(1));
        assert_eq!(
            hit(1440.0, 10.0),
            Some(2),
            "the left edge belongs to the 1x screen"
        );
        assert_eq!(hit(1300.0, 1000.0), None, "below the shorter Retina screen");
        // The physical rectangles cannot tell: both claim x = 2000.
        assert!(retina.contains_physical(2000.0, 10.0) && plain.contains_physical(2000.0, 10.0));
        // The unit `pointer` uses is the one `placement` uses.
        assert_eq!(
            plain.contains_placement_point(2000.0, 10.0),
            if cfg!(target_os = "macos") {
                plain.contains_logical(2000.0, 10.0)
            } else {
                plain.contains_physical(2000.0, 10.0)
            }
        );
    }

    #[test]
    fn stub_activate_app_records_each_call() {
        let before = StubPlatform.activations();
        assert_eq!(StubPlatform.activate_app(), Ok(()));
        assert_eq!(StubPlatform.activations(), before + 1);
    }

    #[test]
    fn previous_gives_the_app_once_and_forget_drops_it() {
        let previous = Previous::new();
        assert_eq!(previous.take(), None::<u32>);
        previous.remember(7);
        assert_eq!(previous.take(), Some(7));
        assert_eq!(previous.take(), None);
        previous.remember(7);
        previous.remember(8);
        previous.forget();
        assert_eq!(previous.take(), None);
    }

    /// The stub's front app, noted app and restore log are process-wide; these tests share them.
    static FOCUS_TESTS: Mutex<()> = Mutex::new(());

    #[test]
    fn stub_restores_the_app_that_was_in_front_once() {
        let _one_at_a_time = FOCUS_TESTS.lock().unwrap_or_else(PoisonError::into_inner);
        let before = StubPlatform.restored().len();
        StubPlatform.set_frontmost(42);
        assert_eq!(StubPlatform.remember_frontmost(), Ok(()));
        assert_eq!(StubPlatform.restore_previous(), Ok(()));
        assert_eq!(StubPlatform.restore_previous(), Ok(()));
        assert_eq!(StubPlatform.restored()[before..], [42]);
    }

    #[test]
    fn stub_restores_nothing_when_nothing_was_remembered() {
        let _one_at_a_time = FOCUS_TESTS.lock().unwrap_or_else(PoisonError::into_inner);
        let before = StubPlatform.restored().len();
        assert_eq!(StubPlatform.restore_previous(), Ok(()));
        // Tolon itself in front (the settings window) and nothing noted: nothing to restore.
        StubPlatform.set_frontmost(0);
        assert_eq!(StubPlatform.remember_frontmost(), Ok(()));
        assert_eq!(StubPlatform.restore_previous(), Ok(()));
        // An app noted and then forgotten (after Join, or a failed open) is not restored either.
        StubPlatform.set_frontmost(42);
        assert_eq!(StubPlatform.remember_frontmost(), Ok(()));
        assert_eq!(StubPlatform.forget_frontmost(), Ok(()));
        assert_eq!(StubPlatform.restore_previous(), Ok(()));
        assert_eq!(StubPlatform.restored().len(), before);
    }

    #[test]
    fn stub_keeps_the_noted_app_when_tolon_is_in_front_at_the_next_reminder() {
        let _one_at_a_time = FOCUS_TESTS.lock().unwrap_or_else(PoisonError::into_inner);
        let before = StubPlatform.restored().len();
        StubPlatform.set_frontmost(42);
        assert_eq!(StubPlatform.remember_frontmost(), Ok(()));
        // The first reminder hands the screen to a waiting one: Tolon is in front.
        StubPlatform.set_frontmost(0);
        assert_eq!(StubPlatform.remember_frontmost(), Ok(()));
        assert_eq!(StubPlatform.restore_previous(), Ok(()));
        assert_eq!(StubPlatform.restored()[before..], [42]);
    }

    #[test]
    fn stub_title_bar_covers_nothing() {
        let w = fake::Fake(RawWindowHandle::Xlib(XlibWindowHandle::new(1)));
        assert_eq!(StubPlatform.title_bar_overlap(&w), Ok(0.0));
    }

    #[test]
    fn stub_takeover_is_degraded_as_unsupported() {
        assert_eq!(
            StubPlatform.takeover_support(),
            Takeover::Degraded(Reason::Unsupported)
        );
    }

    #[test]
    fn ac9_4_stub_login_item_is_read_back_from_memory() {
        assert_eq!(StubPlatform.set_start_at_login(true), Ok(()));
        assert_eq!(StubPlatform.start_at_login(), Ok(StartAtLogin::Enabled));
        assert_eq!(StubPlatform.set_start_at_login(false), Ok(()));
        assert_eq!(StubPlatform.start_at_login(), Ok(StartAtLogin::Disabled));
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

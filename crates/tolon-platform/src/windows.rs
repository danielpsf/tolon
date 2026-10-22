//! Windows implementation on Win32: topmost window and monitor enumeration.
//!
//! Foreground lock: Windows lets only the foreground process, or the process that got the last
//! input event, move a window to the foreground. A tray app at alarm time is neither, so a plain
//! `SetForegroundWindow` is refused and the taskbar button only flashes. Tolon uses the
//! `AttachThreadInput` technique: it joins its input queue to the foreground window's thread for
//! the duration of the call, which passes that check, then detaches. `AllowSetForegroundWindow`
//! is not an option: it lets *another* process take the foreground and needs the caller to be
//! the foreground process already. Not used: the Alt-key trick (`keybd_event`) opens menus in
//! the app underneath, and `SPI_SETFOREGROUNDLOCKTIMEOUT` changes a system-wide setting.
//!
//! `HWND_TOPMOST` does not depend on the foreground lock, so the window covers other windows
//! even when the OS refuses focus. In that case `raise_above_everything` still returns `Ok`, with
//! `Raised { focus: Focus::No }`; the app decides whether to add the F4.5 notification.

use crate::login::{startup_approved_disabled, windows_run_value, windows_run_value_is};
use crate::monitors::{screens_from, RawMonitor, BASE_DPI};
use crate::{
    hwnd_of, Focus, Platform, PlatformError, Previous, Raised, Screen, StartAtLogin, Takeover,
    TrayStyle,
};
use raw_window_handle::HasWindowHandle;
use std::ffi::c_void;
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_UNSUPPORTED_TYPE, HWND, LPARAM, POINT, RECT,
};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
    REG_ROUTINE_FLAGS, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ,
};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId, IsWindow,
    SetForegroundWindow, SetWindowPos, HWND_TOPMOST, MONITORINFOF_PRIMARY, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW,
};

pub struct WindowsPlatform;

/// The foreground window when the interrupt opened: the `HWND` as an integer (so the static is
/// `Sync`) and the id of its process, which tells a reused handle from the window noted.
static PREVIOUS: Previous<(isize, u32)> = Previous::new();

fn os_error(call: &'static str, e: &windows::core::Error) -> PlatformError {
    // Reinterpret the HRESULT bits; `{:#010x}` in the message then reads like 0x80070005.
    PlatformError::Os {
        call,
        code: u32::from_ne_bytes(e.code().0.to_ne_bytes()),
    }
}

/// One monitor, or `None` when the OS will not describe it.
fn describe(hmonitor: HMONITOR) -> Option<RawMonitor> {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a live MONITORINFO with `cbSize` set, as the call requires.
    if !unsafe { GetMonitorInfoW(hmonitor, &mut info) }.as_bool() {
        return None;
    }
    let (mut dpi_x, mut dpi_y) = (BASE_DPI, BASE_DPI);
    // SAFETY: both out-pointers are live u32s. A failure leaves the 96 DPI default (scale 1.0).
    let _ = unsafe { GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
    Some(RawMonitor {
        // HMONITOR values are 32-bit significant on 64-bit Windows, so the cast loses nothing.
        id: hmonitor.0 as usize as u32,
        left: info.rcMonitor.left,
        top: info.rcMonitor.top,
        right: info.rcMonitor.right,
        bottom: info.rcMonitor.bottom,
        dpi: dpi_x,
        primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
    })
}

unsafe extern "system" fn collect(
    hmonitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    // SAFETY: `data` is the `&mut Vec<RawMonitor>` that `screens` passed to EnumDisplayMonitors,
    // which runs this callback only during that call, on the same thread.
    let found = unsafe { &mut *(data.0 as *mut Vec<RawMonitor>) };
    found.extend(describe(hmonitor));
    BOOL::from(true)
}

/// Our input queue joined to another thread's; dropping it detaches again, so no exit path
/// leaves the two queues attached.
struct InputAttach {
    this_thread: u32,
    other_thread: u32,
}

impl InputAttach {
    /// `None` when the threads are the same or Windows refuses to attach.
    fn new(this_thread: u32, other_thread: u32) -> Option<Self> {
        if other_thread == 0 || other_thread == this_thread {
            return None;
        }
        // SAFETY: plain Win32 call on thread ids; Windows rejects ids that do not exist.
        unsafe { AttachThreadInput(this_thread, other_thread, true) }
            .as_bool()
            .then_some(Self {
                this_thread,
                other_thread,
            })
    }
}

impl Drop for InputAttach {
    fn drop(&mut self) {
        // SAFETY: same ids that were attached in `new`.
        let _ = unsafe { AttachThreadInput(self.this_thread, self.other_thread, false) };
    }
}

/// Try to make `hwnd` the foreground window; `false` when Windows refuses.
fn take_foreground(hwnd: HWND) -> bool {
    // SAFETY: plain Win32 calls on handles that may be stale; Windows rejects stale handles.
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground == hwnd {
            return true;
        }
        let foreground_thread = if foreground.0.is_null() {
            0
        } else {
            GetWindowThreadProcessId(foreground, None)
        };
        let _attached = InputAttach::new(GetCurrentThreadId(), foreground_thread);
        let _ = BringWindowToTop(hwnd);
        SetForegroundWindow(hwnd).as_bool()
    }
}

/// Start at login: the per-user `Run` key, no admin rights. The key exists in every profile.
const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const RUN_VALUE: PCWSTR = w!("Tolon");

fn current_exe() -> Result<String, PlatformError> {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| PlatformError::Io(e.to_string()))
}

/// Where Task Manager, Startup apps keeps its on/off choice for a `Run` value. Tolon reads it
/// and never writes it.
const APPROVED_KEY: PCWSTR =
    w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run");

/// The raw data of the `Tolon` value under `key`, read with the type filter `flags`. `None` when
/// the value is not there or holds another type (`ERROR_UNSUPPORTED_TYPE`): both mean nothing
/// usable is set.
fn read_value(key: PCWSTR, flags: REG_ROUTINE_FLAGS) -> Result<Option<Vec<u8>>, PlatformError> {
    read_raw(HKEY_CURRENT_USER, key, RUN_VALUE, flags)
}

/// The raw data of `value` under `root\key`, read with the type filter `flags`; `None` as above.
fn read_raw(
    root: HKEY,
    key: PCWSTR,
    value: PCWSTR,
    flags: REG_ROUTINE_FLAGS,
) -> Result<Option<Vec<u8>>, PlatformError> {
    let mut bytes = 0u32;
    // SAFETY: with no data buffer the call only reports the size in `bytes`, a live u32.
    let size = unsafe { RegGetValueW(root, key, value, flags, None, None, Some(&mut bytes)) };
    if size == ERROR_FILE_NOT_FOUND || size == ERROR_UNSUPPORTED_TYPE {
        return Ok(None);
    }
    size.ok().map_err(|e| os_error("RegGetValueW", &e))?;
    let mut buf = vec![0u8; bytes as usize];
    // SAFETY: `buf` holds `bytes` bytes and stays alive for the call.
    let read = unsafe {
        RegGetValueW(
            root,
            key,
            value,
            flags,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    if read == ERROR_FILE_NOT_FOUND || read == ERROR_UNSUPPORTED_TYPE {
        return Ok(None);
    }
    read.ok().map_err(|e| os_error("RegGetValueW", &e))?;
    buf.truncate(bytes as usize);
    Ok(Some(buf))
}

/// The `Tolon` value of the `Run` key as text, or `None`.
fn read_run_value() -> Result<Option<String>, PlatformError> {
    Ok(read_value(RUN_KEY, RRF_RT_REG_SZ)?.map(|raw| utf16_text(&raw)))
}

/// A `REG_SZ` value as text, without its trailing NUL.
fn utf16_text(raw: &[u8]) -> String {
    let units: Vec<u16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    String::from_utf16_lossy(&units)
        .trim_end_matches('\0')
        .to_owned()
}

/// Where Windows keeps its product name, feature update and build number.
const VERSION_KEY: PCWSTR = w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");

/// One text value of [`VERSION_KEY`], or `None`.
fn version_value(value: PCWSTR) -> Option<String> {
    read_raw(HKEY_LOCAL_MACHINE, VERSION_KEY, value, RRF_RT_REG_SZ)
        .ok()
        .flatten()
        .map(|raw| utf16_text(&raw))
        .filter(|t| !t.is_empty())
}

impl Platform for WindowsPlatform {
    fn name(&self) -> &'static str {
        "windows"
    }

    fn prepare_app(&self) -> Result<(), PlatformError> {
        Ok(())
    }

    /// Nothing: Tauri's `set_focus` brings the window forward here.
    fn activate_app(&self) -> Result<(), PlatformError> {
        Ok(())
    }

    /// Nothing: the settings window is already listed in the taskbar or task list here.
    fn set_regular(&self, _regular: bool) -> Result<(), PlatformError> {
        Ok(())
    }

    /// `GetForegroundWindow` and its process, unless the window is this process's: then what was
    /// noted stays (a reminder that opens right after another).
    fn remember_frontmost(&self) -> Result<(), PlatformError> {
        // SAFETY: plain Win32 calls; a null result (no foreground window) is handled.
        let front = unsafe {
            let hwnd = GetForegroundWindow();
            let mut owner = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut owner));
            (!hwnd.0.is_null() && owner != std::process::id()).then_some((hwnd.0 as isize, owner))
        };
        if let Some(window) = front {
            PREVIOUS.remember(window);
        }
        Ok(())
    }

    fn forget_frontmost(&self) -> Result<(), PlatformError> {
        PREVIOUS.forget();
        Ok(())
    }

    /// `SetForegroundWindow` on the noted window while it is still that window: `IsWindow`, and the
    /// handle still belongs to the process noted (Windows reuses handles). Windows may refuse; that
    /// is ignored, the window then simply is not in front.
    fn restore_previous(&self) -> Result<(), PlatformError> {
        let Some((raw, owner)) = PREVIOUS.take() else {
            return Ok(());
        };
        let hwnd = HWND(raw as *mut c_void);
        let mut now_owner = 0u32;
        // SAFETY: IsWindow accepts any handle value and only reports whether it is a window; the
        // process id goes to a live u32.
        let same = unsafe {
            IsWindow(Some(hwnd)).as_bool() && {
                GetWindowThreadProcessId(hwnd, Some(&mut now_owner));
                now_owner == owner
            }
        };
        if same && !take_foreground(hwnd) {
            tracing::debug!("Windows refused to bring the previous window forward");
        }
        Ok(())
    }

    /// Nothing: Windows gives the foreground to the window that is on top.
    fn remember_join_target(&self, _url: &str) -> Result<(), PlatformError> {
        Ok(())
    }

    /// Nothing, as for `remember_join_target`.
    fn forget_join_target(&self) -> Result<(), PlatformError> {
        Ok(())
    }

    /// Nothing: Windows has no app-level activation without a window; the foreground window is
    /// whatever the system put there when the interrupt hid.
    fn yield_focus(&self) -> Result<(), PlatformError> {
        Ok(())
    }

    /// Tauri's inner size is already below the title bar here.
    fn title_bar_overlap(&self, _window: &dyn HasWindowHandle) -> Result<f64, PlatformError> {
        Ok(0.0)
    }

    /// Needs the process to be per-monitor DPI aware (Tauri's manifest is) for real scales; an
    /// unaware process sees virtualised coordinates at 96 DPI, which stay consistent.
    fn screens(&self) -> Result<Vec<Screen>, PlatformError> {
        let mut found: Vec<RawMonitor> = Vec::new();
        // SAFETY: `found` outlives the call and nothing else touches it meanwhile; `collect`
        // casts the pointer back to the same type.
        unsafe {
            EnumDisplayMonitors(
                None,
                None,
                Some(collect),
                LPARAM(std::ptr::from_mut(&mut found) as isize),
            )
        }
        .ok()
        .map_err(|e| os_error("EnumDisplayMonitors", &e))?;
        Ok(screens_from(&found))
    }

    fn raise_above_everything(
        &self,
        window: &dyn HasWindowHandle,
    ) -> Result<Raised, PlatformError> {
        let hwnd = HWND(hwnd_of(window)?.get() as *mut c_void);
        // SAFETY: IsWindow accepts any handle value and only reports whether it is a window.
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            return Err(PlatformError::NoWindow);
        }
        // SAFETY: `hwnd` is a live window (checked above). The flags keep size and position.
        unsafe {
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            )
        }
        .map_err(|e| os_error("SetWindowPos", &e))?;
        Ok(Raised {
            focus: if take_foreground(hwnd) {
                Focus::Yes
            } else {
                Focus::No
            },
        })
    }

    fn takeover_support(&self) -> Takeover {
        Takeover::Full
    }

    fn tray_style(&self) -> TrayStyle {
        TrayStyle {
            template_icon: false,
            clicks: true,
        }
    }

    /// `GetCursorPos`: physical pixels of the virtual screen (Tolon is per-monitor DPI aware).
    fn pointer(&self) -> Result<(f64, f64), PlatformError> {
        let mut p = POINT::default();
        // SAFETY: `p` is a valid, writable POINT for the duration of the call.
        unsafe { GetCursorPos(&mut p) }.map_err(|e| PlatformError::Os {
            call: "GetCursorPos",
            code: u32::from_ne_bytes(e.code().0.to_ne_bytes()),
        })?;
        Ok((f64::from(p.x), f64::from(p.y)))
    }

    /// "Windows 10 Pro 23H2, build 22631": the product name says 10 on Windows 11 too, so the
    /// build number (22000 and up is 11) is what tells them apart.
    fn os_version(&self) -> String {
        let name = version_value(w!("ProductName")).unwrap_or_else(|| "Windows".to_owned());
        let feature = version_value(w!("DisplayVersion"))
            .map(|v| format!(" {v}"))
            .unwrap_or_default();
        match version_value(w!("CurrentBuildNumber")) {
            Some(build) => format!("{name}{feature}, build {build}"),
            None => format!("{name}{feature}"),
        }
    }

    /// `%SystemRoot%\explorer.exe /select,"<path>"`: the full path, so no `explorer.exe` earlier
    /// on the search path runs instead. The argument is passed raw: Explorer parses its own
    /// command line and does not follow the quoting `Command::arg` writes.
    fn reveal_file(&self, path: &std::path::Path) -> Result<(), PlatformError> {
        use std::os::windows::process::CommandExt;
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        let mut explorer =
            std::process::Command::new(std::path::Path::new(&root).join("explorer.exe"));
        explorer.raw_arg(format!("/select,\"{}\"", path.display()));
        crate::run_launcher("explorer /select", &mut explorer, false)
    }

    /// Enabled only when the value starts this exe (an entry left by a moved install is off) and
    /// the person has not turned it off in Task Manager, Startup apps.
    fn start_at_login(&self) -> Result<StartAtLogin, PlatformError> {
        let exe = current_exe()?;
        let run = read_run_value()?.is_some_and(|v| windows_run_value_is(&v, &exe));
        if !run {
            return Ok(StartAtLogin::Disabled);
        }
        let approved = read_value(APPROVED_KEY, RRF_RT_REG_BINARY)?;
        Ok(if approved.is_some_and(|d| startup_approved_disabled(&d)) {
            StartAtLogin::Disabled
        } else {
            StartAtLogin::Enabled
        })
    }

    fn set_start_at_login(&self, enabled: bool) -> Result<(), PlatformError> {
        if !enabled {
            // SAFETY: plain registry call on constant, NUL-terminated names.
            let gone = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE) };
            return if gone == ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                gone.ok().map_err(|e| os_error("RegDeleteKeyValueW", &e))
            };
        }
        let data: Vec<u16> = windows_run_value(&current_exe()?)
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let bytes =
            u32::try_from(data.len() * 2).map_err(|_| PlatformError::Io("path too long".into()))?;
        // SAFETY: `data` is a NUL-terminated UTF-16 string of `bytes` bytes, alive for the call.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                RUN_VALUE,
                REG_SZ.0,
                Some(data.as_ptr().cast()),
                bytes,
            )
        }
        .ok()
        .map_err(|e| os_error("RegSetKeyValueW", &e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::{Closed, Fake};
    use crate::monitors::scale_from_dpi;
    use raw_window_handle::{RawWindowHandle, Win32WindowHandle, XlibWindowHandle};
    use std::num::NonZeroIsize;

    #[test]
    fn s4_2_real_screens_have_one_main_and_sane_scale() {
        let screens = WindowsPlatform.screens().unwrap_or_default();
        // A headless runner may report no monitor; any monitor list has exactly one main.
        assert_eq!(
            screens.iter().filter(|s| s.is_main).count(),
            usize::from(!screens.is_empty())
        );
        assert!(screens.iter().all(|s| s.width > 0.0
            && s.height > 0.0
            && s.scale_factor >= scale_from_dpi(BASE_DPI)));
        assert!(screens.first().is_none_or(|s| s.is_main));
    }

    #[test]
    fn l056_restoring_with_nothing_remembered_is_a_no_op() {
        assert_eq!(WindowsPlatform.restore_previous(), Ok(()));
        assert_eq!(
            WindowsPlatform.remember_join_target("https://x.example/j"),
            Ok(())
        );
        assert_eq!(WindowsPlatform.forget_join_target(), Ok(()));
        assert_eq!(WindowsPlatform.yield_focus(), Ok(()));
        assert_eq!(WindowsPlatform.forget_frontmost(), Ok(()));
    }

    #[test]
    fn a_handle_that_is_no_window_is_an_error_not_a_crash() {
        let raw = RawWindowHandle::Win32(Win32WindowHandle::new(NonZeroIsize::MIN));
        assert_eq!(
            WindowsPlatform.raise_above_everything(&Fake(raw)),
            Err(PlatformError::NoWindow)
        );
    }

    #[test]
    fn a_foreign_handle_is_rejected() {
        let raw = RawWindowHandle::Xlib(XlibWindowHandle::new(1));
        assert_eq!(
            WindowsPlatform.raise_above_everything(&Fake(raw)),
            Err(PlatformError::WrongHandleKind("X11"))
        );
    }

    #[test]
    fn a_closed_window_is_handle_unavailable() {
        assert!(matches!(
            WindowsPlatform.raise_above_everything(&Closed),
            Err(PlatformError::HandleUnavailable(_))
        ));
    }

    #[test]
    fn takeover_is_full() {
        assert_eq!(WindowsPlatform.takeover_support(), crate::Takeover::Full);
    }
}

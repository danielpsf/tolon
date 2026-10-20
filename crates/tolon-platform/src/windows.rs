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
use crate::{hwnd_of, Focus, Platform, PlatformError, Raised, Screen, StartAtLogin, Takeover};
use raw_window_handle::HasWindowHandle;
use std::ffi::c_void;
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_UNSUPPORTED_TYPE, HWND, LPARAM, RECT,
};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_ROUTINE_FLAGS,
    REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ,
};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsWindow, SetForegroundWindow,
    SetWindowPos, HWND_TOPMOST, MONITORINFOF_PRIMARY, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
};

pub struct WindowsPlatform;

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
    let mut bytes = 0u32;
    // SAFETY: with no data buffer the call only reports the size in `bytes`, a live u32.
    let size = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            RUN_VALUE,
            flags,
            None,
            None,
            Some(&mut bytes),
        )
    };
    if size == ERROR_FILE_NOT_FOUND || size == ERROR_UNSUPPORTED_TYPE {
        return Ok(None);
    }
    size.ok().map_err(|e| os_error("RegGetValueW", &e))?;
    let mut buf = vec![0u8; bytes as usize];
    // SAFETY: `buf` holds `bytes` bytes and stays alive for the call.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            RUN_VALUE,
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
    Ok(read_value(RUN_KEY, RRF_RT_REG_SZ)?.map(|raw| {
        let units: Vec<u16> = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        String::from_utf16_lossy(&units)
            .trim_end_matches('\0')
            .to_owned()
    }))
}

impl Platform for WindowsPlatform {
    fn name(&self) -> &'static str {
        "windows"
    }

    fn prepare_app(&self) -> Result<(), PlatformError> {
        Ok(())
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

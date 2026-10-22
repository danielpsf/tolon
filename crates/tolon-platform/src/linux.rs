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
//! Start at login is an XDG autostart entry (`login.rs` holds the logic every OS tests).
//!
//! `screens` returns `Err(Unsupported)`: GDK and XRandR are the only sources, both need GTK or a
//! new X11 dependency, and Tauri's `available_monitors` already reads them. An error, not an
//! empty list, so a successful result always has exactly one main screen.

use crate::login::{
    appimage_program, autostart_file, read_autostart, remove_autostart, write_autostart,
};
use crate::session::{detect_session, session_of_handle, takeover_for};
use crate::{Focus, Platform, PlatformError, Raised, Screen, StartAtLogin, Takeover, TrayStyle};
use raw_window_handle::HasWindowHandle;
use std::path::PathBuf;

pub struct LinuxPlatform;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// The autostart file and the program it must start. An absolute `$APPIMAGE` wins over the running exe,
/// which inside an AppImage is a temporary mount that is gone after the next start. A Flatpak
/// sandbox has its own config home that the host never reads, so it is unsupported here.
fn login_target() -> Result<(PathBuf, String), PlatformError> {
    if env("FLATPAK_ID").is_some() {
        return Err(PlatformError::Unsupported);
    }
    let file = autostart_file(env("XDG_CONFIG_HOME").as_deref(), env("HOME").as_deref())
        .ok_or_else(|| PlatformError::Io("no config directory: HOME is not set".into()))?;
    let exe = match appimage_program(env("APPIMAGE").as_deref()) {
        Some(p) => p.to_owned(),
        None => std::env::current_exe()
            .map_err(|e| PlatformError::Io(e.to_string()))?
            .to_string_lossy()
            .into_owned(),
    };
    Ok((file, exe))
}

impl Platform for LinuxPlatform {
    fn name(&self) -> &'static str {
        "linux"
    }

    fn prepare_app(&self) -> Result<(), PlatformError> {
        Ok(())
    }

    /// Nothing: Tauri's `set_focus` brings the window forward here.
    fn activate_app(&self) -> Result<(), PlatformError> {
        Ok(())
    }

    /// Nothing: the window manager decides what is in front when the interrupt hides.
    fn remember_frontmost(&self) -> Result<(), PlatformError> {
        Ok(())
    }

    /// Nothing, as for `remember_frontmost`.
    fn restore_previous(&self) -> Result<(), PlatformError> {
        Ok(())
    }

    /// Nothing, as for `remember_frontmost`.
    fn forget_frontmost(&self) -> Result<(), PlatformError> {
        Ok(())
    }

    /// Nothing: no app-level activation without a window.
    fn yield_focus(&self) -> Result<(), PlatformError> {
        Ok(())
    }

    /// Tauri's inner size is already below the title bar here.
    fn title_bar_overlap(&self, _window: &dyn HasWindowHandle) -> Result<f64, PlatformError> {
        Ok(0.0)
    }

    fn screens(&self) -> Result<Vec<Screen>, PlatformError> {
        Err(PlatformError::Unsupported)
    }

    fn raise_above_everything(
        &self,
        window: &dyn HasWindowHandle,
    ) -> Result<Raised, PlatformError> {
        let handle = window
            .window_handle()
            .map_err(|e| PlatformError::HandleUnavailable(e.to_string()))?;
        session_of_handle(handle.as_raw())
            .map(|_| Raised {
                focus: Focus::Unknown,
            })
            .ok_or(PlatformError::WrongHandleKind("not X11 or Wayland"))
    }

    /// AppIndicator trays report no clicks, only menus.
    fn tray_style(&self) -> TrayStyle {
        TrayStyle {
            template_icon: false,
            clicks: false,
        }
    }

    /// X11 and Wayland give the pointer to the toolkit, not to this crate: the app asks Tauri.
    fn pointer(&self) -> Result<(f64, f64), PlatformError> {
        Err(PlatformError::Unsupported)
    }

    fn takeover_support(&self) -> Takeover {
        takeover_for(detect_session(
            env("XDG_SESSION_TYPE").as_deref(),
            env("WAYLAND_DISPLAY").as_deref(),
        ))
    }

    fn start_at_login(&self) -> Result<StartAtLogin, PlatformError> {
        let (file, exe) = login_target()?;
        read_autostart(&file, &exe)
    }

    fn set_start_at_login(&self, enabled: bool) -> Result<(), PlatformError> {
        let (file, exe) = login_target()?;
        if enabled {
            write_autostart(&file, &exe)
        } else {
            remove_autostart(&file)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::{Closed, Fake};
    use raw_window_handle::{AppKitWindowHandle, RawWindowHandle, XlibWindowHandle};
    use std::ptr::NonNull;

    #[test]
    fn l056_focus_calls_do_nothing() {
        assert_eq!(LinuxPlatform.remember_frontmost(), Ok(()));
        assert_eq!(LinuxPlatform.restore_previous(), Ok(()));
        assert_eq!(LinuxPlatform.yield_focus(), Ok(()));
        assert_eq!(LinuxPlatform.forget_frontmost(), Ok(()));
    }

    #[test]
    fn screens_are_left_to_tauri() {
        assert_eq!(LinuxPlatform.screens(), Err(PlatformError::Unsupported));
    }

    #[test]
    fn raise_accepts_an_x11_window() {
        let w = Fake(RawWindowHandle::Xlib(XlibWindowHandle::new(1)));
        assert_eq!(
            LinuxPlatform.raise_above_everything(&w),
            Ok(Raised {
                focus: Focus::Unknown
            })
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

    // The only test that touches these variables, so it cannot race with the others.
    #[test]
    fn ac9_4_linux_login_item_follows_the_environment() -> Result<(), PlatformError> {
        let home = std::env::temp_dir().join(format!("tolon-linux-{}", std::process::id()));
        let file = home.join("autostart").join("dev.tolon.desktop");
        std::env::set_var("XDG_CONFIG_HOME", &home);
        std::env::remove_var("FLATPAK_ID");
        std::env::set_var("APPIMAGE", "/home/a b/Tolon.AppImage");

        assert_eq!(LinuxPlatform.start_at_login()?, StartAtLogin::Disabled);
        LinuxPlatform.set_start_at_login(true)?;
        let written =
            std::fs::read_to_string(&file).map_err(|e| PlatformError::Io(e.to_string()))?;
        assert!(written.contains("Exec=\"/home/a b/Tolon.AppImage\"\n"));
        assert_eq!(LinuxPlatform.start_at_login()?, StartAtLogin::Enabled);
        // The same file no longer matches once $APPIMAGE points elsewhere: read, never cached.
        std::env::set_var("APPIMAGE", "/home/a/Other.AppImage");
        assert_eq!(LinuxPlatform.start_at_login()?, StartAtLogin::Disabled);
        LinuxPlatform.set_start_at_login(false)?;
        assert!(!file.exists());

        // A relative $APPIMAGE is ignored: the entry names the running exe instead.
        std::env::set_var("APPIMAGE", "Relative.AppImage");
        LinuxPlatform.set_start_at_login(true)?;
        let written =
            std::fs::read_to_string(&file).map_err(|e| PlatformError::Io(e.to_string()))?;
        assert!(!written.contains("Relative.AppImage"));
        assert_eq!(LinuxPlatform.start_at_login()?, StartAtLogin::Enabled);
        LinuxPlatform.set_start_at_login(false)?;

        std::env::set_var("FLATPAK_ID", "dev.tolon.desktop");
        assert_eq!(
            LinuxPlatform.start_at_login(),
            Err(PlatformError::Unsupported)
        );
        std::env::remove_var("FLATPAK_ID");
        std::env::remove_var("APPIMAGE");
        let _ = std::fs::remove_dir_all(&home);
        Ok(())
    }
}

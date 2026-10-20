//! macOS implementation on AppKit. Every call needs the main thread.

use crate::login::from_sm_app_service_status;
use crate::macos_level::{top_left_y, INTERRUPT_COLLECTION_BEHAVIOR, INTERRUPT_WINDOW_LEVEL};
use crate::monitors::{to_physical_i32, to_physical_u32};
use crate::{ns_view_of, Focus, Platform, PlatformError, Raised, Screen, StartAtLogin, Takeover};
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSScreen, NSView, NSWindow, NSWindowCollectionBehavior};
use objc2_foundation::{ns_string, NSError, NSNumber};
use objc2_service_management::SMAppService;
use raw_window_handle::HasWindowHandle;

pub struct MacPlatform;

fn main_thread() -> Result<MainThreadMarker, PlatformError> {
    MainThreadMarker::new().ok_or(PlatformError::NotMainThread)
}

fn display_id(screen: &NSScreen, fallback: u32) -> u32 {
    screen
        .deviceDescription()
        .objectForKey(ns_string!("NSScreenNumber"))
        .and_then(|n| n.downcast::<NSNumber>().ok())
        .map_or(fallback, |n| n.unsignedIntValue())
}

/// An `NSError` from SMAppService as an OS error. The code is reinterpreted bit for bit, as for
/// Win32; the description (no personal data) goes to the log.
fn sm_error(call: &'static str, e: &NSError) -> PlatformError {
    tracing::warn!(call, error = %e.localizedDescription(), "login item call failed");
    let code = i32::try_from(e.code()).unwrap_or(i32::MIN);
    PlatformError::Os {
        call,
        code: u32::from_ne_bytes(code.to_ne_bytes()),
    }
}

impl Platform for MacPlatform {
    fn name(&self) -> &'static str {
        "macos"
    }

    fn screens(&self) -> Result<Vec<Screen>, PlatformError> {
        let mtm = main_thread()?;
        // screens[0] is the screen with the menu bar; NSScreen.main is the focused one instead.
        let all = NSScreen::screens(mtm);
        let primary_height = all.iter().next().map_or(0.0, |s| s.frame().size.height);
        Ok(all
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let f = s.frame();
                let scale = s.backingScaleFactor();
                let y = top_left_y(f.origin.y, f.size.height, primary_height);
                Screen {
                    id: display_id(&s, u32::try_from(i).unwrap_or(u32::MAX)),
                    x: f.origin.x,
                    y,
                    width: f.size.width,
                    height: f.size.height,
                    physical_x: to_physical_i32(f.origin.x, scale),
                    physical_y: to_physical_i32(y, scale),
                    physical_width: to_physical_u32(f.size.width, scale),
                    physical_height: to_physical_u32(f.size.height, scale),
                    scale_factor: scale,
                    is_main: i == 0,
                }
            })
            .collect())
    }

    fn raise_above_everything(
        &self,
        window: &dyn HasWindowHandle,
    ) -> Result<Raised, PlatformError> {
        let _mtm = main_thread()?;
        let view = ns_view_of(window)?;
        // SAFETY: an AppKit handle carries a live NSView. `window` is borrowed for this call and
        // we are on the main thread (checked above), so nothing frees the view meanwhile.
        let view: &NSView = unsafe { view.cast::<NSView>().as_ref() };
        let ns_window: Retained<NSWindow> = view.window().ok_or(PlatformError::NoWindow)?;
        ns_window.setLevel(INTERRUPT_WINDOW_LEVEL);
        ns_window.setCollectionBehavior(NSWindowCollectionBehavior(INTERRUPT_COLLECTION_BEHAVIOR));
        ns_window.makeKeyAndOrderFront(None);
        // Not yet key is not a refusal: a tray app often becomes key a moment later.
        let focus = if ns_window.isKeyWindow() {
            Focus::Yes
        } else {
            Focus::Unknown
        };
        Ok(Raised { focus })
    }

    fn takeover_support(&self) -> Takeover {
        Takeover::Full
    }

    fn start_at_login(&self) -> Result<StartAtLogin, PlatformError> {
        main_thread()?;
        // SAFETY: `mainAppService` and `status` take no arguments and have no preconditions.
        let status = unsafe { SMAppService::mainAppService().status() };
        Ok(from_sm_app_service_status(status.0))
    }

    fn set_start_at_login(&self, enabled: bool) -> Result<(), PlatformError> {
        main_thread()?;
        // SAFETY: as in `start_at_login`; the error is returned, not thrown.
        let service = unsafe { SMAppService::mainAppService() };
        if enabled {
            unsafe { service.registerAndReturnError() }.map_err(|e| sm_error("register", &e))
        } else {
            unsafe { service.unregisterAndReturnError() }.map_err(|e| sm_error("unregister", &e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macos_level::SCREEN_SAVER_WINDOW_LEVEL;
    use objc2_app_kit::NSScreenSaverWindowLevel;

    #[test]
    fn constants_match_appkit() {
        assert_eq!(SCREEN_SAVER_WINDOW_LEVEL, NSScreenSaverWindowLevel);
        assert_eq!(
            INTERRUPT_COLLECTION_BEHAVIOR,
            (NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary)
                .0
        );
    }

    #[test]
    fn ac9_4_status_values_match_service_management() {
        use objc2_service_management::SMAppServiceStatus as S;
        for (raw, state) in [
            (S::NotRegistered, StartAtLogin::Disabled),
            (S::Enabled, StartAtLogin::Enabled),
            (S::RequiresApproval, StartAtLogin::RequiresApproval),
            (S::NotFound, StartAtLogin::Disabled),
        ] {
            assert_eq!(from_sm_app_service_status(raw.0), state);
        }
    }

    // libtest runs these on a worker thread, so the main-thread check answers before any
    // SMAppService call: the tests never read or change the real login item.
    #[test]
    fn ac9_4_login_item_calls_off_main_thread_are_errors_not_calls() {
        assert_eq!(
            MacPlatform.start_at_login(),
            Err(PlatformError::NotMainThread)
        );
        assert_eq!(
            MacPlatform.set_start_at_login(true),
            Err(PlatformError::NotMainThread)
        );
    }

    #[test]
    fn takeover_is_full() {
        assert_eq!(MacPlatform.takeover_support(), Takeover::Full);
    }

    #[test]
    fn off_main_thread_is_an_error_not_a_crash() {
        // libtest runs this on a worker thread.
        assert_eq!(MacPlatform.screens(), Err(PlatformError::NotMainThread));
    }
}

//! Windows implementation. Not yet: L-012 adds `SetWindowPos(HWND_TOPMOST)` and `EnumDisplayMonitors`.

use crate::{Platform, PlatformError, Screen};
use raw_window_handle::HasWindowHandle;

/// Placeholder until L-012: reports no screens, and raising succeeds without doing anything.
pub struct WindowsPlatform;

impl Platform for WindowsPlatform {
    fn name(&self) -> &'static str {
        "windows (not yet implemented, L-012)"
    }

    fn screens(&self) -> Result<Vec<Screen>, PlatformError> {
        Ok(Vec::new())
    }

    fn raise_above_everything(&self, _window: &dyn HasWindowHandle) -> Result<(), PlatformError> {
        Ok(())
    }
}

//! Linux implementation. Not yet: L-012 adds X11 always-on-top, Wayland detection and the F4.5 fallback.

use crate::{Platform, PlatformError, Screen};
use raw_window_handle::HasWindowHandle;

/// Placeholder until L-012: reports no screens, and raising succeeds without doing anything.
pub struct LinuxPlatform;

impl Platform for LinuxPlatform {
    fn name(&self) -> &'static str {
        "linux (not yet implemented, L-012)"
    }

    fn screens(&self) -> Result<Vec<Screen>, PlatformError> {
        Ok(Vec::new())
    }

    fn raise_above_everything(&self, _window: &dyn HasWindowHandle) -> Result<(), PlatformError> {
        Ok(())
    }
}

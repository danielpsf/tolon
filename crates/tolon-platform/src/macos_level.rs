//! The numbers the macOS interrupt window needs, free of AppKit so they are testable on every
//! OS. `macos.rs` checks them against AppKit's own constants.

/// `NSScreenSaverWindowLevel`.
pub const SCREEN_SAVER_WINDOW_LEVEL: isize = 1000;
/// One above the screen saver, which is what sits above full-screen apps (AC4.1).
pub const INTERRUPT_WINDOW_LEVEL: isize = SCREEN_SAVER_WINDOW_LEVEL + 1;

/// `NSWindowCollectionBehaviorCanJoinAllSpaces`.
pub const CAN_JOIN_ALL_SPACES: usize = 1 << 0;
/// `NSWindowCollectionBehaviorStationary`.
pub const STATIONARY: usize = 1 << 4;
/// `NSWindowCollectionBehaviorFullScreenAuxiliary`.
pub const FULL_SCREEN_AUXILIARY: usize = 1 << 8;
/// `canJoinAllSpaces | fullScreenAuxiliary | stationary`.
pub const INTERRUPT_COLLECTION_BEHAVIOR: usize =
    CAN_JOIN_ALL_SPACES | FULL_SCREEN_AUXILIARY | STATIONARY;

/// Cocoa frames have their origin at the bottom-left of the primary screen; [`crate::Screen`]
/// wants the top-left corner. `primary_height` is the height of the primary screen.
pub fn top_left_y(frame_y: f64, frame_height: f64, primary_height: f64) -> f64 {
    primary_height - (frame_y + frame_height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_is_one_above_screen_saver() {
        assert_eq!(INTERRUPT_WINDOW_LEVEL, 1001);
    }

    #[test]
    fn behavior_joins_all_spaces_over_full_screen_and_stays_put() {
        assert_eq!(INTERRUPT_COLLECTION_BEHAVIOR, 0b1_0001_0001);
    }

    #[test]
    fn primary_screen_keeps_y_zero() {
        assert_eq!(top_left_y(0.0, 900.0, 900.0), 0.0);
    }

    #[test]
    fn screen_above_primary_gets_negative_y() {
        // Cocoa: a 1080 high screen above a 900 high primary has frame y = 900.
        assert_eq!(top_left_y(900.0, 1080.0, 900.0), -1080.0);
    }

    #[test]
    fn screen_below_primary_gets_positive_y() {
        assert_eq!(top_left_y(-1080.0, 1080.0, 900.0), 900.0);
    }
}

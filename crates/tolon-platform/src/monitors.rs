//! Monitor geometry as Windows reports it, turned into [`Screen`]s. Free of Win32 calls so the
//! maths is testable on every OS; `windows.rs` fills [`RawMonitor`] from the OS.

use crate::Screen;

/// The DPI Windows calls 100 percent scaling.
pub const BASE_DPI: u32 = 96;

/// One monitor as `GetMonitorInfoW` and `GetDpiForMonitor` report it, in physical pixels of the
/// virtual screen (the primary monitor's top-left corner is `(0, 0)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawMonitor {
    pub id: u32,
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    /// Effective DPI of the monitor; 0 when the OS gave none.
    pub dpi: u32,
    /// `MONITORINFOF_PRIMARY`.
    pub primary: bool,
}

/// Scale factor for a DPI: 96 is 1.0, 144 is 1.5, 192 is 2.0. A DPI of 0 counts as 1.0.
pub fn scale_from_dpi(dpi: u32) -> f64 {
    if dpi == 0 {
        1.0
    } else {
        f64::from(dpi) / f64::from(BASE_DPI)
    }
}

/// Physical pixels to logical points on a monitor with `scale`.
pub fn to_logical(physical: i64, scale: f64) -> f64 {
    physical as f64 / scale
}

/// `end - start` as a size; an inverted range gives 0.
fn extent(start: i32, end: i32) -> u32 {
    u32::try_from(i64::from(end) - i64::from(start)).unwrap_or(0)
}

/// Logical points to physical pixels, rounded; out-of-range values saturate.
pub fn to_physical_i32(logical: f64, scale: f64) -> i32 {
    (logical * scale).round() as i32
}

/// Like [`to_physical_i32`], for a size; negative values become 0.
pub fn to_physical_u32(logical: f64, scale: f64) -> u32 {
    (logical * scale).round() as u32
}

/// Convert monitors to [`Screen`]s, primary first, the others in the order given.
///
/// The physical fields are the native rectangle. Each monitor's logical fields are divided by
/// its own scale, so `x * scale_factor` is the physical origin. Windows has no shared logical
/// space: with monitors of different scales, the logical `x` and `y` of two monitors are not
/// comparable, which is why windows are placed with the physical fields. Exactly one screen
/// has `is_main`: the first monitor flagged primary, else the first monitor.
pub fn screens_from(monitors: &[RawMonitor]) -> Vec<Screen> {
    let main = monitors.iter().position(|m| m.primary).unwrap_or(0);
    let order = std::iter::once(main).chain((0..monitors.len()).filter(|&i| i != main));
    order
        .filter_map(|i| monitors.get(i).map(|m| (i == main, m)))
        .map(|(is_main, m)| {
            let scale = scale_from_dpi(m.dpi);
            Screen {
                id: m.id,
                x: to_logical(i64::from(m.left), scale),
                y: to_logical(i64::from(m.top), scale),
                width: to_logical(i64::from(m.right) - i64::from(m.left), scale),
                height: to_logical(i64::from(m.bottom) - i64::from(m.top), scale),
                physical_x: m.left,
                physical_y: m.top,
                physical_width: extent(m.left, m.right),
                physical_height: extent(m.top, m.bottom),
                scale_factor: scale,
                is_main,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(id: u32, l: i32, t: i32, r: i32, b: i32, dpi: u32, primary: bool) -> RawMonitor {
        RawMonitor {
            id,
            left: l,
            top: t,
            right: r,
            bottom: b,
            dpi,
            primary,
        }
    }

    #[test]
    fn dpi_converts_to_scale() {
        assert_eq!(scale_from_dpi(96), 1.0);
        assert_eq!(scale_from_dpi(120), 1.25);
        assert_eq!(scale_from_dpi(144), 1.5);
        assert_eq!(scale_from_dpi(192), 2.0);
    }

    #[test]
    fn missing_dpi_means_no_scaling() {
        assert_eq!(scale_from_dpi(0), 1.0);
    }

    #[test]
    fn physical_to_logical_divides_by_scale() {
        assert_eq!(to_logical(3840, 2.0), 1920.0);
        assert_eq!(to_logical(-1920, 1.5), -1280.0);
    }

    #[test]
    fn single_monitor_at_150_percent() {
        let s = screens_from(&[mon(7, 0, 0, 2880, 1800, 144, true)]);
        assert_eq!(
            s,
            vec![Screen {
                id: 7,
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1200.0,
                physical_x: 0,
                physical_y: 0,
                physical_width: 2880,
                physical_height: 1800,
                scale_factor: 1.5,
                is_main: true,
            }]
        );
    }

    #[test]
    fn s4_2_mixed_dpi_placement_by_physical_fields_lands_on_the_second_monitor() {
        // Primary at 100 percent; the second monitor sits to its right at 200 percent.
        let primary = mon(1, 0, 0, 1920, 1080, 96, true);
        let second = mon(2, 1920, 0, 5760, 2160, 192, false);
        let s = screens_from(&[primary, second]);
        assert_eq!(s[1].id, 2);
        assert_eq!((s[1].physical_x, s[1].physical_y), (1920, 0));
        assert_eq!((s[1].physical_width, s[1].physical_height), (3840, 2160));
        // The logical origin is not 1920: using it as a physical position would land on the
        // primary monitor.
        assert_eq!(s[1].x, 960.0);

        let inside = |scr: &Screen, px: i64, py: i64| {
            px >= i64::from(scr.physical_x)
                && px < i64::from(scr.physical_x) + i64::from(scr.physical_width)
                && py >= i64::from(scr.physical_y)
                && py < i64::from(scr.physical_y) + i64::from(scr.physical_height)
        };
        // A window opened at the second screen's physical origin is on the second screen only.
        let (wx, wy) = (i64::from(s[1].physical_x), i64::from(s[1].physical_y));
        assert!(inside(&s[1], wx, wy) && !inside(&s[0], wx, wy));
        // The logical origin read as physical pixels would be on the primary screen.
        assert!(inside(&s[0], s[1].x as i64, s[1].y as i64));
    }

    #[test]
    fn logical_to_physical_rounds_and_saturates() {
        assert_eq!(to_physical_i32(960.0, 2.0), 1920);
        assert_eq!(to_physical_i32(-0.5, 1.0), -1);
        assert_eq!(to_physical_i32(1e30, 2.0), i32::MAX);
        assert_eq!(to_physical_u32(1440.5, 2.0), 2881);
        assert_eq!(to_physical_u32(-5.0, 2.0), 0);
    }

    #[test]
    fn s4_2_two_monitors_primary_first_and_negative_origin() {
        // Windows lists the left monitor first, but the primary is on the right of it.
        let left = mon(1, -1920, -100, 0, 980, 96, false);
        let primary = mon(2, 0, 0, 3840, 2160, 192, true);
        let s = screens_from(&[left, primary]);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].id, 2);
        assert!(s[0].is_main);
        assert_eq!(
            (s[0].x, s[0].y, s[0].width, s[0].height),
            (0.0, 0.0, 1920.0, 1080.0)
        );
        assert_eq!(s[1].id, 1);
        assert!(!s[1].is_main);
        assert_eq!((s[1].x, s[1].y), (-1920.0, -100.0));
    }

    #[test]
    fn origin_times_scale_is_the_physical_origin() {
        let m = mon(3, 2560, -480, 5120, 960, 144, false);
        let s = screens_from(&[mon(1, 0, 0, 2560, 1440, 96, true), m]);
        assert_eq!(s[1].x * s[1].scale_factor, 2560.0);
        assert_eq!(s[1].y * s[1].scale_factor, -480.0);
    }

    #[test]
    fn exactly_one_main_even_without_a_primary_flag() {
        let s = screens_from(&[
            mon(1, 0, 0, 100, 100, 96, false),
            mon(2, 100, 0, 200, 100, 96, false),
        ]);
        assert_eq!(s.iter().filter(|s| s.is_main).count(), 1);
        assert_eq!(s[0].id, 1);
    }

    #[test]
    fn no_monitors_no_screens() {
        assert!(screens_from(&[]).is_empty());
    }

    #[test]
    fn extreme_rectangle_does_not_overflow() {
        let s = screens_from(&[mon(1, i32::MIN, i32::MIN, i32::MAX, i32::MAX, 96, true)]);
        assert_eq!(s[0].width, f64::from(u32::MAX));
        assert_eq!(s[0].physical_width, u32::MAX);
        assert_eq!(extent(10, 5), 0);
    }
}

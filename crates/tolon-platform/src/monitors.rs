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

/// Convert monitors to [`Screen`]s, primary first, the others in the order given.
///
/// Each monitor is divided by its own scale, so `x * scale_factor` is exactly the physical
/// origin Windows reports. Windows has no shared logical space: with monitors of different
/// scales, the logical `x` and `y` of two monitors are not comparable. Exactly one screen has
/// `is_main`: the first monitor flagged primary, else the first monitor.
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
                scale_factor: 1.5,
                is_main: true,
            }]
        );
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
    }
}

//! The tray icon and its panel (F7, mockup 02).
//!
//! - The icon is a template image on macOS (black and alpha, tinted by the menu bar) and the moss
//!   colour elsewhere; a paused variant shows two bars (AC7.3). Placeholder until the logo is
//!   chosen: `icons/tray/draw.py` draws all four.
//! - A click on the icon shows the panel under it (above it when the icon sits low on its screen,
//!   as on the Windows taskbar) or hides it; the panel hides when it loses focus. Linux trays
//!   (AppIndicator) report no clicks, only menus, so there the icon carries a one-item menu,
//!   "Tolon", which shows the panel at the cursor.
//! - The tooltip is the settings window's text (`tray_tooltip`): the page has the catalogues and
//!   the language, Rust does not (decision "Tolon is translated"). An empty text keeps "Tolon".
//! - The panel window is created on the first click, not at start, so nothing but the tray is
//!   there until the person asks (AC10.2).

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Rect, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tolon_platform::{Placement, Platform, Screen, NATIVE};

/// The panel's window label; `capabilities/panel.json` and the command checks use it.
pub const PANEL: &str = "panel";
/// The settings window.
pub const MAIN: &str = "main";
const TRAY_ID: &str = "tolon";
const MENU_PANEL: &str = "panel";
/// Mockup 02, in logical pixels.
const PANEL_SIZE: (f64, f64) = (420.0, 560.0);
/// Space between the icon (or the screen edge) and the panel, logical pixels.
const GAP: f64 = 8.0;
/// A click on the icon right after the panel hid for the blur that click caused toggles nothing.
const BLUR_GRACE: Duration = Duration::from_millis(300);
/// Windows keeps at most 127 UTF-16 units of a tray tooltip (`NOTIFYICONDATAW.szTip`).
const MAX_TOOLTIP_UNITS: usize = 127;

/// The macOS menu bar takes template images; other trays get the colour icon.
fn template() -> bool {
    NATIVE.tray_style().template_icon
}

fn icon(paused: bool) -> Image<'static> {
    match (template(), paused) {
        (true, false) => tauri::include_image!("icons/tray/template.png"),
        (true, true) => tauri::include_image!("icons/tray/template-paused.png"),
        (false, false) => tauri::include_image!("icons/tray/color.png"),
        (false, true) => tauri::include_image!("icons/tray/color-paused.png"),
    }
}

/// When the panel last hid because it lost focus.
#[derive(Default)]
pub struct PanelState {
    hidden_at: Mutex<Option<Instant>>,
}

/// Puts the icon in the tray. A tray that cannot be created (a Linux session without a status
/// notifier host) is logged by the caller; Tolon still runs and alarms still fire.
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    app.manage(PanelState::default());
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon(false))
        .icon_as_template(template())
        .tooltip("Tolon")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(on_icon_event);
    if !NATIVE.tray_style().clicks {
        // "Tolon" is the product name and never translated, so this menu needs no catalogue.
        let open = MenuItem::with_id(app, MENU_PANEL, "Tolon", true, None::<&str>)?;
        builder = builder
            .menu(&Menu::with_items(app, &[&open])?)
            .on_menu_event(|app, event| {
                if event.id().as_ref() == MENU_PANEL {
                    toggle_panel(app, None);
                }
            });
    }
    builder.build(app)?;
    Ok(())
}

fn on_icon_event(tray: &TrayIcon, event: TrayIconEvent) {
    let app = tray.app_handle();
    match event {
        TrayIconEvent::Click {
            rect,
            button_state: MouseButtonState::Up,
            ..
        } => toggle_panel(app, Some(rect)),
        // The tooltip is about to show: the settings window recomputes "in N min".
        TrayIconEvent::Enter { .. } => {
            if let Err(e) = app.emit_to(MAIN, "tray:hover", ()) {
                tracing::debug!(error = %e, "could not ask for the tray tooltip");
            }
        }
        _ => {}
    }
}

/// The paused icon, or the normal one (AC7.3).
pub fn set_paused(app: &AppHandle, paused: bool) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Err(e) = tray.set_icon_with_as_template(Some(icon(paused)), template()) {
            tracing::warn!(error = %e, "could not change the tray icon");
        }
    }
}

/// The tooltip as the page may send it. Line breaks stay (the pause goes on its own line); every
/// other control character becomes a space; Unicode format characters (zero-width, bidi overrides
/// such as U+202E) are dropped, except the first-strong isolate pair U+2068 and U+2069 the page
/// puts around the meeting title. At most 127 UTF-16 units, cut between characters, so a
/// surrogate pair is never split. Empty means "Tolon".
pub fn tooltip_text(text: &str) -> String {
    let mut clean = String::new();
    let mut units = 0;
    for c in text.chars() {
        let c = match c {
            '\n' | '\u{2068}' | '\u{2069}' => c,
            c if c.is_control() => ' ',
            c if crate::scheduler::is_format_char(c) => continue,
            c => c,
        };
        units += c.len_utf16();
        if units > MAX_TOOLTIP_UNITS {
            break;
        }
        clean.push(c);
    }
    let clean = clean.trim();
    if clean.is_empty() {
        "Tolon".to_owned()
    } else {
        clean.to_owned()
    }
}

pub fn set_tooltip(app: &AppHandle, text: &str) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Err(e) = tray.set_tooltip(Some(tooltip_text(text))) {
            tracing::warn!(error = %e, "could not set the tray tooltip");
        }
    }
}

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Px {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Where the panel's top-left corner goes, in physical pixels: centred on the icon, under it when
/// the icon is in the upper half of its screen (the macOS menu bar, a top panel), above it
/// otherwise (the Windows taskbar), and always `gap` inside the screen.
pub fn panel_origin(icon: Px, screen: Px, panel: (f64, f64), gap: f64) -> (f64, f64) {
    let (w, h) = panel;
    let x = icon.x + icon.w / 2.0 - w / 2.0;
    let below = icon.y + icon.h / 2.0 < screen.y + screen.h / 2.0;
    let y = if below {
        icon.y + icon.h + gap
    } else {
        icon.y - h - gap
    };
    let clamp = |v: f64, lo: f64, hi: f64| if hi < lo { lo } else { v.clamp(lo, hi) };
    (
        clamp(x, screen.x + gap, screen.x + screen.w - w - gap),
        clamp(y, screen.y + gap, screen.y + screen.h - h - gap),
    )
}

/// The panel window, created hidden the first time.
fn panel(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(window) = app.get_webview_window(PANEL) {
        return Ok(window);
    }
    WebviewWindowBuilder::new(app, PANEL, WebviewUrl::App("panel.html".into()))
        .title("Tolon")
        .inner_size(PANEL_SIZE.0, PANEL_SIZE.1)
        .decorations(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .visible(false)
        .build()
}

/// Shows the panel at the icon (`rect`), or at the cursor when the tray gave no rect, or hides it.
pub fn toggle_panel(app: &AppHandle, rect: Option<Rect>) {
    let window = match panel(app) {
        Ok(window) => window,
        Err(e) => {
            tracing::warn!(error = %e, "could not open the tray panel");
            return;
        }
    };
    if window.is_visible().unwrap_or(false) {
        hide_panel(app);
        return;
    }
    let just_hid = app.try_state::<PanelState>().is_some_and(|s| {
        s.hidden_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some_and(|at| at.elapsed() < BLUR_GRACE)
    });
    if just_hid {
        return;
    }
    place(app, &window, rect);
    for step in [window.show(), window.set_focus()] {
        if let Err(e) = step {
            tracing::warn!(error = %e, "could not show the tray panel");
        }
    }
}

/// Hides the panel; a blur calls this too.
pub fn hide_panel(app: &AppHandle) {
    if let Some(state) = app.try_state::<PanelState>() {
        *state
            .hidden_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
    }
    if let Some(window) = app.get_webview_window(PANEL) {
        if let Err(e) = window.hide() {
            tracing::warn!(error = %e, "could not hide the tray panel");
        }
    }
}

/// Where the panel goes, in the unit `Screen::placement` uses on this OS (`logical`: points on
/// macOS, else physical pixels). `pointer` is the click, in that unit; `icon` the tray icon's
/// rectangle as Tauri reports it, in physical pixels of the icon's own screen. The screen is the
/// one under the pointer (else the main one), and the icon is converted with that screen's scale,
/// never the panel window's: on a Retina screen beside a 1x one the two differ (decision
/// "Platform contract after L-012"). Without an icon the pointer stands in for it.
pub fn panel_placement(
    screens: &[Screen],
    pointer: Option<(f64, f64)>,
    icon: Option<Px>,
    logical: bool,
) -> Option<Placement> {
    let on = |s: &&Screen, (x, y): (f64, f64)| {
        if logical {
            s.contains_logical(x, y)
        } else {
            s.contains_physical(x, y)
        }
    };
    let screen = pointer
        .and_then(|p| screens.iter().find(|s| on(s, p)))
        .or_else(|| screens.iter().find(|s| s.is_main))
        .or_else(|| screens.first())?;
    let (to_unit, unit) = if logical {
        (screen.scale_factor.max(f64::MIN_POSITIVE), 1.0)
    } else {
        (1.0, screen.scale_factor)
    };
    let icon = match (icon, pointer) {
        (Some(i), _) => Px {
            x: i.x / to_unit,
            y: i.y / to_unit,
            w: i.w / to_unit,
            h: i.h / to_unit,
        },
        (None, Some((x, y))) => Px {
            x,
            y,
            w: 1.0,
            h: 1.0,
        },
        (None, None) => Px {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        },
    };
    let area = if logical {
        Px {
            x: screen.x,
            y: screen.y,
            w: screen.width,
            h: screen.height,
        }
    } else {
        Px {
            x: f64::from(screen.physical_x),
            y: f64::from(screen.physical_y),
            w: f64::from(screen.physical_width),
            h: f64::from(screen.physical_height),
        }
    };
    let (w, h) = (PANEL_SIZE.0 * unit, PANEL_SIZE.1 * unit);
    let (x, y) = panel_origin(icon, area, (w, h), GAP * unit);
    if logical {
        return Some(Placement::Logical {
            x,
            y,
            width: PANEL_SIZE.0,
            height: PANEL_SIZE.1,
        });
    }
    let px = |v: f64| v.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
    Some(Placement::Physical {
        x: px(x),
        y: px(y),
        width: px(w).unsigned_abs(),
        height: px(h).unsigned_abs(),
    })
}

/// Runs on the main thread (tray and menu events do), where `Platform` calls belong.
fn place(app: &AppHandle, window: &WebviewWindow, rect: Option<Rect>) {
    let screens =
        crate::interrupt::screens_or(NATIVE.screens(), || crate::interrupt::monitor_screens(app));
    let logical = screens
        .first()
        .is_some_and(|s| matches!(s.placement(), Placement::Logical { .. }));
    // The platform's pointer is in the placement unit; Tauri's cursor is physical, which is the
    // unit only where placement is physical (Linux, where the platform has no pointer).
    let pointer = NATIVE.pointer().ok().or_else(|| {
        app.cursor_position()
            .ok()
            .filter(|_| !logical)
            .map(|p| (p.x, p.y))
    });
    let icon = rect.map(|r| {
        let (p, s) = (
            r.position.to_physical::<f64>(1.0),
            r.size.to_physical::<f64>(1.0),
        );
        Px {
            x: p.x,
            y: p.y,
            w: s.width,
            h: s.height,
        }
    });
    let Some(placement) = panel_placement(&screens, pointer, icon, logical) else {
        return;
    };
    if let Err(e) = crate::interrupt::place(window, placement) {
        tracing::warn!(error = %e, "could not place the tray panel");
    }
}

/// Settings from the panel (AC7.2): the settings window shows and takes focus. With no Dock icon
/// on macOS the tray is the only way back to it (L-042).
pub fn show_settings(app: &AppHandle) {
    hide_panel(app);
    if let Some(window) = app.get_webview_window(MAIN) {
        for step in [window.unminimize(), window.show(), window.set_focus()] {
            if let Err(e) = step {
                tracing::warn!(error = %e, "could not show the settings window");
            }
        }
    }
}

/// What closing a window does (S7.2): the settings window only hides, so Tolon keeps running in
/// the tray; every other window closes.
pub fn hides_on_close(label: &str) -> bool {
    label == MAIN
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC_SCREEN: Px = Px {
        x: 0.0,
        y: 0.0,
        w: 2880.0,
        h: 1800.0,
    };

    #[test]
    fn ac7_2_the_panel_hangs_under_a_menu_bar_icon() {
        let icon = Px {
            x: 2000.0,
            y: 0.0,
            w: 44.0,
            h: 48.0,
        };
        let (x, y) = panel_origin(icon, MAC_SCREEN, (840.0, 1120.0), 16.0);
        assert_eq!((x, y), (2022.0 - 420.0, 64.0));
    }

    #[test]
    fn ac7_2_the_panel_stands_above_a_taskbar_icon_and_stays_on_screen() {
        let screen = Px {
            x: 0.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
        };
        let icon = Px {
            x: 1890.0,
            y: 1040.0,
            w: 24.0,
            h: 40.0,
        };
        let (x, y) = panel_origin(icon, screen, (420.0, 560.0), 8.0);
        assert_eq!(x, 1920.0 - 420.0 - 8.0, "clamped to the right edge");
        assert_eq!(y, 1040.0 - 560.0 - 8.0);
    }

    #[test]
    fn ac7_2_the_panel_follows_a_screen_left_of_the_primary() {
        // A second screen at negative x, icon on its top edge.
        let screen = Px {
            x: -1440.0,
            y: -1120.0,
            w: 1440.0,
            h: 2560.0,
        };
        let icon = Px {
            x: -1460.0,
            y: -1120.0,
            w: 30.0,
            h: 24.0,
        };
        let (x, y) = panel_origin(icon, screen, (420.0, 560.0), 8.0);
        assert_eq!(
            x,
            -1440.0 + 8.0,
            "clamped to the left edge of its own screen"
        );
        assert_eq!(y, -1120.0 + 24.0 + 8.0);
    }

    #[test]
    fn a_panel_larger_than_the_screen_starts_at_its_corner() {
        let tiny = Px {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 300.0,
        };
        let icon = Px {
            x: 150.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        assert_eq!(panel_origin(icon, tiny, (420.0, 560.0), 8.0), (8.0, 8.0));
    }

    #[test]
    fn ac7_1_the_tooltip_is_cleaned_and_cut() {
        assert_eq!(tooltip_text(""), "Tolon");
        assert_eq!(tooltip_text("  \n "), "Tolon");
        assert_eq!(tooltip_text("Standup\tin 5 min\r"), "Standup in 5 min");
        assert_eq!(
            tooltip_text("Paused until 16:00\nStandup in 5 min"),
            "Paused until 16:00\nStandup in 5 min"
        );
        assert_eq!(tooltip_text(&"x".repeat(500)).encode_utf16().count(), 127);
    }

    /// N2: format characters cannot reorder or hide the tooltip; the page's isolates stay; the
    /// 127-unit cut never splits a surrogate pair.
    #[test]
    fn ac7_1_the_tooltip_drops_format_characters_and_cuts_between_characters() {
        assert_eq!(
            tooltip_text("Stand\u{200B}up\u{202E} in 5 min"),
            "Standup in 5 min"
        );
        assert_eq!(
            tooltip_text("\u{2068}Standup\u{2069} in 5 min"),
            "\u{2068}Standup\u{2069} in 5 min"
        );
        // 126 units, then an emoji of two: it does not fit, so it is left out whole.
        let cut = tooltip_text(&format!("{}\u{1F600}", "x".repeat(126)));
        assert_eq!(cut.encode_utf16().count(), 126);
        assert!(!cut.contains('\u{1F600}'));
        let fits = tooltip_text(&format!("{}\u{1F600}", "x".repeat(125)));
        assert_eq!(fits.encode_utf16().count(), 127);
        assert!(fits.ends_with('\u{1F600}'));
    }

    fn screen(id: u32, (x, y, w, h): (f64, f64, f64, f64), scale: f64, main: bool) -> Screen {
        let px = |v: f64| (v * scale).round() as i32;
        Screen {
            id,
            x,
            y,
            width: w,
            height: h,
            physical_x: px(x),
            physical_y: px(y),
            physical_width: px(w).unsigned_abs(),
            physical_height: px(h).unsigned_abs(),
            scale_factor: scale,
            is_main: main,
        }
    }

    /// N3, AC7.2 on macOS (logical points): a Retina primary at 2x and a 1x screen to its right.
    /// Each icon is converted with its own screen's scale, so the panel lands under it on the
    /// right screen; the old code divided by the panel window's scale and put a 1x icon at half
    /// its x, on the Retina screen.
    #[test]
    fn ac7_2_the_panel_lands_under_the_icon_on_a_retina_plus_1x_setup() {
        let screens = [
            screen(1, (0.0, 0.0, 1440.0, 900.0), 2.0, true),
            screen(2, (1440.0, 0.0, 1920.0, 1080.0), 1.0, false),
        ];
        // Icon on the 1x screen at 2500 points: tray-icon reports 2500 px (scale 1).
        let icon = Px {
            x: 2500.0,
            y: 0.0,
            w: 22.0,
            h: 24.0,
        };
        let on_1x = panel_placement(&screens, Some((2505.0, 10.0)), Some(icon), true);
        assert_eq!(
            on_1x,
            Some(Placement::Logical {
                x: 2511.0 - 210.0,
                y: 32.0,
                width: 420.0,
                height: 560.0
            })
        );
        // Icon on the Retina screen at 900 points: tray-icon reports 1800 px (scale 2).
        let icon = Px {
            x: 1800.0,
            y: 0.0,
            w: 44.0,
            h: 48.0,
        };
        let on_retina = panel_placement(&screens, Some((905.0, 10.0)), Some(icon), true);
        assert_eq!(
            on_retina,
            Some(Placement::Logical {
                x: 911.0 - 210.0,
                y: 32.0,
                width: 420.0,
                height: 560.0
            })
        );
    }

    /// AC7.2 on Windows (physical pixels): a 150 % taskbar icon, the panel above it in pixels.
    #[test]
    fn ac7_2_the_panel_is_placed_in_physical_pixels_where_placement_is_physical() {
        let screens = [screen(1, (0.0, 0.0, 1280.0, 720.0), 1.5, true)];
        let icon = Px {
            x: 1800.0,
            y: 1035.0,
            w: 36.0,
            h: 45.0,
        };
        let placed = panel_placement(&screens, Some((1810.0, 1050.0)), Some(icon), false);
        assert_eq!(
            placed,
            Some(Placement::Physical {
                x: 1920 - 630 - 12,
                y: 1035 - 840 - 12,
                width: 630,
                height: 840
            })
        );
        // No pointer and no icon: the main screen's corner, never a crash.
        assert!(panel_placement(&screens, None, None, false).is_some());
        assert_eq!(panel_placement(&[], None, None, false), None);
    }

    #[test]
    fn s7_2_closing_the_settings_window_only_hides_it() {
        assert!(hides_on_close("main"));
        for label in ["panel", "interrupt-1", "Main", ""] {
            assert!(!hides_on_close(label), "{label:?}");
        }
    }
}

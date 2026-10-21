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
use tolon_platform::{Platform, Screen, NATIVE};

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
/// Windows keeps at most 127 characters of a tray tooltip.
const MAX_TOOLTIP_CHARS: usize = 120;

/// The macOS menu bar takes template images; other trays get the colour icon. shortcut: the OS is
/// read from the platform's name, as rule 9 keeps OS checks out of this crate; move it behind a
/// `Platform` call if more tray differences appear.
fn template() -> bool {
    NATIVE.name() == "macos"
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
    if NATIVE.name() == "linux" {
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

/// The tooltip as the page may send it: line breaks kept (the pause goes on its own line), every
/// other control character a space, at most 120 characters. Empty means "Tolon".
pub fn tooltip_text(text: &str) -> String {
    let clean: String = text
        .chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .take(MAX_TOOLTIP_CHARS)
        .collect();
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

fn px_of(rect: &Rect, scale: f64) -> Px {
    let p = rect.position.to_physical::<f64>(scale);
    let s = rect.size.to_physical::<f64>(scale);
    Px {
        x: p.x,
        y: p.y,
        w: s.width,
        h: s.height,
    }
}

/// The screen under `point` (physical), else the primary one: `(rect, scale factor)`.
fn screen_at(app: &AppHandle, x: f64, y: f64) -> Option<(Px, f64)> {
    let as_px = |m: &tauri::Monitor| {
        let (p, s) = (m.position(), m.size());
        Px {
            x: f64::from(p.x),
            y: f64::from(p.y),
            w: f64::from(s.width),
            h: f64::from(s.height),
        }
    };
    let monitors = app.available_monitors().unwrap_or_default();
    monitors
        .iter()
        .find(|m| {
            let r = as_px(m);
            r.x <= x && x < r.x + r.w && r.y <= y && y < r.y + r.h
        })
        .map(|m| (as_px(m), m.scale_factor()))
        .or_else(|| {
            app.primary_monitor()
                .ok()
                .flatten()
                .map(|m| (as_px(&m), m.scale_factor()))
        })
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

fn place(app: &AppHandle, window: &WebviewWindow, rect: Option<Rect>) {
    let scale_guess = window.scale_factor().unwrap_or(1.0);
    let icon = match rect {
        Some(rect) => px_of(&rect, scale_guess),
        None => match app.cursor_position() {
            Ok(p) => Px {
                x: p.x,
                y: p.y,
                w: 1.0,
                h: 1.0,
            },
            Err(_) => Px {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
        },
    };
    let Some((screen, scale)) = screen_at(app, icon.x + icon.w / 2.0, icon.y + icon.h / 2.0) else {
        return;
    };
    let (w, h) = (PANEL_SIZE.0 * scale, PANEL_SIZE.1 * scale);
    let (x, y) = panel_origin(icon, screen, (w, h), GAP * scale);
    // The panel's rectangle as a `Screen`, so `Screen::placement` picks the unit the OS places
    // windows in (decision "Platform contract after L-012": logical points on macOS).
    let px = |v: f64| v.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
    let rect = Screen {
        id: 0,
        x: x / scale,
        y: y / scale,
        width: PANEL_SIZE.0,
        height: PANEL_SIZE.1,
        physical_x: px(x),
        physical_y: px(y),
        physical_width: px(w).unsigned_abs(),
        physical_height: px(h).unsigned_abs(),
        scale_factor: scale,
        is_main: false,
    };
    if let Err(e) = crate::interrupt::place(window, rect.placement()) {
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
        assert_eq!(tooltip_text(&"x".repeat(500)).chars().count(), 120);
    }

    #[test]
    fn s7_2_closing_the_settings_window_only_hides_it() {
        assert!(hides_on_close("main"));
        for label in ["panel", "interrupt-1", "Main", ""] {
            assert!(!hides_on_close(label), "{label:?}");
        }
    }
}

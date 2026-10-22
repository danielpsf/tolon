//! Manual check for S4.1 (macOS only): the interrupt window shows over a full-screen app on every
//! display.
//!
//! Steps:
//! 1. Put an app in native full screen on one display: QuickTime (play any file, View, Enter Full
//!    Screen), Keynote (Play), or `raise -- fullscreen` below.
//! 2. From a terminal, start the control: `cargo run -p tolon-platform --example raise -- plain --regular`.
//!    Expected: nothing appears over the full-screen app. The windows stay hidden behind it.
//!    (Plain `plain` now shows over full screen too, because `prepare_app` makes Tolon an
//!    accessory app since L-042; only `--regular` reproduces the hidden case.)
//! 3. Run the real call: `cargo run -p tolon-platform --example raise -- raise`.
//!    Expected: a moss green window with the label "Tolon raise test" covers every display,
//!    including the one with the full-screen app.
//! 4. All windows close by themselves after 5 seconds. `--seconds N` changes that.
//!
//! `raise` and `plain` call `Platform::prepare_app` first, as Tolon does at startup (L-042).
//! `--regular` skips it and keeps the Regular activation policy: that reproduces L-042, where the
//! window on the full-screen display stays off screen.
//!
//! `fullscreen` opens a paper-coloured stand-in full-screen window, for when no QuickTime or
//! Keynote is at hand. It activates itself, so macOS may switch to its Space.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macOS only");
}

#[cfg(target_os = "macos")]
fn main() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSScreen,
        NSTextAlignment, NSTextField, NSWindow, NSWindowStyleMask,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
    use raw_window_handle::{
        AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle,
    };
    use std::ffi::c_void;
    use std::ptr::NonNull;
    use tolon_platform::{MacPlatform, Platform};

    struct View(NonNull<c_void>);
    impl HasWindowHandle for View {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let raw = RawWindowHandle::AppKit(AppKitWindowHandle::new(self.0));
            // SAFETY: the view is owned by a window that lives until the process exits.
            Ok(unsafe { WindowHandle::borrow_raw(raw) })
        }
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or_default();
    if !matches!(mode, "raise" | "plain" | "fullscreen") {
        eprintln!("usage: raise <raise|plain|fullscreen> [--seconds N] [--regular]");
        return;
    }
    let seconds = args
        .iter()
        .position(|a| a == "--seconds")
        .and_then(|i| args.get(i + 1))
        .and_then(|n| n.parse::<u64>().ok())
        .unwrap_or(5);
    let regular = mode == "fullscreen" || args.iter().any(|a| a == "--regular");

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    if regular {
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    } else if let Err(e) = MacPlatform.prepare_app() {
        eprintln!("prepare_app failed: {e}");
    }

    let frames: Vec<NSRect> = if mode == "fullscreen" {
        let rect = NSRect::new(NSPoint::new(100.0, 100.0), NSSize::new(600.0, 400.0));
        vec![rect]
    } else {
        NSScreen::screens(mtm).iter().map(|s| s.frame()).collect()
    };
    let style = if mode == "fullscreen" {
        NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Resizable
    } else {
        NSWindowStyleMask::Borderless
    };
    // Swiss Moss: paper #FBF8F0 for the stand-in, moss #4A7A52 for the raised windows.
    let (r, g, b) = if mode == "fullscreen" {
        (0xFB, 0xF8, 0xF0)
    } else {
        (0x4A, 0x7A, 0x52)
    };
    let channel = |v: u8| f64::from(v) / 255.0;
    let color = NSColor::colorWithSRGBRed_green_blue_alpha(channel(r), channel(g), channel(b), 1.0);

    let mut windows = Vec::new();
    for rect in frames {
        // SAFETY: standard AppKit window construction on the main thread.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                rect,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: `windows` keeps the window alive until the process exits.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setBackgroundColor(Some(&color));
        if mode != "fullscreen" {
            let label = NSTextField::labelWithString(
                &NSString::from_str(&format!("Tolon raise test, closes in {seconds} s")),
                mtm,
            );
            label.setAlignment(NSTextAlignment::Center);
            label.setTextColor(Some(&NSColor::whiteColor()));
            label.setFrame(NSRect::new(
                NSPoint::new(0.0, rect.size.height / 2.0 - 12.0),
                NSSize::new(rect.size.width, 24.0),
            ));
            if let Some(content) = window.contentView() {
                content.addSubview(&label);
            }
        }
        // What Tauri's `show()` does before the app calls `raise_above_everything`.
        window.makeKeyAndOrderFront(None);
        windows.push(window);
    }

    for window in &windows {
        match mode {
            "raise" => {
                let Some(content) = window.contentView() else {
                    eprintln!("no content view");
                    return;
                };
                let view = View(NonNull::from(&*content).cast());
                match MacPlatform.raise_above_everything(&view) {
                    Ok(raised) => println!(
                        "raised: window={} level={} behavior={:#x} focus={:?}",
                        window.windowNumber(),
                        window.level(),
                        window.collectionBehavior().0,
                        raised.focus
                    ),
                    Err(e) => eprintln!("raise failed: {e}"),
                }
            }
            "plain" => println!(
                "plain: window={} level={} (not raised)",
                window.windowNumber(),
                window.level()
            ),
            _ => {
                // A full-screen request needs an active app.
                #[allow(deprecated)]
                app.activateIgnoringOtherApps(true);
                window.toggleFullScreen(None);
                println!("fullscreen window requested");
            }
        }
    }

    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(seconds));
        std::process::exit(0);
    });
    app.run();
}

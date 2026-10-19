//! Manual check for S4.1 (macOS only): the interrupt window shows over a full-screen app.
//!
//! Steps:
//! 1. Put QuickTime (play any file, View, Enter Full Screen) or Keynote (Play) in full screen.
//! 2. From a terminal, start the control: `cargo run -p tolon-platform --example raise -- plain`.
//!    Expected: nothing appears over the full-screen app. The window stays hidden behind it.
//! 3. Run the real call: `cargo run -p tolon-platform --example raise -- raise`.
//!    Expected: a moss green full-screen window with the label "Tolon raise test" appears over the
//!    full-screen app, on that screen.
//! 4. Both windows close by themselves after 5 seconds. `--seconds N` changes that.
//!
//! `fullscreen` opens a paper-coloured stand-in full-screen window, for when no QuickTime or
//! Keynote is at hand. It activates itself, so macOS may switch to its Space.
//!
//! The example runs as an accessory app (no Dock icon), like the tray app, so `plain` and `raise`
//! do not switch Spaces or pull the full-screen app out of full screen.

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
        eprintln!("usage: raise <raise|plain|fullscreen> [--seconds N]");
        return;
    }
    let seconds = args
        .iter()
        .position(|a| a == "--seconds")
        .and_then(|i| args.get(i + 1))
        .and_then(|n| n.parse::<u64>().ok())
        .unwrap_or(5);

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let policy = if mode == "fullscreen" {
        NSApplicationActivationPolicy::Regular
    } else {
        NSApplicationActivationPolicy::Accessory
    };
    app.setActivationPolicy(policy);

    let screen_frame = NSScreen::screens(mtm).iter().next().map_or(
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(800.0, 600.0)),
        |s| s.frame(),
    );
    let (style, rect) = if mode == "fullscreen" {
        let titled =
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Resizable;
        (
            titled,
            NSRect::new(NSPoint::new(100.0, 100.0), NSSize::new(600.0, 400.0)),
        )
    } else {
        (NSWindowStyleMask::Borderless, screen_frame)
    };
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
    // SAFETY: the window is kept alive by this scope until the process exits.
    unsafe { window.setReleasedWhenClosed(false) };

    // Swiss Moss: paper #FBF8F0 for the stand-in, moss #4A7A52 for the raised window.
    let (r, g, b) = if mode == "fullscreen" {
        (0xFB, 0xF8, 0xF0)
    } else {
        (0x4A, 0x7A, 0x52)
    };
    let channel = |v: u8| f64::from(v) / 255.0;
    let color = NSColor::colorWithSRGBRed_green_blue_alpha(channel(r), channel(g), channel(b), 1.0);
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

    window.makeKeyAndOrderFront(None);

    match mode {
        "raise" => {
            let Some(content) = window.contentView() else {
                eprintln!("no content view");
                return;
            };
            let view = View(NonNull::from(&*content).cast());
            match MacPlatform.raise_above_everything(&view) {
                Ok(raised) => println!(
                    "raised: level={} behavior={:#x} focused={}",
                    window.level(),
                    window.collectionBehavior().0,
                    raised.focused
                ),
                Err(e) => eprintln!("raise failed: {e}"),
            }
        }
        "plain" => println!("plain: level={} (not raised)", window.level()),
        _ => {
            // A full-screen request needs an active app.
            #[allow(deprecated)]
            app.activateIgnoringOtherApps(true);
            window.toggleFullScreen(None);
            println!("fullscreen window requested");
        }
    }

    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(seconds));
        std::process::exit(0);
    });
    app.run();
}

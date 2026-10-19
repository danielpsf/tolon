//! Manual check for S4.1 (macOS only). Two modes, run in two terminals:
//!   cargo run -p tolon-platform --example raise -- fullscreen   # stand-in full-screen app
//!   cargo run -p tolon-platform --example raise -- raise        # borderless window, raised
//! Both exit by themselves after 25 s. Look at (or screenshot) the screen showing the full-screen
//! window: the red "raise" window must be on top of it.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macOS only");
}

#[cfg(target_os = "macos")]
fn main() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSScreen,
        NSWindow, NSWindowStyleMask,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};
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

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let mode = std::env::args().nth(1).unwrap_or_default();
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    let screen_frame = NSScreen::screens(mtm).iter().next().map_or(
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(800.0, 600.0)),
        |s| s.frame(),
    );

    let (style, rect) = if mode == "raise" {
        (NSWindowStyleMask::Borderless, screen_frame)
    } else {
        let titled =
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Resizable;
        (
            titled,
            NSRect::new(NSPoint::new(100.0, 100.0), NSSize::new(600.0, 400.0)),
        )
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
    let color = if mode == "raise" {
        NSColor::redColor()
    } else {
        NSColor::blueColor()
    };
    window.setBackgroundColor(Some(&color));
    window.makeKeyAndOrderFront(None);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);

    if mode == "raise" {
        let Some(content) = window.contentView() else {
            eprintln!("no content view");
            return;
        };
        let view = View(NonNull::from(&*content).cast());
        match MacPlatform.raise_above_everything(&view) {
            Ok(()) => println!(
                "raised: level={} behavior={:#x}",
                window.level(),
                window.collectionBehavior().0
            ),
            Err(e) => eprintln!("raise failed: {e}"),
        }
    } else {
        window.toggleFullScreen(None);
        println!("fullscreen window requested");
    }

    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(25));
        std::process::exit(0);
    });
    app.run();
}

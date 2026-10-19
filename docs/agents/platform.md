# Platform agent (crates/tolon-platform)

The only crate with `#[cfg(target_os)]` code. Implement the `Platform` trait three times plus a stub for CI.

- macOS: raise the interrupt window above everything with `objc2-app-kit`: `NSWindow.setLevel(NSScreenSaverWindowLevel + 1)`, `setCollectionBehavior(canJoinAllSpaces | fullScreenAuxiliary | stationary)`, `makeKeyAndOrderFront`. Screens from `NSScreen.screens`. Sound through `rodio`.
- Windows: `SetWindowPos(HWND_TOPMOST)` after Tauri creates the window; screens from `EnumDisplayMonitors`.
- Linux: X11 via Tauri's always-on-top and fullscreen; detect Wayland (`XDG_SESSION_TYPE` or `WAYLAND_DISPLAY`); if the interrupt window does not receive focus within 500 ms, report `Degraded::NoTakeover` so the app runs the F4.5 fallback through `tauri-plugin-notification` with actions.
- Never let a platform failure crash the app: every call returns `Result`, and the fallback is always available.
- Document each OS's limits in `docs/install.md` under "Known limits".

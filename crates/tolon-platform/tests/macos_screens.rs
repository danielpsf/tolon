//! Lists the real screens: at least one, exactly one main. No harness, because AppKit needs the
//! main thread and libtest runs tests on workers.

#[cfg(target_os = "macos")]
fn main() {
    use tolon_platform::{MacPlatform, Platform};
    let screens = match MacPlatform.screens() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("FAIL screens(): {e}");
            std::process::exit(1);
        }
    };
    let mains = screens.iter().filter(|s| s.is_main).count();
    if screens.is_empty() || mains != 1 {
        eprintln!("FAIL: {} screens, {mains} main: {screens:?}", screens.len());
        std::process::exit(1);
    }
    if let Some(s) = screens.iter().find(|s| {
        s.physical_width == 0
            || s.physical_height == 0
            || f64::from(s.physical_width) != (s.width * s.scale_factor).round()
    }) {
        eprintln!("FAIL: physical size does not match logical size times scale: {s:?}");
        std::process::exit(1);
    }
    println!("ok: {} screen(s), one main: {screens:?}", screens.len());
}

#[cfg(not(target_os = "macos"))]
fn main() {}

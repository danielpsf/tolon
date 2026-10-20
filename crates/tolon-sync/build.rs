//! Second lock for the e2e-loopback feature: refuse a release build that enables it.
//! The `compile_error!` in `src/ics.rs` keys on `debug_assertions`, which
//! `CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true` can switch on; `PROFILE` cannot be faked that way.

fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_E2E_LOOPBACK");
    println!("cargo:rerun-if-env-changed=PROFILE");
    let feature = std::env::var_os("CARGO_FEATURE_E2E_LOOPBACK").is_some();
    let release = std::env::var("PROFILE").is_ok_and(|p| p == "release");
    if feature && release {
        eprintln!("the e2e-loopback feature must never be built with the release profile");
        std::process::exit(1);
    }
}

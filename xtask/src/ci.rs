//! `cargo xtask ci`: the gate. Runs each step in order and stops at the first failure.

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

use crate::{layers, release_features, scenario_ids};

const CLIPPY_ARGS: &[&str] = &[
    "clippy",
    "--workspace",
    "--all-targets",
    "--",
    "-D",
    "warnings",
    "-D",
    "clippy::unwrap_used",
    "-D",
    "clippy::expect_used",
    "-D",
    "clippy::panic",
];

fn run_step(root: &Path, name: &str, program: &str, args: &[&str]) -> Result<()> {
    println!("\n==> {name}: {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .current_dir(root)
        .status()
        .with_context(|| format!("ci step `{name}`: cannot start `{program}`"))?;
    if status.success() {
        Ok(())
    } else {
        bail!("ci step `{name}` failed ({status})")
    }
}

fn run_internal(name: &str, step: impl FnOnce() -> Result<()>) -> Result<()> {
    println!("\n==> {name}");
    step().with_context(|| format!("ci step `{name}` failed"))
}

pub fn run(root: &Path) -> Result<()> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    run_step(root, "fmt", &cargo, &["fmt", "--all", "--check"])?;
    run_step(root, "clippy", &cargo, CLIPPY_ARGS)?;
    run_step(root, "test", &cargo, &["test", "--workspace"])?;
    // Ignored tests hold all of Tauri's runtime workers for seconds (the quit revoke with a full
    // runtime, L-057); alone in their step they stall no other test.
    run_step(
        root,
        "test ignored (alone)",
        &cargo,
        &["test", "-p", "tolon-app", "--", "--ignored"],
    )?;
    // The e2e-only seam is off in the steps above; lint and test it too (debug profile only).
    let mut seam_lint = vec!["clippy", "-p", "tolon-app", "--features", "e2e-loopback"];
    seam_lint.extend(&CLIPPY_ARGS[2..]);
    run_step(root, "clippy e2e-loopback", &cargo, &seam_lint)?;
    run_step(
        root,
        "test e2e-loopback",
        &cargo,
        &["test", "-p", "tolon-sync", "--features", "e2e-loopback"],
    )?;
    run_internal("check-release-features", || release_features::run(root))?;
    run_internal("layers", || layers::run(root))?;
    run_internal("check-scenario-ids", || scenario_ids::run(root))?;

    // ui/ lands with L-002; once package.json exists, typecheck, lint and test are mandatory.
    // vitest strips types, so only tsc catches a catalogue that misses a key.
    if root.join("ui/package.json").is_file() {
        let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
        run_step(
            root,
            "ui typecheck",
            npm,
            &["--prefix", "ui", "run", "typecheck"],
        )?;
        run_step(root, "ui lint", npm, &["--prefix", "ui", "run", "lint"])?;
        run_step(root, "ui test", npm, &["--prefix", "ui", "run", "test"])?;
    } else {
        println!("\n==> ui typecheck, ui lint, ui test");
        println!("!!!! skipped: ui/ not present (ui/package.json missing); ui typecheck, lint and test did NOT run !!!!");
    }
    println!("\nci: all steps passed");
    Ok(())
}

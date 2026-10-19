//! Repo automation: `cargo xtask <command>`.

mod ci;
mod fixtures;
mod fsutil;
mod ids;
mod layers;
mod scenario_ids;

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <command>\n\n\
commands:\n  ci                   fmt, clippy, tests, layers, scenario ids, ui lint and tests\n  \
layers               check crate dependency direction and target_os confinement\n  \
check-scenario-ids   check scenario and AC ids against docs/plan.md\n  \
fixtures             rewrite fixtures/ics/*.expected.json (review the diff)\n  \
bump                 not implemented yet (L-019)";

fn repo_root() -> Result<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .map(PathBuf::from)
        .context("xtask manifest has no parent directory")
}

fn run(command: &str) -> Result<ExitCode> {
    let root = repo_root()?;
    match command {
        "ci" => ci::run(&root)?,
        "layers" => layers::run(&root)?,
        "check-scenario-ids" => scenario_ids::run(&root)?,
        "fixtures" => fixtures::run(&root)?,
        "bump" => {
            eprintln!("bump: not implemented yet (L-019)");
            return Ok(ExitCode::FAILURE);
        }
        other => {
            eprintln!("unknown command: {other}\n{USAGE}");
            return Ok(ExitCode::from(2));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let Some(command) = std::env::args().nth(1) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match run(&command) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("xtask {command} failed: {err:#}");
            ExitCode::FAILURE
        }
    }
}

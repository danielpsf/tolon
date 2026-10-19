//! `cargo xtask fixtures`: rewrite `fixtures/ics/*.expected.json` from the fixed clock and zone
//! in `crates/tolon-core/tests/common`.
//!
//! The snapshots come from the S2.1 test itself (with TOLON_UPDATE_FIXTURES set), not from a
//! tolon-core dependency, because `cargo xtask layers` keeps xtask free of workspace crates.
//! The test and the generator therefore cannot disagree.

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

pub fn run(root: &Path) -> Result<()> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let status = Command::new(&cargo)
        .args([
            "test",
            "-p",
            "tolon-core",
            "--test",
            "fixtures",
            "s2_1_fixtures_match_their_snapshots",
        ])
        .env("TOLON_UPDATE_FIXTURES", "1")
        .current_dir(root)
        .status()
        .with_context(|| format!("cannot start `{cargo} test`"))?;
    if !status.success() {
        bail!("fixture generation failed ({status})");
    }
    println!("fixtures: snapshots rewritten under fixtures/ics; read `git diff fixtures/` before committing");
    Ok(())
}

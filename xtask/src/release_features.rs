//! `cargo xtask check-release-features`: the `e2e-loopback` test seam (plain http to loopback hosts)
//! must stay out of every release path. Every git-tracked file is scanned, so a new script, composite
//! action or workflow cannot switch the feature on unnoticed. Two other locks sit in tolon-sync: a
//! `compile_error!` without `debug_assertions` and a build script that refuses `PROFILE=release`.
//!
//! Where the name may appear:
//! - the two feature definitions in the manifests, as exact lines;
//! - the cfg gate line of the alarm command for the WebDriver suite, as an exact line, in the two
//!   tolon-app files that gate on it (`CFG_GATES`);
//! - the e2e build line of `ci.yml` (`tauri build --debug ... --features e2e-loopback`), in a job that
//!   uploads no artifact;
//! - the Rust sources that gate on it or lint and test it (`EXEMPT`) and Markdown docs.
//!
//! `all-features` may appear nowhere, Markdown docs and this file apart.

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

const FEATURE: &str = "e2e-loopback";
const ALL_FEATURES: &str = "all-features";
const CI_WORKFLOW: &str = ".github/workflows/ci.yml";

/// The only lines in a manifest that may name the feature.
const DEFINITIONS: &[(&str, &str)] = &[
    ("crates/tolon-sync/Cargo.toml", "e2e-loopback = []"),
    (
        "crates/tolon-app/Cargo.toml",
        "e2e-loopback = [\"tolon-sync/e2e-loopback\"]",
    ),
];

/// The only line that may name the feature in the two tolon-app sources that gate the WebDriver
/// suite's read-only alarm command (its definition, and its registration in the handler list).
/// Exact lines, like `DEFINITIONS`: any other mention in those files is a violation.
const CFG_GATES: &[(&str, &str)] = &[
    (
        "crates/tolon-app/src/commands.rs",
        "#[cfg(feature = \"e2e-loopback\")]",
    ),
    (
        "crates/tolon-app/src/main.rs",
        "#[cfg(feature = \"e2e-loopback\")]",
    ),
];

/// Sources that name the feature by design: the cfg gates, the build script lock, the gate steps.
const EXEMPT: &[&str] = &[
    "crates/tolon-sync/src/ics.rs",
    "crates/tolon-sync/build.rs",
    "xtask/src/ci.rs",
];

fn is_markdown(rel: &str) -> bool {
    rel.ends_with(".md")
}

/// The e2e build step in `ci.yml`: a debug build that passes the feature.
fn is_e2e_build_line(line: &str) -> bool {
    line.starts_with("run:")
        && line.contains("tauri build")
        && line.contains("--debug")
        && !line.contains("--release")
        && line.contains("--features e2e-loopback")
}

/// Violations in one file. `rel` is the path from the repo root with `/` separators.
fn violations(rel: &str, text: &str) -> Vec<String> {
    if is_markdown(rel) || rel == "xtask/src/release_features.rs" {
        return Vec::new();
    }
    let mut found = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        let n = i + 1;
        if line.contains(ALL_FEATURES) {
            found.push(format!(
                "{rel}:{n}: `{ALL_FEATURES}` is not allowed in tracked files"
            ));
        }
        if !line.contains(FEATURE) || EXEMPT.contains(&rel) {
            continue;
        }
        let allowed = if rel == CI_WORKFLOW {
            is_e2e_build_line(line)
        } else {
            DEFINITIONS
                .iter()
                .chain(CFG_GATES)
                .any(|(p, l)| *p == rel && *l == line)
        };
        if !allowed {
            found.push(format!("{rel}:{n}: `{FEATURE}` outside its allowed places"));
        }
    }
    if rel == CI_WORKFLOW {
        found.extend(artifact_violations(rel, text));
    }
    found
}

/// A job that builds with the feature must not upload an artifact: the build could leave the repo.
fn artifact_violations(rel: &str, text: &str) -> Vec<String> {
    let mut jobs: Vec<(&str, Vec<&str>)> = Vec::new();
    let mut in_jobs = false;
    for line in text.lines() {
        if line.trim_end() == "jobs:" {
            in_jobs = true;
        } else if in_jobs
            && line.starts_with("  ")
            && !line.starts_with("   ")
            && line.trim_end().ends_with(':')
        {
            jobs.push((line.trim().trim_end_matches(':'), Vec::new()));
        } else if let Some((_, body)) = jobs.last_mut() {
            body.push(line);
        }
    }
    jobs.iter()
        .filter(|(_, body)| {
            body.iter().any(|l| l.contains(FEATURE))
                && body.iter().any(|l| l.contains("upload-artifact"))
        })
        .map(|(name, _)| {
            format!("{rel}: job `{name}` builds with `{FEATURE}` and uploads an artifact")
        })
        .collect()
}

/// Every tracked file, as paths from the repo root.
fn tracked_files(root: &Path) -> Result<Vec<String>> {
    let out = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .context("cannot run `git ls-files`")?;
    if !out.status.success() {
        bail!(
            "`git ls-files` failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect())
}

pub fn run(root: &Path) -> Result<()> {
    let files = tracked_files(root)?;
    let mut found = Vec::new();
    for rel in &files {
        let path = root.join(rel);
        // A tracked file deleted in the working tree has nothing to scan; binaries are read lossily.
        if let Ok(bytes) = std::fs::read(&path) {
            found.extend(violations(rel, &String::from_utf8_lossy(&bytes)));
        }
    }
    if found.is_empty() {
        println!(
            "release-features: {} tracked files checked; {FEATURE} appears only in its two definitions, the e2e build line of ci.yml and its gating sources",
            files.len()
        );
        Ok(())
    } else {
        bail!("{}", found.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CI_E2E: &str = "jobs:\n  e2e:\n    steps:\n      - name: Build debug app\n        run: npm --prefix ui run tauri build -- --debug --no-bundle --features e2e-loopback\n";

    #[test]
    fn release_workflow_may_not_pass_the_feature() {
        let yml = "      - run: npm run tauri build -- --features e2e-loopback\n";
        assert_eq!(violations(".github/workflows/release.yml", yml).len(), 1);
    }

    #[test]
    fn a_comment_in_a_workflow_still_counts() {
        assert_eq!(
            violations(".github/workflows/release.yml", "      # e2e-loopback\n").len(),
            1
        );
    }

    #[test]
    fn a_root_script_may_not_use_the_feature() {
        let sh = "#!/bin/sh\ncargo build --release -p tolon-app --features e2e-loopback\n";
        assert_eq!(violations("build.sh", sh).len(), 1);
    }

    #[test]
    fn a_composite_action_may_not_use_the_feature() {
        let action = "runs:\n  using: composite\n  steps:\n    - run: cargo build --features e2e-loopback\n      shell: bash\n";
        assert_eq!(
            violations(".github/actions/build/action.yml", action).len(),
            1
        );
    }

    #[test]
    fn all_features_fails_in_any_tracked_file() {
        let json = "\"build\": \"cargo build --all-features\",\n";
        assert_eq!(violations("ui/package.json", json).len(), 1);
        assert_eq!(
            violations(".github/workflows/release.yml", "args: --all-features\n").len(),
            1
        );
        assert_eq!(
            violations("scripts/x.sh", "cargo test --all-features\n").len(),
            1
        );
    }

    #[test]
    fn ci_may_use_the_feature_only_on_the_e2e_build_line() {
        assert!(violations(CI_WORKFLOW, CI_E2E).is_empty());
        let release_build = "jobs:\n  e2e:\n    steps:\n      - name: Build\n        run: npm run tauri build -- --features e2e-loopback\n";
        assert_eq!(
            violations(CI_WORKFLOW, release_build).len(),
            1,
            "no --debug"
        );
        let other = "jobs:\n  gate:\n    steps:\n      - name: Test\n        run: cargo test --features e2e-loopback\n";
        assert_eq!(violations(CI_WORKFLOW, other).len(), 1);
        let comment = "      # e2e-loopback\n";
        assert_eq!(violations(CI_WORKFLOW, comment).len(), 1);
        let all = "      - run: cargo test --all-features\n";
        assert_eq!(violations(CI_WORKFLOW, all).len(), 1);
    }

    #[test]
    fn ci_may_not_upload_an_artifact_from_a_job_built_with_the_feature() {
        let upload = format!(
            "{CI_E2E}      - uses: actions/upload-artifact@v4\n        with:\n          path: target/debug/tolon-app\n"
        );
        let found = violations(CI_WORKFLOW, &upload);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("uploads an artifact"));
        // An upload in another job is fine.
        let other_job =
            format!("{CI_E2E}  gate:\n    steps:\n      - uses: actions/upload-artifact@v4\n");
        assert!(violations(CI_WORKFLOW, &other_job).is_empty());
    }

    #[test]
    fn the_two_definitions_pass_and_other_manifest_lines_fail() {
        assert!(violations("crates/tolon-sync/Cargo.toml", "e2e-loopback = []\n").is_empty());
        assert!(violations(
            "crates/tolon-app/Cargo.toml",
            "e2e-loopback = [\"tolon-sync/e2e-loopback\"]\n"
        )
        .is_empty());
        let default = "default = [\"e2e-loopback\"]\n";
        assert_eq!(violations("crates/tolon-app/Cargo.toml", default).len(), 1);
        let dep = "tolon-sync = { path = \"../tolon-sync\", features = [\"e2e-loopback\"] }\n";
        assert_eq!(violations("crates/tolon-app/Cargo.toml", dep).len(), 1);
        // A definition is only allowed in its own manifest.
        assert_eq!(
            violations("crates/tolon-store/Cargo.toml", "e2e-loopback = []\n").len(),
            1
        );
    }

    #[test]
    fn gating_sources_and_markdown_may_name_it() {
        let cfg = "#[cfg(feature = \"e2e-loopback\")]\n";
        assert!(violations("crates/tolon-sync/src/ics.rs", cfg).is_empty());
        assert_eq!(violations("crates/tolon-app/src/pipeline.rs", cfg).len(), 1);
        assert!(violations(
            "docs/decisions.md",
            "the e2e-loopback feature, never --all-features\n"
        )
        .is_empty());
    }

    #[test]
    fn the_app_gate_lines_pass_and_nothing_else_in_those_files_does() {
        let gate = "    #[cfg(feature = \"e2e-loopback\")]\n";
        // The two files that gate the alarm command may carry the exact gate line ...
        for file in [
            "crates/tolon-app/src/commands.rs",
            "crates/tolon-app/src/main.rs",
        ] {
            assert!(violations(file, gate).is_empty(), "{file}");
            assert!(violations(file, &gate.repeat(3)).is_empty(), "{file}");
        }
        // ... and are not exempt: every other mention fails, in code or in a comment.
        for file in [
            "crates/tolon-app/src/commands.rs",
            "crates/tolon-app/src/main.rs",
        ] {
            for other in [
                "#[cfg(all(feature = \"e2e-loopback\", test))]\n",
                "#[cfg(not(feature = \"e2e-loopback\"))]\n",
                "let on = cfg!(feature = \"e2e-loopback\");\n",
                "// exists only with the e2e-loopback feature\n",
                "#[cfg(feature = \"e2e-loopback\")] fn x() {}\n",
            ] {
                assert_eq!(violations(file, other).len(), 1, "{file}: {other}");
            }
            // A gate line next to a bad one: only the bad one is reported.
            let mixed = format!("{gate}let on = cfg!(feature = \"e2e-loopback\");\n");
            assert_eq!(violations(file, &mixed).len(), 1, "{file}");
        }
        // The gate line is allowed in those two files only.
        for file in [
            "crates/tolon-app/src/interrupt.rs",
            "crates/tolon-app/src/scheduler.rs",
            "crates/tolon-store/src/lib.rs",
        ] {
            assert_eq!(violations(file, gate).len(), 1, "{file}");
        }
    }

    #[test]
    fn the_repo_passes() -> Result<()> {
        run(&crate::repo_root()?)
    }
}

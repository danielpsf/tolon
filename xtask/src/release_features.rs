//! `cargo xtask check-release-features`: the `e2e-loopback` test seam (plain http to loopback hosts)
//! must stay out of every release path. Every git-tracked file is scanned, so a new script, composite
//! action or workflow cannot switch the feature on unnoticed. Two other locks sit in tolon-sync: a
//! `compile_error!` without `debug_assertions` and a build script that refuses `PROFILE=release`.
//!
//! Where the name may appear:
//! - the two feature definitions in the manifests, as exact lines;
//! - the cfg gate line of the WebDriver suite's commands (alarm state, updater endpoint), as an exact
//!   line, in the three tolon-app files that gate on it (`CFG_GATES`);
//! - the e2e build line of `ci.yml` (`tauri build --debug ... --features e2e-loopback`), in a job that
//!   uploads no artifact;
//! - the Rust sources that gate on it or lint and test it (`EXEMPT`) and Markdown docs.
//!
//! `all-features` may appear nowhere, Markdown docs and this file apart.
//!
//! `release.yml` (tag builds, which `ci.yml` never runs on) must run this check itself in its
//! preflight job, and may pass no `--features` and no `--debug` to the build.

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

const FEATURE: &str = "e2e-loopback";
const ALL_FEATURES: &str = "all-features";
const CI_WORKFLOW: &str = ".github/workflows/ci.yml";
const RELEASE_WORKFLOW: &str = ".github/workflows/release.yml";
const RELEASE_SELF_CHECK: &str = "run: cargo xtask check-release-features";

/// The only lines in a manifest that may name the feature.
const DEFINITIONS: &[(&str, &str)] = &[
    ("crates/tolon-sync/Cargo.toml", "e2e-loopback = []"),
    (
        "crates/tolon-app/Cargo.toml",
        "e2e-loopback = [\"tolon-sync/e2e-loopback\"]",
    ),
];

/// The only line that may name the feature in the three tolon-app sources that gate the WebDriver
/// suite's commands (the alarm-state read, the settings window's visibility and the updater's
/// loopback endpoint: their definitions, their registration in the handler list, and the endpoint
/// setter). Exact lines, like `DEFINITIONS`: any other mention in those files is a violation.
const CFG_GATES: &[(&str, &str)] = &[
    (
        "crates/tolon-app/src/commands.rs",
        "#[cfg(feature = \"e2e-loopback\")]",
    ),
    (
        "crates/tolon-app/src/main.rs",
        "#[cfg(feature = \"e2e-loopback\")]",
    ),
    (
        "crates/tolon-app/src/updater.rs",
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
    if rel == RELEASE_WORKFLOW {
        found.extend(release_workflow_violations(rel, text));
    }
    found
}

/// Short forms of `--debug` and `--features` (tauri build: `-d`, `-f`; cargo: `-F`).
const SHORT_FLAGS: &[&str] = &["-d", "-f", "-F"];

/// Jobs that must not start before `preflight` (secrets, tag, version and feature checks) has passed.
const NEEDS_PREFLIGHT: &[&str] = &["build", "publish"];

/// A release build is a plain release-profile build, the tag run re-checks this very rule, and no
/// job builds or publishes before `preflight` passed.
fn release_workflow_violations(rel: &str, text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        let n = i + 1;
        // `--features=x` is covered by the same prefix.
        for flag in ["--features", "--debug"] {
            if line.contains(flag) {
                found.push(format!(
                    "{rel}:{n}: `{flag}` is not allowed in the release workflow"
                ));
            }
        }
        if !line.starts_with('#') {
            for token in line.split_whitespace() {
                if SHORT_FLAGS.contains(&token) {
                    found.push(format!(
                        "{rel}:{n}: `{token}` (short for --debug or --features) is not allowed in the release workflow"
                    ));
                }
            }
        }
    }
    if !text.lines().any(|l| l.trim() == RELEASE_SELF_CHECK) {
        found.push(format!(
            "{rel}: must contain the step `{RELEASE_SELF_CHECK}`"
        ));
    }
    let jobs = split_jobs(text);
    for name in NEEDS_PREFLIGHT {
        let ok = jobs.iter().any(|(job, body)| {
            job == name
                && body
                    .iter()
                    .any(|l| l.starts_with("    needs:") && l.contains("preflight"))
        });
        if !ok {
            found.push(format!("{rel}: job `{name}` must have `needs: preflight`"));
        }
    }
    found
}

/// The jobs of a workflow as (name, body lines); a job header is a two-space-indented `name:` under `jobs:`.
fn split_jobs(text: &str) -> Vec<(&str, Vec<&str>)> {
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
    jobs
}

/// A job that builds with the feature must not upload an artifact: the build could leave the repo.
fn artifact_violations(rel: &str, text: &str) -> Vec<String> {
    split_jobs(text)
        .iter()
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

    const RELEASE_OK: &str = "jobs:\n  preflight:\n    steps:\n      - name: Check\n        run: cargo xtask check-release-features\n  build:\n    needs: preflight\n    steps:\n      - run: npm run tauri build -- --ci\n  publish:\n    needs: [preflight, build]\n    steps:\n      - run: gh release create\n";

    #[test]
    fn release_workflow_may_not_pass_the_feature() {
        let yml =
            format!("{RELEASE_OK}      - run: npm run tauri build -- --features e2e-loopback\n");
        // The feature name and the `--features` flag are one violation each.
        assert_eq!(violations(RELEASE_WORKFLOW, &yml).len(), 2);
    }

    #[test]
    fn release_workflow_must_self_check_and_build_in_release_mode() {
        assert!(violations(RELEASE_WORKFLOW, RELEASE_OK).is_empty());
        let no_check = RELEASE_OK.replace("cargo xtask check-release-features", "true");
        assert_eq!(violations(RELEASE_WORKFLOW, &no_check).len(), 1);
        let debug = format!("{RELEASE_OK}      - run: npm run tauri build -- --debug\n");
        assert_eq!(violations(RELEASE_WORKFLOW, &debug).len(), 1);
        let features = format!("{RELEASE_OK}      - run: cargo build --features foo\n");
        assert_eq!(violations(RELEASE_WORKFLOW, &features).len(), 1);
    }

    #[test]
    fn release_workflow_rejects_the_short_and_equals_flag_forms() {
        for flag in ["-d", "-f", "-F", "--features=foo", "-f foo"] {
            let yml = format!("{RELEASE_OK}      - run: npm run tauri build -- --ci {flag}\n");
            assert_eq!(violations(RELEASE_WORKFLOW, &yml).len(), 1, "{flag}");
        }
        // Words that merely contain the letters, and prose in comments, are fine.
        let fine = format!(
            "{RELEASE_OK}      - run: find . -type f -printf x\n      # -d and -f in a comment\n"
        );
        assert!(violations(RELEASE_WORKFLOW, &fine).is_empty());
    }

    #[test]
    fn release_build_and_publish_need_preflight() {
        for job in ["build", "publish"] {
            let (head, rest) = RELEASE_OK
                .split_once(&format!("  {job}:\n"))
                .unwrap_or_default();
            // Drop the `needs:` line of that job only.
            let (needs, tail) = rest.split_once('\n').unwrap_or_default();
            assert!(needs.starts_with("    needs:"));
            let yml = format!("{head}  {job}:\n{tail}");
            let found = violations(RELEASE_WORKFLOW, &yml);
            assert_eq!(found.len(), 1, "{job}: {found:?}");
            assert!(found[0].contains(job));
        }
        // `publish` that needs only `build` does not name preflight.
        let only_build = RELEASE_OK.replace("[preflight, build]", "build");
        assert_eq!(violations(RELEASE_WORKFLOW, &only_build).len(), 1);
    }

    #[test]
    fn a_comment_in_a_workflow_still_counts() {
        let comment = format!("{RELEASE_OK}      # e2e-loopback\n");
        assert_eq!(violations(RELEASE_WORKFLOW, &comment).len(), 1);
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
            violations(".github/workflows/other.yml", "args: --all-features\n").len(),
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
            "crates/tolon-app/src/updater.rs",
        ] {
            assert!(violations(file, gate).is_empty(), "{file}");
            assert!(violations(file, &gate.repeat(3)).is_empty(), "{file}");
        }
        // ... and are not exempt: every other mention fails, in code or in a comment.
        for file in [
            "crates/tolon-app/src/commands.rs",
            "crates/tolon-app/src/main.rs",
            "crates/tolon-app/src/updater.rs",
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

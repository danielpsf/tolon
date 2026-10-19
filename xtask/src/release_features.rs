//! `cargo xtask check-release-features`: the `e2e-loopback` test seam (plain http to loopback hosts)
//! must stay out of every release path. Only `ci.yml` (the e2e job) may pass it. The feature is also
//! refused at compile time in release mode (tolon-sync), so this check is the second lock.

use anyhow::{bail, Context, Result};
use std::path::Path;

use crate::fsutil::files_with_ext;

const FEATURE: &str = "e2e-loopback";

/// The only lines outside comments and `ci.yml` that may name the feature.
const DEFINITIONS: &[(&str, &str)] = &[
    ("crates/tolon-sync/Cargo.toml", "e2e-loopback = []"),
    (
        "crates/tolon-app/Cargo.toml",
        "e2e-loopback = [\"tolon-sync/e2e-loopback\"]",
    ),
];

/// Violations in one file. `rel` is the path from the repo root with `/` separators.
fn violations(rel: &str, text: &str) -> Vec<String> {
    let workflow = rel.starts_with(".github/workflows/");
    text.lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let line = line.trim();
            let names_feature = line.contains(FEATURE);
            // A release workflow may not turn every feature on either.
            let all_features = workflow && line.contains("all-features");
            let defined_here = DEFINITIONS.iter().any(|(p, l)| *p == rel && *l == line);
            let comment = !workflow && line.starts_with('#');
            ((names_feature || all_features) && !defined_here && !comment).then(|| {
                format!(
                    "{rel}:{}: `{FEATURE}` or `all-features` on a release path",
                    i + 1
                )
            })
        })
        .collect()
}

pub fn run(root: &Path) -> Result<()> {
    let mut files = vec![
        "Cargo.toml".to_owned(),
        "xtask/Cargo.toml".to_owned(),
        ".cargo/config.toml".to_owned(),
        "ui/package.json".to_owned(),
    ];
    for (dir, exts) in [
        ("crates", &["toml", "json"][..]),
        ("ui/scripts", &["mjs", "js", "json"][..]),
    ] {
        for p in files_with_ext(&root.join(dir), exts)? {
            let rel = p.strip_prefix(root).unwrap_or(&p);
            files.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    // files_with_ext skips dot directories, so the workflows are listed by hand. ci.yml is the one allowed user.
    let workflows = root.join(".github/workflows");
    if workflows.is_dir() {
        for entry in std::fs::read_dir(&workflows)? {
            let name = entry?.file_name().to_string_lossy().into_owned();
            if (name.ends_with(".yml") || name.ends_with(".yaml")) && name != "ci.yml" {
                files.push(format!(".github/workflows/{name}"));
            }
        }
    }

    let mut found = Vec::new();
    for rel in &files {
        let path = root.join(rel);
        if path.is_file() {
            let text =
                std::fs::read_to_string(&path).with_context(|| format!("cannot read {rel}"))?;
            found.extend(violations(rel, &text));
        }
    }
    if found.is_empty() {
        println!(
            "release-features: {} files checked; {FEATURE} appears only in its two definitions and ci.yml",
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

    #[test]
    fn release_workflow_may_not_pass_the_feature() {
        let yml = "      - run: npm run tauri build -- --features e2e-loopback\n";
        assert_eq!(violations(".github/workflows/release.yml", yml).len(), 1);
    }

    #[test]
    fn release_workflow_may_not_use_all_features() {
        let yml = "        args: --all-features\n";
        assert_eq!(violations(".github/workflows/release.yml", yml).len(), 1);
    }

    #[test]
    fn a_comment_in_a_workflow_still_counts() {
        let yml = "      # e2e-loopback\n";
        assert_eq!(violations(".github/workflows/release.yml", yml).len(), 1);
    }

    #[test]
    fn the_two_definitions_and_toml_comments_pass() {
        assert!(violations("crates/tolon-sync/Cargo.toml", "e2e-loopback = []\n").is_empty());
        assert!(violations(
            "crates/tolon-app/Cargo.toml",
            "# e2e-loopback is for tests\n"
        )
        .is_empty());
        assert!(violations(
            "crates/tolon-app/Cargo.toml",
            "e2e-loopback = [\"tolon-sync/e2e-loopback\"]\n"
        )
        .is_empty());
    }

    #[test]
    fn a_default_feature_or_dependency_on_it_fails() {
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
    fn the_npm_scripts_may_not_bake_it_in() {
        let json = "\"tauri\": \"node scripts/tauri.mjs --features e2e-loopback\",\n";
        assert_eq!(violations("ui/package.json", json).len(), 1);
    }

    #[test]
    fn the_repo_passes() -> Result<()> {
        run(&crate::repo_root()?)
    }
}

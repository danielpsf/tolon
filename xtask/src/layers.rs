//! Dependency direction between workspace crates, and the `target_os` confinement rule.

use anyhow::{Context, Result};
use serde_json::Value;
use std::path::Path;

use crate::fsutil::files_with_ext;

/// A workspace package and the names it depends on (any kind).
pub struct Package {
    pub name: String,
    pub deps: Vec<String>,
}

enum Allowed {
    Only(&'static [&'static str]),
    AnyWorkspaceCrate,
}

fn rule(name: &str) -> Option<Allowed> {
    match name {
        "tolon-core" | "xtask" => Some(Allowed::Only(&[])),
        "tolon-store" | "tolon-sync" | "tolon-platform" => Some(Allowed::Only(&["tolon-core"])),
        "tolon-app" => Some(Allowed::AnyWorkspaceCrate),
        _ => None,
    }
}

/// Parse `cargo metadata --no-deps` output into workspace packages.
pub fn parse_metadata(json: &str) -> Result<Vec<Package>> {
    let value: Value = serde_json::from_str(json).context("cargo metadata is not valid JSON")?;
    let packages = value["packages"]
        .as_array()
        .context("cargo metadata has no `packages` array")?;
    Ok(packages
        .iter()
        .filter_map(|p| {
            let name = p["name"].as_str()?.to_string();
            let deps = p["dependencies"]
                .as_array()?
                .iter()
                .filter_map(|d| d["name"].as_str().map(str::to_string))
                .collect();
            Some(Package { name, deps })
        })
        .collect())
}

/// One message per violated rule; empty means the layering holds.
pub fn check(packages: &[Package]) -> Vec<String> {
    let workspace: Vec<&str> = packages.iter().map(|p| p.name.as_str()).collect();
    let mut violations = Vec::new();
    for pkg in packages {
        let Some(allowed) = rule(&pkg.name) else {
            violations.push(format!(
                "{}: no layer rule; add it to xtask/src/layers.rs",
                pkg.name
            ));
            continue;
        };
        for dep in pkg.deps.iter().filter(|d| workspace.contains(&d.as_str())) {
            let ok = match &allowed {
                Allowed::Only(list) => list.contains(&dep.as_str()),
                Allowed::AnyWorkspaceCrate => true,
            };
            if dep == "tolon-app" {
                violations.push(format!("{} depends on tolon-app; nothing may", pkg.name));
            } else if !ok {
                violations.push(format!("{} must not depend on {dep}", pkg.name));
            }
        }
    }
    violations
}

/// Rule 9: `target_os` may appear only under `crates/tolon-platform`.
pub fn target_os_violations(path: &str, src: &str) -> Vec<String> {
    src.lines()
        .enumerate()
        .filter(|(_, line)| line.contains("target_os"))
        .map(|(n, _)| format!("{path}:{}: target_os outside crates/tolon-platform", n + 1))
        .collect()
}

pub fn run(root: &Path) -> Result<()> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = std::process::Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .context("failed to run cargo metadata")?;
    if !output.status.success() {
        anyhow::bail!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let packages = parse_metadata(&String::from_utf8_lossy(&output.stdout))?;
    let mut violations = check(&packages);

    for file in files_with_ext(&root.join("crates"), &["rs"])? {
        let rel = file.strip_prefix(root).unwrap_or(&file);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if rel_str.starts_with("crates/tolon-platform/") {
            continue;
        }
        let src = std::fs::read_to_string(&file)
            .with_context(|| format!("cannot read {}", file.display()))?;
        violations.extend(target_os_violations(&rel_str, &src));
    }

    if violations.is_empty() {
        println!("layers: ok ({} workspace crates)", packages.len());
        Ok(())
    } else {
        for v in &violations {
            eprintln!("layers: {v}");
        }
        anyhow::bail!("{} layer violation(s)", violations.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, deps: &[&str]) -> Package {
        Package {
            name: name.into(),
            deps: deps.iter().map(|d| (*d).to_string()).collect(),
        }
    }

    fn workspace() -> Vec<Package> {
        vec![
            pkg("tolon-core", &[]),
            pkg("tolon-store", &["tolon-core", "rusqlite"]),
            pkg("tolon-sync", &["tolon-core"]),
            pkg("tolon-platform", &["tolon-core"]),
            pkg(
                "tolon-app",
                &["tolon-core", "tolon-store", "tolon-sync", "tolon-platform"],
            ),
            pkg("xtask", &["anyhow"]),
        ]
    }

    #[test]
    fn valid_layering_has_no_violations() {
        assert!(check(&workspace()).is_empty());
    }

    #[test]
    fn core_depending_on_app_is_rejected() {
        let mut ws = workspace();
        ws[0].deps.push("tolon-app".into());
        assert_eq!(
            check(&ws),
            vec!["tolon-core depends on tolon-app; nothing may"]
        );
    }

    #[test]
    fn sibling_and_xtask_edges_are_rejected() {
        let mut ws = workspace();
        ws[1].deps.push("tolon-sync".into());
        ws[5].deps.push("tolon-core".into());
        assert_eq!(
            check(&ws),
            vec![
                "tolon-store must not depend on tolon-sync",
                "xtask must not depend on tolon-core"
            ]
        );
    }

    #[test]
    fn unknown_workspace_crate_needs_a_rule() {
        assert_eq!(check(&[pkg("tolon-new", &[])]).len(), 1);
    }

    #[test]
    fn parses_metadata_json() -> Result<()> {
        let json = r#"{"packages":[{"name":"a","dependencies":[{"name":"b"},{"name":"c"}]}]}"#;
        let pkgs = parse_metadata(json)?;
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].deps, vec!["b", "c"]);
        Ok(())
    }

    #[test]
    fn flags_target_os_lines() {
        let v = target_os_violations("crates/x/src/a.rs", "ok\n#[cfg(target_os = \"macos\")]\n");
        assert_eq!(
            v,
            vec!["crates/x/src/a.rs:2: target_os outside crates/tolon-platform"]
        );
    }
}

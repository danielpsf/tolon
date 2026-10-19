use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Files under `dir` (recursive) whose name ends in one of `exts`; skips `node_modules`, `target`, dot dirs.
/// A missing `dir` yields an empty list.
pub fn files_with_ext(dir: &Path, exts: &[&str]) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if dir.is_dir() {
        walk(dir, exts, &mut out)?;
    }
    out.sort();
    Ok(out)
}

fn walk(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))? {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if name != "node_modules" && name != "target" && !name.starts_with('.') {
                walk(&path, exts, out)?;
            }
        } else if exts.iter().any(|e| name.ends_with(&format!(".{e}"))) {
            out.push(path);
        }
    }
    Ok(())
}

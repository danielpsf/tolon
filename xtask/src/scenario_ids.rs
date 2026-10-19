//! `check-scenario-ids`: the rule in docs/decisions.md ("What `check-scenario-ids` enforces").
//!
//! Fails when (a) a test name or a manual-checklist line names an id missing from `docs/plan.md`,
//! or (b) an id named in a `done` ledger row has neither a test nor a checklist line.

use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::Path;

use crate::fsutil::files_with_ext;
use crate::ids::{extract_ids, rust_test_ids, ts_test_ids};

/// Ids named by ledger rows whose status is `done`, as `(row id, ids)`.
pub fn done_row_ids(ledger: &str) -> Vec<(String, BTreeSet<String>)> {
    ledger
        .lines()
        .filter_map(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            // `| id | milestone | task | owner | status | commit | proof |` splits to 9 cells.
            let (id, status) = (cells.get(1)?, cells.get(5)?);
            (id.starts_with("L-") && *status == "done").then(|| (id.to_string(), extract_ids(line)))
        })
        .collect()
}

/// Rule violations; empty means the check passes.
pub fn evaluate(
    plan: &BTreeSet<String>,
    test_ids: &BTreeSet<String>,
    checklist_ids: &BTreeSet<String>,
    done_rows: &[(String, BTreeSet<String>)],
) -> Vec<String> {
    let mut violations = Vec::new();
    for id in test_ids.iter().filter(|i| !plan.contains(*i)) {
        violations.push(format!("{id}: named in a test but not in docs/plan.md"));
    }
    for id in checklist_ids.iter().filter(|i| !plan.contains(*i)) {
        violations.push(format!(
            "{id}: named in docs/manual-checklist.md but not in docs/plan.md"
        ));
    }
    for (row, ids) in done_rows {
        for id in ids {
            if !test_ids.contains(id) && !checklist_ids.contains(id) {
                violations.push(format!(
                    "{id}: ledger row {row} is done but no test or checklist line names it"
                ));
            }
        }
    }
    violations
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))
}

fn is_ts_test_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    [".test.ts", ".test.tsx", ".spec.ts", ".spec.tsx"]
        .iter()
        .any(|suffix| name.ends_with(suffix))
}

pub fn run(root: &Path) -> Result<()> {
    let plan = extract_ids(&read(&root.join("docs/plan.md"))?);
    let checklist = extract_ids(&read(&root.join("docs/manual-checklist.md"))?);
    let done_rows = done_row_ids(&read(&root.join("docs/ledger.md"))?);

    let mut tests = BTreeSet::new();
    for file in files_with_ext(&root.join("crates"), &["rs"])? {
        tests.extend(rust_test_ids(&read(&file)?));
    }
    for file in files_with_ext(&root.join("ui/src"), &["ts", "tsx"])? {
        if is_ts_test_file(&file) {
            tests.extend(ts_test_ids(&read(&file)?));
        }
    }

    let covered = plan
        .iter()
        .filter(|id| tests.contains(*id) || checklist.contains(*id))
        .count();
    let violations = evaluate(&plan, &tests, &checklist, &done_rows);
    println!(
        "check-scenario-ids: {} ids in plan, {covered} covered by a test or checklist line",
        plan.len()
    );
    if violations.is_empty() {
        Ok(())
    } else {
        for v in &violations {
            eprintln!("check-scenario-ids: {v}");
        }
        anyhow::bail!("{} scenario id violation(s)", violations.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn passes_when_everything_known_and_done_rows_covered() {
        let done = vec![("L-006".to_string(), set(&["S1.1"]))];
        let v = evaluate(&set(&["S1.1", "S1.2"]), &set(&["S1.1"]), &set(&[]), &done);
        assert!(v.is_empty());
    }

    #[test]
    fn unknown_test_id_fails() {
        let v = evaluate(&set(&["S1.1"]), &set(&["S99.9"]), &set(&[]), &[]);
        assert_eq!(v, vec!["S99.9: named in a test but not in docs/plan.md"]);
    }

    #[test]
    fn unknown_checklist_id_fails() {
        let v = evaluate(&set(&["S1.1"]), &set(&[]), &set(&["AC9.9"]), &[]);
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn done_row_with_uncovered_id_fails_but_checklist_counts() {
        let done = vec![("L-011".to_string(), set(&["S4.1", "S4.2"]))];
        let v = evaluate(&set(&["S4.1", "S4.2"]), &set(&[]), &set(&["S4.1"]), &done);
        assert_eq!(
            v,
            vec!["S4.2: ledger row L-011 is done but no test or checklist line names it"]
        );
    }

    #[test]
    fn only_done_rows_are_picked_from_the_ledger() {
        let ledger = "| id | m | t | o | status | c | p |\n\
| --- | --- | --- | --- | --- | --- | --- |\n\
| L-001 | M0 | a (S1.1) | core | done | abc | AC1.1 |\n\
| L-002 | M0 | b (S1.2) | core | todo | | |\n";
        let rows = done_row_ids(ledger);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "L-001");
        assert_eq!(rows[0].1, set(&["S1.1", "AC1.1"]));
    }
}

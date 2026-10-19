//! S2.1: every fixture parses and expands to its committed snapshot.
//!
//! `cargo xtask fixtures` runs this with TOLON_UPDATE_FIXTURES=1 (exactly "1"), which rewrites the
//! `<file>.expected.json` files instead of comparing. Read the diff before committing.

mod common;

use common::{fixture_text, fixtures_dir, now, TestResult, FIXTURE_FILES, HUGE, LOCAL};
use tolon_core::{expand, parse_ics, ExpandOptions, ParseOptions};

/// Snapshots are taken under the default options, except all-day.ics: it keeps its all-day
/// events so their spans can be checked by eye (AC2.4 has its own test for the default).
fn snapshot(name: &str) -> TestResult<String> {
    let parsed = parse_ics(
        &fixture_text(name)?,
        &common::cal(),
        &ParseOptions::default(),
    );
    let opts = ExpandOptions {
        include_all_day: name == "all-day.ics",
        ..ExpandOptions::default()
    };
    let occurrences = expand(&parsed.events, now()?, LOCAL, &opts);
    let doc = serde_json::json!({
        "now": now()?,
        "zone": LOCAL.name(),
        "calendar_name": parsed.name,
        "events": parsed.events.len(),
        "skipped_components": parsed.skipped_components,
        "occurrences": occurrences,
    });
    Ok(serde_json::to_string_pretty(&doc)? + "\n")
}

#[test]
fn s2_1_fixtures_match_their_snapshots() -> TestResult {
    let update = std::env::var("TOLON_UPDATE_FIXTURES").is_ok_and(|v| v == "1");
    let mut failures = Vec::new();
    for name in FIXTURE_FILES {
        let actual = snapshot(name)?;
        let path = fixtures_dir().join(format!("{name}.expected.json"));
        if update {
            std::fs::write(&path, &actual)?;
            continue;
        }
        match std::fs::read_to_string(&path) {
            Err(_) => failures.push(format!("{name}: no snapshot; run `cargo xtask fixtures`")),
            Ok(expected) if expected != actual => {
                let line = expected
                    .lines()
                    .zip(actual.lines())
                    .position(|(e, a)| e != a)
                    .unwrap_or_else(|| expected.lines().count().min(actual.lines().count()))
                    + 1;
                failures.push(format!("{name}: differs from snapshot at line {line}"));
            }
            Ok(_) => {}
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

#[test]
fn s2_1_the_twelve_fixtures_are_the_twelve_in_the_plan() -> TestResult {
    let plan = std::fs::read_to_string(common::fixtures_dir().join("../../docs/plan.md"))?;
    for name in FIXTURE_FILES {
        assert!(
            plan.contains(&format!("| {name} |")),
            "{name} missing in plan"
        );
    }
    assert_eq!(FIXTURE_FILES.len(), 12);
    Ok(())
}

#[test]
fn s2_1_huge_feed_has_10000_events_and_none_skipped() -> TestResult {
    let parsed = common::parse_fixture(HUGE)?;
    assert_eq!(parsed.events.len(), 10_000);
    assert_eq!(parsed.skipped_components, 0);
    Ok(())
}

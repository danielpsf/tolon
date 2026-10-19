# Core agent (crates/tolon-core, tolon-store, tolon-sync)

Pure Rust. No UI, no platform code. Tests first where the scenario is precise (parsing, expansion, scheduling, link detection).

Rules on top of `AGENTS.md`:
- `tolon-core` has no I/O and no `tokio`. If you need the clock, take `now` as a parameter.
- Every scenario id you implement appears in a `#[test]` name: `fn s2_2_dst_change()`.
- Fixtures: add the anonymised `.ics` under `fixtures/ics/`, run `cargo xtask fixtures`, commit the expected JSON, and read the diff before committing.
- Property tests for the scheduler: alarms fire at most once per Scheduled→Firing transition; a Dismissed alarm never fires; plan() is idempotent.
- Secrets: any URL that enters a log passes through `redact_url()`. Write a test that proves the secret is absent.

# Ledger

The running record. One row per task. Status is one of: todo, doing, review, done, blocked, dropped. Keep rows short; details go in the PR. Never delete a row; mark it dropped with the reason.

Conventions: ids are `L-NNN` in creation order. The orchestrator creates rows; the assigned agent updates status, commit and follow-ups. A milestone is closed by a row of its own whose "proof" lists the green gate run.

| id | milestone | task | owner (agent role) | status | commit | proof / follow-ups |
| --- | --- | --- | --- | --- | --- | --- |
| L-001 | M0 | Create the Cargo workspace and crate skeletons per `docs/architecture.md` | core | done | d4e85cf | reviewer pass; bootstrap gate (fmt, clippy, test) green on macOS. Gate run by hand (fmt, clippy, test) green. No LICENSE file in repo yet; manifests say `MIT OR Apache-2.0`, confirm and add it. Toolchain pinned to 1.99. |
| L-002 | M0 | Tauri 2 app boots on macOS, Windows, Linux with an empty Swiss Moss page | ui | todo | | |
| L-003 | M0 | Vendor Swiss Moss into `ui/` as an npm dependency and bundle Inter + Inter Tight | ui | todo | | |
| L-004 | M0 | SQLite store with migrations V1, `settings` repo, startup migration check | core | todo | | |
| L-005 | M0 | `xtask ci`, `layers`, `check-scenario-ids`; `.github/workflows/ci.yml` green on 3 OS | release | todo | | |
| L-023 | M0 | WebDriver smoke suite (`npm --prefix ui run e2e`): the app boots and shows the Swiss Moss paper background; the `e2e` CI job is green on Linux and Windows | qa | todo | | |
| L-024 | M0 | Close M0: `cargo xtask ci` green on macOS, Windows, Linux in CI on `main`; empty Swiss Moss paper window opens | orchestrator | todo | | |
| L-006 | M1 | ICS fetcher with ETag, timeout, size cap, redacted logging (S1.1 to S1.5) | core | todo | | |
| L-007 | M1 | Event parsing and expansion with the 12 fixtures (S2.1 to S2.3) | core | todo | | |
| L-008 | M1 | Calendars settings page per mockup 03 (add, list, enable, errors) | ui | todo | | |
| L-009 | M2 | Alarm planning and state machine with proptest (S3.1 to S3.6) | core | todo | | |
| L-010 | M2 | Interrupt window: creation per screen, keyboard, close-all (S4.2, S4.3, S4.6) | ui | todo | | |
| L-011 | M2 | macOS window level above full-screen apps via objc2 (S4.1) | platform | todo | | manual checklist |
| L-012 | M2 | Windows topmost and Linux X11 / Wayland fallback (S4.4) | platform | todo | | |
| L-013 | M2 | Join link detection and opener (S5.1, S5.2) | core | todo | | |
| L-014 | M2 | Alarm sound with repeat and preview (F6) | platform | todo | | |
| L-015 | M3 | Tray icon, panel per mockup 02, pause, quit semantics (S7.1, S7.2) | ui | todo | | |
| L-016 | M3 | Quiet hours and ignore rules with live re-plan (S8.1, S8.2) | core | todo | | |
| L-017 | M3 | Alarms, Quiet hours, Appearance, Startup settings pages (F9) | ui | todo | | |
| L-018 | M3 | First-run flow with test alarm (F10) | ui | todo | | |
| L-019 | M4 | Updater with minisign key and skip-version (F11) | release | todo | | |
| L-020 | M4 | Release workflow, install docs in release notes, SHA256SUMS (F12) | release | todo | | |
| L-021 | M4 | Clean-machine install test on each OS, signed in this ledger (AC12.3) | qa | todo | | |
| L-022 | M5 | Google OAuth with PKCE, keyring, incremental sync (F13) | core | todo | | |

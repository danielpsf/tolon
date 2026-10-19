# Decisions

One entry per decision, newest first. Format: date, decision, why, what it replaces, who. Entries are never edited after the fact; a later entry supersedes.

## 2026-10-19 CSP allows `data:` images
Add `img-src 'self' data:` to the CSP in `tauri.conf.json` before the first Swiss Moss form control ships (M1, L-008). Why: `swiss-moss.css` draws the checkbox tick, the indeterminate dash and the select chevron as `data:image/svg+xml` backgrounds, which `default-src 'self'` blocks. No network origin is added. Replaces the CSP line in `docs/architecture.md`. Who: orchestrator, after the L-002 and L-003 reviews.

## 2026-10-19 Mockups live in `mockups/mockups.figcraft`
The person reworked the SVG artboards in FigCraft; `mockups/mockups.figcraft` is now the design source and the SVGs are removed. One page, "Mockups", 12 frames named like the old files (`01-interrupt-dark` ... `06-snoozed-banner-light`). Agents read it headless with `figcraft-cli` (no app needed): `figcraft-cli info mockups/mockups.figcraft --json` for frame ids and sizes, `figcraft-cli export mockups/mockups.figcraft -o <dir> --scale 1` (add `--format svg` for SVG, `--node <id|name>` for one frame) to measure, and `figcraft-cli run steps.json --in mockups/mockups.figcraft` with `node.get` for exact bounds, fills and text. Edits go through `figcraft-cli run steps.json --in mockups/mockups.figcraft --out <new file>` (or the `figcraft` MCP server), only when the person asks or rule 10 applies, and are recorded here. Plan and agent references to `mockups/*.svg` mean the frame of the same name. Why: the person asked for it. Replaces the SVG mockups. Who: orchestrator.

## 2026-10-19 What `check-scenario-ids` enforces, and local CI first
`cargo xtask check-scenario-ids` fails when (a) a test name (`fn s2_2_...`) or a `docs/manual-checklist.md` line names an id (S*.n or AC*.n) that is not in `docs/plan.md`, or (b) a scenario or AC id named in a ledger row whose status is done has no test name and no checklist line. Ids of milestones not yet done may be uncovered. Why: the literal rule (every id in the plan covered) cannot be green before M2, and M0 must be green. Replaces nothing; tightens as rows close.
Before any push, the gate runs locally: macOS natively, Linux through `act` (Docker, `catthehacker/ubuntu:act-22.04`, arm64), Windows as `cargo xwin clippy --target x86_64-pc-windows-msvc` (compile and lint only; Windows tests and e2e run in GitHub CI). Why: the person asked to avoid waiting on GitHub runners. Who: orchestrator.

## 2026-10-19 Gate bootstrap for L-001 and the order of M0
Until L-005 lands, `cargo xtask ci` does not exist, so L-001 to L-004 merge on the commands it will run, executed by hand on the branch: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic`, `cargo test --workspace`, and `npm --prefix ui run lint` and `npm --prefix ui run test` once `ui/` exists. From the L-005 merge on, `cargo xtask ci` is the only gate. M0 also gets L-023 (WebDriver smoke) because `ci.yml` already runs an `e2e` job that needs a suite, and L-024 to close the milestone. Why: the hard rule needs a gate, and the gate is itself an M0 task. Replaces nothing. Who: orchestrator.

## 2026-10-19 Tolon is the meeting alarm and nothing else
Calendars in, a full-screen alarm out. Chat, email and task trackers are not on the roadmap for this product. Why: a one-person, agent-built project needs one shippable thing, and a small tool is easier to trust.

## 2026-10-19 iCal subscriptions before Google OAuth
Beta 1 ships with iCal URL subscriptions as the only source; Google OAuth is beta 2. Why: an iCal link needs no app registration, no verification, and works for Google, Outlook and anything else on day one.

## 2026-10-19 Tauri 2, Rust core, React view, Swiss Moss
As recorded in `docs/architecture.md`. Why: one language for logic, small binaries, tray and windows from plugins, the design system already exists.

## 2026-10-19 Ad-hoc signed macOS builds, no notarisation
No Apple Developer membership. The install guide carries the Open Anyway steps. Revisit when the beta has users who ask.

## 2026-10-19 Wayland gets a fallback, not a fight
Compositors that refuse always-on-top get a notification with actions plus sound. Documented as expected behaviour.

## 2026-10-19 No telemetry
None in beta. If a crash reporter is ever added it will be opt-in and documented.

## 2026-10-19 WebDriver e2e is non-blocking for now
The e2e jobs (Linux, Windows) run with `continue-on-error` so `main` is not red while Windows WebDriver is unreliable on hosted runners (L-027, actions/runner-images#14738). The `cargo xtask ci` gate stays blocking on all three OS. Revert to blocking once L-027 shows Windows e2e green on several runs in a row.

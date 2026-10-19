# Tolon: rules for agents and people

You are building Tolon beta 1, a desktop app that takes over the screen before a meeting starts. Read, in this order: `docs/plan.md` (what and why, with every scenario), `docs/architecture.md` (how), `docs/ledger.md` (where we are), `docs/decisions.md` (what was already decided). The mockups in `mockups/mockups.figcraft` are the UI (read them with `figcraft-cli`, see `docs/decisions.md`). The design system is Swiss Moss (https://github.com/danielpsf/swiss-moss, docs at https://danielpsf.github.io/swiss-moss/).

## How work is organised

- The ledger (`docs/ledger.md`) is the only source of truth for status. A task exists when it has a row; it is done when its row says done with a commit hash and the gate that proves it.
- The orchestrator (the main agent session, whichever harness you use) owns the ledger and the milestone order in `docs/plan.md`. It breaks a milestone into tasks of at most half a day each, writes the rows, and delegates to sub-agents by role (`docs/agents/*.md`). It never implements and reviews the same task.
- Sub-agents work on one task each, on their own branch, and finish by updating their ledger row (status, commit, follow-ups) and nothing else in the ledger.
- A reviewer sub-agent with fresh context checks every PR against the scenarios it claims and the conventions below, before the orchestrator merges.
- Decisions that change `docs/architecture.md` or `docs/plan.md` go through a dated entry in `docs/decisions.md` first; the orchestrator writes it, the change follows.

## Hard rules

1. `cargo xtask ci` must be green before any merge. No exceptions, no "will fix after".
2. No `unwrap`, `expect` or `panic` in shipped code. Clippy enforces it.
3. No new dependency without a line in the PR saying why and a `cargo deny` pass.
4. Never log calendar URLs with secrets, OAuth tokens or event titles above debug level. `redact_url()` on every URL that reaches a log or an error message.
5. UI uses Swiss Moss classes and `--sm-*` tokens only. No inline hex colours, no new component styles outside `ui/src/styles/local.css`, and that file needs a reviewer's sign-off.
6. Every scenario id (S1.1 ...) you implement appears in a test name or in `docs/manual-checklist.md`. `cargo xtask check-scenario-ids` fails otherwise.
7. Do not write to anyone's calendar. Tolon is read only.
8. No telemetry, no analytics, no network calls beyond the ones listed in `docs/architecture.md`.
9. Platform-specific code lives only in `crates/tolon-platform`, behind the `Platform` trait, with a stub for CI.
10. Ask before deviating from the mockups; if you must, record it in `docs/decisions.md`.

## Writing rules (copy in the UI, docs, release notes)

- Sentence case. Full sentences with a full stop in headlines: "Never miss a meeting again."
- Buttons say what happens: "Join the call", "Snooze 5 min", "Add calendar".
- One-line consequences next to anything irreversible.
- No emoji, no exclamation marks, no "Oops". The signal colour is for things that are wrong or irreversible only.
- "Tolon" is the product name; never "the app" in user-facing copy.

## Commands you will use

```
cargo xtask ci                      # the gate
cargo run -p tolon-app              # run the desktop app in dev (uses ui/ dev server)
cargo test -p tolon-core            # fast core tests
cargo xtask fixtures                # regenerate expected JSON for fixtures (review the diff!)
npm --prefix ui run dev | build | lint | test
```

## Definition of a good PR

Title: `[L-023] Expand RRULE with EXDATE and overrides`. Body: which scenarios it makes pass, what it deliberately does not do, how it was tested on which OS. Small, focused, green.

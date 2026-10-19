# Tolon beta 1 kit

Everything a coding agent needs to build Tolon, a desktop app that takes over the screen before a meeting starts. Read only, local only, open source, Swiss Moss design.

## What is here

```
AGENTS.md          rules for agents and people; AGENTS.md is read by Codex, OpenCode, Cursor and others, and CLAUDE.md is a symlink for Claude Code
docs/plan.md       scope, 13 features, 40+ BDD scenarios, acceptance criteria, milestones, gates
docs/architecture.md  Tauri 2 + Rust core + React view + SQLite; crates, windows, scheduler, storage, security
docs/ledger.md     22 starting tasks; the running record agents keep
docs/decisions.md  the decisions already taken, dated
docs/install.md    per-OS install steps (also the release notes)
docs/manual-checklist.md  what CI cannot test on macOS, Windows, Linux
docs/agents/       briefs for orchestrator, core, ui, platform, qa, reviewer, release
mockups/           12 SVG artboards (6 screens, light and dark); open them in FigCraft
.github/workflows/ ci.yml (gate on 3 OS, e2e on 2) and release.yml (ad-hoc macOS, Windows, Linux, updater JSON, checksums)
fixtures/ics/      drop anonymised real calendar feeds here (see docs/agents/qa.md)
```

## Start

1. Create an empty public repository named `tolon`, copy this kit's contents in, commit, push.
2. Add the updater secrets once: run `npx @tauri-apps/cli signer generate -w ~/.tauri/tolon.key`, put the public key aside for `tauri.conf.json`, and add `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` as repository secrets.
3. Open your coding agent in the repo (Claude Code, Codex, OpenCode or any agent that reads AGENTS.md) and say:

> Read AGENTS.md and act as the orchestrator in docs/agents/orchestrator.md. Start milestone M0 from docs/ledger.md: write any missing rows, then work the todo rows, in parallel sub-agents if this harness has them and one role at a time if it does not. Report when the M0 gate is green on all three OS.

4. Repeat per milestone. The kit never assumes a specific harness: `AGENTS.md` is the standard file Codex, OpenCode, Cursor and others read, and `CLAUDE.md` is only a symlink to it for Claude Code. Tag `v0.1.0-beta.1` when M4 closes; the release workflow does the rest.

## Mockups in FigCraft

File, Open, pick any `mockups/*.svg`. Text stays editable and every shape is a vector. Colours are the Swiss Moss tokens; the exact values are in https://github.com/danielpsf/swiss-moss/blob/main/tokens/tokens.json.

# Building Tolon with coding agents

Tolon is built mostly by coding agents working from a written plan, with a human deciding what ships. Any harness that reads `AGENTS.md` can take part, including Claude Code, Codex, OpenCode and Cursor. `CLAUDE.md` is a symlink to `AGENTS.md` for Claude Code.

## The documents agents work from

```
AGENTS.md                  rules for agents and people
docs/plan.md               scope, features, BDD scenarios, acceptance criteria, milestones, gates
docs/architecture.md       Tauri 2 + Rust core + React view + SQLite: crates, windows, scheduler, storage, security
docs/ledger.md             the task list and the running record of what is done
docs/decisions.md          dated decisions
docs/manual-checklist.md   what CI cannot test on macOS, Windows and Linux
docs/agents/*.md           role briefs: orchestrator, core, ui, platform, qa, reviewer, release
mockups/mockups.figcraft   the screens; read and export them with figcraft-cli (see docs/decisions.md)
```

## How a milestone runs

1. The **orchestrator** ([orchestrator.md](orchestrator.md)) reads the ledger and picks the lowest open milestone. It splits the milestone into tasks of at most half a day. Each task gets one owner role and the scenario ids it must make pass.
2. Each task goes to a sub-agent with the matching brief: [core](core.md), [ui](ui.md), [platform](platform.md), [qa](qa.md) or [release](release.md). The sub-agent works on its own branch. Independent tasks run in parallel. Two agents never work in the same crate at once.
3. A **reviewer** ([reviewer.md](reviewer.md)) starts from a fresh context and checks the branch. It runs the gate itself, reads the tests behind every claimed scenario, and checks the rules in `AGENTS.md`.
4. The orchestrator merges only on a green `cargo xtask ci` and a reviewer pass. It then updates the ledger row with the commit and the proof.
5. Each session ends with a three-line status at the top of the ledger: what moved, what is blocked, what is next.

Changes to the plan or the architecture always start as a dated entry in `docs/decisions.md`.

## Starting a session

Open your agent in the repository and say:

> Read AGENTS.md and act as the orchestrator in docs/agents/orchestrator.md. Continue with the lowest open milestone in docs/ledger.md: write any missing rows, then work the todo rows, in parallel sub-agents if this harness has them and one role at a time if it does not. Report when the milestone gate is green on all three systems.

If a harness has no sub-agents, the brief tells you how to cope. Play each role in turn, and start every role from a clean context, so the reviewer never shares memory with the implementer.

## One-time repository setup

Release signing needs an updater key pair. Generate it once with `npx @tauri-apps/cli signer generate -w ~/.tauri/tolon.key`. Put the public key in `crates/tolon-app/tauri.conf.json`. Add `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` as repository secrets. Never commit the private key. Tagging `v0.1.0-beta.1` when M4 closes runs the release workflow.

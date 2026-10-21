# Contributing to Tolon

Thank you for helping Tolon get people to their meetings on time. This guide covers what you need to make a change that can be merged.

## Ways to help

- **Try a beta and report what breaks.** Open an issue with your system, the Tolon version and what you expected to happen.
- **Send a calendar feed that Tolon gets wrong.** Real feeds from Google, Outlook, Apple, Fastmail or Nextcloud find the bugs that synthetic ones miss. Anonymise it first: replace names, emails, UIDs and links, and keep the structure, time zones and repeat rules. Add it under `fixtures/ics/`.
- **Pick up a task.** Open tasks are rows marked `todo` in [docs/ledger.md](docs/ledger.md). Comment on an issue or open a draft pull request so nobody duplicates your work.
- **Improve the copy or the docs.** Small, clear wording fixes are always welcome.

## Set up

You need:
- Rust. `rust-toolchain.toml` pins the version, and `rustup` picks it up automatically.
- Node 22.
- The [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/) for your system.

```sh
git clone https://github.com/danielpsf/tolon.git
cd tolon
npm --prefix ui ci
cargo run -p tolon-app
```

Commands you will use:

```sh
cargo xtask ci          # the full check; must be green before any merge
cargo test -p tolon-core
cargo xtask fixtures    # regenerate expected JSON for fixtures, then read the diff
npm --prefix ui run dev | build | lint | test
```

Quit an installed Tolon before `make dev` or `cargo run -p tolon-app`. Both use the same app id (`dev.tolon.desktop`), so the development build hands over to the Tolon that is already running, which only shows its window, and then exits.

### Google Calendar sign-in

"Sign in with Google" shows only in a build that has a Google OAuth client. Create a client of type "Desktop app" in your own Google Cloud project with the scope `https://www.googleapis.com/auth/calendar.readonly`, then set `TOLON_GOOGLE_CLIENT_ID` and `TOLON_GOOGLE_CLIENT_SECRET` when you build (they are compiled in) or when you run Tolon (they win over the built-in ones). Google issues a client secret with a Desktop app client and asks for it when tokens are exchanged, but its installed-app guidance says a desktop app cannot keep it confidential, so it is configuration, not a credential. Tolon never logs it, and keeps each refresh token in the system keychain, never in `tolon.db`. A client in "Testing" publishing status gets refresh tokens that expire after 7 days, so people then need to sign in again.

## Where things live

```
crates/tolon-core/      events, repeat rules, ignore rules, alarm state machine; no I/O
crates/tolon-store/     SQLite storage and migrations
crates/tolon-sync/      calendar fetching (iCal, Google)
crates/tolon-platform/  per-OS window behaviour, behind one trait
crates/tolon-app/       the Tauri app: commands, windows, tray, scheduler
ui/                     React view, styled with Swiss Moss
xtask/                  the CI gate and project tooling
fixtures/ics/           calendar feeds used by the parser tests
mockups/                the screens, in mockups.figcraft (open with FigCraft)
docs/                   plan, architecture, ledger, decisions, install guide
```

The design is in [docs/architecture.md](docs/architecture.md). The features, with every scenario and acceptance criterion, are in [docs/plan.md](docs/plan.md).

## Rules every change follows

The full list is in [AGENTS.md](AGENTS.md). It applies to people and coding agents alike. The ones you will meet first:

1. **The gate is green.** `cargo xtask ci` runs formatting, clippy, all tests, the crate layering check, the scenario id check and the UI lint and tests.
2. **No `unwrap`, `expect` or `panic` in shipped code.** Clippy enforces this.
3. **New dependencies need a reason.** Say why in the pull request, and make sure `cargo deny check` passes.
4. **Private links never reach a log.** Every URL that reaches a log or an error goes through `redact_url()`.
5. **The UI uses Swiss Moss only.** Use its classes and `--sm-*` tokens, with no inline colours. The mockups are the spec.
6. **Scenarios are tested.** Every scenario id you implement (S1.1, S2.2 and so on) appears in a test name, or in [docs/manual-checklist.md](docs/manual-checklist.md) when it cannot be automated.
7. **Tolon is read only.** It never writes to a calendar and never phones home.

## Commits and pull requests

- **Conventional Commits.** The scope is the crate or `ui`, for example `feat(sync): send If-None-Match`.
- **Sign off every commit** (Developer Certificate of Origin) with `git commit -s`.
- **One task per branch.** Put the ledger row id in the pull request title, for example `[L-007] Expand RRULE with EXDATE and overrides`.
- **Describe the change.** The pull request body says which scenarios now pass, what it deliberately leaves out, and which systems you tested on.
- **Record decisions.** A change to the architecture or the plan starts with a dated entry in [docs/decisions.md](docs/decisions.md).

## Writing style

Copy in the UI, docs and release notes follows a few rules:
- Sentence case.
- Buttons say what happens, for example "Join the call".
- No emoji, no exclamation marks.
- Call the product "Tolon", never "the app".

## Working with coding agents

Tolon is set up so that coding agents can do much of the work from the plan, and they follow the same rules as everyone else. See [docs/agents/README.md](docs/agents/README.md) for how that is organised.

## License

By contributing, you agree that your work is licensed under the [Apache License 2.0](LICENSE), the same as Tolon.

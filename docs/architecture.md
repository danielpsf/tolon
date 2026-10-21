# Architecture, Tolon beta 1

Read this before writing code. `AGENTS.md` points here, and every sub-agent brief assumes it.

## Stack

| Layer | Choice | Why |
| --- | --- | --- |
| Shell | Tauri 2 (stable), one Rust binary per OS | Small binaries, native windows and tray, mobile later from the same codebase |
| Core | Rust 2021, edition pinned in `rust-toolchain.toml` | One language for logic, no business logic in the UI |
| UI | React 18 + TypeScript + Vite, styled only with Swiss Moss | Thin view layer; `swiss-moss` from `github:danielpsf/swiss-moss` as an npm dependency; no Tailwind, no CSS-in-JS |
| Storage | SQLite through `rusqlite` (bundled) with `refinery` migrations | Single file, zero setup, synchronous and simple for this data size |
| Async | `tokio` runtime owned by Tauri | Scheduler tick, HTTP fetches, OAuth loopback |
| HTTP | `reqwest` with rustls | ICS fetch, Google API |
| Calendar parsing | `ical` crate for tokenising, own expansion on top of `rrule` and `chrono-tz` | The expansion rules in F2 need control the high-level crates do not give |
| Secrets | `keyring` (macOS Keychain, Windows Credential Manager, Secret Service on Linux) | Google refresh tokens never touch SQLite |
| Tray, autostart, notifications, updater, single instance, opener | Official Tauri plugins | Avoid custom native code where a plugin exists |
| Native window level (macOS) | `objc2` + `objc2-app-kit`, only in `crates/tolon-platform` | Required to sit above full-screen apps |
| Tests | `cargo test`, `proptest`, `insta` snapshots, `tauri-driver` + WebDriver on Linux and Windows | See the test strategy in `docs/plan.md` |
| CI | GitHub Actions, the Tauri CLI in a draft-release workflow (decision 2026-10-21) | Free for a public repo |

## Repository layout

```
tolon/
  AGENTS.md                 rules for humans and agents, harness-neutral; CLAUDE.md is a symlink for Claude Code
  docs/
    plan.md                 features, scenarios, acceptance criteria, milestones
    architecture.md         this file
    install.md              per-OS install steps, embedded in release notes
    manual-checklist.md     what CI cannot test, signed off per release
    ledger.md               the running record of what is done, by whom, with commits
    decisions.md            ADRs: one entry per decision, dated
    agents/                 one brief per sub-agent role
  mockups/                  mockups.figcraft (FigCraft file, one frame per screen and theme); the UI follows it
  fixtures/ics/             real-world calendar feeds, anonymised, with expected JSON
  crates/
    tolon-core/             events, occurrences, expansion, ignore rules, alarm state machine, join-link detection; no I/O
    tolon-store/            SQLite schema, migrations, repositories
    tolon-sync/             ICS fetcher, Google Calendar client, OAuth
    tolon-platform/         OS-specific window levels, screen enumeration, sound; trait + three impls
    tolon-app/              the Tauri app: commands, windows, tray, scheduler wiring
  ui/                       Vite + React; pages: first-run, settings/*, tray-panel, interrupt
  xtask/                    cargo xtask ci | fixtures | check-scenario-ids
  .github/workflows/        ci.yml, release.yml
```

Dependency direction: `tolon-app` → `tolon-sync`, `tolon-store`, `tolon-platform` → `tolon-core`. `tolon-core` depends on nothing with I/O. `cargo xtask layers` fails the build if a lower crate imports a higher one.

## Domain model (tolon-core)

```rust
pub struct Calendar { id: CalendarId, kind: CalendarKind, name: String, enabled: bool, color: Option<String> }
pub enum CalendarKind { Ics { url: Url }, Google { account: String } }

pub struct Event {            // one VEVENT or Google event, possibly recurring
  id: EventId, calendar: CalendarId, uid: String, title: String,
  start: Start, end: End, all_day: bool, rrule: Option<Rrule>, exdates: Vec<DateTime<Utc>>,
  overrides: Vec<Override>, status: EventStatus, my_response: Option<Response>,
  attendees_count: u32, location: Option<String>, description: Option<String>,
  conference: Option<Url>, html_link: Option<Url>, updated: DateTime<Utc>,
}

pub struct Occurrence {      // one concrete start, after expansion
  key: OccurrenceKey,        // (event id, original start) stable across syncs
  event: EventId, start: DateTime<Local>, end: DateTime<Local>, title: String,
  join: JoinTarget, people: u32,
}
pub enum JoinTarget { Link(Url), EventPage(Url), None }

pub struct Alarm { occurrence: OccurrenceKey, at: DateTime<Local>, kind: AlarmKind, state: AlarmState }
pub enum AlarmKind { Lead, Start }
pub enum AlarmState { Scheduled, Firing { since }, Snoozed { until }, Dismissed, Joined, Expired }
```

Pure functions, all unit-tested:
- `expand(events, window, now) -> Vec<Occurrence>`: RRULE expansion, EXDATE, RECURRENCE-ID overrides, time zones.
- `apply_rules(occurrences, rules) -> Vec<(Occurrence, Verdict)>`: Verdict::Keep or Verdict::Ignored(reason).
- `plan(occurrences, settings, existing_alarms, now) -> Vec<Alarm>`: idempotent; keeps state for alarms already Dismissed/Joined/Snoozed.
- `tick(alarms, now, pause_until, quiet_hours) -> Vec<Transition>`: what to fire, expire, or resume.
- `detect_join(event) -> JoinTarget`: the order and hosts in F5.

## Scheduler (tolon-app)

One tokio task, `SchedulerLoop`:
1. Every 15 s, or immediately on wake (`tauri::RunEvent::Resumed` on mobile; on desktop, detect a tick gap larger than 60 s and treat it as wake) and on any settings or sync change (broadcast channel).
2. Loads occurrences for the next 7 days from the store, applies rules, plans alarms, persists alarm state, runs `tick`.
3. For each Firing transition: asks `tolon-platform` for the screens, emits `alarm:show` with the payload (title, start, end, countdown base, join target, people, and any sibling occurrences within the same minute), then shows one `interrupt` window per screen (or one): a window built the first time, a hidden one reused after that.
4. Snooze/Dismiss/Join/Already-there arrive as Tauri commands, update state, hide all interrupt windows (they are kept for the next alarm), and emit `alarm:hide`.
5. Sound: `rodio` plays the bundled file through `tolon-platform`; repeat timer lives in the loop.

Interrupt windows are created from a single `interrupt.html` route with a `?screen=` query. Settings: `decorations: false`, `fullscreen: true` on Windows and Linux (macOS covers the screen by frame size and window level instead, not `simple_fullscreen`; decision 2026-10-20, "Interrupt window deviations from mockup 01"), `always_on_top: true`, `visible_on_all_workspaces: true`, `skip_taskbar: true`, `focus: true`, `closable: false`. On macOS, every time the window is shown (built the first time or reused), `Platform::raise_above_everything(&window)` (`tolon-platform`'s macOS implementation) sets `NSWindow.level = NSScreenSaverWindowLevel + 1` and `collectionBehavior = canJoinAllSpaces | fullScreenAuxiliary | stationary`. On Linux, if the compositor reports Wayland and the window does not get focus within 500 ms, fall back to F4.5. A window is destroyed only when its screen is gone (checked when the next alarm opens); closing one on macOS leaks its `NSWindow`, so closing hides it (decision 2026-10-22, "Interrupt windows are reused"). "An interrupt is open" means a window is visible, never that one exists.

Focus goes back when the last reminder closes (L-056, decision 2026-10-22, "Closing a reminder gives focus back"): when the first interrupt window opens, before anything of Tolon's comes forward, `open_windows` calls `Platform::remember_frontmost` (macOS: the pid from `NSWorkspace.frontmostApplication`; Windows: `GetForegroundWindow` and its process; Linux: nothing). While Tolon itself is in front the call changes nothing, so a reminder that opens right after another keeps the app noted for the first. Closing only hides the windows; then the scheduler presents any waiting reminder and calls `Host::settle_focus`: with no reminder left it calls `Platform::restore_previous` (macOS `NSRunningApplication.activateWithOptions`, Windows `SetForegroundWindow` where allowed), unless a meeting of the group was joined (the `joined` flag; the browser has focus), where it calls `Platform::forget_frontmost`. A failed open forgets the noted app too.

## Storage (tolon-store)

SQLite file at the Tauri app data dir, `tolon.db`, WAL mode, foreign keys on.

```sql
create table calendars (id text primary key, kind text not null, name text not null, enabled integer not null default 1,
  url text, account text, color text, etag text, last_sync_at text, last_error text, created_at text not null);
create table events (id text primary key, calendar_id text not null references calendars(id) on delete cascade,
  uid text not null, payload text not null, updated_at text not null, unique(calendar_id, uid));          -- payload: canonical JSON of Event
create table alarms (occurrence_key text not null, kind text not null, at text not null, state text not null,
  state_at text not null, until text, start text, primary key(occurrence_key, kind));     -- start: V2, the meeting start
create table settings (key text primary key, value text not null);                                            -- JSON values
create table sync_log (id integer primary key, calendar_id text, at text not null, outcome text not null, detail text);
```

Rules: URLs with secrets are stored in `calendars.url` only; `sync_log.detail` and all `tracing` output pass through `redact_url()` which keeps scheme and host. Refresh tokens go to `keyring` under service `dev.tolon`, account = calendar id.

Migrations live in `crates/tolon-store/migrations/V{n}__{name}.sql` and run on startup; a failed migration shows a dialog and stops rather than running on a half-applied schema.

## Sync (tolon-sync)

- `IcsSource`: GET with `If-None-Match`, 30 s timeout, 5 MB cap, `webcal://` rewritten to `https://`; on 2xx parse and upsert by UID; on 304 touch `last_sync_at`; on error write `last_error` and a sync_log row. Runs every 5 minutes per calendar, staggered.
- `GoogleSource` (beta 2): `oauth2` crate, PKCE, loopback on `127.0.0.1:0`, scope `https://www.googleapis.com/auth/calendar.readonly`; `events.list` with `singleEvents=false` to keep recurrence rules, `syncToken` for increments, full resync on 410; `conferenceData.entryPoints` for the join link; `attendees[].self.responseStatus` for the decline rule.

## UI (ui/)

- Pages: `first-run`, `settings/calendars`, `settings/alarms`, `settings/quiet-hours`, `settings/appearance`, `settings/startup`, `settings/about`, `tray-panel`, `interrupt`.
- One state store (zustand) fed by Tauri events; all persistence through commands, never from the view.
- Styling only via Swiss Moss classes and `--sm-*` tokens; the interrupt uses the moss-fill variant from the mockups; the theme setting maps to `data-theme` on `<html>`.
- Copy lives in `ui/src/copy.ts`, one place, following the writing rules.
- Keyboard: the interrupt page traps focus, binds Enter, Esc, D, Space; the settings pages are fully keyboard navigable.

## Tauri commands (the whole API between UI and core)

```
calendars.list | calendars.add_ics(url) | calendars.add_google() | calendars.remove(id) | calendars.set_enabled(id, bool) | calendars.sync_now(id?)
occurrences.today | occurrences.next
alarms.snooze(key, minutes) | alarms.dismiss(key) | alarms.join(key) | alarms.already_there(key) | alarms.test
settings.get | settings.set(key, value)
app.pause(minutes) | app.resume | app.quit | app.version | app.check_updates
```

Events from core to UI: `alarm:show`, `alarm:hide`, `calendars:changed`, `occurrences:changed`, `settings:changed`, `app:paused`.

## Security and privacy

- No network calls except to the calendar URLs the person added, `github.com` for updates, and, for a Google sign-in, `accounts.google.com` (the consent page, opened in the system browser), `oauth2.googleapis.com` (token exchange, refresh, revocation) and `www.googleapis.com` (Calendar API). The OAuth redirect lands on a listener Tolon opens on `127.0.0.1` with a random port only while a sign-in waits.
- CSP in `tauri.conf.json`: `default-src 'self'; style-src 'self' 'unsafe-inline'; font-src 'self'; img-src 'self' data:` (the `img-src` part from M1, see `docs/decisions.md`); fonts are bundled, not loaded from Google Fonts.
- No telemetry, no crash reporting in beta. A local `tolon.log` with rotation, redacted.
- Updater: minisign public key in `tauri.conf.json`; the private key only in the `TAURI_SIGNING_PRIVATE_KEY` repository secret.

## Conventions

- `cargo xtask ci` = `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic`, `cargo test --workspace`, `cargo xtask layers`, `cargo xtask check-scenario-ids`, `npm run lint`, `npm run test`.
- Errors: `thiserror` in crates, `anyhow` only in `tolon-app` and `xtask`. Every error that reaches the UI carries a short, human sentence.
- Logging: `tracing`, levels info and below in release; never log event titles at info (they are personal data); debug level only.
- Commits: Conventional Commits, scope = crate or `ui`, signed off (DCO).
- Branches: `main` protected; one branch per ledger task, PR with the ledger row id in the title.

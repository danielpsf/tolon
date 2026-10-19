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

## 2026-10-19 reqwest uses the OS trust store
`reqwest` runs with `rustls-tls-native-roots`, not `rustls-tls`, because the bundled `webpki-roots` is CDLA-Permissive-2.0, which `deny.toml` does not allow. Side effect: corporate TLS-inspection CAs installed on the machine are trusted, which suits a desktop app.

## 2026-10-19 What counts as a calendar response
A feed is accepted when the response is `text/calendar`, or when the body starts with `BEGIN:VCALENDAR` whatever the content type, because several hosts serve `.ics` as `text/plain`. Redirects are followed only to `https` and for at most 10 requests; the 30 s timeout covers the body too. `redact_url()` keeps scheme and host only, dropping the port.

## 2026-10-19 Apache License 2.0 only
Tolon is licensed under Apache-2.0 alone, not the Rust-usual "MIT OR Apache-2.0". Why: the explicit patent grant protects users and contributors. `LICENSE` holds the full text; every crate inherits `license = "Apache-2.0"` from the workspace.

## 2026-10-19 Fixtures are synthetic for now
The plan asks for 12 real-world `.ics` files. The 12 in `fixtures/ics/` are synthetic, written to match the shapes Google and Outlook produce, with every address and link on `example.com`. `huge-10000-events.ics` is generated by the tests rather than committed (about 2 MB); only its snapshot is in the repo. Real feeds, anonymised, replace or join them as people contribute them (see `CONTRIBUTING.md`).

## 2026-10-19 How the parser reads a feed
- `ical` is used only to tokenise and unfold lines; Tolon reads the component structure itself, because `ical`'s parser drops the whole calendar on one bad component, which breaks AC2.5.
- RRULE `UNTIL` is normalised to the form of `DTSTART` before expansion (a date is inclusive through that day in the event zone; floating time is read in the event zone).
- Long-running daily, weekly, monthly and yearly series skip ahead to the window; a rule with `COUNT` never skips, so the count stays anchored to the real start. Sub-daily rules still walk from the start, capped at 100,000 steps.
- Cancelled events are an ignore rule that defaults on, as AC2.3 words it, alongside declined (on) and all-day (off, AC2.4).
- "Declined by the owner": the owner is the account addresses the caller passes; failing that, a calendar name containing `@` (Google's primary calendar); failing that, nobody, and nothing is dropped as declined.
- A `RECURRENCE-ID` without its series in the feed is kept as a standalone event with uid `<uid>#<recurrence-id>`, so `unique(calendar_id, uid)` holds. `Event.id` is `<calendar id>:<uid>`.
- Unknown `TZID`s fall back to floating time; IANA names and the common Windows zone names are mapped.

## 2026-10-20 Platform trait shape
- `Platform::raise_above_everything(&dyn HasWindowHandle)` takes a `raw-window-handle` window, so `tolon-platform` does not depend on `tauri`; Tauri's `WebviewWindow` implements the trait. This is the public entry the architecture's `macos::raise_above_everything(&window)` refers to.
- macOS calls need the main thread, checked with `MainThreadMarker`; off the main thread they return `PlatformError::NotMainThread`. The app calls them through `run_on_main_thread`.
- `Screen` uses logical points, top-left origin, primary screen at (0, 0), y down. `is_main` is the menu-bar screen (`NSScreen.screens[0]`), not the focused one.
- Windows and Linux return stubs until L-012.

## 2026-10-20 Tolon is translated from beta 1
Every piece of UI copy is translated, starting with en-US, en-GB, pt-BR and pt-PT; more locales follow (list to be confirmed by the maintainer). Rules:
- The core and the app never send finished sentences to the UI. Errors and statuses travel as codes (`http:404`, `timeout`, `unsupported_scheme` ...) and the view turns them into copy.
- `ui/src/copy.ts` becomes one message catalogue per locale under `ui/src/i18n/`, with en-US as the source and a typed key set, so a missing key fails the build. Dates, times and plurals use `Intl` (`DateTimeFormat`, `RelativeTimeFormat`, `PluralRules`).
- The locale follows the system language, with an override in Settings, Appearance. Unknown locales fall back by language (pt-AO to pt-PT, en-IE to en-GB), then to en-US.
- The writing rules in `AGENTS.md` apply in every language. pt-BR and pt-PT are separate catalogues, not one with tweaks; each needs a native reviewer before a release.
- Logs stay in English.

## 2026-10-20 Scenario copy lives in the UI
`docs/plan.md` quotes English wording in S1.1 ("Synced just now"), S1.3 ("Sync failed: 404"), S2.3 ("1 component skipped") and AC1.2. Since Tolon is translated, the core and the app store and send codes (`http:404`, `timeout`, `connect`, `not_calendar`, `too_large`, `network`, `skipped:N`; command errors `{code}`), and the UI renders the sentence per locale. Those scenarios are proven in two halves: a Rust test for the code, and a UI test that maps the code to the quoted en-US sentence. Other app decisions from L-029: a 304 clears a stale `last_error`; `sync_now(id)` syncs a disabled calendar; ignore-rule settings are `ignore_all_day`, `ignore_declined`, `ignore_cancelled` (JSON bools, default true); the webview may only `listen`/`unlisten` to events (capability `default.json`).

## 2026-10-20 Alarm behaviour (L-009)
- `Alarm` carries the meeting `start` as well as `at`, so expiry (start + 10 min) and the countdown work without the occurrence. It is persisted (store V2 migration, L-035) because `plan` compares it to spot a moved meeting.
- "I am already there" ends in Joined, not Dismissed: the person is in the meeting, so the "again at the start" alarm stays silent. A plain Dismiss of the lead alarm still lets the start alarm fire. AC4.4's "dismisses" is read as "closes".
- One interrupt per meeting: the start alarm expires silently while the lead alarm of the same meeting is firing or snoozed (and the mirror case).
- Pause and quiet hours hold firing back only; an alarm due meanwhile fires afterwards if start + 10 min is still open, else expires. Expiry beats snooze.
- `plan` keeps Dismissed and Joined alarms even while their meeting is filtered out, so toggling a rule cannot revive them; an Expired alarm re-arms when its meeting moved later and the window is open.
- "Events without other attendees" means no ATTENDEE line at all (`people == 0`): Outlook lists the organiser outside ATTENDEE, so one attendee is a real two-person meeting.
- Default lead time 2 minutes (from S3.1), clamped to 1 to 60; to be confirmed against mockup 04.

## 2026-10-20 Locale fallback rules (L-034)
- Portuguese: PT, AO, MZ, CV, GW, ST, TL and MO resolve to pt-PT; Brazil, bare `pt` and every other region to pt-BR.
- English: US, CA, PH and bare `en` resolve to en-US; every other English region (GB, IE, IN, ZA, AU, NZ, SG ...) to en-GB.
- Anything else falls back to en-US. The override (later a setting in Appearance) wins over the system list.
- Times: 2-digit hours ("09:05") everywhere except en-US ("9:05 AM").
- HTTP failures render "Sync failed: N"; 401, 403, 429 and 5xx add a short hint ("Sync failed: 401, link expired" per mockup 03); 404 has none, per S1.3. Unknown codes render a generic "Sync failed.".

## 2026-10-20 Join links are treated as untrusted (L-013)
A calendar feed controls every field the join link comes from, and Tolon opens that link. So:
- Only `https://` links whose host is plain ASCII letters, digits, dots and hyphens (optional numeric port) are accepted; userinfo, backslashes, IPv6 literals, control and invisible characters are refused, in every field including provider conference data.
- At most 8 KB of each field is scanned, and the link is detected once per event.
- `JoinTarget::Link` carries `host` and `recognised` (the host is a known provider: Meet, Zoom incl. zoomgov, Teams, Webex, Jitsi). Per AC5.1 any https link in the location is still used, but the interrupt (L-010) must show the host next to the join button when `recognised` is false, and never open a link without a key press or click.
- The Teams `X-MICROSOFT-SKYPETEAMSMEETINGURL` property counts as conference data.

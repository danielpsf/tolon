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

## 2026-10-20 Calendars page deviations from mockup 03 (L-008)
- A calendar link shows as scheme and host only (`https://calendar.google.com`), never path or query: iCloud, Nextcloud and many published feeds put the secret in the last segment. This matches `redact_url()` in the core. Mockup 03 shows a path and will be updated.
- Each row has a quiet Remove button with a confirm dialog and a one-line consequence. The mockup has none, but AC1.2 ("so the person can fix the link") needs a way to remove a broken one.
- The Google row is left out until beta 2 (M5); the ignore rules section is left out until their settings commands exist (L-016, M3). Neither shows a disabled stub.
- "Add calendar" and "Paste an iCal URL" both open the iCal form until Google sign-in gives the first one its own path.

## 2026-10-20 Platform contract after L-012
- `Screen` carries logical and physical coordinates. Place windows with `Screen::placement()`: logical on macOS (one global point space; tao converts physical values with the window's current screen scale, which puts a window on the wrong screen in a Retina plus 1x setup), physical on Windows (per-monitor DPI makes logical values incomparable). Linux `screens()` returns `Unsupported`; the app uses Tauri's monitors there.
- `raise_above_everything` returns `Raised { focus: Yes | No | Unknown }`. The app re-checks focus about 500 ms later and runs the F4.5 notification fallback only on a confirmed `No` or still-missing focus; `Unknown` alone is not a reason to fall back.
- `takeover_support()` is required on every platform. Wayland is detected statically and always degrades (safer reading of AC4.5 than the 500 ms probe alone); X11 is `Full`. An unknown `XDG_SESSION_TYPE` (tty from startx, unspecified) falls back to `WAYLAND_DISPLAY`, then X11. The stub and unknown OSes report `Degraded(Unsupported)`.
- Every `Platform` call runs on the main (UI) thread on every OS (macOS checks it).

## 2026-10-20 Alarm sound (L-014)
- Two sounds, soft bell (1.5 s) and chime (1 s), are synthesised by `crates/tolon-platform/sounds/src/synth.py` (stdlib only, deterministic) and embedded; they are original work under Apache-2.0. "None" is a third choice.
- `SoundPlayer` plays one shot on a dedicated audio thread; repeat-every-30-s stays in the scheduler loop (L-032). Playback uses the default output through rodio, so the system mute applies; Tolon never calls a volume API (AC6.2, checked by a source grep and a full-scale player test, plus a manual checklist line).
- A lost device is reopened on the next play; a sound that never drains ages out; a play that times out (3 s) is cancelled so it cannot start late. No device is an error the app can ignore, never a crash.

## 2026-10-20 End-to-end fetch seam: the `e2e-loopback` feature (L-037)
Production fetches https only. To prove S1.1 end to end, `tolon-sync` and `tolon-app` have a Cargo feature `e2e-loopback`, off by default. With it, plain http is accepted for `localhost`, 127.0.0.0/8 and `::1` only (checked on the parsed, normalised host), in the URL check, the add flow and redirects; every other http target stays refused. The permission exists only behind the feature: `FetchConfig::allow_http` stays private, there is no runtime flag or environment variable, and without the feature the loopback code is not compiled at all.
- Release builds are protected three ways: the feature is in no `default` list; `tolon-sync` refuses to build with it in the release profile (`compile_error!` on missing debug assertions and a `build.rs` check on `PROFILE=release`, so `CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS` cannot bypass it); and `cargo xtask check-release-features`, part of `cargo xtask ci`, scans every tracked file and fails on the feature name outside its definitions, its gating sources and the one `ci.yml` e2e build line (`tauri build --debug --features e2e-loopback`), on `all-features` anywhere, and on an artifact upload from a job built with it.
- The WebDriver test `s1_1 a valid feed is added, named from X-WR-CALNAME and synced` serves a fixture feed from a Node server bound to 127.0.0.1. S1.5 over the UI waits for a sync-now control; the Rust tests cover it.

## 2026-10-20 Interrupt window deviations from mockup 01 (L-010)
- The consequence line reads "Dismissing silences this reminder on this computer." Tolon has no accounts or sync, and a plain Dismiss of the lead alarm still lets the start alarm fire, so "this meeting on every device" would be false.
- The title uses Swiss Moss `sm-display-xl` (Inter Tight, as AC4.3 and the UI brief require), not the mockup's Inter. All lengths scale with the viewport (`100vmin / 800`) so the title reaches about 300 px on a 4K screen; very long titles step down.
- All text on the moss fill uses `--sm-on-moss`: the mockup's muted colour is 4.0:1 on light moss and fails WCAG AA.
- The details line shows a people count, not names (the core keeps only a count; names are personal data). The count may include you.
- The countdown says "min" (AC4.7 wording) and the key hint also explains Space.
- The join link never reaches the page: the payload carries only the host and whether it is a recognised provider. Joining is a Rust command, wired in L-032.
- macOS covers the screen by frame size and window level (above the menu bar and Dock), not `simple_fullscreen`, whose app-wide presentation options tao must restore; Windows and Linux use fullscreen. Coverage of the menu bar and Dock is on the macOS manual checklist. This updates the architecture's "macOS: `simple_fullscreen`".
- The bell sits on the 96 px margin. The payload has no countdown base: page and core share the system clock.

## 2026-10-20 Scheduler wiring (L-032)
- One store lock per tick, in this order: plan, upsert the plan (alarm starts follow the occurrence), prune final alarms whose start + 10 min has passed, tick, upsert the changed alarms, delete alarms of occurrences that are gone. Pruning after the upsert means a dismissed or joined alarm stays final when its meeting moves later, even if the move is seen after the old window closed. An alarm that expires in a tick stays stored until the next tick, so S3.5 can show it.
- The plan looks back one day, so a wake after a short sleep still catches a missed alarm; a sleep of more than a day expires without firing. Alarms of meetings a rule now filters out are deleted only while not final, so a dismissal survives a rule change.
- Snooze and Dismiss act on the firing alarms of the open group. Join and "already there" close every alarm of the meeting that is not final (lead and start), per "Alarm behaviour". Keys from the page are resolved against the scheduler's open group, never trusted as given.
- Settings keys read by the scheduler: `lead_minutes`, `again_at_start`, `ignore_keywords`, `ignore_no_attendees`, `sound`, `sound_repeat`. The settings page (L-016) must call `commands::notify` after a change.
- Joining opens the link from Rust through `tauri-plugin-opener`'s `open_url`; the plugin is never registered, so the webview cannot call it. The link is re-checked before opening (https, ASCII host, no Unicode format characters such as U+200B or U+202E anywhere); a refused link is an `internal` error and only `redact_url` is logged.
- Fallback notifications (F4.5) carry an id, stored with the open interrupt. An action whose id is not the current one, or that arrives without an id, does nothing, so an old notification cannot join or snooze a later meeting. Every change to the open meetings re-announces on a degraded system. Snooze is the first action. The copy is English only for now (follow-up L-038).
- The sound repeats every 30 s from the loop until the interrupt closes; the stop and a late play decide under one lock, so nothing plays after a close.
- `alarms_test` only answers the `main` window. `alarms_e2e_state` (key, kind, state, until; no titles or URLs) exists only with `e2e-loopback`; `check-release-features` allows that feature in `commands.rs` and `main.rs` on the exact `#[cfg(...)]` lines only.
- `deny.toml` ignores RUSTSEC-2026-0370, -0371 and -0372 (zbus 5 and zvariant, informational "unmaintained", no fixed version). zbus 5 reaches Linux builds through tauri-plugin-notification, notify-rust 4.18 and tauri-plugin-opener; drop the ignores when they move to zbus 6.

## 2026-10-21 Locale list for beta 1
The maintainer confirmed the languages: Portuguese, German, English, Spanish, Arabic, Turkish, Hindi, Chinese and Japanese. Tolon adds `de`, `es`, `ar`, `tr`, `hi`, `zh-CN` and `ja` to the existing en-US, en-GB, pt-BR and pt-PT catalogues (L-039).
- Language matching keeps the existing rules for en and pt. Every `de-*`, `es-*`, `tr-*`, `hi-*` and `ja-*` tag maps to its language; every `zh-*` tag, including zh-TW and zh-HK, maps to zh-CN (Simplified) until a Traditional catalogue exists; `ar-*` maps to ar.
- Arabic is right to left: the webview sets `dir="rtl"` for ar and layouts use logical CSS properties. Text that is not Latin falls back to system fonts per script; Tolon does not ship CJK fonts.
- "Tolon" is never translated. Copy rules per language: no exclamation marks or emoji; sentence case where the script has case; German uses "du", Spanish neutral international Spanish with "tú".
- Translations are drafted by an agent and checked by a second model; native review is still required before release, as for pt-BR and pt-PT.

## 2026-10-21 Hebrew joins the locale list
The maintainer added Hebrew. L-039 also ships `he`: every `he-*` tag and the legacy `iw` map to it, it is right to left like ar, and its script falls back to system fonts ("Arial Hebrew", "Segoe UI", "Noto Sans Hebrew").

## 2026-10-21 Clock, digits and keys across locales (L-039)
- Times follow each locale's own clock. 12-hour locales (en-US, ar, hi) drop the leading zero ("2:05 pm", "2:05 م"); 24-hour locales keep two digits ("09:05"). This replaces the L-034 rule "two digits everywhere except en-US".
- Digits follow the `Intl` defaults; `numberingSystem` is not pinned. Tests normalise digits before comparing, because ICU versions differ for Arabic.
- Interrupt keys match the physical key as well as the character: the D character always dismisses, and `KeyD` also dismisses when it types a non-Latin character, so the D shown in the hint works on Arabic, Hebrew, Hindi and other layouts. `KeyD` does not dismiss when it types another Latin letter (Dvorak "e", Colemak "s"), so no other key silently dismisses. Enter, Escape and Space are layout independent already.
- Sentences in key hints are joined per locale: a space for most languages, nothing after the full-width full stop in zh-CN and ja.

## 2026-10-21 Overnight M3 plan and the settings contract
The maintainer handed the project over for an unattended run and asked that the recommended option be taken wherever a choice comes up, recorded here. Choices made under that rule are marked "(overnight)".
- (overnight) M2 closes for macOS from the orchestrator's own screen checks (screenshots, recorded in the ledger); Windows and Linux manual lines stay open for a person. M3 starts in parallel, because M2's remaining work is manual only.
- (overnight) Work is split so no two agents touch the same crate at once (orchestrator brief): wave 1 runs the settings backend (tolon-core, tolon-app), the settings and polish UI (ui), start at login (tolon-platform) and the release workflow (.github, docs); wave 2 runs the tray, first run, notification copy and updater, which all touch tolon-app.
- Settings contract, shared by L-016 and L-017. Commands: `settings_get() -> SettingsDto`, `settings_set(patch: SettingsPatch) -> SettingsDto`, `sound_preview(sound)`. `SettingsDto` is `{ lead_minutes: 1..=60, again_at_start, sound: "bell" | "chime" | "none", sound_repeat, quiet: { enabled, start: "HH:MM", end: "HH:MM", weekends }, ignore: { all_day, declined, cancelled, no_attendees, keywords: [string] }, appearance: "auto" | "light" | "dark", locale: null | "<tag>", start_at_login }`. `SettingsPatch` has the same fields, all optional. The backend validates (lead 1 to 60, times as HH:MM, keywords trimmed, at most 50 of up to 64 characters each, locale one of the shipped catalogues or null), saves each change at once (AC9.1), emits `settings:changed`, and wakes the scheduler so a rule change re-plans within 15 seconds (AC8.3). Errors are codes (`invalid:<field>`). Storage keeps the existing keys (`lead_minutes`, `again_at_start`, `sound`, `sound_repeat`, `ignore_*`) and adds `quiet_*`, `appearance`, `locale`, `start_at_login`.
- (overnight) Defaults: quiet hours off, 18:00 to 09:00, weekends quiet when on; keywords "Focus, Lunch, OOO" (AC8.2); appearance auto; locale follows the system.
- (overnight) Start at login lives in `tolon-platform` behind the `Platform` trait (rule 9): macOS `SMAppService.mainApp`, Windows the `HKCU\…\Run` value, Linux an XDG autostart `.desktop` file. The switch reads the real state each time the page opens (AC9.4).

## 2026-10-21 Tolon is an accessory app on macOS (overnight)
The orchestrator's screen check found that a regular app's windows cannot appear over another app's full-screen Space, even with CanJoinAllSpaces and FullScreenAuxiliary (L-042). (overnight) Tolon runs with the Accessory activation policy on macOS: no Dock icon and no app menu, the tray icon (L-015) is the way in, and the settings window still opens and takes focus when asked. This matches F7, where the tray is the only trace after first run (AC10.2).

## 2026-10-21 Releases use the Tauri CLI, not tauri-action (L-020)
(overnight) The release workflow calls the Tauri CLI directly in a `build` job per OS and publishes from a separate `publish` job, so only that job holds `contents: write`. A `preflight` job checks the signing secrets, the tag against all version numbers, `check-release-features`, and that the key opens and matches the pubkey in `tauri.conf.json` (key id D3C5EBF2CE6A842B, the maintainer's `~/.tauri/tolon.key`). Releases are created as drafts and are not marked pre-release, because the updater reads `releases/latest/download/latest.json` and GitHub's "latest" skips pre-releases. This replaces "tauri-action for releases" in `docs/architecture.md`.

## 2026-10-21 Quiet hours and settings details (L-016)
- (overnight) Quiet hours go by the meeting start. A meeting that starts inside quiet hours never fires; its alarms stay planned so the tray can list it, and expire silently. An alarm whose own time falls inside quiet hours for a meeting that starts after them waits and fires at the meeting start, even when "again at the start" is off, so a 09:00 meeting after 18:00 to 09:00 quiet hours still gets one alarm. This replaces "hold back and fire afterwards" from "Alarm behaviour" for quiet hours only; pause (L-015) keeps that rule.
- Settings commands check the calling window in the command, as `alarms_test` does, because capability files cannot limit app commands: `settings_get` answers the main and interrupt windows (so the interrupt follows appearance and language), `settings_set`, `sound_preview` and `alarms_test` the main window only.
- Error codes: `invalid:<field>` with the dotted path for nested fields (`invalid:quiet.start`, `invalid:ignore.keywords`), `invalid:patch` for an unknown field or a patch that is not an object; a patch with any refused field saves nothing. A `null` plain field means "unchanged"; `locale: null` means "follow the system". `settings:changed` carries the full `SettingsDto` to every window.
- Storage keeps the existing values: the DTO's `sound: "bell"` is stored as `soft_bell`, and keywords stay a comma-separated string, so a keyword containing a comma is refused.
- `occurrences_today` items carry `ignored: null | "keyword" | "all_day" | "declined" | "cancelled" | "no_attendees"` for the tray's muted tag (S8.1).

## 2026-10-21 All-day events, weekends and window checks (L-016 review)
- (overnight) All-day occurrences are anchored at a local morning time instead of midnight: the end of quiet hours when quiet hours are on, else 09:00. They get one alarm at the anchor (no lead time, no "again at the start") and expire at the anchor plus 10 minutes. This stops holidays and OOO days from taking over the screen at 23:58 the night before, and makes S8.2 hold at any hour before the anchor.
- (overnight) In beta 1 the weekend for quiet hours is Saturday and Sunday in every locale; the Quiet hours page names the days. Locale weekends (Friday and Saturday for he and some ar regions, from `Intl.Locale.getWeekInfo`) are a later contract change.
- An alarm already firing when quiet hours begin stays until it is handled or expires; a snooze that ends inside quiet hours follows "nothing fires inside them".
- `occurrences_today` no longer carries join URLs: its items are `{ key, title, start, end, all_day, people, ignored, join: { kind: "link" | "event_page" | "none", host, recognised } }`, and joining goes through a main-only `occurrences_join(key)` that resolves the key in Rust and re-checks the link like `alarms_join`. This restores "the join link never reaches the page" for every window.
- Every command that changes or reveals calendar data (`calendars_*`, `occurrences_*`) answers the main window only; interrupt windows may call only the alarm actions for their own group and `settings_get`.

## 2026-10-21 Settings pages deviations from mockups 03 and 04 (L-017)
(overnight) Recorded under AGENTS.md rule 10:
- Rules is its own page; mockup 03 put "Ignore" under Calendars. The settings nav grows from 6 to 8 entries (Calendars, Alarms, Quiet hours, Rules, Appearance, Language, Startup, About).
- Mockup 04's "On which screens" and "Join button" sections are left out until they are in the settings contract.
- "Again at the start" is its own switch (AC9.5), not part of the lead-time sentence; the sound select lists sounds only, with a separate repeat switch and a "Play the sound" button.
- Quiet hours, Rules, Appearance, Language and Startup have no mockup; they follow the Alarms page layout. Mockups 03 and 04 should be updated to match.
- The language list is a native `sm-select`, not `sm-menu`, because the app does not load `swiss-moss.js`.
- The minute unit after the lead time uses each locale's plural rules.
- (overnight, L-016 re-review) A Saturday or Sunday all-day event never fires while weekends are quiet, because its anchor falls inside quiet hours; it is still listed. `occurrences_join` may open ignored or cancelled meetings: it needs a click and the link re-check still applies.

## 2026-10-21 Tray panel, pause and window rights (L-015)
- The tray panel is its own window, labelled `panel`. It may call `occurrences_today`, `occurrences_join`, `app_status` and `settings_get` (shared with the main window; `settings_get` also with interrupts), and alone may call `app_pause`, `app_resume`, `app_show_settings`, `app_hide_panel`, `app_quit`, `occurrences_snooze` and `occurrences_dismiss`. This widens "All-day events, weekends and window checks" and "Quiet hours and settings details": the panel joins the main window for the listing and join commands. No URL reaches the panel.
- Panel Snooze and Dismiss resolve the key against today's list in Rust. When that meeting's interrupt is on screen they are exactly the interrupt's buttons; otherwise they act ahead on the stored alarms: Snooze moves the alarm to N minutes after it was due (never earlier; repeated snoozes that pass start + 10 min expire it, and the panel says so), Dismiss marks every alarm of the meeting dismissed, so "again at the start" stays silent too. A final Dismiss shows its one-line consequence.
- Pause is stored as `paused_until` (RFC 3339) in the settings table, outside the settings contract (`settings_set` refuses it); it survives a restart and reads as not paused once in the past. `app_pause` takes 1 to 60 minutes. While paused nothing fires and no interrupt opens, including alarms already firing in the store; held alarms fire when the pause ends if their meeting is still within start + 10 min ("Alarm behaviour"). The scheduler wakes at the pause end.
- (overnight) "Paused until HH:MM" uses `--sm-ink`, not `--sm-signal`: a pause is a choice, not something wrong or irreversible (AGENTS.md writing rules), so mockup 02's orange is not followed.
- Close is not quit: closing the settings window hides it; only an exit with no code (the last window going away) is refused. Cmd+Q, logout and shutdown still quit, and the app flushes the store on `RunEvent::Exit`.
- Mockup 02 deviations: no "N calendars, synced" line (calendar commands stay main-only); today only, no tomorrow rows (AC7.2); Join reads "Join the call", "Join on <host>" or "Open the event" and is hidden without a link; Pause becomes "Resume alarms" while paused; ignored rows are muted with an "ignored" tag (S8.1); the panel has square corners because its window is opaque.
- The tray icon is a placeholder (ring and dot, paused: two bars) until the logo is chosen; macOS uses a template image; Linux, whose trays report no clicks, gets a one-item "Tolon" menu that opens the panel. Platform differences come from a `Platform::tray_style()` method, not from the platform name (rule 9).

## 2026-10-21 Notification copy comes from the UI (L-038)
- The fallback notification (F4.5) speaks the language of the settings window. Rust has no catalogues, so the settings window builds a `NotifyCopy` bundle from the active catalogue and sends it with the main-only command `notify_copy_set(copy)` once the first settings load fixes the language, and again whenever the language setting changes. Rust validates it, keeps it in memory and stores it in the settings table under `notify_copy` (outside the settings contract, like `paused_until`). At start Rust loads the stored bundle; with none, or one that no longer validates, it uses the built-in English. So notifications work with no window open and after a restart. The settings window is created at start (`tauri.conf.json`), so the bundle is refreshed on every launch. A system language change while Tolon keeps running is picked up on the next launch, because the page reads `navigator.languages` once.
- The bundle holds finished sentences, not templates with plural logic: `test_title`, `one_title` (with `{title}`), `many_titles` for 2 to 20 meetings, `many_over` for a bigger group (no number), `body` (with `{time}`), `snooze` (already for `SNOOZE_MINUTES`, pinned by a test that reads `ui/src/interrupt.ts`), `join` and `locale`. There is no `{minutes}` placeholder: Snooze is always `SNOOZE_MINUTES`. The two buttons and the test title reuse the interrupt strings; the new catalogue strings are `notification.body`, `notification.many` and `notification.manyOver`.
- The time is a table of the 1440 minutes of a day, formatted by `Intl` in the page (UTC clock, so no day lacks a minute). Rust indexes it with the machine-local hour and minute of the meeting start. The clock zone is the machine's IANA zone (`local_zone`), falling back to UTC. This was chosen over a pattern the page sends (`h:mm a` and the like) because Rust would then need the digit set, the day-period words and their position per locale; the table is exact for every `Intl` quirk and needs no locale data in Rust. It was chosen over an event round trip because that fails when no window is open. The cost is the payload: about 12 to 17 KB measured on ICU 78.3 (12.3 KB for de to 16.9 KB for hi), at most about 215 KB under validation. The page sends it only on a language change, and Rust writes the store only when the bundle changed.
- Validation at the trust boundary: the locale is a shipped one; every sentence is 1 to 200 characters (times 32) after cleaning; control and Unicode Cf characters are dropped (the isolates U+2068 and U+2069 stay in `one_title`, as one pair around `{title}`); placeholders are exactly `{title}` in `one_title` and `{time}` in `body` once each, and no braces anywhere else; the list lengths are exact. The `times` list must have exactly 1440 entries and `many_titles` exactly 19, checked on the raw value before it is parsed, so an oversized array is never deserialised. A refused bundle is `invalid:notify_copy` and changes nothing. `notify_copy_set` holds the store lock across the comparison, the save and the in-memory swap, so concurrent calls cannot leave the stored and the active bundle different.
- Arabic and Hebrew wrap `{title}` in FSI and PDI in `one_title`; other languages do not. The meeting title is stripped of control and format characters and of U+2028 and U+2029 in Rust before it is put in, so a feed cannot reorder text or break out of the isolates. A meeting whose title is empty after that gets the localised `notification.untitled` ("Untitled meeting") instead of empty isolates; the bundle carries it as `untitled`.
- The time shown is converted to the machine's zone in Rust from the RFC 3339 start, which also fixes the test alarm (its start is in UTC). The buttons are registered with the current labels each time a notification shows.
- Not done: the language is resolved in the page only; Rust never reads the system language.

## 2026-10-21 First run (L-018)
- First run is not stored: it shows whenever there is no calendar (Rust at start, and the page from its first calendar list). Tolon starts in the tray; the settings window is always created, hidden, so the notification copy (L-038) is refreshed on every launch, and it is shown only on first run. If the calendar list fails, Rust still shows the window so Tolon is never invisible, and the page shows the Calendars page with the error instead of first run.
- (overnight) A first calendar whose sync fails does not close first run: the page moves to the Calendars page with the error row, so a mistyped link is never left behind silently in the tray. AC10.2 applies once a calendar is added without an error.
- (overnight) Skipping: closing the window, or a quiet "Skip for now" button with the consequence "Tolon does nothing until you add a calendar.", hides first run; it shows again on the next launch until a calendar exists.
- Step 3 says "menu bar" or "system tray" from `Platform::tray_style()`, never from the OS name.
- Mockup 05 deviations: no "Sign in with Google" (beta 2), so "Paste an iCal link" is the primary button; the steps follow AC10.1 (add a calendar, try a test alarm, where Tolon lives) instead of the mockup's "Pick a lead time"; no start-at-login switch; buttons are the default 40 px; the preview reuses the interrupt's labels and the locale's clock and is not interactive.

## 2026-10-21 Updater (L-019)
- Updates contact `github.com` for `latest.json` and the GitHub release-asset host that downloads redirect to; nothing else. This refines "`github.com` for updates" in `docs/architecture.md`; `docs/install.md` ("It contacts your calendar links and GitHub") stays true.
- `tauri-plugin-updater` is used without its `zip` feature (zip 4 carries unfixed advisories, and Tauri 2 signs the installers themselves), so the release workflow publishes only signed installers to the updater: the `.nsis.zip` and `.msi.zip` fallbacks are dropped from `release_assets.py`.
- The check runs on launch and 24 hours after the last good check, looking at the clock hourly so a sleeping laptop catches up; failures are silent (debug log, no URLs) and retried. Debug and e2e builds never check unless a test sets a loopback endpoint through the `e2e-loopback` seam.
- The offer lives on the About page ("Tolon <version> is available." with Install and restart, Later, Skip this version, and a one-line restart consequence) and as one quiet line in the tray panel. No takeover, notification or window ever opens for an update. "Skip this version" is stored as `update_skipped` outside the settings contract; only a newer version prompts again. "Later" lasts until the next check.
- (L-019 review) The update check times out after 30 seconds and a download after 10 minutes, so one hung request cannot stop later checks. `requireSignedVersion` is on: the version inside the signed artifact must match `latest.json`, so a tampered manifest cannot roll Tolon back to an older signed build. On Windows the installer exits the process, so the store is settled before that exit.

## 2026-10-22 Reminder colour (L-043)
The maintainer asked that people may choose the interrupt's background from the Swiss Moss palette, with the text matched to it; the layout of mockup 01 stays the same.
- Settings contract gains `interrupt_color: "theme" | "moss" | "moss-deep" | "ink" | "paper" | "info" | "signal" | "danger"`, default `"theme"` (today's look: moss, following the light or dark appearance).
- Each choice is a pair of Swiss Moss tokens, not a free colour. (overnight, L-043 review) Only "theme" (shown as "Default") follows the light or dark appearance; every other choice is pinned to Swiss Moss's light-appearance values, so "Ink" stays near-black and "Paper" stays cream in both appearances and the colour a person picks is the colour they get. The pin reuses Swiss Moss's own light token block (`data-tokens="light"` on the interrupt root), never hex. "Moss" is therefore the light moss in both appearances, distinct from "Default". Text and controls on the interrupt (title, details, hint, buttons, consequence line) take the foreground; the primary button inverts it. Every pair must reach WCAG AA (4.5:1 for text, 3:1 for button boundaries) in both appearances; a pair that fails is not offered.
- The danger and signal colours stay allowed because the person picks them for their own screen; Tolon itself never uses them for the reminder by default (AGENTS.md writing rules).
- Fonts stay Swiss Moss (Inter Tight display, Inter text); only colour is customisable in beta 1. The Appearance page shows the swatches and a live preview, with a "Preview in dark / light" toggle for the Default choice and a test alarm button; the preview injects Swiss Moss's own token blocks at runtime, which the CSP (`style-src 'self' 'unsafe-inline'`) allows.
- An unrecognised join host is cut from its start when it is long ("…ferencing.example.com"), so the registrable domain stays visible, and the details line shows the full host. The choice applies to the interrupt and to the first-run and Appearance previews; the settings windows keep the normal theme.

## 2026-10-22 Reaching Tolon without the tray, and first-run focus (QA on a MacBook)
The orchestrator's QA pass on a 1728x1117 MacBook found that a full menu bar can push Tolon's status item behind the notch, where it can be neither seen nor clicked; with calendars, Tolon then had no visible way back. It also found the first-run window opening behind the front app, because an accessory app's window does not come forward on `show()` alone.
- Opening Tolon again while it runs (Finder, Spotlight, Launchpad or `open`; `RunEvent::Reopen` on macOS, a second launch on Windows and Linux through a single-instance guard) shows and focuses the settings window. This is the documented way back when the tray icon is hidden; `docs/install.md` says so, and the tray step of first run mentions it.
- Showing the settings window (first run, the panel's Settings, a reopen) activates the app through the platform layer, as the interrupt already does, so it comes to the front with focus.
- First run opens at the top and fits the window where it can; focus starts on "Paste an iCal link" without scrolling.
- (overnight, L-044) `RunEvent::Reopen` exists in Tauri only on macOS, so matching it needs a `target_os` condition. That condition lives in one `tolon_platform::macos_only!` macro, used once in `tolon-app/src/main.rs`; rule 9 is read as "every OS condition is written in tolon-platform", and `cargo xtask layers` enforces that no other `cfg(target_os)` appears outside it. The single-instance guard is registered on every OS: on macOS it only catches a second copy of the binary, which is harmless.
- (overnight, L-044 review) The settings page gets 760 points of height. On macOS the title bar sits inside the window's inner size, so the fitting adds the covered height (`Platform::title_bar_overlap`, 0 on Windows and Linux) to the size and the minimum, still capped by the work area; on a small screen the page gets less, never a window off screen.
- (overnight, L-044 review) `macos_only!` may appear only in `crates/tolon-app/src/main.rs`; `cargo xtask layers` fails anywhere else.
- (overnight, L-044 review) The e2e `startApp` retries a launch once, because a second copy started while the first is still closing hands over to it and exits. A retry is logged as a CI warning so a flaky launch stays visible.

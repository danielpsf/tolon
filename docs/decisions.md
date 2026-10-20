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

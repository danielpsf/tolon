# Tolon beta 1

A native desktop app that watches your calendars and takes over the screen before a meeting starts. Read only, local only, open source. Tolon does one thing: it makes sure you never miss a meeting.

Reference to learn from, not copy: In Your Face (macOS, Apple Calendar based). The screens to build are in `mockups/`.

## Outcome

A person installs Tolon on macOS, Windows or Linux, adds a calendar in under two minutes, and from then on never misses a meeting because the whole screen tells them it is starting, with one key or click to join.

## Scope of beta 1

In scope:
- Calendar sources: iCal URL subscriptions (Google "secret address", Outlook published calendars, any `.ics` URL) and Google Calendar through OAuth with a bring-your-own client ID.
- Alarms at a lead time before each meeting, optionally again at the start, with snooze, dismiss, join and "I am already there".
- A full-screen interrupt window on every screen (or one), above everything, including full-screen apps on macOS.
- Join-link detection for Google Meet, Zoom, Microsoft Teams, Webex, Jitsi, and a generic https link in the location.
- Tray / menu bar icon with the next meeting, a panel with today's meetings, pause for an hour, quiet hours.
- Settings: calendars, lead time, screens, sound, ignore rules, appearance (light, dark, auto), start at login.
- Local SQLite storage; no server, no account, no telemetry.
- Signed-by-us updates through the Tauri updater; releases for macOS (ad-hoc signed), Windows, Linux.
- Swiss Moss design system for every pixel of UI.

Out of scope for beta 1 (tracked in `docs/decisions.md` as deferred):
- Anything beyond calendars: chat, email, task trackers.
- Apple Calendar / EventKit, Microsoft Graph OAuth, CalDAV.
- Mobile.
- Writing anything to a calendar (RSVP, decline).
- Notarised macOS builds, Microsoft Store, Flathub.

## Milestones

| Milestone | Contents | Exit gate |
| --- | --- | --- |
| M0 Skeleton | Repo, Tauri 2 app boots on all three OS, Swiss Moss wired, SQLite migrations, CI green, ledger started | `cargo xtask ci` green on macOS, Windows, Linux; an empty window with the Swiss Moss paper background opens |
| M1 Calendars | iCal subscription: add, fetch, parse (incl. recurring events and time zones), cache, list today | Acceptance F1, F2 all pass; fixture-driven tests for 12 real-world `.ics` files |
| M2 Alarms | Scheduler, alarm state machine, interrupt window on every OS, snooze, dismiss, join, keyboard | F3, F4, F5, F6 pass; manual checklist on each OS signed in the ledger |
| M3 Daily use | Tray and panel, pause, quiet hours, ignore rules, sound, settings persistence, start at login | F7, F8, F9, F10 pass |
| M4 Release | Updater, CI release pipeline, install docs, first `v0.1.0-beta.1` on GitHub Releases | F11, F12 pass; a fresh machine of each OS installs from the release page by following `docs/install.md` only |
| M5 Google | Google Calendar OAuth (PKCE, loopback), token in the OS keychain, incremental sync | F13 pass; `v0.1.0-beta.2` |

A milestone is done when every scenario in its features passes in CI (or on the manual checklist where CI cannot run), the ledger row says so with the commit hash, and `docs/decisions.md` records any decision that changed on the way.

## Definition of done, for every task

- Code compiles with zero clippy warnings; no `unwrap`, `expect` or `panic` outside tests.
- Every acceptance criterion the task touches has an automated test, or a manual-checklist line when the platform makes automation impossible, with the reason written down.
- UI uses Swiss Moss classes and tokens only; no new colours, no inline hex.
- Copy follows the writing rules in `AGENTS.md` (sentence case, full sentences in headlines, no emoji, no exclamation marks).
- The ledger row for the task is updated with status, commit and any follow-ups.

## Features, scenarios and acceptance criteria

Scenario ids are stable; tests reference them by id.

### F1 Add an iCal subscription

Acceptance criteria:
- AC1.1 A valid `https://` or `webcal://` URL that returns `text/calendar` is added as a calendar with a name taken from `X-WR-CALNAME` or the host.
- AC1.2 Fetch errors are shown in the calendar row with the HTTP status or error kind, and the calendar stays listed so the person can fix the link.
- AC1.3 Private iCal URLs are stored only in the local database; they never appear in logs.
- AC1.4 Subscriptions refresh every 5 minutes, using `ETag` / `If-None-Match` when the server supports it.

```gherkin
Feature: F1 iCal subscription

  Scenario: S1.1 Add a valid Google secret iCal address
    Given the settings window is open on Calendars
    When I paste "https://calendar.google.com/calendar/ical/abc/private-123/basic.ics" and confirm
    Then a calendar named from the feed's X-WR-CALNAME appears in the list
    And its status reads "Synced just now"
    And today's events from that feed appear in the tray panel

  Scenario: S1.2 webcal scheme is accepted
    When I paste "webcal://example.com/team.ics"
    Then the subscription is stored as "https://example.com/team.ics"

  Scenario: S1.3 Unreachable link
    When I paste a URL that returns 404
    Then the calendar is added with status "Sync failed: 404"
    And no alarm is scheduled from it

  Scenario: S1.4 Secrets stay out of logs
    Given a subscription whose URL contains "private-123"
    When a sync runs and fails
    Then the log line contains the host and status but not "private-123"

  Scenario: S1.5 Conditional refresh
    Given a subscription whose server returned an ETag
    When the next refresh runs
    Then the request carries If-None-Match
    And a 304 response keeps the cached events unchanged
```

### F2 Parse and expand events

Acceptance criteria:
- AC2.1 VEVENTs with DTSTART/DTEND in UTC, floating time, or a TZID are converted to the machine's local zone correctly, including DST transitions.
- AC2.2 Recurring events (RRULE with FREQ, INTERVAL, COUNT, UNTIL, BYDAY, BYMONTHDAY), EXDATE and RECURRENCE-ID overrides expand correctly for the next 7 days.
- AC2.3 Cancelled events (STATUS:CANCELLED or METHOD:CANCEL) and events the person declined (ATTENDEE with PARTSTAT=DECLINED for the calendar owner) are excluded when the matching ignore rule is on.
- AC2.4 All-day events are excluded by default; the setting can turn them on.
- AC2.5 Parsing never panics on malformed input; bad components are skipped and counted.

```gherkin
Feature: F2 Event parsing

  Scenario Outline: S2.1 Fixture feeds parse
    Given the fixture "<file>"
    When it is parsed and expanded for the next 7 days
    Then the occurrence list matches the snapshot "<file>.expected.json"
    Examples:
      | file |
      | google-basic.ics |
      | google-recurring-weekly.ics |
      | outlook-published.ics |
      | exdate-and-override.ics |
      | tz-europe-berlin-dst.ics |
      | floating-time.ics |
      | cancelled-status.ics |
      | declined-attendee.ics |
      | all-day.ics |
      | malformed-line-folding.ics |
      | utf8-emoji-summary.ics |
      | huge-10000-events.ics |

  Scenario: S2.2 DST change
    Given an event every Monday at 09:00 Europe/Berlin
    When expanded across the last Sunday of October
    Then each occurrence is at 09:00 local on both sides of the change

  Scenario: S2.3 Malformed input
    Given a feed with one VEVENT missing DTSTART
    When parsed
    Then the other events are returned
    And the sync log records "1 component skipped"
```

### F3 Scheduler and alarm state

Acceptance criteria:
- AC3.1 For every upcoming occurrence that passes the ignore rules, one alarm is scheduled at `start - lead`, and a second at `start` when "again at the start" is on.
- AC3.2 The alarm state machine is: Scheduled → Firing → (Snoozed(until) → Firing)* → Dismissed | Joined | Expired. Expired happens 10 minutes after the start with no action.
- AC3.3 State survives a restart: a dismissed meeting does not fire again; a snoozed one fires at `until`.
- AC3.4 The scheduler ticks at least every 15 seconds and tolerates sleep: on wake, alarms that fell due during sleep fire at once if the meeting is still within its window, otherwise expire silently.
- AC3.5 Two events starting within the same minute produce one interrupt listing both.

```gherkin
Feature: F3 Scheduler

  Scenario: S3.1 Lead time
    Given lead time is 2 minutes and a meeting at 14:00
    When the clock reaches 13:58:00
    Then the interrupt fires within 15 seconds

  Scenario: S3.2 Snooze returns
    Given an interrupt is showing
    When I snooze for 5 minutes
    Then the interrupt hides
    And it shows again 5 minutes later

  Scenario: S3.3 Dismiss is final
    Given an interrupt is showing
    When I dismiss it
    And the app restarts before the meeting starts
    Then no interrupt fires for that meeting

  Scenario: S3.4 Wake from sleep inside the window
    Given a meeting at 14:00 and the machine asleep from 13:50 to 14:03
    When the machine wakes
    Then the interrupt fires immediately with "Started 3 min ago"

  Scenario: S3.5 Wake from sleep outside the window
    Given a meeting at 14:00 and the machine asleep from 13:50 to 14:30
    When the machine wakes
    Then no interrupt fires and the alarm is marked Expired

  Scenario: S3.6 Overlapping meetings
    Given two meetings both at 14:00
    When the alarm fires
    Then one interrupt shows both titles with their own Join buttons
```

### F4 The interrupt window

Acceptance criteria:
- AC4.1 The window covers the whole screen, has no decorations, is above every other window including full-screen apps (macOS: window level above the screen saver level, joins all Spaces; Windows: topmost; Linux X11: override-redirect or always-on-top; Wayland: best effort plus the F4.5 fallback).
- AC4.2 It appears on every connected screen by default; the setting can limit it to the screen with the cursor or the main screen.
- AC4.3 Layout and copy follow `mockups/01-interrupt-*.svg`: time and countdown on top, title in the display-xl style, details, actions: Join (primary), Snooze 5 min, Dismiss, and the keyboard hint line. Light and dark themes.
- AC4.4 The window takes keyboard focus on open: Enter joins, Esc snoozes, D dismisses, Space marks "I am already there" (which dismisses without opening a link).
- AC4.5 When the window system refuses an always-on-top full-screen window (typically Wayland), the app shows a system notification with Join and Snooze actions, plays the alarm sound, and logs the degraded mode once.
- AC4.6 Dismiss, snooze or join on one screen closes the interrupt on all screens.
- AC4.7 The countdown updates every second; after the start it reads "Started N min ago".

```gherkin
Feature: F4 Interrupt window

  Scenario: S4.1 Covers a full-screen app on macOS
    Given a full-screen Keynote presentation is in front
    When an alarm fires
    Then the interrupt is visible over the presentation on that screen

  Scenario: S4.2 Every screen
    Given two monitors and "Every screen" selected
    When an alarm fires
    Then both monitors show the interrupt
    And pressing Esc on either hides both

  Scenario: S4.3 Keyboard first
    When an alarm fires
    Then the interrupt has keyboard focus
    And Enter opens the join link and closes the interrupt

  Scenario: S4.4 Wayland fallback
    Given a Wayland session that refuses always-on-top
    When an alarm fires
    Then a notification with "Join" and "Snooze" actions appears
    And the alarm sound plays
```

### F5 Join links

Acceptance criteria:
- AC5.1 The join URL is found, in this order: conference data from the provider, then the first recognised link in location, then in description; recognised hosts: meet.google.com, zoom.us (incl. subdomains, `/j/`), teams.microsoft.com and teams.live.com, webex.com, meet.jit.si, and any `https://` link in the location field.
- AC5.2 When no link exists, the primary button reads "Open the event" and opens the event's HTML link if the source has one; otherwise the primary is "I am already there".
- AC5.3 Join opens the link in the default browser, or in the native Zoom / Teams app when the setting is on and the app is installed.

```gherkin
Feature: F5 Join links

  Scenario Outline: S5.1 Recognised links
    Given an event whose <field> contains "<url>"
    Then the join button opens "<url>"
    Examples:
      | field | url |
      | location | https://meet.google.com/abc-defg-hij |
      | description | https://us02web.zoom.us/j/123456789?pwd=x |
      | location | https://teams.microsoft.com/l/meetup-join/19%3Ameeting |
      | description | https://company.webex.com/meet/ana |

  Scenario: S5.2 No link
    Given an event with no link anywhere
    Then the primary button reads "I am already there"
```

### F6 Alarm sound

Acceptance criteria:
- AC6.1 Three bundled sounds (soft bell, chime, none) and "repeat every 30 seconds until dismissed".
- AC6.2 Sound respects the system mute; it never changes the system volume.
- AC6.3 A "Play" button in settings previews the sound.

### F7 Tray and panel

Acceptance criteria:
- AC7.1 A tray / menu bar icon shows the Tolon mark; its tooltip and the panel's first line show the next meeting and the minutes until it.
- AC7.2 The panel (see `mockups/02-tray-panel-*.svg`) lists today's remaining meetings, the next with Join / Snooze / Dismiss, and offers Pause for 1 hour, Settings, Quit.
- AC7.3 While paused, no interrupt fires; the icon shows a paused state and the panel shows "Paused until HH:MM"; it resumes automatically.
- AC7.4 Quit from the panel stops the scheduler; closing the settings window does not quit.

```gherkin
Feature: F7 Tray

  Scenario: S7.1 Pause for an hour
    Given a meeting in 20 minutes
    When I choose Pause for 1 hour
    Then no interrupt fires for that meeting
    And the panel reads "Paused until <now + 1h>"

  Scenario: S7.2 Close is not quit
    When I close the settings window
    Then the tray icon stays and alarms keep firing
```

### F8 Quiet hours and ignore rules

Acceptance criteria:
- AC8.1 Quiet hours: a daily window and a weekend switch; inside them nothing fires, but the tray still shows what is coming.
- AC8.2 Ignore rules: all-day events (default on), declined events (default on), events without other attendees (default off), title keywords (default "Focus, Lunch, OOO", case-insensitive, comma separated).
- AC8.3 Rule changes re-plan alarms within 15 seconds without a restart.

```gherkin
Feature: F8 Rules

  Scenario: S8.1 Keyword ignore
    Given the keyword list contains "Focus"
    And an event titled "Focus time"
    Then no alarm is scheduled for it
    And it is listed in the panel with a muted "ignored" tag

  Scenario: S8.2 Turning a rule off re-plans
    Given "Ignore all-day events" is on and an all-day event today
    When I turn the rule off
    Then an alarm for that event appears within 15 seconds
```

### F9 Settings and persistence

Acceptance criteria:
- AC9.1 Every setting saves on change (no Save button) and survives a restart.
- AC9.2 The settings window follows `mockups/03-*.svg` and `04-*.svg`: left nav, one topic per page, Swiss Moss components.
- AC9.3 Appearance: light, dark, auto; the interrupt follows it.
- AC9.4 Start at login works on all three OS and the switch reflects the real state.
- AC9.5 Lead time accepts 1 to 60 minutes; "again at the start" is a separate switch.

### F10 First run

Acceptance criteria:
- AC10.1 On first launch the window in `mockups/05-first-run-*.svg` appears with three steps; "Try a test alarm" fires a real interrupt with a sample meeting.
- AC10.2 After one calendar is added the first-run window closes and the tray icon is the only trace.

### F11 Updates

Acceptance criteria:
- AC11.1 The app checks GitHub Releases for updates on launch and daily, verifies the minisign signature, and asks before installing. "Skip this version" is remembered.
- AC11.2 Updating on macOS does not require the Open Anyway dance again (verify; if it does, document it in `docs/install.md`).

### F12 Release pipeline and install

Acceptance criteria:
- AC12.1 Pushing a tag `v*` builds and publishes: macOS universal `.dmg` (ad-hoc signed), Windows `.msi` and `.exe` (NSIS), Linux `.AppImage` and `.deb`, plus `latest.json` for the updater and `SHA256SUMS`.
- AC12.2 The release notes embed the install instructions from `docs/install.md`.
- AC12.3 A person following `docs/install.md` on a clean machine of each OS reaches the first-run window without any other help. Record who tested which OS in the ledger.

### F13 Google Calendar (beta 2)

Acceptance criteria:
- AC13.1 Sign in uses OAuth 2.0 with PKCE and a loopback redirect; the client ID is configurable (default is a project-run Internal-less public client for testing; people may paste their own).
- AC13.2 Scope is `calendar.readonly` only.
- AC13.3 Refresh tokens live in the OS keychain (`keyring`), never in SQLite or logs.
- AC13.4 Sync uses `syncToken` for incremental updates and falls back to a full sync on 410.
- AC13.5 Declined events are detected from the owner's `responseStatus`; conference links from `conferenceData`.

## Test strategy

- Rust unit tests for parsing, expansion, scheduling and link detection, with the fixture feeds under `fixtures/ics/`. Property tests (proptest) for the scheduler: for any set of events and any sequence of ticks, every alarm fires at most once per state transition.
- SQLite migration tests: apply all migrations to an empty database, then to a database from the previous release.
- Frontend: Vitest for the small amount of view logic; Playwright component tests are not needed for beta.
- End to end: `tauri-driver` with WebDriver on Linux and Windows in CI for the settings flows (S1.x, S8.x, S9.x). macOS has no WebDriver support for Tauri; the macOS scenarios (S4.1, S4.2, S4.3, install) run from `docs/manual-checklist.md` and are signed off in the ledger.
- Every scenario id above appears in at least one test name or checklist line. A CI step greps for orphan ids.

## Risks called out

- Full-screen-over-everything on macOS needs native window level tweaks beyond Tauri's defaults; budget a small Objective-C shim through the `objc2` crate. Try the Tauri `set_always_on_top` + `set_visible_on_all_workspaces` path first and only then the shim.
- Wayland will not allow true takeover; the fallback is a feature, not a bug, and the docs say so.
- Ad-hoc signed macOS builds show the Gatekeeper warning on every fresh install; this is accepted for the beta.
- Google's OAuth app verification: with a public External client in testing mode, refresh tokens expire after 7 days. Beta 2 documents this and recommends a bring-your-own Internal client for Workspace users.

//! Shared by the integration tests: the fixed clock, fixture loading and the huge generator.
#![allow(dead_code)]

use chrono::{DateTime, Duration, TimeZone, Utc};
use chrono_tz::Tz;
use std::error::Error;
use std::fmt::Write;
use std::path::PathBuf;
use tolon_core::{parse_ics, CalendarId, ParseOptions, ParsedCalendar};

pub type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// The clock every snapshot is taken at: Thursday 22 Oct 2026, 10:00 in Berlin. The next seven
/// days contain the end of summer time (Sunday 25 Oct, 03:00 to 02:00 in Berlin).
pub const NOW: (i32, u32, u32, u32, u32) = (2026, 10, 22, 8, 0);
pub const LOCAL: Tz = Tz::Europe__Berlin;
pub const HUGE: &str = "huge-10000-events.ics";

pub fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> TestResult<DateTime<Utc>> {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, 0)
        .single()
        .ok_or_else(|| "invalid test instant".into())
}

pub fn now() -> TestResult<DateTime<Utc>> {
    utc(NOW.0, NOW.1, NOW.2, NOW.3, NOW.4)
}

pub fn cal() -> CalendarId {
    CalendarId("fixture".to_string())
}

pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ics")
}

/// The text of a fixture. The huge feed is generated, not committed (see fixtures/ics/README.md).
pub fn fixture_text(name: &str) -> TestResult<String> {
    if name == HUGE {
        return huge_ics();
    }
    let bytes = std::fs::read(fixtures_dir().join(name))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn parse_fixture(name: &str) -> TestResult<ParsedCalendar> {
    Ok(parse_ics(
        &fixture_text(name)?,
        &cal(),
        &ParseOptions::default(),
    ))
}

/// 10,000 events, one every 3 hours from 1 Jun 2026; every 500th repeats weekly ten times.
pub fn huge_ics() -> TestResult<String> {
    let base = utc(2026, 6, 1, 0, 0)?;
    let mut s = String::from("BEGIN:VCALENDAR\nVERSION:2.0\nPRODID:-//Example Corp//Generated//EN\nX-WR-CALNAME:Huge feed\n");
    for i in 0..10_000i64 {
        let start = base + Duration::hours(3 * i);
        let end = start + Duration::minutes(30);
        writeln!(
            s,
            "BEGIN:VEVENT\nUID:huge-{i:05}@example.com\nDTSTART:{}\nDTEND:{}\nSUMMARY:Event {i:05}\nSTATUS:CONFIRMED",
            start.format("%Y%m%dT%H%M%SZ"),
            end.format("%Y%m%dT%H%M%SZ"),
        )?;
        if i % 500 == 0 {
            s.push_str("RRULE:FREQ=WEEKLY;COUNT=10\n");
        }
        s.push_str("END:VEVENT\n");
    }
    s.push_str("END:VCALENDAR\n");
    Ok(s)
}

/// A time on the test day (Thursday 22 Oct 2026), in UTC.
pub fn at(h: u32, m: u32) -> TestResult<DateTime<Utc>> {
    utc(2026, 10, 22, h, m)
}

/// A one-hour meeting with its own join link, starting at `start`.
pub fn occ(title: &str, start: DateTime<Utc>, people: u32) -> tolon_core::Occurrence {
    use tolon_core::{EventId, JoinTarget, OccurrenceKey};
    tolon_core::Occurrence {
        key: OccurrenceKey {
            event: EventId(format!("cal:{title}")),
            original_start: start,
        },
        event: EventId(format!("cal:{title}")),
        start: start.with_timezone(&LOCAL),
        end: (start + Duration::hours(1)).with_timezone(&LOCAL),
        title: title.to_string(),
        all_day: false,
        join: JoinTarget::Link(format!("https://example.com/{title}")),
        people,
    }
}

/// The 12 files of S2.1, in plan order.
pub const FIXTURE_FILES: &[&str] = &[
    "google-basic.ics",
    "google-recurring-weekly.ics",
    "outlook-published.ics",
    "exdate-and-override.ics",
    "tz-europe-berlin-dst.ics",
    "floating-time.ics",
    "cancelled-status.ics",
    "declined-attendee.ics",
    "all-day.ics",
    "malformed-line-folding.ics",
    "utf8-emoji-summary.ics",
    HUGE,
];

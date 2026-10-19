//! AC2.2 at scale: a series that started years ago must not be walked from its first day on
//! every expansion (the scheduler expands every 15 seconds).

mod common;

use chrono::{DateTime, NaiveDate, Utc};
use common::{cal, now, utc, TestResult, LOCAL};
use std::time::{Duration as Wall, Instant};
use tolon_core::{expand, expand_window, parse_ics, Event, ExpandOptions, ParseOptions};

fn series(dtstart: &str, rule: &str) -> Vec<Event> {
    let feed = format!(
        "BEGIN:VCALENDAR\nVERSION:2.0\nBEGIN:VEVENT\nUID:p@example.com\nSUMMARY:p\n\
DTSTART;TZID=Europe/Berlin:{dtstart}\nDTEND;TZID=Europe/Berlin:{dtstart}\nRRULE:{rule}\nEND:VEVENT\nEND:VCALENDAR\n"
    );
    parse_ics(&feed, &cal(), &ParseOptions::default()).events
}

fn starts(events: &[Event], from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<String> {
    expand_window(events, from, to, LOCAL, &ExpandOptions::default())
        .iter()
        .map(|o| o.start.format("%Y-%m-%d %H:%M %:z").to_string())
        .collect()
}

#[test]
fn ac2_2_weekly_series_started_in_2000_expands_in_under_a_second() -> TestResult {
    let events = series("20000103T090000", "FREQ=WEEKLY;BYDAY=MO");
    let t = Instant::now();
    let occ = expand(&events, now()?, LOCAL, &ExpandOptions::default());
    let took = t.elapsed();
    println!("weekly since 2000: {took:?}");
    assert_eq!(occ.len(), 1);
    assert_eq!(
        occ[0].start.format("%Y-%m-%d %H:%M %:z").to_string(),
        "2026-10-26 09:00 +01:00"
    );
    assert!(took < Wall::from_secs(1), "took {took:?}");
    Ok(())
}

#[test]
fn ac2_2_ten_thousand_series_started_in_2015_expand_in_time() -> TestResult {
    let one = series("20150105T090000", "FREQ=DAILY");
    let events: Vec<Event> = (0..10_000)
        .map(|i| {
            let mut e = one[0].clone();
            e.uid = format!("p{i}");
            e.id = tolon_core::EventId(format!("fixture:p{i}"));
            e
        })
        .collect();
    let t = Instant::now();
    let occ = expand(&events, now()?, LOCAL, &ExpandOptions::default());
    let took = t.elapsed();
    println!("10,000 daily series since 2015: {took:?}");
    assert_eq!(occ.len(), 10_000 * 6);
    assert!(took < Wall::from_secs(10), "took {took:?}");
    Ok(())
}

#[test]
fn ac2_2_skipping_ahead_gives_the_same_instances_as_walking_from_the_start() -> TestResult {
    // A window starting before DTSTART is never skipped, so it is the reference walk.
    let rules = [
        ("20000103T090000", "FREQ=DAILY"),
        ("20000103T090000", "FREQ=DAILY;INTERVAL=3"),
        ("20000103T093000", "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,TH"),
        ("20000103T093000", "FREQ=WEEKLY;BYDAY=SU,MO;WKST=SU"),
        ("20000131T090000", "FREQ=MONTHLY"),
        ("20000131T090000", "FREQ=MONTHLY;INTERVAL=2"),
        ("20000103T090000", "FREQ=MONTHLY;BYDAY=-1FR"),
        ("20000103T090000", "FREQ=MONTHLY;BYDAY=MO;BYSETPOS=2"),
        ("20000229T090000", "FREQ=YEARLY"),
        ("20000229T090000", "FREQ=YEARLY;INTERVAL=4"),
        ("20000103T020000", "FREQ=DAILY"),
        ("20000103T090000", "FREQ=DAILY;UNTIL=20271231"),
    ];
    let windows = [
        (utc(2026, 10, 22, 8, 0)?, utc(2026, 10, 29, 8, 0)?),
        (utc(2024, 2, 20, 0, 0)?, utc(2024, 3, 5, 0, 0)?),
        (utc(2026, 3, 25, 0, 0)?, utc(2026, 4, 2, 0, 0)?),
        (utc(2026, 12, 25, 0, 0)?, utc(2027, 1, 20, 0, 0)?),
    ];
    for (dtstart, rule) in rules {
        let events = series(dtstart, rule);
        for (from, to) in windows {
            let reference: Vec<String> = starts(&events, utc(1999, 1, 1, 0, 0)?, to)
                .into_iter()
                .filter(|s| in_window(s, from, to))
                .collect();
            assert_eq!(
                starts(&events, from, to),
                reference,
                "{dtstart} {rule} {from}"
            );
        }
    }
    Ok(())
}

/// Whether a "%Y-%m-%d %H:%M %:z" start lies in `[from, to)`, or runs into it as a 0-length event.
fn in_window(start: &str, from: DateTime<Utc>, to: DateTime<Utc>) -> bool {
    DateTime::parse_from_str(start, "%Y-%m-%d %H:%M %:z")
        .map(|t| t.with_timezone(&Utc))
        .is_ok_and(|t| t >= from && t < to)
}

#[test]
fn ac2_2_count_is_counted_from_the_original_start() -> TestResult {
    let (from, to) = (utc(2026, 10, 22, 0, 0)?, utc(2026, 10, 30, 0, 0)?);
    // 3 weekly instances from 1 Oct: the series is over by the window.
    let weekly = series("20261001T090000", "FREQ=WEEKLY;COUNT=3");
    assert!(starts(&weekly, from, to).is_empty());
    // 40 daily from 20 Sep end on 29 Oct: 22 to 29 Oct all count. With 39 the last one drops.
    let forty = series("20260920T090000", "FREQ=DAILY;COUNT=40");
    assert_eq!(starts(&forty, from, to).len(), 8);
    let thirty_nine = series("20260920T090000", "FREQ=DAILY;COUNT=39");
    assert_eq!(starts(&thirty_nine, from, to).len(), 7);
    // 26 years in, a count that ends the series on 24 Oct 2026 leaves exactly 22, 23 and 24 Oct.
    let first = NaiveDate::from_ymd_opt(2000, 1, 3).ok_or("date")?;
    let last = NaiveDate::from_ymd_opt(2026, 10, 24).ok_or("date")?;
    let count = (last - first).num_days() + 1;
    let events = series("20000103T090000", &format!("FREQ=DAILY;COUNT={count}"));
    assert_eq!(
        starts(&events, from, to),
        [
            "2026-10-22 09:00 +02:00",
            "2026-10-23 09:00 +02:00",
            "2026-10-24 09:00 +02:00"
        ]
    );
    Ok(())
}

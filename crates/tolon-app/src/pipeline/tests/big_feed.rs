//! L-066: a work calendar with years of history (the owner's was 13.2 MB, 6,359 events, 957 RRULEs)
//! syncs in time, keeps what can still matter and drops what cannot. The feed is generated here, so
//! no big file sits in git.

use std::time::{Duration as Wall, Instant};

use chrono::Duration as Span;
use tolon_core::{plan, AlarmSettings, EventTime};

use super::*;
use crate::scheduler;

const SINGLES: i64 = 11_500;
const MASTERS: usize = 600;
const MIN_BYTES: usize = 15_000_000;
/// About 10 times what a debug build measures here (under 1 s to add, under 1 s to re-sync), for slow CI runners.
const BUDGET: Wall = Wall::from_secs(10);

fn utc(y: i32, mo: u32, d: u32, h: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, mo, d, h, 0, 0)
        .single()
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

fn stamp(t: DateTime<Utc>) -> String {
    t.format("%Y%m%dT%H%M%SZ").to_string()
}

/// About 1 KB of folded text, like a meeting description with an agenda and a dial-in block.
fn description() -> String {
    let words = "Agenda review of the quarterly plan, open questions, decisions and owners. ";
    let mut text = String::new();
    while text.len() < 1000 {
        text.push_str(words);
    }
    let mut out = String::from("DESCRIPTION:");
    for (i, ch) in text.chars().enumerate() {
        if i > 0 && i % 60 == 0 {
            out.push_str("\r\n ");
        }
        out.push(ch);
    }
    out.push_str("\r\n");
    out
}

const ATTENDEES: &str = "ATTENDEE:mailto:a@example.com\r\nATTENDEE:mailto:b@example.com\r\n";

fn event(uid: &str, start: DateTime<Utc>, hours: i64, extra: &str) -> String {
    let end = start + Span::hours(hours);
    vevent(
        uid,
        uid,
        &format!(
            "DTSTART:{}\r\nDTEND:{}\r\n{ATTENDEES}{extra}",
            stamp(start),
            stamp(end)
        ),
    )
}

/// What the generator put in, for the assertions.
struct Generated {
    body: String,
    /// Uids that must be in the store, and the ones that must not.
    kept_singles: Vec<&'static str>,
    dropped_singles: Vec<&'static str>,
}

fn generate() -> Generated {
    let desc = description();
    let mut body = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-WR-CALNAME:Work\r\n");

    // Thousands of past single events, 2021 to mid October 2026: all of them end before any cutoff.
    let first = utc(2021, 1, 4, 9);
    let last = utc(2026, 10, 17, 9);
    let step = (last - first).num_minutes() / SINGLES;
    for i in 0..SINGLES {
        let start = first + Span::minutes(step * i);
        body.push_str(&event(&format!("past-{i}"), start, 1, &desc));
    }

    // Recurring masters from 2021 with an old override, and for the daily ones an EXDATE and an
    // override inside the next seven days. A third of the series ended in 2022.
    for i in 0..MASTERS {
        let weekday = i64::try_from(i % 7).unwrap_or(0);
        let hour = u32::try_from(i % 8).unwrap_or(0);
        let start = utc(2021, 1, 4, 9 + hour) + Span::days(weekday);
        let (rule, daily) = match i % 3 {
            0 => ("FREQ=DAILY", true),
            1 => ("FREQ=WEEKLY", false),
            _ => ("FREQ=DAILY;UNTIL=20220101T000000Z", false),
        };
        let uid = format!("series-{i}");
        let exdate = if daily {
            format!("EXDATE:{}\r\n", stamp(utc(2026, 10, 22, 9 + hour)))
        } else {
            String::new()
        };
        body.push_str(&event(
            &uid,
            start,
            1,
            &format!("RRULE:{rule}\r\n{exdate}{desc}"),
        ));
        let moved = |at: DateTime<Utc>| {
            vevent(
                &uid,
                &uid,
                &format!(
                    "RECURRENCE-ID:{}\r\nDTSTART:{}\r\nDTEND:{}\r\n{ATTENDEES}",
                    stamp(at),
                    stamp(at + Span::hours(1)),
                    stamp(at + Span::hours(2)),
                ),
            )
        };
        body.push_str(&moved(start));
        if daily {
            body.push_str(&moved(utc(2026, 10, 23, 9 + hour)));
        }
    }

    // Around the cutoff, today and far ahead. Whatever the machine's zone, an event that ended on
    // 18 Oct 09:00 UTC is before the cutoff and one that ended on 19 Oct 09:00 UTC is after it.
    let dropped = vec!["just-before-cutoff"];
    body.push_str(&event(dropped[0], utc(2026, 10, 18, 8), 1, ""));
    let kept = vec![
        "just-after-cutoff",
        "today",
        "tomorrow",
        "far-2027-03",
        "far-2027-09",
        "far-2028-01",
        "far-2030-05",
    ];
    for (uid, at) in [
        (kept[0], utc(2026, 10, 19, 8)),
        (kept[1], utc(2026, 10, 20, 14)),
        (kept[2], utc(2026, 10, 21, 10)),
        (kept[3], utc(2027, 3, 1, 9)),
        (kept[4], utc(2027, 9, 15, 9)),
        (kept[5], utc(2028, 1, 10, 9)),
        (kept[6], utc(2030, 5, 5, 9)),
    ] {
        body.push_str(&event(uid, at, 1, ""));
    }
    body.push_str("END:VCALENDAR\r\n");
    Generated {
        body,
        kept_singles: kept,
        dropped_singles: [dropped, vec!["past-0", "past-5750", "past-11499"]].concat(),
    }
}

fn keys(occ: &[tolon_core::Occurrence]) -> Vec<String> {
    // The key starts with the calendar's id, which differs between the two stores.
    let mut keys: Vec<String> = occ
        .iter()
        .map(|o| o.key.to_string())
        .map(|k| {
            k.split_once(':')
                .map_or(k.clone(), |(_, rest)| rest.to_owned())
        })
        .collect();
    keys.sort_unstable();
    keys
}

#[tokio::test]
async fn l066_a_15_mb_feed_with_years_of_history_syncs_in_time_and_keeps_what_matters() -> TestResult
{
    let g = generate();
    let megabytes = g.body.len() as f64 / 1e6;
    assert!(g.body.len() >= MIN_BYTES, "feed is {megabytes:.1} MB");
    // The generated text is itself a valid feed that Tolon refuses today: it is over the old 5 MB cap.
    assert!(g.body.len() > 5 * 1024 * 1024);

    let zone = Tz::UTC;
    let at = now();
    let db = db()?;
    let t = Instant::now();
    let dto = add_ics(
        &db,
        &Fake::new(vec![modified(g.body.clone(), None)]),
        SECRET_URL,
        at,
    )
    .await?;
    let parsed_and_stored = t.elapsed();
    assert_eq!(dto.last_error, None);
    assert_eq!(dto.name, "Work");

    let t = Instant::now();
    let inputs = scheduler::load_inputs(&db, zone, at)?;
    let occurrences = scheduler::occurrences(&inputs, at, zone);
    let alarms = plan(&occurrences, &AlarmSettings::default(), &[], at);
    let loaded_and_planned = t.elapsed();

    // A second sync of the same body (the 5-minute refresh) is the steady state.
    let t = Instant::now();
    let id = dto.id.clone();
    sync_one(
        &db,
        &Fake::new(vec![modified(g.body.clone(), Some("\"v2\""))]),
        &id,
        at,
    )
    .await?;
    let resync = t.elapsed();

    println!(
        "feed {megabytes:.1} MB: add (parse + store) {parsed_and_stored:?}, \
         load + expand + plan {loaded_and_planned:?} ({} occurrences, {} alarms), re-sync {resync:?}",
        occurrences.len(),
        alarms.len()
    );
    assert!(
        parsed_and_stored + loaded_and_planned < BUDGET,
        "{parsed_and_stored:?} {loaded_and_planned:?}"
    );
    assert!(resync < BUDGET, "{resync:?}");

    // What is stored: every master with all its overrides, the recent and far-future singles, and
    // no single event from before the cutoff.
    let stored = decode_events(lock(&db).events().list_by_calendar(&dto.id)?);
    let uids: Vec<&str> = stored.iter().map(|e| e.uid.as_str()).collect();
    assert_eq!(
        stored.len(),
        MASTERS + g.kept_singles.len(),
        "{} stored",
        stored.len()
    );
    for uid in &g.kept_singles {
        assert!(uids.contains(uid), "{uid} was dropped");
    }
    for uid in &g.dropped_singles {
        assert!(!uids.contains(uid), "{uid} was kept");
    }
    for e in stored.iter().filter(|e| e.uid.starts_with("series-")) {
        let i: usize = e.uid["series-".len()..].parse()?;
        assert_eq!(
            e.overrides.len(),
            if i.is_multiple_of(3) { 2 } else { 1 },
            "{}",
            e.uid
        );
        assert_eq!(
            e.exdates.len(),
            usize::from(i.is_multiple_of(3)),
            "{}",
            e.uid
        );
        assert!(e.rrule.is_some());
        assert!(matches!(e.start, EventTime::Utc { .. }));
    }

    // Dropping changes nothing the person can see: the next seven days (and the look-back) expand
    // to the same occurrences as from a store that kept every event.
    let everything = parse_ics(
        &g.body,
        &CalendarId(dto.id.clone()),
        &ParseOptions::default(),
    );
    // Kept ones, the single just before the cutoff, and every past single.
    assert_eq!(
        everything.events.len(),
        MASTERS + g.kept_singles.len() + 1 + usize::try_from(SINGLES)?
    );
    let reference = db_with(everything.events)?;
    let ref_inputs = scheduler::load_inputs(&reference, zone, at)?;
    let ref_occurrences = scheduler::occurrences(&ref_inputs, at, zone);
    assert_eq!(keys(&occurrences), keys(&ref_occurrences));
    // Sanity: the window is not empty and holds today's event and the override inside it.
    assert!(
        occurrences.len() > 1000,
        "{} occurrences",
        occurrences.len()
    );
    assert!(occurrences.iter().any(|o| o.title == "today"));
    assert!(occurrences.iter().any(|o| o.title == "just-after-cutoff"));
    assert!(!occurrences.iter().any(|o| o.title == "just-before-cutoff"));
    Ok(())
}

/// A second store that holds the given events as they are, to compare against.
fn db_with(events: Vec<tolon_core::Event>) -> anyhow::Result<Db> {
    let reference = db()?;
    {
        let store = lock(&reference);
        let cal = store.calendars().insert(
            "ics",
            "Reference",
            Some(SECRET_URL),
            true,
            &now().to_rfc3339(),
        )?;
        let feed = feed_events(
            events
                .into_iter()
                .map(|mut e| {
                    e.calendar = CalendarId(cal.id.clone());
                    e.id = tolon_core::EventId(format!("{}:{}", cal.id, e.uid));
                    e
                })
                .collect(),
        )?;
        store
            .events()
            .replace_for_calendar(&cal.id, &feed, &now().to_rfc3339())?;
    }
    Ok(reference)
}

#[test]
fn l066_the_cutoff_is_local_midnight_yesterday_or_the_lookback_if_that_is_earlier() {
    let at = now(); // Tue 20 Oct 2026, 08:00 UTC
                    // Berlin is UTC+2 then: yesterday 00:00 is 18 Oct 22:00 UTC, before now - 24 h (19 Oct 08:00 UTC).
    assert_eq!(keep_from(at, Tz::Europe__Berlin), utc(2026, 10, 18, 22));
    assert_eq!(keep_from(at, Tz::UTC), utc(2026, 10, 19, 0));
    // Auckland is UTC+13: yesterday 00:00 is 18 Oct 11:00 UTC.
    assert_eq!(keep_from(at, Tz::Pacific__Auckland), utc(2026, 10, 18, 11));
    // Just after midnight on the day after a 23 h day, now - 24 h reaches further back than local
    // midnight yesterday, so the look-back wins: the scheduler can still plan from it.
    let spring = Utc
        .with_ymd_and_hms(2026, 3, 29, 22, 30, 0)
        .single()
        .unwrap_or(at); // 00:30 in Berlin, after the 29 Mar change
    assert_eq!(
        keep_from(spring, Tz::Europe__Berlin),
        spring - scheduler::LOOKBACK
    );
}

fn tiny_feed(events: usize) -> String {
    let mut body = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-WR-CALNAME:Tiny\r\n");
    for i in 0..events {
        body.push_str(&format!(
            "BEGIN:VEVENT\r\nUID:u{i}\r\nDTSTART:20270301T090000Z\r\nEND:VEVENT\r\n"
        ));
    }
    body.push_str("END:VCALENDAR\r\n");
    body
}

#[tokio::test]
async fn l066_a_feed_that_keeps_more_than_the_event_cap_is_too_large_and_changes_nothing(
) -> TestResult {
    let db = db()?;
    let at = now();
    let dto = add_ics(
        &db,
        &Fake::new(vec![modified(tiny_feed(3), None)]),
        SECRET_URL,
        at,
    )
    .await?;
    assert_eq!(dto.last_error, None);
    let stored = |db: &Db| -> anyhow::Result<usize> {
        Ok(lock(db).events().list_by_calendar(&dto.id)?.len())
    };
    assert_eq!(stored(&db)?, 3);

    // One event over the cap: refused like an oversize body, and the events from before stay.
    let over = tiny_feed(MAX_EVENTS + 1);
    assert!(over.len() < 64 * 1024 * 1024);
    sync_one(&db, &Fake::new(vec![modified(over, None)]), &dto.id, at).await?;
    let listed = list(&db)?;
    assert_eq!(
        listed.first().and_then(|c| c.last_error.as_ref()),
        Some(&SyncErrorDto {
            kind: "too_large".to_owned(),
            status: None
        })
    );
    assert_eq!(stored(&db)?, 3);
    let log = lock(&db).sync_log().list_recent(5)?;
    assert!(log
        .iter()
        .any(|row| row.detail.as_deref() == Some("too_large")));
    Ok(())
}

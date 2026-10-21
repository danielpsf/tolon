//! Scenario and acceptance-criteria tests for F1 (calendar name) and F2 (parse and expand).

mod common;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use common::{cal, fixture_text, now, parse_fixture, utc, TestResult, LOCAL};
use tolon_core::{
    expand, expand_window, parse_ics, parse_recurrence, wall_clock, Event, EventStatus, EventTime,
    ExpandOptions, Occurrence, ParseOptions,
};

fn wrap(body: &str) -> String {
    format!("BEGIN:VCALENDAR\nVERSION:2.0\n{body}END:VCALENDAR\n")
}

fn parse(text: &str) -> Vec<Event> {
    parse_ics(text, &cal(), &ParseOptions::default()).events
}

/// "2026-10-26 09:00 +01:00" per occurrence.
fn starts(occ: &[Occurrence]) -> Vec<String> {
    occ.iter()
        .map(|o| o.start.format("%Y-%m-%d %H:%M %:z").to_string())
        .collect()
}

fn window(
    text: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    local: Tz,
    opts: &ExpandOptions,
) -> Vec<Occurrence> {
    expand_window(&parse(text), from, to, local, opts)
}

#[test]
fn s1_1_calendar_name_from_x_wr_calname() {
    let named = parse_ics(
        &wrap("X-WR-CALNAME:Team Calendar\n"),
        &cal(),
        &ParseOptions::default(),
    );
    assert_eq!(named.name.as_deref(), Some("Team Calendar"));

    let escaped = parse_ics(
        &wrap("X-WR-CALNAME:Work\\, home\n"),
        &cal(),
        &ParseOptions::default(),
    );
    assert_eq!(escaped.name.as_deref(), Some("Work, home"));

    // No name, or an empty one: the caller falls back to the host.
    for body in ["", "X-WR-CALNAME:\n", "X-WR-CALNAME:   \n"] {
        let p = parse_ics(&wrap(body), &cal(), &ParseOptions::default());
        assert_eq!(p.name, None, "{body:?}");
    }

    // A name inside a VEVENT or VTIMEZONE is not the calendar's name.
    let nested = wrap(
        "BEGIN:VTIMEZONE\nX-WR-CALNAME:Zone\nEND:VTIMEZONE\nBEGIN:VEVENT\nX-WR-CALNAME:Event\nEND:VEVENT\n",
    );
    assert_eq!(
        parse_ics(&nested, &cal(), &ParseOptions::default()).name,
        None
    );
}

#[test]
fn s2_2_dst_change() -> TestResult {
    let feed = wrap(
        "BEGIN:VEVENT\nUID:monday@example.com\nSUMMARY:Monday 09:00\n\
DTSTART;TZID=Europe/Berlin:20261005T090000\nDTEND;TZID=Europe/Berlin:20261005T100000\n\
RRULE:FREQ=WEEKLY;BYDAY=MO\nEND:VEVENT\n",
    );
    // Mondays 19 Oct (summer time), 26 Oct and 2 Nov (winter time); the change is Sunday 25 Oct.
    let occ = window(
        &feed,
        utc(2026, 10, 19, 0, 0)?,
        utc(2026, 11, 3, 0, 0)?,
        Tz::Europe__Berlin,
        &ExpandOptions::default(),
    );
    assert_eq!(
        starts(&occ),
        [
            "2026-10-19 09:00 +02:00",
            "2026-10-26 09:00 +01:00",
            "2026-11-02 09:00 +01:00"
        ]
    );
    let instants: Vec<String> = occ
        .iter()
        .map(|o| {
            o.start
                .with_timezone(&Utc)
                .format("%m-%d %H:%M")
                .to_string()
        })
        .collect();
    assert_eq!(instants, ["10-19 07:00", "10-26 08:00", "11-02 08:00"]);
    Ok(())
}

#[test]
fn s2_3_malformed_input_skips_and_counts() {
    let feed = wrap(
        "BEGIN:VEVENT\nUID:a@example.com\nSUMMARY:A\nDTSTART:20261022T090000Z\nDTEND:20261022T100000Z\nEND:VEVENT\n\
BEGIN:VEVENT\nUID:no-start@example.com\nSUMMARY:No start\nDTEND:20261022T110000Z\nEND:VEVENT\n\
BEGIN:VEVENT\nUID:b@example.com\nSUMMARY:B\nDTSTART:20261022T120000Z\nDTEND:20261022T130000Z\nEND:VEVENT\n",
    );
    let parsed = parse_ics(&feed, &cal(), &ParseOptions::default());
    let titles: Vec<&str> = parsed.events.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(titles, ["A", "B"]);
    assert_eq!(parsed.skipped_components, 1);
    assert_eq!(
        parsed.skipped_summary().as_deref(),
        Some("1 component skipped")
    );

    let clean = parse_ics(&wrap(""), &cal(), &ParseOptions::default());
    assert_eq!(clean.skipped_summary(), None);
}

#[test]
fn ac2_1_utc_floating_and_tzid_convert_to_the_local_zone() -> TestResult {
    let feed = wrap(
        "BEGIN:VEVENT\nUID:utc\nSUMMARY:utc\nDTSTART:20260701T120000Z\nDTEND:20260701T130000Z\nEND:VEVENT\n\
BEGIN:VEVENT\nUID:floating\nSUMMARY:floating\nDTSTART:20260701T120000\nDTEND:20260701T130000\nEND:VEVENT\n\
BEGIN:VEVENT\nUID:kolkata\nSUMMARY:kolkata\nDTSTART;TZID=Asia/Kolkata:20260701T120000\nDTEND;TZID=Asia/Kolkata:20260701T130000\nEND:VEVENT\n\
BEGIN:VEVENT\nUID:winter\nSUMMARY:winter\nDTSTART:20261201T120000Z\nDTEND:20261201T130000Z\nEND:VEVENT\n",
    );
    let (from, to) = (utc(2026, 6, 1, 0, 0)?, utc(2027, 1, 1, 0, 0)?);
    let opts = ExpandOptions::default();

    let berlin = window(&feed, from, to, Tz::Europe__Berlin, &opts);
    assert_eq!(
        starts(&berlin),
        [
            "2026-07-01 08:30 +02:00", // Kolkata 12:00 = 06:30 UTC
            "2026-07-01 12:00 +02:00", // floating: 12:00 wherever the machine is
            "2026-07-01 14:00 +02:00", // 12:00 UTC in summer time
            "2026-12-01 13:00 +01:00", // 12:00 UTC in winter time
        ]
    );

    let new_york = window(&feed, from, to, Tz::America__New_York, &opts);
    assert_eq!(
        starts(&new_york),
        [
            "2026-07-01 02:30 -04:00",
            "2026-07-01 08:00 -04:00",
            "2026-07-01 12:00 -04:00", // the floating event moved with the machine
            "2026-12-01 07:00 -05:00",
        ]
    );

    // Spring forward: 02:30 on 29 Mar 2026 does not exist in Berlin; it reads as 03:30 summer time.
    let gap = chrono::NaiveDate::from_ymd_opt(2026, 3, 29)
        .and_then(|d| d.and_hms_opt(2, 30, 0))
        .ok_or("date")?;
    let in_gap = wall_clock(gap, Tz::Europe__Berlin).ok_or("gap")?;
    assert_eq!(in_gap.format("%H:%M %:z").to_string(), "03:30 +02:00");
    // Fall back: 02:30 on 25 Oct 2026 happens twice; the first pass wins.
    let twice = chrono::NaiveDate::from_ymd_opt(2026, 10, 25)
        .and_then(|d| d.and_hms_opt(2, 30, 0))
        .ok_or("date")?;
    let first = wall_clock(twice, Tz::Europe__Berlin).ok_or("twice")?;
    assert_eq!(first.format("%H:%M %:z").to_string(), "02:30 +02:00");
    Ok(())
}

/// Local start times of one recurring event over `[from, to)`, in Berlin.
fn recur(rule: &str, dtstart: &str, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<String> {
    let feed = wrap(&format!(
        "BEGIN:VEVENT\nUID:r@example.com\nSUMMARY:r\nDTSTART;TZID=Europe/Berlin:{dtstart}\nDTEND;TZID=Europe/Berlin:{dtstart}\nRRULE:{rule}\nEND:VEVENT\n"
    ));
    starts(&window(&feed, from, to, LOCAL, &ExpandOptions::default()))
}

#[test]
fn ac2_2_freq_interval_count_until_byday_bymonthday() -> TestResult {
    let (oct1, jan1) = (utc(2026, 9, 30, 0, 0)?, utc(2027, 1, 1, 0, 0)?);
    assert_eq!(
        recur(
            "FREQ=DAILY;INTERVAL=3;COUNT=4",
            "20261001T090000",
            oct1,
            jan1
        ),
        [
            "2026-10-01 09:00 +02:00",
            "2026-10-04 09:00 +02:00",
            "2026-10-07 09:00 +02:00",
            "2026-10-10 09:00 +02:00"
        ]
    );
    // UNTIL is inclusive and in UTC: Wed 14 Oct 09:00 Berlin is 07:00Z, after 00:00Z, so it is out.
    assert_eq!(
        recur(
            "FREQ=WEEKLY;BYDAY=MO,WE;UNTIL=20261014T000000Z",
            "20261005T090000",
            oct1,
            jan1
        ),
        [
            "2026-10-05 09:00 +02:00",
            "2026-10-07 09:00 +02:00",
            "2026-10-12 09:00 +02:00"
        ]
    );
    assert_eq!(
        recur(
            "FREQ=WEEKLY;INTERVAL=2;BYDAY=TH",
            "20261001T090000",
            oct1,
            utc(2026, 11, 20, 0, 0)?
        ),
        [
            "2026-10-01 09:00 +02:00",
            "2026-10-15 09:00 +02:00",
            "2026-10-29 09:00 +01:00",
            "2026-11-12 09:00 +01:00"
        ]
    );
    assert_eq!(
        recur(
            "FREQ=MONTHLY;BYMONTHDAY=1,15",
            "20261001T090000",
            oct1,
            utc(2026, 12, 1, 0, 0)?
        ),
        [
            "2026-10-01 09:00 +02:00",
            "2026-10-15 09:00 +02:00",
            "2026-11-01 09:00 +01:00",
            "2026-11-15 09:00 +01:00"
        ]
    );
    assert_eq!(
        recur(
            "FREQ=MONTHLY;BYDAY=-1FR",
            "20261030T090000",
            oct1,
            utc(2027, 1, 1, 0, 0)?
        ),
        [
            "2026-10-30 09:00 +01:00",
            "2026-11-27 09:00 +01:00",
            "2026-12-25 09:00 +01:00"
        ]
    );
    assert_eq!(
        recur(
            "FREQ=YEARLY",
            "20261023T090000",
            oct1,
            utc(2028, 12, 31, 0, 0)?
        ),
        [
            "2026-10-23 09:00 +02:00",
            "2027-10-23 09:00 +02:00",
            "2028-10-23 09:00 +02:00"
        ]
    );
    Ok(())
}

#[test]
fn ac2_2_exdate_and_recurrence_id_overrides() -> TestResult {
    let parsed = parse_fixture("exdate-and-override.ics")?;
    let occ = expand(&parsed.events, now()?, LOCAL, &ExpandOptions::default());
    let got: Vec<(String, String)> = occ
        .iter()
        .map(|o| {
            (
                o.start.format("%Y-%m-%d %H:%M %:z").to_string(),
                o.title.clone(),
            )
        })
        .collect();
    let want = [
        ("2026-10-22 11:00 +02:00", "Daily sync"),
        // 24 Oct: the override moved it to 10:00 and kept the series title.
        ("2026-10-24 10:00 +02:00", "Daily sync"),
        ("2026-10-25 11:00 +01:00", "Daily sync"),
        (
            "2026-10-25 16:00 +01:00",
            "Vendor demo (one instance, series not shared)",
        ),
        ("2026-10-26 14:00 +01:00", "Daily sync (moved to afternoon)"),
        // 23 Oct and 28 Oct are EXDATEs, 27 Oct is a cancelled override, and the instance of
        // 2 Nov was pulled forward into 28 Oct 16:00.
        (
            "2026-10-28 16:00 +01:00",
            "Daily sync (pulled forward from 2 Nov)",
        ),
    ];
    let want: Vec<(String, String)> = want
        .iter()
        .map(|(s, t)| (s.to_string(), t.to_string()))
        .collect();
    assert_eq!(got, want);

    // A moved instance keeps the key of the instance it replaced, so alarm state follows it.
    let mut plain = parsed.events.clone();
    for e in &mut plain {
        e.overrides.clear();
    }
    let plain_occ = expand(&plain, now()?, LOCAL, &ExpandOptions::default());
    let key_of = |list: &[Occurrence], day: &str| {
        list.iter()
            .find(|o| o.start.format("%Y-%m-%d").to_string() == day && o.title.starts_with("Daily"))
            .map(|o| o.key.clone())
    };
    assert_eq!(key_of(&occ, "2026-10-26"), key_of(&plain_occ, "2026-10-26"));
    Ok(())
}

/// One recurring event with the given DTSTART line and RRULE, over the fixed clock's window.
fn until_case(dtstart: &str, rule: &str) -> TestResult<Vec<String>> {
    let feed = wrap(&format!(
        "BEGIN:VEVENT\nUID:u@example.com\nSUMMARY:u\n{dtstart}\nRRULE:{rule}\nEND:VEVENT\n"
    ));
    let parsed = parse_ics(&feed, &cal(), &ParseOptions::default());
    assert_eq!(parsed.skipped_components, 0, "{dtstart} {rule}");
    let opts = ExpandOptions {
        include_all_day: true,
        ..ExpandOptions::default()
    };
    Ok(starts(&expand(&parsed.events, now()?, LOCAL, &opts)))
}

#[test]
fn ac2_2_until_in_any_form_expands() -> TestResult {
    // DATE start, DATE until: inclusive through that day.
    assert_eq!(
        until_case("DTSTART;VALUE=DATE:20261020", "FREQ=DAILY;UNTIL=20261025")?,
        [
            "2026-10-22 00:00 +02:00",
            "2026-10-23 00:00 +02:00",
            "2026-10-24 00:00 +02:00",
            "2026-10-25 00:00 +02:00"
        ]
    );
    // DATE start, weekly BYDAY, DATE until (Google all-day series).
    assert_eq!(
        until_case(
            "DTSTART;VALUE=DATE:20261003",
            "FREQ=WEEKLY;BYDAY=SA,SU;UNTIL=20261025"
        )?,
        ["2026-10-24 00:00 +02:00", "2026-10-25 00:00 +02:00"]
    );
    // TZID start, floating until: read in the event's zone, so 25 Oct 00:00 Berlin ends the series.
    assert_eq!(
        until_case(
            "DTSTART;TZID=Europe/Berlin:20261020T090000",
            "FREQ=DAILY;UNTIL=20261025T000000"
        )?,
        ["2026-10-23 09:00 +02:00", "2026-10-24 09:00 +02:00"]
    );
    // Floating start, floating until: both in the machine's zone, until inclusive.
    assert_eq!(
        until_case(
            "DTSTART:20261020T090000",
            "FREQ=DAILY;UNTIL=20261024T090000"
        )?,
        ["2026-10-23 09:00 +02:00", "2026-10-24 09:00 +02:00"]
    );
    // UTC start, date until: inclusive through that day in UTC.
    assert_eq!(
        until_case("DTSTART:20261020T090000Z", "FREQ=DAILY;UNTIL=20261024")?,
        [
            "2026-10-22 11:00 +02:00",
            "2026-10-23 11:00 +02:00",
            "2026-10-24 11:00 +02:00"
        ]
    );
    // Control: the form RFC 5545 asks for still works.
    assert_eq!(
        until_case(
            "DTSTART;TZID=Europe/Berlin:20261020T090000",
            "FREQ=DAILY;UNTIL=20261024T070000Z"
        )?,
        ["2026-10-23 09:00 +02:00", "2026-10-24 09:00 +02:00"]
    );
    Ok(())
}

#[test]
fn ac2_3_cancelled_events_are_excluded() -> TestResult {
    let feed = |method: &str, status: &str| {
        wrap(&format!(
            "{method}BEGIN:VEVENT\nUID:c@example.com\nSUMMARY:c\nDTSTART:20261022T120000Z\nDTEND:20261022T130000Z\n{status}END:VEVENT\n"
        ))
    };
    let on = ExpandOptions::default();
    let off = ExpandOptions {
        ignore_cancelled: false,
        ..on
    };
    let count = |text: &str, opts: &ExpandOptions| -> TestResult<usize> {
        Ok(expand(&parse(text), now()?, LOCAL, opts).len())
    };

    assert_eq!(count(&feed("", ""), &on)?, 1);
    assert_eq!(count(&feed("", "STATUS:CANCELLED\n"), &on)?, 0);
    assert_eq!(count(&feed("", "STATUS:CANCELLED\n"), &off)?, 1);
    assert_eq!(count(&feed("METHOD:CANCEL\n", ""), &on)?, 0);
    assert_eq!(count(&feed("METHOD:PUBLISH\n", ""), &on)?, 1);
    let events = parse(&feed("METHOD:CANCEL\n", ""));
    assert_eq!(
        events.first().map(|e| e.status),
        Some(EventStatus::Cancelled)
    );

    let fixture = parse_fixture("cancelled-status.ics")?;
    let titles: Vec<String> = expand(&fixture.events, now()?, LOCAL, &on)
        .into_iter()
        .map(|o| o.title)
        .collect();
    assert_eq!(titles, ["Roadmap review", "Maybe: partner intro"]);
    Ok(())
}

#[test]
fn ac2_3_declined_events_are_excluded_when_the_rule_is_on() -> TestResult {
    let feed = wrap(
        "X-WR-CALNAME:Work calendar\n\
BEGIN:VEVENT\nUID:d@example.com\nSUMMARY:declined\nDTSTART:20261022T120000Z\nDTEND:20261022T130000Z\n\
ATTENDEE;PARTSTAT=DECLINED:mailto:Me@Example.com\nATTENDEE;PARTSTAT=ACCEPTED:mailto:other@example.com\nEND:VEVENT\n",
    );
    let with_owner = ParseOptions {
        owner_emails: vec!["me@example.com".to_string()],
    };
    let events = parse_ics(&feed, &cal(), &with_owner).events;
    let on = ExpandOptions::default();
    let off = ExpandOptions {
        ignore_declined: false,
        ..on
    };
    assert_eq!(expand(&events, now()?, LOCAL, &on).len(), 0);
    assert_eq!(expand(&events, now()?, LOCAL, &off).len(), 1);
    assert_eq!(events.first().map(|e| e.attendees_count), Some(2));

    // The calendar name is not an address and no owner was given: nobody is the owner.
    let unknown = parse(&feed);
    assert_eq!(expand(&unknown, now()?, LOCAL, &on).len(), 1);

    // Google names the primary calendar after its owner, which is enough on its own.
    let google = parse_fixture("declined-attendee.ics")?;
    let titles: Vec<String> = expand(&google.events, now()?, LOCAL, &on)
        .into_iter()
        .map(|o| o.title)
        .collect();
    assert_eq!(
        titles,
        [
            "Design critique (someone else declined)",
            "Customer workshop (owner tentative)",
            "Focus time (no attendees)"
        ]
    );
    Ok(())
}

#[test]
fn ac2_4_all_day_events_are_excluded_by_default_and_included_on_request() -> TestResult {
    let parsed = parse_fixture("all-day.ics")?;
    let default = expand(&parsed.events, now()?, LOCAL, &ExpandOptions::default());
    assert_eq!(
        default.iter().map(|o| o.title.as_str()).collect::<Vec<_>>(),
        ["Normal meeting on the day off"]
    );
    let all = expand(
        &parsed.events,
        now()?,
        LOCAL,
        &ExpandOptions {
            include_all_day: true,
            ..ExpandOptions::default()
        },
    );
    assert_eq!(all.len(), 7);
    assert_eq!(all.iter().filter(|o| o.all_day).count(), 6);
    Ok(())
}

#[test]
fn ac2_5_parsing_never_panics_and_skips_bad_components() -> TestResult {
    let hostile = [
        "",
        "\0\0\0",
        "BEGIN:VEVENT",
        "END:VEVENT\nEND:VCALENDAR\nEND:VEVENT",
        "BEGIN:VCALENDAR\nBEGIN:VEVENT\nBEGIN:VEVENT\nBEGIN:VEVENT\n",
        "BEGIN:VEVENT\nDTSTART:99991231T235959Z\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART:00010101T000000Z\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART:20261022T090000Z\nDURATION:P99999999999999999999D\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART:20261022T090000Z\nDURATION:P5000000W\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART:20261022T090000Z\nDURATION:-P1D\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART:20261022T090000Z\nRRULE:FREQ=DAILY;INTERVAL=99999999\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART:20261022T090000Z\nRRULE:FREQ=SECONDLY\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART:20200101T090000Z\nRRULE:FREQ=MONTHLY;BYMONTHDAY=31;BYMONTH=2\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART:20261022T090000Z\nRRULE:\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART;TZID=:20261022T090000\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART;TZID=Nowhere/Land:20261022T090000\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART;;;:\nATTENDEE;=:\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART:20261022T090000Z\nRECURRENCE-ID:garbage\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART:2026\u{1F600}\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART;VALUE=DATE:20261020\nRRULE:FREQ=DAILY;UNTIL=20261025\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART;TZID=Europe/Berlin:20261020T090000\nRRULE:FREQ=DAILY;UNTIL=20261025T000000\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART:20261020T090000Z\nRRULE:FREQ=DAILY;UNTIL=20261024\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART:20261020T090000\nRRULE:FREQ=DAILY;UNTIL=garbage\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART:20261020T090000\nRRULE:FREQ=DAILY;UNTIL=99999999\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART:20261020T090000\nRRULE:FREQ=DAILY;UNTIL=20190101\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART:20261020T090000\nRRULE:FREQ=DAILY;UNTIL=;UNTIL=\nEND:VEVENT",
        "BEGIN:VEVENT\nDTSTART:20261020T090000\nRRULE:UNTIL=20261024;FREQ=DAILY;;\nEND:VEVENT",
    ];
    let (n, opts) = (now()?, ExpandOptions::default());
    for text in hostile {
        let parsed = parse_ics(text, &cal(), &ParseOptions::default());
        let _ = expand(&parsed.events, n, LOCAL, &opts);
    }

    // Every fixture, cut at many points and with lines dropped: still no panic.
    for name in common::FIXTURE_FILES.iter().filter(|n| **n != common::HUGE) {
        let text = fixture_text(name)?;
        for cut in (0..text.len())
            .step_by(41)
            .filter(|c| text.is_char_boundary(*c))
        {
            let parsed = parse_ics(&text[..cut], &cal(), &ParseOptions::default());
            let _ = expand(&parsed.events, n, LOCAL, &opts);
        }
        for k in 2..7 {
            let thinned: String = text
                .lines()
                .enumerate()
                .filter(|(i, _)| i % k != 1)
                .map(|(_, l)| format!("{l}\n"))
                .collect();
            let parsed = parse_ics(&thinned, &cal(), &ParseOptions::default());
            let _ = expand(&parsed.events, n, LOCAL, &opts);
        }
    }

    // A bad component does not cost the good ones next to it.
    let parsed = parse_fixture("malformed-line-folding.ics")?;
    assert_eq!((parsed.events.len(), parsed.skipped_components), (4, 4));
    let last = parsed
        .events
        .iter()
        .find(|e| e.uid == "after-the-trash@example.com")
        .ok_or("event missing")?;
    assert_eq!(last.description.as_deref(), Some("Real description"));
    Ok(())
}

#[test]
fn ac2_5_huge_interval_never_panics() -> TestResult {
    // Event.rrule is a raw string from the store; parse_ics would reject these, expand must still hold.
    let base = parse(&wrap(
        "BEGIN:VEVENT\nUID:h@example.com\nSUMMARY:h\nDTSTART;TZID=Europe/Berlin:20000103T090000\nDTEND;TZID=Europe/Berlin:20000103T100000\nEND:VEVENT\n",
    ));
    let template = base.first().ok_or("event")?;
    for freq in ["DAILY", "WEEKLY", "MONTHLY", "YEARLY"] {
        for interval in [
            "9223372036854775807",
            "4611686018427387904",
            "3074457345618258603",
            "65536",
            "0",
            "-1",
        ] {
            let mut e = template.clone();
            e.rrule = Some(format!("FREQ={freq};INTERVAL={interval}"));
            let _ = expand(&[e], now()?, LOCAL, &ExpandOptions::default());
        }
    }
    Ok(())
}

#[test]
fn ac2_1_crlf_and_lf_feeds_parse_the_same() -> TestResult {
    let lf = fixture_text("google-basic.ics")?;
    let crlf = lf.replace('\n', "\r\n");
    assert_eq!(
        parse_ics(&lf, &cal(), &ParseOptions::default()),
        parse_ics(&crlf, &cal(), &ParseOptions::default())
    );
    Ok(())
}

#[test]
fn duration_and_missing_end_give_an_end() -> TestResult {
    let feed = wrap(
        "BEGIN:VEVENT\nUID:dur\nSUMMARY:dur\nDTSTART:20261022T090000Z\nDURATION:PT1H30M\nEND:VEVENT\n\
BEGIN:VEVENT\nUID:none\nSUMMARY:none\nDTSTART:20261022T120000Z\nEND:VEVENT\n",
    );
    let occ = expand(&parse(&feed), now()?, LOCAL, &ExpandOptions::default());
    let spans: Vec<String> = occ
        .iter()
        .map(|o| format!("{}-{}", o.start.format("%H:%M"), o.end.format("%H:%M")))
        .collect();
    assert_eq!(spans, ["11:00-12:30", "14:00-14:00"]);
    Ok(())
}

#[test]
fn event_serialises_to_stable_json_and_back() -> TestResult {
    for name in common::FIXTURE_FILES {
        for event in parse_fixture(name)?.events.iter().take(50) {
            let json = serde_json::to_string(event)?;
            let back: Event = serde_json::from_str(&json)?;
            assert_eq!(&back, event, "{name}");
            assert_eq!(serde_json::to_string(&back)?, json, "{name}");
        }
    }
    Ok(())
}

#[test]
fn recurrence_lines_parse_as_in_a_feed() -> TestResult {
    let start = EventTime::Zoned {
        tz: chrono_tz::Europe::Berlin,
        local: chrono::NaiveDate::from_ymd_opt(2026, 10, 19)
            .and_then(|d| d.and_hms_opt(10, 0, 0))
            .ok_or("date")?,
    };
    let lines = [
        "RRULE:FREQ=WEEKLY;BYDAY=MO".to_string(),
        "EXDATE;TZID=Europe/Berlin:20261026T100000,20261102T100000".to_string(),
    ];
    let (rrule, exdates) = parse_recurrence(&lines, &start).ok_or("parse")?;
    assert_eq!(rrule.as_deref(), Some("FREQ=WEEKLY;BYDAY=MO"));
    assert_eq!(exdates.len(), 2);
    assert!(matches!(&exdates[0], EventTime::Zoned { tz, .. } if *tz == chrono_tz::Europe::Berlin));
    assert_eq!(parse_recurrence(&[], &start), Some((None, vec![])));
    assert_eq!(
        parse_recurrence(&["RRULE:FREQ=SOMETIMES".to_string()], &start),
        None
    );
    Ok(())
}

//! Alarm model, planning, ticking and grouping: F3 scenarios and the criteria around them.

mod common;

use chrono::Duration;
use common::{at, occ, TestResult};
use tolon_core::{
    already_there, apply_rules, dismiss, expand_window, join, judge_window, parse_ics, snooze,
    snooze_later, Alarm, AlarmKind, AlarmState, EventId, ExpandOptions, IgnoreReason, IgnoreRules,
    OccurrenceKey, ParseOptions, Verdict,
};

fn key(event: &str) -> TestResult<OccurrenceKey> {
    Ok(OccurrenceKey {
        event: EventId(event.to_string()),
        original_start: at(14, 0)?,
    })
}

fn alarm(state: AlarmState) -> TestResult<Alarm> {
    Ok(Alarm {
        occurrence: key("cal:uid")?,
        at: at(13, 58)?,
        start: at(14, 0)?,
        kind: AlarmKind::Lead,
        state,
    })
}

#[test]
fn occurrence_key_string_round_trips() -> TestResult {
    let odd_ids = [
        "cal:abc123@google.com",
        "a@b@c",
        "x",
        "cal:uid with spaces|and:colons",
    ];
    for id in odd_ids {
        let k = key(id)?;
        let text = k.to_string();
        assert_eq!(text.parse::<OccurrenceKey>()?, k, "{text}");
    }
    assert_eq!(
        key("cal:uid")?.to_string(),
        "cal:uid@2026-10-22T14:00:00Z",
        "the column format is stable"
    );
    assert!("no-separator".parse::<OccurrenceKey>().is_err());
    assert!("cal:uid@not-a-time".parse::<OccurrenceKey>().is_err());
    Ok(())
}

#[test]
fn alarm_state_and_kind_round_trip_through_columns_and_json() -> TestResult {
    let states = [
        AlarmState::Scheduled,
        AlarmState::Firing { since: at(13, 58)? },
        AlarmState::Snoozed { until: at(14, 3)? },
        AlarmState::Dismissed,
        AlarmState::Joined,
        AlarmState::Expired,
    ];
    for state in states {
        let again = AlarmState::from_parts(state.name(), state.since(), state.until());
        assert_eq!(again, Some(state), "columns: {}", state.name());

        let a = alarm(state)?;
        let json = serde_json::to_string(&a)?;
        assert_eq!(serde_json::from_str::<Alarm>(&json)?, a, "{json}");
    }
    // A firing or snoozed row without its timestamp is corrupt, not a guess.
    assert_eq!(AlarmState::from_parts("firing", None, None), None);
    assert_eq!(AlarmState::from_parts("snoozed", None, None), None);
    assert_eq!(AlarmState::from_parts("later", None, None), None);

    for kind in [AlarmKind::Lead, AlarmKind::Start] {
        assert_eq!(AlarmKind::parse(kind.as_str()), Some(kind));
    }
    assert_eq!(AlarmKind::parse("end"), None);
    Ok(())
}

#[test]
fn actions_move_open_alarms_and_leave_final_ones_alone() -> TestResult {
    for open in [
        AlarmState::Scheduled,
        AlarmState::Firing { since: at(13, 58)? },
        AlarmState::Snoozed { until: at(14, 3)? },
    ] {
        let mut a = alarm(open)?;
        assert!(snooze(&mut a, 5, at(13, 59)?));
        assert_eq!(a.state, AlarmState::Snoozed { until: at(14, 4)? });
        assert!(dismiss(&mut a));
        assert_eq!(a.state, AlarmState::Dismissed);

        assert!(join(&mut alarm(open)?));
        let mut b = alarm(open)?;
        assert!(already_there(&mut b));
        assert_eq!(b.state, AlarmState::Joined);
    }

    for done in [
        AlarmState::Dismissed,
        AlarmState::Joined,
        AlarmState::Expired,
    ] {
        let mut a = alarm(done)?;
        assert!(!snooze(&mut a, 5, at(13, 59)?));
        assert!(!dismiss(&mut a));
        assert!(!join(&mut a));
        assert!(!already_there(&mut a));
        assert_eq!(a.state, done, "final states stay put");
    }

    let mut zero = alarm(AlarmState::Scheduled)?;
    assert!(!snooze(&mut zero, 0, at(13, 59)?));
    assert_eq!(zero.state, AlarmState::Scheduled);
    Ok(())
}

/// AC7.2: the panel's Snooze before the interrupt is up delays the alarm, never brings it forward.
#[test]
fn ac7_2_panel_snooze_comes_later_than_the_alarm_was_due() -> TestResult {
    // Due at 13:58; snoozed at 13:48 it comes at 14:03, not at 13:53.
    let mut a = alarm(AlarmState::Scheduled)?;
    assert!(snooze_later(&mut a, 5, at(13, 48)?));
    assert_eq!(a.state, AlarmState::Snoozed { until: at(14, 3)? });
    // Snoozed again, it moves on from its own time.
    assert!(snooze_later(&mut a, 5, at(13, 50)?));
    assert_eq!(a.state, AlarmState::Snoozed { until: at(14, 8)? });
    // Overdue or firing: from now.
    let mut late = alarm(AlarmState::Scheduled)?;
    assert!(snooze_later(&mut late, 5, at(13, 59)?));
    assert_eq!(late.state, AlarmState::Snoozed { until: at(14, 4)? });
    let mut firing = alarm(AlarmState::Firing { since: at(13, 58)? })?;
    assert!(snooze_later(&mut firing, 5, at(13, 59)?));
    assert_eq!(firing.state, AlarmState::Snoozed { until: at(14, 4)? });
    for done in [
        AlarmState::Dismissed,
        AlarmState::Joined,
        AlarmState::Expired,
    ] {
        let mut a = alarm(done)?;
        assert!(!snooze_later(&mut a, 5, at(13, 48)?));
        assert_eq!(a.state, done);
    }
    assert!(!snooze_later(
        &mut alarm(AlarmState::Scheduled)?,
        0,
        at(13, 48)?
    ));
    Ok(())
}

fn verdicts(rules: &IgnoreRules, titles: &[(&str, u32)]) -> TestResult<Vec<Verdict>> {
    let start = at(14, 0)?;
    let input = titles.iter().map(|(t, p)| occ(t, start, *p)).collect();
    Ok(apply_rules(input, rules)
        .into_iter()
        .map(|(_, v)| v)
        .collect())
}

#[test]
fn ac8_2_keywords_are_case_insensitive_and_comma_separated() -> TestResult {
    let got = verdicts(
        &IgnoreRules::default(),
        &[
            ("Focus time", 3),
            ("lunch with Sam", 3),
            ("Out of office: OOO", 3),
            ("ooo", 3),
            ("Sprint planning", 3),
        ],
    )?;
    let ignored = |k: &str| Verdict::Ignored(IgnoreReason::Keyword(k.to_string()));
    assert_eq!(
        got,
        [
            ignored("Focus"),
            ignored("Lunch"),
            ignored("OOO"),
            ignored("OOO"),
            Verdict::Keep
        ]
    );

    let custom = IgnoreRules {
        keywords: " standup ,, ,Ünï ".to_string(),
        without_other_attendees: false,
    };
    let got = verdicts(
        &custom,
        &[("Daily STANDUP", 4), ("ÜNÏcode", 4), ("Focus", 4)],
    )?;
    assert_eq!(got, [ignored("standup"), ignored("Ünï"), Verdict::Keep]);

    let empty = IgnoreRules {
        keywords: " , ,".to_string(),
        without_other_attendees: false,
    };
    assert_eq!(verdicts(&empty, &[("Focus", 0)])?, [Verdict::Keep]);
    Ok(())
}

#[test]
fn ac8_2_events_without_other_attendees_default_off() -> TestResult {
    let titles = [("Solo", 0), ("One invitee", 1), ("Pair", 2)];
    assert_eq!(
        verdicts(&IgnoreRules::default(), &titles)?,
        [Verdict::Keep, Verdict::Keep, Verdict::Keep]
    );
    let on = IgnoreRules {
        without_other_attendees: true,
        ..IgnoreRules::default()
    };
    let no_others = Verdict::Ignored(IgnoreReason::NoOtherAttendees);
    assert_eq!(
        verdicts(&on, &titles)?,
        [no_others, Verdict::Keep, Verdict::Keep],
        "one ATTENDEE line is someone else; only none at all is a solo event"
    );
    Ok(())
}

#[test]
fn ac8_2_outlook_organizer_plus_one_invitee_is_kept() -> TestResult {
    // "Design review": ORGANIZER Pat and one ATTENDEE, Casey. The organizer is not counted.
    let events = common::parse_fixture("outlook-published.ics")?.events;
    let from = common::utc(2026, 9, 1, 0, 0)?;
    let occurrences = expand_window(
        &events,
        from,
        from + Duration::days(7),
        common::LOCAL,
        &ExpandOptions::default(),
    );
    let design: Vec<_> = occurrences
        .into_iter()
        .filter(|o| o.title == "Design review")
        .collect();
    assert!(!design.is_empty());
    assert_eq!(design[0].people, 1);
    let on = IgnoreRules {
        without_other_attendees: true,
        ..IgnoreRules::default()
    };
    let verdicts: Vec<_> = apply_rules(design, &on)
        .into_iter()
        .map(|(_, v)| v)
        .collect();
    assert!(verdicts.iter().all(|v| *v == Verdict::Keep), "{verdicts:?}");
    Ok(())
}

/// One event per reason, plus a kept meeting, all on Thursday 22 Oct 2026. The calendar name is
/// the owner's address, which is how the parser finds the owner's answer.
const JUDGED_FEED: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-WR-CALNAME:me@example.com\r\n\
BEGIN:VEVENT\r\nUID:keep\r\nSUMMARY:Planning\r\nDTSTART:20261022T090000Z\r\nDTEND:20261022T100000Z\r\nATTENDEE:mailto:sam@example.com\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:focus\r\nSUMMARY:Focus time\r\nDTSTART:20261022T100000Z\r\nDTEND:20261022T110000Z\r\nATTENDEE:mailto:sam@example.com\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:allday\r\nSUMMARY:Holiday\r\nDTSTART;VALUE=DATE:20261022\r\nDTEND;VALUE=DATE:20261023\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:cancelled\r\nSUMMARY:Sync\r\nDTSTART:20261022T110000Z\r\nDTEND:20261022T120000Z\r\nSTATUS:CANCELLED\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:declined\r\nSUMMARY:Review\r\nDTSTART:20261022T120000Z\r\nDTEND:20261022T130000Z\r\nATTENDEE;PARTSTAT=DECLINED:mailto:me@example.com\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:solo\r\nSUMMARY:Write-up\r\nDTSTART:20261022T130000Z\r\nDTEND:20261022T140000Z\r\nEND:VEVENT\r\n\
END:VCALENDAR\r\n";

#[test]
fn s8_1_an_ignored_event_is_listed_with_its_reason() -> TestResult {
    let events = parse_ics(JUDGED_FEED, &common::cal(), &ParseOptions::default()).events;
    let from = common::utc(2026, 10, 22, 0, 0)?;
    let rules = IgnoreRules {
        without_other_attendees: true,
        ..IgnoreRules::default()
    };
    let judged = judge_window(
        &events,
        from,
        from + Duration::days(1),
        common::LOCAL,
        &ExpandOptions::default(),
        &rules,
    );
    let got: Vec<(&str, Option<&str>)> = judged
        .iter()
        .map(|(o, v)| {
            let code = match v {
                Verdict::Keep => None,
                Verdict::Ignored(reason) => Some(reason.code()),
            };
            (o.title.as_str(), code)
        })
        .collect();
    assert_eq!(
        got,
        [
            ("Holiday", Some("all_day")),
            ("Planning", None),
            ("Focus time", Some("keyword")),
            ("Sync", Some("cancelled")),
            ("Review", Some("declined")),
            ("Write-up", Some("no_attendees")),
        ]
    );
    // What is kept is exactly what the scheduler plans from.
    let planned: Vec<_> = apply_rules(
        expand_window(
            &events,
            from,
            from + Duration::days(1),
            common::LOCAL,
            &ExpandOptions::default(),
        ),
        &rules,
    )
    .into_iter()
    .filter(|(_, v)| *v == Verdict::Keep)
    .collect();
    let kept: Vec<_> = judged
        .into_iter()
        .filter(|(_, v)| *v == Verdict::Keep)
        .collect();
    assert_eq!(kept, planned);
    Ok(())
}

#[test]
fn s8_1_judging_keeps_what_expanding_and_the_rules_keep_for_every_fixture() -> TestResult {
    let from = common::now()?;
    let to = from + Duration::days(7);
    for name in common::FIXTURE_FILES {
        let events = common::parse_fixture(name)?.events;
        for opts in [
            ExpandOptions::default(),
            ExpandOptions {
                ignore_cancelled: false,
                ignore_declined: false,
                include_all_day: true,
            },
        ] {
            let rules = IgnoreRules::default();
            let judged = judge_window(&events, from, to, common::LOCAL, &opts, &rules);
            let planned = apply_rules(
                expand_window(&events, from, to, common::LOCAL, &opts),
                &rules,
            );
            let kept: Vec<_> = judged.iter().filter(|(_, v)| *v == Verdict::Keep).collect();
            let want: Vec<_> = planned
                .iter()
                .filter(|(_, v)| *v == Verdict::Keep)
                .collect();
            assert_eq!(kept, want, "{name}");
            assert!(judged.len() >= planned.len(), "{name}");
        }
    }
    Ok(())
}

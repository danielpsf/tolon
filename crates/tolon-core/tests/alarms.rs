//! Alarm model, planning, ticking and grouping: F3 scenarios and the criteria around them.

mod common;

use chrono::Duration;
use common::{at, occ, TestResult};
use tolon_core::{
    already_there, apply_rules, dismiss, expand_window, join, snooze, Alarm, AlarmKind, AlarmState,
    EventId, ExpandOptions, IgnoreReason, IgnoreRules, OccurrenceKey, Verdict,
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

//! Alarm model, planning, ticking and grouping: F3 scenarios and the criteria around them.

mod common;

use chrono::{DateTime, Utc};
use common::{utc, TestResult};
use tolon_core::{
    already_there, dismiss, join, snooze, Alarm, AlarmKind, AlarmState, EventId, OccurrenceKey,
};

/// 14:00 on the common test day, as the meeting start.
fn at(h: u32, m: u32) -> TestResult<DateTime<Utc>> {
    utc(2026, 10, 22, h, m)
}

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

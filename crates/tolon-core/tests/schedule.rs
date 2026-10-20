//! F3 scenarios and the criteria around them: planning, ticking, grouping and the countdown.

mod common;

use chrono::{DateTime, Duration, NaiveTime, Utc};
use chrono_tz::Tz;
use common::{at, occ, utc, TestResult, LOCAL};
use tolon_core::{
    apply_transitions, countdown, dismiss, group_firing, join, plan, snooze, tick, Alarm,
    AlarmKind, AlarmSettings, AlarmState, Countdown, JoinTarget, QuietHours, Transition,
};

fn settings(lead_minutes: u32, again_at_start: bool) -> AlarmSettings {
    AlarmSettings {
        lead_minutes,
        again_at_start,
    }
}

/// Tick at `now` and apply the result, the way the scheduler loop does.
fn step(alarms: &mut [Alarm], now: DateTime<Utc>) -> Vec<Transition> {
    let transitions = tick(alarms, now, None, None);
    apply_transitions(alarms, &transitions);
    transitions
}

fn states(alarms: &[Alarm]) -> Vec<AlarmState> {
    alarms.iter().map(|a| a.state).collect()
}

fn firing(now: DateTime<Utc>) -> AlarmState {
    AlarmState::Firing { since: now }
}

#[test]
fn s3_1_lead_time() -> TestResult {
    let meeting = occ("review", at(14, 0)?, 3);
    let mut alarms = plan(&[meeting], &settings(2, false), &[], at(9, 0)?);
    assert_eq!(
        alarms.len(),
        1,
        "no second alarm without 'again at the start'"
    );
    assert_eq!(alarms[0].kind, AlarmKind::Lead);
    assert_eq!(alarms[0].at, at(13, 58)?);

    assert!(step(&mut alarms, at(13, 57)?).is_empty());
    let last_second = at(13, 58)? - Duration::seconds(1);
    assert!(step(&mut alarms, last_second).is_empty());
    let fired = step(&mut alarms, at(13, 58)?);
    assert_eq!(fired.len(), 1);
    assert!(fired[0].fires());
    assert_eq!(states(&alarms), [firing(at(13, 58)?)]);
    Ok(())
}

#[test]
fn s3_1_a_15_second_tick_fires_within_15_seconds_of_the_lead_time() -> TestResult {
    let meeting = occ("review", at(14, 0)?, 3);
    // The loop's phase against the wall clock is arbitrary: try every offset in the 15 s cycle.
    for offset in 0..15 {
        let mut alarms = plan(
            std::slice::from_ref(&meeting),
            &settings(2, false),
            &[],
            at(13, 50)?,
        );
        let mut fired_at = None;
        let mut now = at(13, 50)? + Duration::seconds(offset);
        while now < at(14, 5)? && fired_at.is_none() {
            if step(&mut alarms, now).iter().any(|t| t.fires()) {
                fired_at = Some(now);
            }
            now += Duration::seconds(15);
        }
        let late = fired_at.ok_or("never fired")? - at(13, 58)?;
        assert!(
            (Duration::zero()..=Duration::seconds(15)).contains(&late),
            "offset {offset}: fired {late} after 13:58:00"
        );
    }
    Ok(())
}

#[test]
fn ac3_1_again_at_the_start_adds_a_second_alarm() -> TestResult {
    let meeting = occ("review", at(14, 0)?, 3);
    let mut alarms = plan(&[meeting], &settings(5, true), &[], at(9, 0)?);
    let times: Vec<_> = alarms.iter().map(|a| (a.kind, a.at)).collect();
    assert_eq!(
        times,
        [
            (AlarmKind::Lead, at(13, 55)?),
            (AlarmKind::Start, at(14, 0)?)
        ]
    );

    step(&mut alarms, at(13, 55)?);
    assert!(dismiss(&mut alarms[0]));
    assert!(step(&mut alarms, at(13, 59)?).is_empty());
    let at_start = step(&mut alarms, at(14, 0)?);
    assert_eq!(at_start.len(), 1);
    assert_eq!(at_start[0].kind, AlarmKind::Start);
    assert_eq!(
        alarms[1].state,
        firing(at(14, 0)?),
        "dismissing the lead alarm keeps the start one"
    );
    Ok(())
}

#[test]
fn ac3_1_joining_at_the_lead_alarm_silences_the_start_alarm() -> TestResult {
    let meeting = occ("review", at(14, 0)?, 3);
    let mut alarms = plan(&[meeting], &settings(2, true), &[], at(9, 0)?);
    step(&mut alarms, at(13, 58)?);
    assert!(join(&mut alarms[0]));
    let closed = step(&mut alarms, at(13, 59)?);
    assert!(closed.iter().all(|t| !t.fires()));
    assert_eq!(alarms[1].state, AlarmState::Expired);
    assert!(step(&mut alarms, at(14, 0)?).is_empty());
    Ok(())
}

#[test]
fn ac9_5_lead_time_is_clamped_to_1_to_60_minutes() -> TestResult {
    let meeting = occ("review", at(14, 0)?, 3);
    for (given, minutes) in [(0, 1), (1, 1), (60, 60), (61, 60), (u32::MAX, 60)] {
        let alarms = plan(
            std::slice::from_ref(&meeting),
            &settings(given, false),
            &[],
            at(9, 0)?,
        );
        assert_eq!(
            alarms[0].at,
            at(14, 0)? - Duration::minutes(minutes),
            "{given}"
        );
    }
    Ok(())
}

#[test]
fn s3_2_snooze_returns() -> TestResult {
    let meeting = occ("review", at(14, 0)?, 3);
    let mut alarms = plan(
        std::slice::from_ref(&meeting),
        &settings(2, false),
        &[],
        at(9, 0)?,
    );
    step(&mut alarms, at(13, 58)?);
    let snoozed_at = at(13, 58)? + Duration::seconds(20);
    assert!(snooze(&mut alarms[0], 5, snoozed_at));
    let until = snoozed_at + Duration::minutes(5);
    assert_eq!(alarms[0].state, AlarmState::Snoozed { until });

    // A re-plan in between (a sync, a settings change) keeps the snooze.
    alarms = plan(&[meeting], &settings(2, false), &alarms, snoozed_at);
    assert_eq!(alarms[0].state, AlarmState::Snoozed { until });

    assert!(step(&mut alarms, until - Duration::seconds(1)).is_empty());
    let back = step(&mut alarms, until);
    assert_eq!(back.len(), 1);
    assert!(back[0].fires());
    assert_eq!(alarms[0].state, firing(until));

    // And it can be snoozed again: (Snoozed -> Firing)*.
    assert!(snooze(&mut alarms[0], 1, until));
    assert_eq!(step(&mut alarms, until + Duration::minutes(1)).len(), 1);
    Ok(())
}

#[test]
fn s3_3_dismiss_is_final_across_replan() -> TestResult {
    let meeting = occ("review", at(14, 0)?, 3);
    let s = settings(2, false);
    let mut alarms = plan(std::slice::from_ref(&meeting), &s, &[], at(9, 0)?);
    step(&mut alarms, at(13, 58)?);
    assert!(dismiss(&mut alarms[0]));

    // The app restarts: the rows come back from the store and the loop plans again.
    let stored: Vec<Alarm> = serde_json::from_str(&serde_json::to_string(&alarms)?)?;
    let restart = at(13, 58)? + Duration::seconds(30);
    let mut alarms = plan(std::slice::from_ref(&meeting), &s, &stored, restart);
    assert_eq!(states(&alarms), [AlarmState::Dismissed]);
    for minutes in 0..=15 {
        let fired = step(&mut alarms, at(13, 58)? + Duration::minutes(minutes));
        assert!(fired.iter().all(|t| !t.fires()), "minute {minutes}");
    }
    assert_eq!(alarms[0].state, AlarmState::Dismissed, "not even Expired");

    // Changing the lead time re-times alarms but does not bring a dismissed one back.
    let alarms = plan(&[meeting], &settings(10, false), &alarms, at(13, 55)?);
    assert_eq!(states(&alarms), [AlarmState::Dismissed]);
    assert_eq!(alarms[0].at, at(13, 50)?);
    Ok(())
}

#[test]
fn s3_4_wake_inside_window_fires_with_started_minutes() -> TestResult {
    let meeting = occ("review", at(14, 0)?, 3);
    let mut alarms = plan(&[meeting], &settings(2, false), &[], at(13, 50)?);
    // The machine slept from 13:50 and wakes at 14:03; the first tick sees that.
    let woke = at(14, 3)?;
    let fired = step(&mut alarms, woke);
    assert_eq!(fired.len(), 1);
    assert!(fired[0].fires());
    assert_eq!(alarms[0].state, firing(woke));
    assert_eq!(
        countdown(alarms[0].start, woke),
        Countdown::StartedAgo { minutes: 3 }
    );
    Ok(())
}

#[test]
fn s3_5_wake_outside_window_expires() -> TestResult {
    let meeting = occ("review", at(14, 0)?, 3);
    let mut alarms = plan(&[meeting], &settings(2, true), &[], at(13, 50)?);
    let fired = step(&mut alarms, at(14, 30)?);
    assert_eq!(fired.len(), 2);
    assert!(fired.iter().all(|t| !t.fires()), "nothing fires");
    assert_eq!(states(&alarms), [AlarmState::Expired, AlarmState::Expired]);
    Ok(())
}

#[test]
fn ac3_2_expiry_is_ten_minutes_after_the_start() -> TestResult {
    let meeting = occ("review", at(14, 0)?, 3);
    let mut alarms = plan(
        std::slice::from_ref(&meeting),
        &settings(2, false),
        &[],
        at(9, 0)?,
    );
    step(&mut alarms, at(13, 58)?);
    let before = at(14, 10)? - Duration::seconds(1);
    assert!(
        step(&mut alarms, before).is_empty(),
        "still showing at 14:09:59"
    );
    let expired = step(&mut alarms, at(14, 10)?);
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].to, AlarmState::Expired);

    // A snoozed alarm expires at the same moment, even when its snooze outlasts the window.
    let mut alarms = plan(&[meeting], &settings(2, false), &[], at(9, 0)?);
    step(&mut alarms, at(14, 8)?);
    assert!(snooze(&mut alarms[0], 5, at(14, 8)?));
    assert_eq!(step(&mut alarms, at(14, 10)?).len(), 1);
    assert_eq!(alarms[0].state, AlarmState::Expired);
    Ok(())
}

#[test]
fn s3_6_overlapping_meetings_group() -> TestResult {
    let a = occ("design review", at(14, 0)?, 4);
    let b = occ("1:1 with Sam", at(14, 0)? + Duration::seconds(30), 2);
    let later = occ("retro", at(14, 1)?, 6);
    let all = [later, b, a];
    let mut alarms = plan(&all, &settings(2, false), &[], at(9, 0)?);
    step(&mut alarms, at(13, 59)?);
    assert_eq!(
        alarms.iter().filter(|a| a.state.since().is_some()).count(),
        3
    );

    let groups = group_firing(&alarms, &all);
    assert_eq!(groups.len(), 2, "14:00 and 14:01");
    assert_eq!(groups[0].minute, at(14, 0)?);
    let titles: Vec<_> = groups[0]
        .occurrences
        .iter()
        .map(|o| o.title.as_str())
        .collect();
    assert_eq!(titles, ["design review", "1:1 with Sam"]);
    let links: Vec<_> = groups[0]
        .occurrences
        .iter()
        .map(|o| o.join.clone())
        .collect();
    assert_eq!(
        links,
        [
            JoinTarget::Link {
                url: "https://example.com/design review".to_string(),
                host: "example.com".to_string(),
                recognised: false
            },
            JoinTarget::Link {
                url: "https://example.com/1:1 with Sam".to_string(),
                host: "example.com".to_string(),
                recognised: false
            }
        ],
        "each keeps its own Join target"
    );
    assert_eq!(groups[1].minute, at(14, 1)?);
    assert_eq!(groups[1].occurrences.len(), 1);

    // Lead and start alarms of one meeting both firing list it once; alarms not firing do not count.
    let one = [occ("review", at(14, 0)?, 3)];
    let mut both = plan(&one, &settings(2, true), &[], at(9, 0)?);
    assert!(group_firing(&both, &one).is_empty());
    step(&mut both, at(14, 0)?);
    assert_eq!(group_firing(&both, &one)[0].occurrences.len(), 1);
    Ok(())
}

#[test]
fn ac4_7_countdown_counts_to_the_start_then_from_it() -> TestResult {
    let start = at(14, 0)?;
    let s = |secs: i64| start + Duration::seconds(secs);
    let cases = [
        (s(-120), Countdown::StartsIn { minutes: 2 }),
        (s(-61), Countdown::StartsIn { minutes: 2 }),
        (s(-60), Countdown::StartsIn { minutes: 1 }),
        (s(-1), Countdown::StartsIn { minutes: 1 }),
        (s(0), Countdown::StartingNow),
        (s(59), Countdown::StartingNow),
        (s(60), Countdown::StartedAgo { minutes: 1 }),
        (s(180), Countdown::StartedAgo { minutes: 3 }),
        (s(599), Countdown::StartedAgo { minutes: 9 }),
    ];
    for (now, want) in cases {
        assert_eq!(countdown(start, now), want, "{now}");
    }
    Ok(())
}

#[test]
fn plan_drops_alarms_of_gone_or_ignored_occurrences() -> TestResult {
    let (a, b) = (occ("a", at(14, 0)?, 3), occ("b", at(15, 0)?, 3));
    let s = settings(2, true);
    let both = plan(&[a.clone(), b], &s, &[], at(9, 0)?);
    assert_eq!(both.len(), 4);
    let only_a = plan(&[a], &s, &both, at(9, 0)?);
    assert_eq!(only_a.len(), 2);
    assert!(only_a.iter().all(|x| x.occurrence.event.0 == "cal:a"));
    Ok(())
}

#[test]
fn plan_adds_no_alarm_for_a_meeting_whose_window_closed() -> TestResult {
    let old = occ("old", at(14, 0)?, 3);
    assert!(plan(
        std::slice::from_ref(&old),
        &settings(2, true),
        &[],
        at(14, 10)?
    )
    .is_empty());
    assert_eq!(plan(&[old], &settings(2, true), &[], at(14, 9)?).len(), 2);
    Ok(())
}

#[test]
fn ac3_3_final_alarms_survive_their_occurrence_being_filtered_out_and_back() -> TestResult {
    let meeting = occ("review", at(14, 0)?, 3);
    let one = std::slice::from_ref(&meeting);
    let s = settings(2, true);
    let mut alarms = plan(one, &s, &[], at(9, 0)?);
    step(&mut alarms, at(13, 58)?);
    assert!(dismiss(&mut alarms[0]));
    step(&mut alarms, at(14, 0)?);
    assert!(join(&mut alarms[1]));

    // A keyword rule switched on: the occurrence leaves the input, the final alarms stay.
    let hidden = plan(&[], &s, &alarms, at(14, 1)?);
    assert_eq!(states(&hidden), [AlarmState::Dismissed, AlarmState::Joined]);
    assert_eq!(
        plan(&[], &s, &hidden, at(14, 1)?),
        hidden,
        "and it is stable"
    );

    // Switched off again: nothing is re-created as Scheduled, so nothing fires.
    let mut back = plan(one, &s, &hidden, at(14, 2)?);
    assert_eq!(states(&back), [AlarmState::Dismissed, AlarmState::Joined]);
    for minutes in 0..=12 {
        assert!(step(&mut back, at(14, 0)? + Duration::minutes(minutes))
            .iter()
            .all(|t| !t.fires()));
    }

    // "Again at the start" switched off and on: the start alarm keeps its state.
    let mut alarms = plan(one, &s, &[], at(9, 0)?);
    step(&mut alarms, at(13, 58)?);
    assert!(dismiss(&mut alarms[0]));
    step(&mut alarms, at(14, 0)?);
    assert!(dismiss(&mut alarms[1]));
    let off = plan(one, &settings(2, false), &alarms, at(14, 1)?);
    assert_eq!(states(&off), [AlarmState::Dismissed, AlarmState::Dismissed]);
    let on = plan(one, &s, &off, at(14, 1)?);
    assert_eq!(states(&on), [AlarmState::Dismissed, AlarmState::Dismissed]);

    // A Scheduled alarm is not kept when its occurrence goes.
    let fresh = plan(one, &s, &[], at(9, 0)?);
    assert!(plan(&[], &s, &fresh, at(9, 0)?).is_empty());
    Ok(())
}

#[test]
fn ac3_3_a_series_instance_moved_later_after_expiry_fires_at_the_new_time() -> TestResult {
    let meeting = occ("review", at(14, 0)?, 3);
    let s = settings(2, false);
    let mut alarms = plan(std::slice::from_ref(&meeting), &s, &[], at(13, 0)?);
    step(&mut alarms, at(14, 12)?);
    assert_eq!(states(&alarms), [AlarmState::Expired]);

    // The same instance (same key) is moved to 15:00 while 14:20 is "now".
    let mut moved = meeting.clone();
    moved.start = at(15, 0)?.with_timezone(&LOCAL);
    let mut alarms = plan(std::slice::from_ref(&moved), &s, &alarms, at(14, 20)?);
    assert_eq!(states(&alarms), [AlarmState::Scheduled]);
    assert_eq!(alarms[0].at, at(14, 58)?);
    assert!(step(&mut alarms, at(14, 57)?).is_empty());
    assert!(step(&mut alarms, at(14, 58)?)[0].fires());

    // Moved later but into a window that is already closed: stays Expired.
    let mut alarms = plan(std::slice::from_ref(&meeting), &s, &[], at(13, 0)?);
    step(&mut alarms, at(14, 12)?);
    moved.start = at(14, 5)?.with_timezone(&LOCAL);
    let alarms = plan(std::slice::from_ref(&moved), &s, &alarms, at(14, 20)?);
    assert_eq!(states(&alarms), [AlarmState::Expired]);

    // Dismissed and Joined stay final when the meeting moves.
    moved.start = at(15, 0)?.with_timezone(&LOCAL);
    for done in [dismiss as fn(&mut Alarm) -> bool, join] {
        let mut alarms = plan(std::slice::from_ref(&meeting), &s, &[], at(13, 0)?);
        assert!(done(&mut alarms[0]));
        let kept = plan(std::slice::from_ref(&moved), &s, &alarms, at(14, 20)?);
        assert_eq!(kept[0].state, alarms[0].state);
        assert_eq!(kept[0].at, at(14, 58)?, "re-timed, still final");
    }
    Ok(())
}

#[test]
fn ac3_3_a_joined_meeting_stays_closed_when_it_moves_later() -> TestResult {
    // The Expired start alarm here is the join closing it, not a missed meeting.
    let meeting = occ("review", at(14, 0)?, 3);
    let s = settings(2, true);
    let mut alarms = plan(std::slice::from_ref(&meeting), &s, &[], at(9, 0)?);
    step(&mut alarms, at(13, 58)?);
    assert!(join(&mut alarms[0]));
    step(&mut alarms, at(13, 59)?);
    assert_eq!(states(&alarms), [AlarmState::Joined, AlarmState::Expired]);
    let mut moved = meeting;
    moved.start = at(14, 30)?.with_timezone(&LOCAL);
    let kept = plan(&[moved], &s, &alarms, at(14, 0)?);
    assert_eq!(states(&kept), [AlarmState::Joined, AlarmState::Expired]);
    Ok(())
}

#[test]
fn ac3_3_joined_alarm_of_a_gone_occurrence_stays_joined() -> TestResult {
    let meeting = occ("gone", at(14, 0)?, 3);
    let s = settings(2, true);
    let (lead_at, start) = (at(13, 58)?, at(14, 0)?);
    let row = |kind, state| Alarm {
        occurrence: meeting.key.clone(),
        at: lead_at,
        start,
        kind,
        state,
    };
    let joined = row(AlarmKind::Lead, AlarmState::Joined);
    let kept = plan(&[], &s, std::slice::from_ref(&joined), at(14, 1)?);
    assert_eq!(kept, vec![joined.clone()]);

    // Two rows for one identity (the store's primary key forbids it, but a caller can pass them):
    // the last one wins, the same rule `plan` applies when the occurrence is present, so a
    // Joined row never turns into the Dismissed row that came before it, nor the reverse.
    let dismissed = row(AlarmKind::Lead, AlarmState::Dismissed);
    for (first, last) in [(&dismissed, &joined), (&joined, &dismissed)] {
        let rows = [first.clone(), last.clone()];
        assert_eq!(plan(&[], &s, &rows, at(14, 1)?), vec![last.clone()]);
        assert_eq!(
            plan(std::slice::from_ref(&meeting), &s, &rows, at(14, 1)?)
                .iter()
                .find(|a| a.kind == AlarmKind::Lead)
                .map(|a| a.state),
            Some(last.state)
        );
    }
    // A final row followed by a Firing one for the same identity: the Firing one is the row.
    let firing_row = row(AlarmKind::Lead, firing(at(13, 58)?));
    let rows = [joined, firing_row];
    assert!(plan(&[], &s, &rows, at(14, 1)?).is_empty());
    Ok(())
}

fn firing_count(alarms: &[Alarm]) -> usize {
    alarms.iter().filter(|a| a.state.since().is_some()).count()
}

#[test]
fn ac3_1_lead_and_start_never_put_two_interrupts_up_for_one_meeting() -> TestResult {
    let meeting = occ("review", at(14, 0)?, 3);
    let s = settings(2, true);

    // Both due in the same tick (wake at 14:03): the lead one fires, the start one expires.
    let mut alarms = plan(std::slice::from_ref(&meeting), &s, &[], at(13, 50)?);
    let t = step(&mut alarms, at(14, 3)?);
    assert_eq!(t.len(), 2);
    assert_eq!(
        states(&alarms),
        [firing(at(14, 3)?), AlarmState::Expired],
        "lead fires, start is skipped"
    );

    // The start alarm comes due while the lead one is still showing.
    let mut alarms = plan(std::slice::from_ref(&meeting), &s, &[], at(9, 0)?);
    step(&mut alarms, at(13, 58)?);
    let t = step(&mut alarms, at(14, 0)?);
    assert_eq!(firing_count(&alarms), 1);
    assert!(t.iter().all(|t| !t.fires()));
    assert_eq!(alarms[1].state, AlarmState::Expired);

    // ... or while the lead one is snoozed past the start.
    let mut alarms = plan(std::slice::from_ref(&meeting), &s, &[], at(9, 0)?);
    step(&mut alarms, at(13, 58)?);
    assert!(snooze(&mut alarms[0], 5, at(13, 58)?));
    step(&mut alarms, at(14, 0)?);
    assert_eq!(alarms[1].state, AlarmState::Expired);
    step(&mut alarms, at(14, 3)?);
    assert_eq!(
        alarms[0].state,
        firing(at(14, 3)?),
        "the snoozed lead still returns"
    );

    // The start alarm snoozed from the panel before it was due fires early; the lead alarm,
    // due later in the same minute or after, is then skipped.
    let mut alarms = plan(std::slice::from_ref(&meeting), &s, &[], at(9, 0)?);
    assert!(snooze(&mut alarms[1], 1, at(13, 0)?));
    step(&mut alarms, at(13, 1)?);
    assert_eq!(alarms[1].state, firing(at(13, 1)?));
    step(&mut alarms, at(13, 58)?);
    assert_eq!(firing_count(&alarms), 1);
    assert_eq!(alarms[0].state, AlarmState::Expired);

    // Once the lead alarm was dismissed, the start alarm is the second chance and fires.
    let mut alarms = plan(std::slice::from_ref(&meeting), &s, &[], at(9, 0)?);
    step(&mut alarms, at(13, 58)?);
    assert!(dismiss(&mut alarms[0]));
    step(&mut alarms, at(14, 0)?);
    assert_eq!(alarms[1].state, firing(at(14, 0)?));
    Ok(())
}

fn quiet(start: (u32, u32), end: (u32, u32), weekends: bool) -> TestResult<QuietHours> {
    let time = |(h, m): (u32, u32)| NaiveTime::from_hms_opt(h, m, 0).ok_or("bad time");
    Ok(QuietHours {
        start: time(start)?,
        end: time(end)?,
        weekends,
        tz: Tz::UTC,
    })
}

#[test]
fn ac7_3_pause_suppresses_firing_and_resumes_inside_the_window() -> TestResult {
    let meeting = occ("review", at(14, 0)?, 3);
    let mut alarms = plan(&[meeting], &settings(2, false), &[], at(9, 0)?);
    let paused_until = Some(at(14, 3)?);
    // Due at 13:58, but paused: nothing fires and nothing changes.
    assert!(tick(&alarms, at(13, 58)?, paused_until, None).is_empty());
    assert!(tick(&alarms, at(14, 2)?, paused_until, None).is_empty());
    // Pause over at 14:03, window open until 14:10: it fires at once, "Started 3 min ago".
    let resumed = tick(&alarms, at(14, 3)?, paused_until, None);
    assert_eq!(resumed.len(), 1);
    assert!(resumed[0].fires());
    // A pause that outlasts the window expires the alarm, silently, even while still paused.
    let long = Some(at(15, 0)?);
    let expired = tick(&alarms, at(14, 10)?, long, None);
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].to, AlarmState::Expired);
    apply_transitions(&mut alarms, &expired);
    assert!(tick(&alarms, at(15, 0)?, long, None).is_empty());
    Ok(())
}

#[test]
fn ac8_1_quiet_hours_cover_the_window_across_midnight_and_weekends() -> TestResult {
    let night = quiet((22, 0), (7, 30), false)?;
    assert!(night.is_quiet(utc(2026, 10, 22, 23, 0)?));
    assert!(night.is_quiet(utc(2026, 10, 23, 7, 29)?));
    assert!(!night.is_quiet(utc(2026, 10, 23, 7, 30)?));
    assert!(!night.is_quiet(utc(2026, 10, 22, 21, 59)?));

    let noon = quiet((12, 0), (13, 0), false)?;
    assert!(noon.is_quiet(utc(2026, 10, 22, 12, 30)?));
    assert!(!noon.is_quiet(utc(2026, 10, 22, 13, 0)?));

    let empty = quiet((9, 0), (9, 0), false)?;
    assert!(!empty.is_quiet(utc(2026, 10, 22, 9, 0)?));

    // 24 Oct 2026 is a Saturday.
    let weekend = quiet((22, 0), (7, 0), true)?;
    assert!(weekend.is_quiet(utc(2026, 10, 24, 14, 0)?));
    assert!(!night.is_quiet(utc(2026, 10, 24, 14, 0)?));
    assert!(!weekend.is_quiet(utc(2026, 10, 22, 14, 0)?));

    // A meeting that starts inside the window never fires, also not after the window ends while
    // its own window is still open; the alarm stays planned (the tray lists it) until it expires.
    let alarms = plan(
        &[occ("early", at(7, 25)?, 3)],
        &settings(2, false),
        &[],
        at(1, 0)?,
    );
    assert!(tick(&alarms, at(7, 24)?, None, Some(&night)).is_empty());
    assert!(tick(&alarms, at(7, 30)?, None, Some(&night)).is_empty());
    let t = tick(&alarms, at(7, 35)?, None, Some(&night));
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].to, AlarmState::Expired);

    // Expiry is not suppressed: still quiet at 07:11, but the 07:00 meeting's window closed at 07:10.
    let missed = plan(
        &[occ("early", at(7, 0)?, 3)],
        &settings(2, false),
        &[],
        at(1, 0)?,
    );
    let t = tick(&missed, at(7, 11)?, None, Some(&night));
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].to, AlarmState::Expired);
    Ok(())
}

/// Tick with quiet hours at `now` and apply the result.
fn quiet_step(alarms: &mut [Alarm], now: DateTime<Utc>, q: &QuietHours) -> Vec<Transition> {
    let transitions = tick(alarms, now, None, Some(q));
    apply_transitions(alarms, &transitions);
    transitions
}

#[test]
fn ac8_1_an_alarm_due_inside_quiet_hours_comes_at_the_start_of_a_meeting_after_them() -> TestResult
{
    let night = quiet((18, 0), (9, 0), false)?;
    // 09:00 meeting, lead 2 min: the lead time 08:58 is quiet, the start is not.
    for again_at_start in [false, true] {
        let mut alarms = plan(
            &[occ("standup", at(9, 0)?, 3)],
            &settings(2, again_at_start),
            &[],
            at(1, 0)?,
        );
        assert!(quiet_step(&mut alarms, at(8, 58)?, &night).is_empty());
        assert!(quiet_step(&mut alarms, at(8, 59)?, &night).is_empty());
        let fired = quiet_step(&mut alarms, at(9, 0)?, &night);
        assert_eq!(
            fired.iter().filter(|t| t.fires()).count(),
            1,
            "one interrupt at the start, again_at_start {again_at_start}"
        );
        assert_eq!(alarms[0].state, firing(at(9, 0)?));
    }
    // Quiet hours that end before the meeting starts leave a lead time after them alone.
    let mut alarms = plan(
        &[occ("later", at(9, 30)?, 3)],
        &settings(2, false),
        &[],
        at(1, 0)?,
    );
    assert!(quiet_step(&mut alarms, at(9, 27)?, &night).is_empty());
    assert_eq!(quiet_step(&mut alarms, at(9, 28)?, &night).len(), 1);
    Ok(())
}

#[test]
fn ac8_1_nothing_fires_inside_quiet_hours_on_a_weekend() -> TestResult {
    let weekends = quiet((18, 0), (9, 0), true)?;
    // Saturday 24 Oct 2026, 14:00: inside the weekend, outside the daily window.
    let saturday = utc(2026, 10, 24, 14, 0)?;
    let mut alarms = plan(
        &[occ("sat", saturday, 3)],
        &settings(2, true),
        &[],
        saturday - chrono::Duration::hours(1),
    );
    for minute in 0..20 {
        let now = saturday - chrono::Duration::minutes(5) + chrono::Duration::minutes(minute);
        assert!(quiet_step(&mut alarms, now, &weekends)
            .iter()
            .all(|t| !t.fires()));
    }
    assert!(alarms.iter().all(|a| a.state == AlarmState::Expired));
    Ok(())
}

/// Quiet hours read local wall-clock time, so a window keeps its hours across a DST change.
#[test]
fn ac8_1_quiet_hours_follow_local_time_across_dst() -> TestResult {
    let berlin = |start: (u32, u32), end: (u32, u32)| -> TestResult<QuietHours> {
        Ok(QuietHours {
            tz: LOCAL,
            ..quiet(start, end, false)?
        })
    };
    // Sunday 29 Mar 2026: 02:00 local jumps to 03:00 (00:59 UTC is 01:59 CET, 01:00 UTC is 03:00 CEST).
    let spring = berlin((1, 30), (3, 30))?;
    assert!(!spring.is_quiet(utc(2026, 3, 29, 0, 29)?), "01:29 CET");
    assert!(spring.is_quiet(utc(2026, 3, 29, 0, 30)?), "01:30 CET");
    assert!(spring.is_quiet(utc(2026, 3, 29, 1, 0)?), "03:00 CEST");
    assert!(spring.is_quiet(utc(2026, 3, 29, 1, 29)?), "03:29 CEST");
    assert!(!spring.is_quiet(utc(2026, 3, 29, 1, 30)?), "03:30 CEST");
    // A window inside the missing hour is empty that night.
    let gap = berlin((2, 15), (2, 45))?;
    for minute in 0..180 {
        let t = utc(2026, 3, 29, 0, 0)? + chrono::Duration::minutes(minute);
        assert!(!gap.is_quiet(t), "{t}");
    }
    // Sunday 25 Oct 2026: 03:00 CEST goes back to 02:00 CET, so 02:00 to 03:00 local happens twice.
    let autumn = berlin((2, 0), (3, 0))?;
    assert!(autumn.is_quiet(utc(2026, 10, 25, 0, 30)?), "02:30 CEST");
    assert!(autumn.is_quiet(utc(2026, 10, 25, 1, 30)?), "02:30 CET");
    assert!(!autumn.is_quiet(utc(2026, 10, 25, 2, 0)?), "03:00 CET");
    // 18:00 to 09:00 across the change: the night is an hour longer, still 18:00 to 09:00 local.
    let night = berlin((18, 0), (9, 0))?;
    assert!(night.is_quiet(utc(2026, 10, 24, 16, 0)?), "18:00 CEST");
    assert!(night.is_quiet(utc(2026, 10, 25, 7, 59)?), "08:59 CET");
    assert!(!night.is_quiet(utc(2026, 10, 25, 8, 0)?), "09:00 CET");
    Ok(())
}

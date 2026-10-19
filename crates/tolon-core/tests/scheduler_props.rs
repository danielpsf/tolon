//! Property tests for the scheduler (docs/agents/core.md): for any events and any sequence of
//! ticks and actions, an alarm fires at most once per Scheduled-to-Firing transition, a Dismissed
//! alarm never fires, `plan` is idempotent and leaves a non-Scheduled state alone, and no meeting
//! ever has two interrupts up at once. Ticks run with random pauses and quiet hours.

mod common;

use chrono::{DateTime, Duration, NaiveTime, Utc};
use chrono_tz::Tz;
use common::{at, occ};
use proptest::prelude::*;
use std::collections::{HashMap, HashSet};
use tolon_core::{
    already_there, apply_transitions, dismiss, join, plan, snooze, tick, Alarm, AlarmKind,
    AlarmSettings, AlarmState, Occurrence, OccurrenceKey, QuietHours,
};

const CASES: u32 = 512;

fn base() -> DateTime<Utc> {
    at(12, 0).unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
}

fn secs(s: i64) -> DateTime<Utc> {
    base() + Duration::seconds(s)
}

/// Up to six meetings starting up to two hours after `base`, to the second, so several share a
/// minute and some share a start. Titles are unique so keys are.
fn occurrences() -> impl Strategy<Value = Vec<Occurrence>> {
    prop::collection::vec((0i64..7200, 0u32..8), 0..6).prop_map(|v| {
        v.into_iter()
            .enumerate()
            .map(|(i, (start, people))| occ(&format!("m{i}"), secs(start), people))
            .collect()
    })
}

fn alarm_settings() -> impl Strategy<Value = AlarmSettings> {
    (0u32..=90, any::<bool>()).prop_map(|(lead_minutes, again_at_start)| AlarmSettings {
        lead_minutes,
        again_at_start,
    })
}

#[derive(Debug, Clone)]
enum Op {
    /// `now` offset in seconds from the base (negative: before it), an optional pause end, and
    /// optional quiet hours.
    Tick(i64, Option<i64>, Option<Quiet>),
    Snooze(usize, u32),
    Dismiss(usize),
    Join(usize),
    AlreadyThere(usize),
    /// A re-plan, as after a sync or a settings change.
    Replan(i64),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        6 => (-600i64..10_000, prop::option::of(-600i64..10_000), quiet())
            .prop_map(|(n, p, q)| Op::Tick(n, p, q)),
        2 => (0usize..16, 0u32..12).prop_map(|(i, m)| Op::Snooze(i, m)),
        1 => (0usize..16).prop_map(Op::Dismiss),
        1 => (0usize..16).prop_map(Op::Join),
        1 => (0usize..16).prop_map(Op::AlreadyThere),
        1 => (-600i64..10_000).prop_map(Op::Replan),
    ]
}

/// Daily window as minutes of the day (start, end) and the weekend switch. The test day is a
/// Thursday and the ticks span under three hours, so the window is what matters.
type Quiet = (u32, u32, bool);

fn quiet() -> impl Strategy<Value = Option<Quiet>> {
    prop::option::of((0u32..1440, 0u32..1440, any::<bool>()))
}

fn quiet_hours((start, end, weekends): Quiet) -> Option<QuietHours> {
    let time = |m: u32| NaiveTime::from_hms_opt(m / 60, m % 60, 0);
    Some(QuietHours {
        start: time(start)?,
        end: time(end)?,
        weekends,
        tz: Tz::UTC,
    })
}

type Id = (OccurrenceKey, AlarmKind);

fn id(a: &Alarm) -> Id {
    (a.occurrence.clone(), a.kind)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(CASES))]

    /// Ticks alone, in any order, with repeated and backwards `now`: each alarm gets at most one
    /// Scheduled-to-Firing transition, and no tick reports an alarm twice.
    #[test]
    fn fires_at_most_once_per_scheduled_transition(
        occs in occurrences(),
        settings in alarm_settings(),
        nows in prop::collection::vec((-600i64..10_000, 1usize..4, quiet()), 1..40),
    ) {
        let mut alarms = plan(&occs, &settings, &[], base());
        let mut fires: HashMap<Id, u32> = HashMap::new();
        for (now, repeat, q) in nows {
            let q = q.and_then(quiet_hours);
            for _ in 0..repeat {
                let transitions = tick(&alarms, secs(now), None, q.as_ref());
                let seen: HashSet<Id> = transitions.iter().map(|t| (t.occurrence.clone(), t.kind)).collect();
                prop_assert_eq!(seen.len(), transitions.len(), "an alarm twice in one tick");
                for t in transitions.iter().filter(|t| t.fires()) {
                    *fires.entry((t.occurrence.clone(), t.kind)).or_default() += 1;
                }
                apply_transitions(&mut alarms, &transitions);
            }
        }
        for (alarm, n) in &fires {
            prop_assert!(*n <= 1, "{alarm:?} fired {n} times");
        }
    }

    /// Ticks, snoozes, dismissals, joins and re-plans in any order. A snooze buys one more fire;
    /// dismiss, join and already-there end the alarm for good.
    #[test]
    fn dismissed_never_fires_and_fires_are_bounded_by_snoozes(
        occs in occurrences(),
        settings in alarm_settings(),
        ops in prop::collection::vec(op(), 1..60),
    ) {
        let mut alarms = plan(&occs, &settings, &[], base());
        let mut fires: HashMap<Id, u32> = HashMap::new();
        let mut allowed: HashMap<Id, u32> = HashMap::new();
        let mut ended: HashSet<Id> = HashSet::new();
        let mut clock = 0i64;

        for op in ops {
            match op {
                Op::Tick(now, pause, q) => {
                    clock = now;
                    let q = q.and_then(quiet_hours);
                    let transitions = tick(&alarms, secs(now), pause.map(secs), q.as_ref());
                    for t in &transitions {
                        let key = (t.occurrence.clone(), t.kind);
                        prop_assert!(!ended.contains(&key) || !t.fires(), "ended alarm fired: {key:?}");
                        if t.fires() {
                            *fires.entry(key).or_default() += 1;
                        }
                    }
                    apply_transitions(&mut alarms, &transitions);
                    // One interrupt per meeting: never two alarms of one occurrence Firing.
                    for a in alarms.iter().filter(|a| a.state.since().is_some()) {
                        let n = alarms
                            .iter()
                            .filter(|b| b.occurrence == a.occurrence && b.state.since().is_some())
                            .count();
                        prop_assert_eq!(n, 1, "two Firing alarms for {:?}", a.occurrence);
                    }
                }
                Op::Replan(now) => {
                    alarms = plan(&occs, &settings, &alarms, secs(now));
                }
                Op::Snooze(i, minutes) => {
                    if let Some(a) = pick(&mut alarms, i) {
                        if snooze(a, minutes, secs(clock)) {
                            *allowed.entry(id(a)).or_default() += 1;
                        }
                    }
                }
                Op::Dismiss(i) | Op::Join(i) | Op::AlreadyThere(i) => {
                    if let Some(a) = pick(&mut alarms, i) {
                        let done = match op {
                            Op::Dismiss(_) => dismiss(a),
                            Op::Join(_) => join(a),
                            _ => already_there(a),
                        };
                        if done {
                            ended.insert(id(a));
                        }
                    }
                }
            }
            for a in &alarms {
                if ended.contains(&id(a)) {
                    prop_assert!(
                        matches!(a.state, AlarmState::Dismissed | AlarmState::Joined),
                        "{a:?} left its final state"
                    );
                }
            }
        }
        for (alarm, n) in &fires {
            let limit = 1 + allowed.get(alarm).copied().unwrap_or(0);
            prop_assert!(*n <= limit, "{alarm:?} fired {n} times, limit {limit}");
        }
    }

    /// `plan(plan(x)) == plan(x)`, from stored alarms in any state, planned under other settings
    /// (so some are stale or missing) and mixed with alarms of occurrences that are gone.
    #[test]
    fn plan_is_idempotent(
        occs in occurrences(),
        settings in alarm_settings(),
        stored_settings in alarm_settings(),
        states in prop::collection::vec(state(), 0..12),
        shifts in prop::collection::vec(0i64..3600, 12),
        strays in prop::collection::vec(stray_alarm(), 0..4),
        now in -600i64..10_000,
    ) {
        let stored = stored_alarms(&occs, &stored_settings, states, &shifts, strays);
        let once = plan(&occs, &settings, &stored, secs(now));
        let twice = plan(&occs, &settings, &once, secs(now));
        prop_assert_eq!(&once, &twice);

        // Re-planning from the plan after the clock moved still settles: a second pass is a no-op.
        let later = secs(now + 300);
        let moved = plan(&occs, &settings, &once, later);
        prop_assert_eq!(&moved, &plan(&occs, &settings, &moved, later));

        // Every alarm planned belongs to a given occurrence, or is a final one kept as stored.
        for a in &once {
            prop_assert!(a.state.is_final() || occs.iter().any(|o| o.key == a.occurrence));
        }
    }

    /// `plan` leaves a non-Scheduled state alone, and keeps every final alarm. The one change it
    /// may make is Expired to Scheduled, for an instance that moved later into an open window.
    #[test]
    fn plan_never_changes_a_non_scheduled_state_except_a_moved_expired_one(
        occs in occurrences(),
        settings in alarm_settings(),
        stored_settings in alarm_settings(),
        states in prop::collection::vec(state(), 0..12),
        shifts in prop::collection::vec(0i64..3600, 12),
        strays in prop::collection::vec(stray_alarm(), 0..4),
        now in -600i64..10_000,
    ) {
        let stored = stored_alarms(&occs, &stored_settings, states, &shifts, strays);
        let out = plan(&occs, &settings, &stored, secs(now));
        // The last row for an identity is the one `plan` reads.
        let last: HashMap<Id, &Alarm> = stored.iter().map(|a| (id(a), a)).collect();
        for (key, old) in last {
            if old.state == AlarmState::Scheduled {
                continue;
            }
            let new = out.iter().find(|a| id(a) == key);
            if old.state.is_final() {
                prop_assert!(new.is_some(), "final alarm dropped: {old:?}");
            }
            if let Some(new) = new {
                let rearmed = old.state == AlarmState::Expired
                    && new.state == AlarmState::Scheduled
                    && new.start > old.start
                    && new.expires_at() > secs(now);
                prop_assert!(new.state == old.state || rearmed, "{old:?} became {new:?}");
            }
        }
    }
}

fn state() -> impl Strategy<Value = AlarmState> {
    prop_oneof![
        Just(AlarmState::Scheduled),
        (0i64..10_000).prop_map(|s| AlarmState::Firing { since: secs(s) }),
        (0i64..10_000).prop_map(|s| AlarmState::Snoozed { until: secs(s) }),
        Just(AlarmState::Dismissed),
        Just(AlarmState::Joined),
        Just(AlarmState::Expired),
    ]
}

/// A stored alarm of an occurrence no feed has any more (the key names no generated meeting).
fn stray_alarm() -> impl Strategy<Value = Alarm> {
    (0i64..7200, any::<bool>(), state()).prop_map(|(start, is_lead, state)| Alarm {
        occurrence: occ("gone", secs(start), 2).key,
        at: secs(start),
        start: secs(start),
        kind: if is_lead {
            AlarmKind::Lead
        } else {
            AlarmKind::Start
        },
        state,
    })
}

fn pick(alarms: &mut [Alarm], i: usize) -> Option<&mut Alarm> {
    let n = alarms.len();
    alarms.get_mut(i.checked_rem(n)?)
}

/// Stored rows as the store could hand them back: the alarms of `occs` planned under other
/// settings, given random states and a `start` that is earlier than the occurrence's by a random
/// shift (a meeting moved later since), plus rows of occurrences that are gone.
fn stored_alarms(
    occs: &[Occurrence],
    settings: &AlarmSettings,
    states: Vec<AlarmState>,
    shifts: &[i64],
    strays: Vec<Alarm>,
) -> Vec<Alarm> {
    let mut stored: Vec<Alarm> = plan(occs, settings, &[], base())
        .into_iter()
        .zip(states)
        .zip(shifts)
        .map(|((mut a, state), shift)| {
            a.state = state;
            a.start -= Duration::seconds(*shift);
            a
        })
        .collect();
    stored.extend(strays);
    stored
}

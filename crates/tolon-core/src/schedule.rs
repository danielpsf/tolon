//! Planning alarms from occurrences, ticking them through the state machine, grouping the ones
//! that fire together, and the countdown value the interrupt shows. Pure: no clock, no I/O.

use chrono::{DateTime, Datelike, Duration, DurationRound, NaiveTime, Utc, Weekday};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::alarm::{Alarm, AlarmKind, AlarmState};
use crate::model::{Occurrence, OccurrenceKey};

pub const MIN_LEAD_MINUTES: u32 = 1;
pub const MAX_LEAD_MINUTES: u32 = 60;

/// AC9.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlarmSettings {
    /// 1 to 60; `plan` clamps anything else.
    pub lead_minutes: u32,
    pub again_at_start: bool,
}

impl Default for AlarmSettings {
    fn default() -> Self {
        Self {
            lead_minutes: 2,
            again_at_start: false,
        }
    }
}

/// AC8.1: a daily window and a weekend switch. Inside them nothing fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuietHours {
    /// The window starts at this local time and ends at `end`, past midnight when `end <= start`.
    /// `start == end` is an empty window.
    pub start: NaiveTime,
    pub end: NaiveTime,
    /// Saturday and Sunday are quiet all day.
    pub weekends: bool,
    /// The zone the window and the weekend are read in: the machine's.
    pub tz: Tz,
}

impl QuietHours {
    pub fn is_quiet(&self, now: DateTime<Utc>) -> bool {
        let local = now.with_timezone(&self.tz);
        if self.weekends && matches!(local.weekday(), Weekday::Sat | Weekday::Sun) {
            return true;
        }
        let t = local.time();
        if self.start < self.end {
            self.start <= t && t < self.end
        } else {
            self.start != self.end && (t >= self.start || t < self.end)
        }
    }
}

/// One alarm per occurrence and kind (AC3.1), in `(at, occurrence, kind)` order. `occurrences`
/// are the ones that passed the ignore rules.
///
/// - An alarm whose occurrence is not in the list is dropped, which covers a deleted event and a
///   newly ignored one, except a final one (Dismissed, Joined, Expired): it stays as stored, so
///   a keyword or "again at the start" switched off and on again does not bring a dismissed
///   meeting back (AC3.3). The store prunes final alarms once their meeting is past.
/// - An alarm already in `existing` keeps a Firing, Snoozed, Dismissed, Joined or Expired state
///   (AC3.3); a Scheduled one is re-timed from the settings. `at` and `start` follow the
///   occurrence either way.
/// - The one exception: an Expired alarm whose meeting moved later, to a window that is still
///   open, is Scheduled again, so a moved instance of a series fires at its new time. Not when
///   the meeting was joined: that Expired is the join closing the sibling alarm.
/// - A new alarm is not created once its meeting's window has closed (start + 10 min <= `now`),
///   so a feed read after the fact adds no stale alarms. One created inside the window with
///   `at` already past fires at the next tick.
/// - Idempotent: `plan(o, s, &plan(o, s, e, now), now) == plan(o, s, e, now)`.
pub fn plan(
    occurrences: &[Occurrence],
    settings: &AlarmSettings,
    existing: &[Alarm],
    now: DateTime<Utc>,
) -> Vec<Alarm> {
    let lead = Duration::minutes(i64::from(
        settings
            .lead_minutes
            .clamp(MIN_LEAD_MINUTES, MAX_LEAD_MINUTES),
    ));
    let known: HashMap<(&OccurrenceKey, AlarmKind), &Alarm> = existing
        .iter()
        .map(|a| ((&a.occurrence, a.kind), a))
        .collect();
    let joined: HashSet<&OccurrenceKey> = existing
        .iter()
        .filter(|a| a.state == AlarmState::Joined)
        .map(|a| &a.occurrence)
        .collect();
    let mut planned: HashSet<(&OccurrenceKey, AlarmKind)> = HashSet::new();

    let mut out = Vec::new();
    for occ in occurrences {
        let start = occ.start.with_timezone(&Utc);
        let mut wanted = vec![(AlarmKind::Lead, start.checked_sub_signed(lead))];
        if settings.again_at_start {
            wanted.push((AlarmKind::Start, Some(start)));
        }
        for (kind, at) in wanted {
            let Some(at) = at else { continue };
            let mut alarm = Alarm {
                occurrence: occ.key.clone(),
                at,
                start,
                kind,
                state: AlarmState::Scheduled,
            };
            match known.get(&(&occ.key, kind)) {
                Some(old) => {
                    let moved_later = old.state == AlarmState::Expired
                        && start > old.start
                        && alarm.expires_at() > now
                        && !joined.contains(&occ.key);
                    if !moved_later {
                        alarm.state = old.state;
                    }
                }
                None if alarm.expires_at() <= now => continue,
                None => {}
            }
            if planned.insert((&occ.key, kind)) {
                out.push(alarm);
            }
        }
    }
    // Read through `known`, so a repeated identity gives the same row (the last) as above.
    for old in existing {
        let is_row = known
            .get(&(&old.occurrence, old.kind))
            .is_some_and(|row| std::ptr::eq(*row, old));
        if is_row && old.state.is_final() && planned.insert((&old.occurrence, old.kind)) {
            out.push(old.clone());
        }
    }
    out.sort_by(|a, b| (a.at, &a.occurrence, a.kind).cmp(&(b.at, &b.occurrence, b.kind)));
    out
}

/// One alarm changing state. Apply it with [`apply_transitions`]; a transition to
/// `Firing` is the order to show the interrupt, one to `Expired` is silent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    pub occurrence: OccurrenceKey,
    pub kind: AlarmKind,
    pub to: AlarmState,
}

impl Transition {
    pub fn fires(&self) -> bool {
        matches!(self.to, AlarmState::Firing { .. })
    }
}

/// What changes at `now`. Pure: nothing is applied until the caller calls [`apply_transitions`],
/// so asking twice with the same `now` gives the same answer. A tick never moves a Scheduled or
/// Snoozed alarm anywhere but Firing or Expired, and never touches a final one.
///
/// - Scheduled fires once `now >= at`; Snoozed fires once `now >= until` (S3.2).
/// - Any open alarm expires at start + 10 min (AC3.2). Expiry wins over firing, so a `now` that
///   jumped past the whole window (sleep, AC3.4) expires silently (S3.5), and one that jumped
///   past `at` but not the window fires at once (S3.4).
/// - While `pause_until` is in the future or `quiet` covers `now`, nothing fires, and nothing
///   is lost: an alarm due meanwhile fires on the first tick after, if its window is still
///   open, otherwise it expires. Expiry is not suppressed.
/// - Quiet hours (AC8.1) also go by the meeting: one that starts inside them never fires, also
///   not after they end; its alarms expire silently. A Scheduled alarm whose own time falls
///   inside them, for a meeting that starts outside, waits for the start: the meeting's single
///   alarm then comes at the start, also when "again at the start" is off.
/// - A Joined alarm closes the other alarms of its occurrence, so joining at the lead alarm
///   silences "again at the start". Dismissing does not: that is what the second alarm is for.
/// - One interrupt per meeting: a Start alarm that would fire while the Lead alarm of the same
///   occurrence is Firing or Snoozed, or fires in this same tick (a wake after sleep, the end of a
///   pause), expires silently instead. So does a Lead alarm that would fire while the Start alarm
///   is Firing, which only happens when a person snoozed the Start alarm before it was due.
pub fn tick(
    alarms: &[Alarm],
    now: DateTime<Utc>,
    pause_until: Option<DateTime<Utc>>,
    quiet: Option<&QuietHours>,
) -> Vec<Transition> {
    let muted =
        pause_until.is_some_and(|until| now < until) || quiet.is_some_and(|q| q.is_quiet(now));
    let joined: HashSet<&OccurrenceKey> = alarms
        .iter()
        .filter(|a| a.state == AlarmState::Joined)
        .map(|a| &a.occurrence)
        .collect();
    let next: Vec<Option<AlarmState>> = alarms
        .iter()
        .map(|a| next_state(a, now, muted, joined.contains(&a.occurrence), quiet))
        .collect();
    // Occurrences whose Lead alarm is, or is about to be, on screen or waiting out a snooze.
    let lead_busy: HashSet<&OccurrenceKey> = alarms
        .iter()
        .zip(&next)
        .filter(|(a, to)| {
            a.kind == AlarmKind::Lead
                && match to {
                    Some(to) => to.since().is_some(),
                    None => matches!(
                        a.state,
                        AlarmState::Firing { .. } | AlarmState::Snoozed { .. }
                    ),
                }
        })
        .map(|(a, _)| &a.occurrence)
        .collect();
    let start_firing: HashSet<&OccurrenceKey> = alarms
        .iter()
        .filter(|a| a.kind == AlarmKind::Start && matches!(a.state, AlarmState::Firing { .. }))
        .map(|a| &a.occurrence)
        .collect();
    alarms
        .iter()
        .zip(next)
        .filter_map(|(a, to)| {
            let blocked = match a.kind {
                AlarmKind::Start => lead_busy.contains(&a.occurrence),
                AlarmKind::Lead => start_firing.contains(&a.occurrence),
            };
            let to = match to {
                Some(AlarmState::Firing { .. }) if blocked => AlarmState::Expired,
                other => other?,
            };
            Some(Transition {
                occurrence: a.occurrence.clone(),
                kind: a.kind,
                to,
            })
        })
        .collect()
}

fn next_state(
    a: &Alarm,
    now: DateTime<Utc>,
    muted: bool,
    sibling_joined: bool,
    quiet: Option<&QuietHours>,
) -> Option<AlarmState> {
    if a.state.is_final() {
        return None;
    }
    if sibling_joined || now >= a.expires_at() {
        return Some(AlarmState::Expired);
    }
    let quiet_at = |t| quiet.is_some_and(|q| q.is_quiet(t));
    let due = match a.state {
        AlarmState::Scheduled if quiet_at(a.at) => now >= a.at.max(a.start),
        AlarmState::Scheduled => now >= a.at,
        AlarmState::Snoozed { until } => now >= until,
        _ => false,
    };
    (due && !muted && !quiet_at(a.start)).then_some(AlarmState::Firing { since: now })
}

/// Write `transitions` into `alarms`, matching on `(occurrence, kind)`.
pub fn apply_transitions(alarms: &mut [Alarm], transitions: &[Transition]) {
    for t in transitions {
        if let Some(a) = alarms
            .iter_mut()
            .find(|a| a.occurrence == t.occurrence && a.kind == t.kind)
        {
            a.state = t.to;
        }
    }
}

/// The interrupt payload for meetings that start within the same minute (AC3.5, S3.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InterruptGroup {
    /// The start minute shared by every meeting in the group.
    pub minute: DateTime<Utc>,
    /// Each meeting once, by start then key, each with its own join target.
    pub occurrences: Vec<Occurrence>,
}

/// Group the Firing alarms by the minute their meeting starts, earliest first. A meeting whose
/// lead and start alarms both fire is listed once. An alarm with no matching occurrence is skipped.
pub fn group_firing(alarms: &[Alarm], occurrences: &[Occurrence]) -> Vec<InterruptGroup> {
    let mut by_minute: BTreeMap<DateTime<Utc>, Vec<Occurrence>> = BTreeMap::new();
    for a in alarms.iter().filter(|a| a.state.since().is_some()) {
        let Some(occ) = occurrences.iter().find(|o| o.key == a.occurrence) else {
            continue;
        };
        let start = occ.start.with_timezone(&Utc);
        let minute = start.duration_trunc(Duration::minutes(1)).unwrap_or(start);
        let group = by_minute.entry(minute).or_default();
        if !group.iter().any(|o| o.key == occ.key) {
            group.push(occ.clone());
        }
    }
    by_minute
        .into_iter()
        .map(|(minute, mut occurrences)| {
            occurrences.sort_by(|a, b| (a.start, &a.key).cmp(&(b.start, &b.key)));
            InterruptGroup {
                minute,
                occurrences,
            }
        })
        .collect()
}

/// The numbers behind "Starts in N min" and "Started N min ago" (S3.4, AC4.7). The words live in
/// `ui/src/copy.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Countdown {
    /// Before the start, rounded up: 61 s left is 2 min, 1 s is 1 min.
    StartsIn { minutes: u32 },
    /// Under a minute after the start, so the copy does not say "Started 0 min ago".
    StartingNow,
    /// From the first full minute after the start, rounded down.
    StartedAgo { minutes: u32 },
}

pub fn countdown(start: DateTime<Utc>, now: DateTime<Utc>) -> Countdown {
    let minutes = |secs: i64| u32::try_from(secs).unwrap_or(u32::MAX);
    let left = (start - now).num_seconds();
    if left > 0 {
        return Countdown::StartsIn {
            minutes: minutes((left + 59) / 60),
        };
    }
    match minutes(-left / 60) {
        0 => Countdown::StartingNow,
        minutes => Countdown::StartedAgo { minutes },
    }
}

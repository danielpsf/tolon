//! The alarm model and its state machine (AC3.2). Pure: `now` is always a parameter.
//!
//! Scheduled -> Firing -> (Snoozed -> Firing)* -> Dismissed | Joined | Expired. The three last
//! states are final. Planning and ticking are in `schedule.rs`.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::model::OccurrenceKey;

/// AC3.2: an alarm with no action expires this long after the meeting starts.
pub const EXPIRY_MINUTES: i64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlarmKind {
    /// At `start - lead`.
    Lead,
    /// At `start`, when "again at the start" is on.
    Start,
}

impl AlarmKind {
    /// The text kept in `alarms.kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Start => "start",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "lead" => Some(Self::Lead),
            "start" => Some(Self::Start),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AlarmState {
    Scheduled,
    Firing { since: DateTime<Utc> },
    Snoozed { until: DateTime<Utc> },
    Dismissed,
    Joined,
    Expired,
}

impl AlarmState {
    /// The text kept in `alarms.state`.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Scheduled => "scheduled",
            Self::Firing { .. } => "firing",
            Self::Snoozed { .. } => "snoozed",
            Self::Dismissed => "dismissed",
            Self::Joined => "joined",
            Self::Expired => "expired",
        }
    }

    /// `Firing.since`: the one timestamp that belongs in `alarms.state_at`. For every other
    /// state the store records when it wrote the row.
    pub fn since(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::Firing { since } => Some(*since),
            _ => None,
        }
    }

    /// `Snoozed.until`: the `alarms.until` column.
    pub fn until(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::Snoozed { until } => Some(*until),
            _ => None,
        }
    }

    /// Rebuild a state from the `state`, `state_at` and `until` columns. `None` for an unknown
    /// name, or a firing or snoozed row missing the column it needs.
    pub fn from_parts(
        name: &str,
        state_at: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> Option<Self> {
        match name {
            "scheduled" => Some(Self::Scheduled),
            "firing" => state_at.map(|since| Self::Firing { since }),
            "snoozed" => until.map(|until| Self::Snoozed { until }),
            "dismissed" => Some(Self::Dismissed),
            "joined" => Some(Self::Joined),
            "expired" => Some(Self::Expired),
            _ => None,
        }
    }

    /// Dismissed, Joined and Expired never change again.
    pub fn is_final(&self) -> bool {
        matches!(self, Self::Dismissed | Self::Joined | Self::Expired)
    }
}

/// One alarm for one occurrence. Identity is `(occurrence, kind)`, the store's primary key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alarm {
    pub occurrence: OccurrenceKey,
    /// When it fires.
    pub at: DateTime<Utc>,
    /// The meeting's start, which the expiry window and the countdown count from. It is derived
    /// data: `plan` rewrites it from the occurrence every time.
    pub start: DateTime<Utc>,
    pub kind: AlarmKind,
    pub state: AlarmState,
}

impl Alarm {
    /// When an alarm without an action expires (AC3.2).
    pub fn expires_at(&self) -> DateTime<Utc> {
        self.start
            .checked_add_signed(Duration::minutes(EXPIRY_MINUTES))
            .unwrap_or(DateTime::<Utc>::MAX_UTC)
    }
}

/// Snooze: Scheduled, Firing or Snoozed becomes `Snoozed { now + minutes }` (S3.2). `false`, and
/// no change, for a final state, zero minutes or a time that overflows.
pub fn snooze(alarm: &mut Alarm, minutes: u32, now: DateTime<Utc>) -> bool {
    let until = now.checked_add_signed(Duration::minutes(i64::from(minutes)));
    match until {
        Some(until) if minutes > 0 && !alarm.state.is_final() => {
            alarm.state = AlarmState::Snoozed { until };
            true
        }
        _ => false,
    }
}

/// The tray panel's Snooze for a meeting whose interrupt is not on screen (F7): the alarm comes
/// `minutes` later than it was due, never sooner. A Scheduled alarm counts from `at`, a Snoozed
/// one from `until`, a Firing one (waiting behind another interrupt) from `now`. `false`, and no
/// change, for a final state, zero minutes or a time that overflows.
pub fn snooze_later(alarm: &mut Alarm, minutes: u32, now: DateTime<Utc>) -> bool {
    let due = match alarm.state {
        AlarmState::Scheduled => alarm.at,
        AlarmState::Snoozed { until } => until,
        AlarmState::Firing { .. } => now,
        _ => return false,
    };
    snooze(alarm, minutes, due.max(now))
}

fn settle(alarm: &mut Alarm, to: AlarmState) -> bool {
    if alarm.state.is_final() {
        return false;
    }
    alarm.state = to;
    true
}

/// Dismiss: final (S3.3). `false` when the alarm is already final.
pub fn dismiss(alarm: &mut Alarm) -> bool {
    settle(alarm, AlarmState::Dismissed)
}

/// Join: final. Opening the link is the caller's job. `false` when already final.
pub fn join(alarm: &mut Alarm) -> bool {
    settle(alarm, AlarmState::Joined)
}

/// "I am already there": the person is in the meeting, so it is Joined without a link to open.
/// Joined, not Dismissed, so the "again at the start" alarm of the same meeting stays quiet too.
pub fn already_there(alarm: &mut Alarm) -> bool {
    settle(alarm, AlarmState::Joined)
}

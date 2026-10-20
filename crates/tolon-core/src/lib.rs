//! Domain types and pure functions: events, occurrences, expansion, ignore rules,
//! the alarm state machine and join-link detection. No I/O, no async runtime.

mod alarm;
mod expand;
mod join;
mod model;
mod parse;
mod rules;
mod schedule;
mod tzid;

pub use alarm::{
    already_there, dismiss, join, snooze, Alarm, AlarmKind, AlarmState, EXPIRY_MINUTES,
};
pub use expand::{expand, expand_window, ExpandOptions, WINDOW_DAYS};
pub use join::detect_join;
pub use model::{
    wall_clock, CalendarId, Event, EventId, EventStatus, EventTime, JoinTarget, Occurrence,
    OccurrenceKey, Override, ParseKeyError, Response,
};
pub use parse::{parse_ics, ParseOptions, ParsedCalendar};
pub use rules::{apply_rules, judge_window, IgnoreReason, IgnoreRules, Verdict};
pub use schedule::{
    apply_transitions, countdown, group_firing, plan, tick, AlarmSettings, Countdown,
    InterruptGroup, QuietHours, Transition, ALL_DAY_AT, MAX_LEAD_MINUTES, MIN_LEAD_MINUTES,
};

/// Crate name, a placeholder export until the real API lands.
pub const NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_set() {
        assert_eq!(NAME, "tolon-core");
    }
}

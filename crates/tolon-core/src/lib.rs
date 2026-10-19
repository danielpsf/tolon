//! Domain types and pure functions: events, occurrences, expansion, ignore rules,
//! the alarm state machine and join-link detection. No I/O, no async runtime.

mod alarm;
mod expand;
mod model;
mod parse;
mod tzid;

pub use alarm::{
    already_there, dismiss, join, snooze, Alarm, AlarmKind, AlarmState, EXPIRY_MINUTES,
};
pub use expand::{expand, expand_window, ExpandOptions, WINDOW_DAYS};
pub use model::{
    wall_clock, CalendarId, Event, EventId, EventStatus, EventTime, JoinTarget, Occurrence,
    OccurrenceKey, Override, ParseKeyError, Response,
};
pub use parse::{parse_ics, ParseOptions, ParsedCalendar};

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

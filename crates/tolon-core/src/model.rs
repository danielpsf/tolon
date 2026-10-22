//! Domain types. `Event` is stored as canonical JSON (`events.payload`), so field order is the
//! serialisation order: do not reorder fields without a store migration.

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Offset, SecondsFormat, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CalendarId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventId(pub String);

/// A DTSTART, DTEND, EXDATE or RECURRENCE-ID value in the form the feed gave it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventTime {
    /// `VALUE=DATE`: an all-day date, no zone.
    Date {
        date: NaiveDate,
    },
    /// No `Z` and no `TZID`: wall-clock time in whatever zone the machine is in.
    Floating {
        local: NaiveDateTime,
    },
    Utc {
        at: DateTime<Utc>,
    },
    /// Wall-clock time in a named zone; DST rules apply.
    Zoned {
        tz: Tz,
        local: NaiveDateTime,
    },
}

impl EventTime {
    /// The zone recurrence is computed in: the event's own zone, else the machine's.
    pub fn zone_or(&self, local: Tz) -> Tz {
        match self {
            Self::Zoned { tz, .. } => *tz,
            Self::Utc { .. } => Tz::UTC,
            Self::Date { .. } | Self::Floating { .. } => local,
        }
    }

    /// The instant this value names, as seen in `local`. `None` only when the calendar date
    /// cannot be represented.
    pub fn resolve(&self, local: Tz) -> Option<DateTime<Tz>> {
        match self {
            Self::Utc { at } => Some(at.with_timezone(&local)),
            Self::Date { date } => wall_clock(date.and_hms_opt(0, 0, 0)?, local),
            Self::Floating { local: l } => wall_clock(*l, local),
            Self::Zoned { tz, local: l } => wall_clock(*l, *tz).map(|dt| dt.with_timezone(&local)),
        }
    }

    /// This value moved by `d` on its own clock (wall-clock for floating and zoned values).
    pub fn plus(&self, d: Duration) -> Option<Self> {
        Some(match self {
            Self::Date { date } => Self::Date {
                date: date.checked_add_signed(d)?,
            },
            Self::Floating { local } => Self::Floating {
                local: local.checked_add_signed(d)?,
            },
            Self::Utc { at } => Self::Utc {
                at: at.checked_add_signed(d)?,
            },
            Self::Zoned { tz, local } => Self::Zoned {
                tz: *tz,
                local: local.checked_add_signed(d)?,
            },
        })
    }
}

/// Local wall-clock time in `tz`. An ambiguous time (clocks go back) takes the first pass; a
/// time in the gap (clocks go forward) uses the offset from before the gap, as RFC 5545 says.
pub fn wall_clock(local: NaiveDateTime, tz: Tz) -> Option<DateTime<Tz>> {
    if let Some(dt) = tz.from_local_datetime(&local).earliest() {
        return Some(dt);
    }
    let before = tz
        .from_local_datetime(&local.checked_sub_signed(Duration::days(1))?)
        .earliest()?;
    let offset = Duration::seconds(i64::from(before.offset().fix().local_minus_utc()));
    let utc = local.checked_sub_signed(offset)?;
    Some(Utc.from_utc_datetime(&utc).with_timezone(&tz))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    Confirmed,
    Tentative,
    Cancelled,
}

/// The calendar owner's answer (`PARTSTAT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Response {
    Accepted,
    Declined,
    Tentative,
    NeedsAction,
}

/// One edited instance of a recurring event (a VEVENT with `RECURRENCE-ID`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Override {
    /// Which generated instance this replaces.
    pub recurrence_id: EventTime,
    /// Its own start, end, title and status. Its `overrides` is always empty.
    pub event: Event,
}

/// One VEVENT, possibly recurring, with its overrides folded in.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: EventId,
    pub calendar: CalendarId,
    pub uid: String,
    pub title: String,
    pub start: EventTime,
    pub end: EventTime,
    pub all_day: bool,
    /// The raw `RRULE` value, for example `FREQ=WEEKLY;BYDAY=MO`. Validated at parse time.
    pub rrule: Option<String>,
    pub exdates: Vec<EventTime>,
    pub overrides: Vec<Override>,
    pub status: EventStatus,
    pub my_response: Option<Response>,
    pub attendees_count: u32,
    pub location: Option<String>,
    pub description: Option<String>,
    /// Video-call link the feed names explicitly (`X-GOOGLE-CONFERENCE`, else
    /// `X-MICROSOFT-SKYPETEAMSMEETINGURL`).
    pub conference: Option<String>,
    /// The `URL` property.
    pub html_link: Option<String>,
    pub updated: DateTime<Utc>,
}

/// Titles, places, descriptions, links and ids can be private: a `{:?}` shows the times and the
/// shape only. `Override` derives `Debug` and prints its event through this.
impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Event")
            .field("start", &self.start)
            .field("end", &self.end)
            .field("all_day", &self.all_day)
            .field("status", &self.status)
            .field("recurring", &self.rrule.is_some())
            .field("overrides", &self.overrides.len())
            .finish_non_exhaustive()
    }
}

/// Stable across syncs: the event and the instant the generator first produced, so a moved
/// instance keeps its key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OccurrenceKey {
    pub event: EventId,
    pub original_start: DateTime<Utc>,
}

/// The stable text form kept in `alarms.occurrence_key`: `<event id>@<original start, RFC 3339 UTC>`.
/// Event ids can contain `@`, the timestamp cannot, so parsing splits on the last one.
impl fmt::Display for OccurrenceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let at = self
            .original_start
            .to_rfc3339_opts(SecondsFormat::AutoSi, true);
        write!(f, "{}@{at}", self.event.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseKeyError;

impl fmt::Display for ParseKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not an occurrence key: expected <event id>@<RFC 3339 time>")
    }
}

impl std::error::Error for ParseKeyError {}

impl FromStr for OccurrenceKey {
    type Err = ParseKeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (event, at) = s.rsplit_once('@').ok_or(ParseKeyError)?;
        let original_start = DateTime::parse_from_rfc3339(at)
            .map_err(|_| ParseKeyError)?
            .with_timezone(&Utc);
        Ok(Self {
            event: EventId(event.to_string()),
            original_start,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinTarget {
    /// A call link. `host` is lower-case (punycode stays `xn--`) so the UI can show where it
    /// goes before opening it. `recognised` is true when it came from the feed's conference
    /// data or its host is a known call provider; false for any other `https://` link found
    /// in `location`.
    Link {
        url: String,
        host: String,
        recognised: bool,
    },
    EventPage(String),
    None,
}

/// One concrete start, after expansion, in the machine's zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Occurrence {
    pub key: OccurrenceKey,
    pub event: EventId,
    pub start: DateTime<Tz>,
    pub end: DateTime<Tz>,
    pub title: String,
    pub all_day: bool,
    pub join: JoinTarget,
    pub people: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn private_event(tag: &str) -> Event {
        let at = DateTime::<Utc>::UNIX_EPOCH;
        Event {
            id: EventId(format!("id-{tag}")),
            calendar: CalendarId(format!("cal-{tag}")),
            uid: format!("uid-{tag}"),
            title: format!("title-{tag}"),
            start: EventTime::Utc { at },
            end: EventTime::Utc { at },
            all_day: false,
            rrule: Some("FREQ=DAILY".into()),
            exdates: vec![],
            overrides: vec![],
            status: EventStatus::Confirmed,
            my_response: None,
            attendees_count: 3,
            location: Some(format!("place-{tag}")),
            description: Some(format!("notes-{tag}")),
            conference: Some(format!("https://meet.example/{tag}")),
            html_link: Some(format!("https://cal.example/{tag}")),
            updated: at,
        }
    }

    #[test]
    fn debug_output_of_an_event_and_its_overrides_omits_private_text() {
        let mut event = private_event("SECRET");
        event.overrides.push(Override {
            recurrence_id: EventTime::Utc {
                at: DateTime::<Utc>::UNIX_EPOCH,
            },
            event: private_event("SECRET-2"),
        });
        let text = format!("{event:?} {:?}", event.overrides[0]);
        assert!(!text.contains("SECRET"), "{text}");
        assert!(text.contains("Event") && text.contains("recurring: true"));
    }
}

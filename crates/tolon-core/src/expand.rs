//! Expansion: events to the concrete occurrences inside a window, in the machine's zone.

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};
use chrono_tz::Tz;
use rrule::{RRule, RRuleSet, Tz as RTz, Unvalidated};
use std::collections::HashSet;

use crate::model::{
    wall_clock, Event, EventStatus, EventTime, JoinTarget, Occurrence, OccurrenceKey, Response,
};

/// How far ahead `expand` looks (AC2.2).
pub const WINDOW_DAYS: i64 = 7;

/// Generator steps per event before giving up; stops a rule that never matches from spinning.
const MAX_STEPS: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpandOptions {
    /// AC2.3: drop STATUS:CANCELLED and METHOD:CANCEL events. Default on.
    pub ignore_cancelled: bool,
    /// AC2.3: drop events the calendar owner declined. Default on.
    pub ignore_declined: bool,
    /// AC2.4: keep all-day events. Default off.
    pub include_all_day: bool,
}

impl Default for ExpandOptions {
    fn default() -> Self {
        Self {
            ignore_cancelled: true,
            ignore_declined: true,
            include_all_day: false,
        }
    }
}

/// Occurrences from `now` to `now + 7 days`, sorted by start. An occurrence already running at
/// `now` is included. `local` is the machine's zone.
pub fn expand(
    events: &[Event],
    now: DateTime<Utc>,
    local: Tz,
    opts: &ExpandOptions,
) -> Vec<Occurrence> {
    let to = now
        .checked_add_signed(Duration::days(WINDOW_DAYS))
        .unwrap_or(DateTime::<Utc>::MAX_UTC);
    expand_window(events, now, to, local, opts)
}

/// Same as [`expand`] over an explicit `[from, to)` window.
pub fn expand_window(
    events: &[Event],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    local: Tz,
    opts: &ExpandOptions,
) -> Vec<Occurrence> {
    let mut out = Vec::new();
    for event in events {
        expand_event(event, from, to, local, opts, &mut out);
    }
    out.sort_by(|a, b| (a.start, &a.event).cmp(&(b.start, &b.event)));
    out
}

/// The recurrence generator for `start` and `rrule`, with EXDATEs applied. `None` when the rule
/// does not validate against the start.
pub(crate) fn rrule_set(
    rrule: &str,
    start: &EventTime,
    exdates: &[EventTime],
    local: Tz,
) -> Option<RRuleSet> {
    let zone = start.zone_or(local);
    let rtz = RTz::from(zone);
    let dt_start = start.resolve(zone)?.with_timezone(&rtz);
    let rule: RRule<Unvalidated> = normalise_until(rrule, zone).parse().ok()?;
    let mut set = rule.build(dt_start).ok()?.limit();
    for ex in exdates {
        if let Some(x) = ex.resolve(zone) {
            set = set.exdate(x.with_timezone(&rtz));
        }
    }
    Some(set)
}

/// Rewrite UNTIL to UTC, the one form rrule accepts next to a zoned DTSTART. RFC 5545 asks for
/// UNTIL in DTSTART's form; feeds differ, so a DATE reads as inclusive through that day in the
/// event's zone and a floating value reads in the event's zone (the machine's, for a floating event).
fn normalise_until(rule: &str, zone: Tz) -> String {
    let convert = |value: &str| -> Option<String> {
        if value.ends_with(['Z', 'z']) {
            return None;
        }
        let naive = if value.len() == 8 {
            NaiveDate::parse_from_str(value, "%Y%m%d")
                .ok()?
                .and_hms_opt(23, 59, 59)?
        } else {
            NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S").ok()?
        };
        let utc = wall_clock(naive, zone)?.with_timezone(&Utc);
        Some(utc.format("%Y%m%dT%H%M%SZ").to_string())
    };
    rule.split(';')
        .map(|part| match part.split_once('=') {
            Some((key, value)) if key.eq_ignore_ascii_case("UNTIL") => {
                format!(
                    "UNTIL={}",
                    convert(value.trim()).unwrap_or_else(|| value.to_string())
                )
            }
            _ => part.to_string(),
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// How long an instance lasts: whole local days for all-day events, an exact span otherwise.
struct Shape {
    span: Duration,
    days: Option<i64>,
}

impl Shape {
    fn of(e: &Event, local: Tz) -> Option<Self> {
        let start = e.start.resolve(local)?;
        let end = e.end.resolve(local)?;
        Some(Self {
            span: (end - start).max(Duration::zero()),
            days: e
                .all_day
                .then(|| (end.date_naive() - start.date_naive()).num_days().max(1)),
        })
    }

    fn end_for(&self, start: DateTime<Tz>) -> Option<DateTime<Tz>> {
        match self.days {
            Some(days) => {
                let midnight = start.date_naive().and_hms_opt(0, 0, 0)?;
                wall_clock(
                    midnight.checked_add_signed(Duration::days(days))?,
                    start.timezone(),
                )
            }
            None => start.checked_add_signed(self.span),
        }
    }
}

fn expand_event(
    e: &Event,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    local: Tz,
    opts: &ExpandOptions,
    out: &mut Vec<Occurrence>,
) {
    if opts.ignore_cancelled && e.status == EventStatus::Cancelled {
        return;
    }
    let Some(shape) = Shape::of(e, local) else {
        return;
    };
    let replaced: HashSet<DateTime<Utc>> = e
        .overrides
        .iter()
        .filter_map(|o| Some(o.recurrence_id.resolve(local)?.with_timezone(&Utc)))
        .collect();

    let mut emit =
        |src: &Event, start: DateTime<Tz>, end: DateTime<Tz>, original: DateTime<Utc>| {
            let hidden = (opts.ignore_cancelled && src.status == EventStatus::Cancelled)
                || (opts.ignore_declined && src.my_response == Some(Response::Declined))
                || (src.all_day && !opts.include_all_day);
            let overlaps = start.with_timezone(&Utc) < to
                && (end.with_timezone(&Utc) > from || start.with_timezone(&Utc) >= from);
            if hidden || !overlaps {
                return;
            }
            out.push(Occurrence {
                key: OccurrenceKey {
                    event: e.id.clone(),
                    original_start: original,
                },
                event: e.id.clone(),
                start,
                end,
                title: src.title.clone(),
                all_day: src.all_day,
                join: match (&src.conference, &src.html_link) {
                    (Some(link), _) => JoinTarget::Link(link.clone()),
                    (None, Some(page)) => JoinTarget::EventPage(page.clone()),
                    (None, None) => JoinTarget::None,
                },
                people: src.attendees_count,
            });
        };

    match &e.rrule {
        None => {
            if let Some(start) = e.start.resolve(local) {
                if let Some(end) = shape.end_for(start) {
                    emit(e, start, end, start.with_timezone(&Utc));
                }
            }
        }
        Some(rule) => {
            if let Some(set) = rrule_set(rule, &e.start, &e.exdates, local) {
                for dt in set.into_iter().take(MAX_STEPS) {
                    let original = dt.with_timezone(&Utc);
                    if original >= to {
                        break;
                    }
                    if replaced.contains(&original) {
                        continue;
                    }
                    let start = dt.with_timezone(&local);
                    if let Some(end) = shape.end_for(start) {
                        emit(e, start, end, original);
                    }
                }
            }
        }
    }

    for o in &e.overrides {
        let (Some(rid), Some(start), Some(oshape)) = (
            o.recurrence_id.resolve(local),
            o.event.start.resolve(local),
            Shape::of(&o.event, local),
        ) else {
            continue;
        };
        if let Some(end) = oshape.end_for(start) {
            emit(&o.event, start, end, rid.with_timezone(&Utc));
        }
    }
}

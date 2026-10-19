//! iCalendar text to `Event`s. The `ical` crate tokenises lines (unfolding included); the
//! component structure is read here so that one bad VEVENT never takes the feed down (AC2.5).

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, Utc};
use chrono_tz::Tz;
use ical::property::Property;
use ical::PropertyParser;
use std::collections::HashMap;

use crate::expand::rrule_set;
use crate::model::{CalendarId, Event, EventId, EventStatus, EventTime, Override, Response};
use crate::tzid;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParseOptions {
    /// Addresses that are the calendar owner, for the declined rule. When empty and the calendar
    /// name is an address (Google names the primary calendar that way), the name is used.
    pub owner_emails: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCalendar {
    /// `X-WR-CALNAME`; the caller falls back to the host (S1.1).
    pub name: Option<String>,
    pub events: Vec<Event>,
    /// VEVENTs dropped for a missing or invalid DTSTART or RRULE, or for being cut off (S2.3).
    pub skipped_components: usize,
}

impl ParsedCalendar {
    /// The sync-log wording from S2.3: "1 component skipped".
    pub fn skipped_summary(&self) -> Option<String> {
        match self.skipped_components {
            0 => None,
            1 => Some("1 component skipped".to_string()),
            n => Some(format!("{n} components skipped")),
        }
    }
}

/// Parse a feed. Never fails and never panics: bad components are skipped and counted, bad
/// property lines are ignored, unknown components (VTIMEZONE, VTODO, VALARM) are passed over.
/// Callers hand in `String::from_utf8_lossy` of the body.
pub fn parse_ics(input: &str, calendar: &CalendarId, opts: &ParseOptions) -> ParsedCalendar {
    let mut name = None;
    let mut cancel_method = false;
    let mut skipped = 0;
    let mut raw: Vec<Raw> = Vec::new();
    let mut current: Option<Vec<Property>> = None;
    // Depth inside a component we do not read: VALARM in a VEVENT, VTIMEZONE and friends outside.
    let mut nested = 0usize;

    for item in PropertyParser::from_reader(input.as_bytes()) {
        let Ok(prop) = item else { continue };
        let value = prop.value.as_deref().unwrap_or("").trim();
        match prop.name.to_ascii_uppercase().as_str() {
            "BEGIN" if value.eq_ignore_ascii_case("VEVENT") => {
                if current.is_some() {
                    skipped += 1;
                }
                current = Some(Vec::new());
                nested = 0;
            }
            "BEGIN" if !value.eq_ignore_ascii_case("VCALENDAR") => nested += 1,
            "END" if value.eq_ignore_ascii_case("VEVENT") => {
                if let Some(props) = current.take() {
                    match build(&props, calendar) {
                        Some(r) => raw.push(r),
                        None => skipped += 1,
                    }
                }
                nested = 0;
            }
            "END" if !value.eq_ignore_ascii_case("VCALENDAR") => {
                nested = nested.saturating_sub(1);
            }
            "BEGIN" | "END" => {}
            _ if nested > 0 => {}
            upper => match &mut current {
                Some(props) => props.push(prop),
                None if upper == "X-WR-CALNAME" && name.is_none() => {
                    name = Some(unescape(value)).filter(|n| !n.is_empty());
                }
                None if upper == "METHOD" => cancel_method = value.eq_ignore_ascii_case("CANCEL"),
                None => {}
            },
        }
    }
    if current.is_some() {
        skipped += 1;
    }

    let owners = owners(opts, name.as_deref());
    for r in &mut raw {
        r.event.my_response = r
            .attendees
            .iter()
            .find(|(addr, _)| owners.contains(addr))
            .and_then(|(_, answer)| *answer);
    }
    let mut events = fold_overrides(raw);
    if cancel_method {
        for e in &mut events {
            e.status = EventStatus::Cancelled;
        }
    }
    ParsedCalendar {
        name,
        events,
        skipped_components: skipped,
    }
}

struct Raw {
    event: Event,
    recurrence_id: Option<EventTime>,
    /// Lower-cased addresses with their answers; matched against the owner once the name is known.
    attendees: Vec<(String, Option<Response>)>,
}

fn owners(opts: &ParseOptions, name: Option<&str>) -> Vec<String> {
    let mut list: Vec<String> = opts.owner_emails.iter().map(|e| e.to_lowercase()).collect();
    if list.is_empty() {
        list.extend(name.filter(|n| n.contains('@')).map(str::to_lowercase));
    }
    list
}

fn param<'a>(p: &'a Property, key: &str) -> Option<&'a str> {
    p.params
        .as_ref()?
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))?
        .1
        .first()
        .map(String::as_str)
}

/// Years outside this range are corrupt data; rejecting them keeps date arithmetic far from
/// chrono's limits, where it panics.
fn sane_year(year: i32) -> bool {
    (1900..=2200).contains(&year)
}

fn parse_value(v: &str, p: &Property) -> Option<EventTime> {
    let v = v.trim();
    let is_date = v.len() == 8 || param(p, "VALUE").is_some_and(|x| x.eq_ignore_ascii_case("DATE"));
    if is_date {
        let date = NaiveDate::parse_from_str(v.get(..8)?, "%Y%m%d").ok()?;
        return sane_year(date.year()).then_some(EventTime::Date { date });
    }
    let (body, utc) = match v.strip_suffix(['Z', 'z']) {
        Some(body) => (body, true),
        None => (v, false),
    };
    let local = NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S").ok()?;
    if !sane_year(local.year()) {
        return None;
    }
    Some(if utc {
        EventTime::Utc {
            at: local.and_utc(),
        }
    } else {
        // shortcut: a TZID we cannot name reads as floating time; add it to tzid.rs when a feed needs it.
        match param(p, "TZID").and_then(tzid::lookup) {
            Some(tz) => EventTime::Zoned { tz, local },
            None => EventTime::Floating { local },
        }
    })
}

fn parse_time(p: &Property) -> Option<EventTime> {
    parse_value(p.value.as_deref()?, p)
}

fn parse_duration(v: &str) -> Option<Duration> {
    let v = v.trim();
    let (negative, v) = match v.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, v.strip_prefix('+').unwrap_or(v)),
    };
    let mut total = Duration::zero();
    let mut in_time = false;
    let mut digits = String::new();
    for c in v.strip_prefix('P')?.chars() {
        if c == 'T' {
            in_time = true;
        } else if c.is_ascii_digit() {
            digits.push(c);
        } else {
            let n: i64 = std::mem::take(&mut digits).parse().ok()?;
            let part = match (c, in_time) {
                ('W', false) => Duration::try_weeks(n),
                ('D', false) => Duration::try_days(n),
                ('H', true) => Duration::try_hours(n),
                ('M', true) => Duration::try_minutes(n),
                ('S', true) => Duration::try_seconds(n),
                _ => None,
            }?;
            total = total.checked_add(&part)?;
        }
    }
    if !digits.is_empty() {
        return None;
    }
    Some(if negative { -total } else { total })
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
        } else if let Some(next) = chars.next() {
            out.push(if matches!(next, 'n' | 'N') {
                '\n'
            } else {
                next
            });
        }
    }
    out.trim().to_string()
}

fn http_url(s: String) -> Option<String> {
    let lower = s.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://")).then_some(s)
}

fn answer(partstat: &str) -> Option<Response> {
    match partstat.to_ascii_uppercase().as_str() {
        "ACCEPTED" => Some(Response::Accepted),
        "DECLINED" => Some(Response::Declined),
        "TENTATIVE" => Some(Response::Tentative),
        "NEEDS-ACTION" => Some(Response::NeedsAction),
        _ => None,
    }
}

/// One VEVENT's properties to an `Event`; `None` when DTSTART or RRULE is missing or invalid.
fn build(props: &[Property], calendar: &CalendarId) -> Option<Raw> {
    let get = |name: &str| props.iter().find(|p| p.name.eq_ignore_ascii_case(name));
    let text = |name: &str| {
        get(name)
            .and_then(|p| p.value.as_deref())
            .map(unescape)
            .filter(|s| !s.is_empty())
    };

    let start = parse_time(get("DTSTART")?)?;
    let all_day = matches!(start, EventTime::Date { .. });
    let end = get("DTEND")
        .and_then(parse_time)
        .or_else(|| {
            let d = get("DURATION")?.value.as_deref().and_then(parse_duration)?;
            start.plus(d)
        })
        .or_else(|| {
            if all_day {
                start.plus(Duration::days(1))
            } else {
                None
            }
        })
        .unwrap_or_else(|| start.clone());
    let exdates: Vec<EventTime> = props
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case("EXDATE"))
        .flat_map(|p| {
            p.value
                .as_deref()
                .unwrap_or("")
                .split(',')
                .filter_map(|v| parse_value(v, p))
        })
        .collect();
    let rrule = match get("RRULE").and_then(|p| p.value.clone()) {
        Some(rule) => {
            rrule_set(&rule, &start, &exdates, Tz::UTC, None)?;
            Some(rule)
        }
        None => None,
    };
    let recurrence_id = match get("RECURRENCE-ID") {
        Some(p) => Some(parse_time(p)?),
        None => None,
    };
    let title = text("SUMMARY").unwrap_or_default();
    let uid = text("UID").unwrap_or_else(|| format!("{title}@{}", compact(&start)));
    let status = match text("STATUS").map(|s| s.to_ascii_uppercase()).as_deref() {
        Some("CANCELLED") => EventStatus::Cancelled,
        Some("TENTATIVE") => EventStatus::Tentative,
        _ => EventStatus::Confirmed,
    };
    let attendees: Vec<(String, Option<Response>)> = props
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case("ATTENDEE"))
        .map(|p| {
            let addr = p.value.as_deref().unwrap_or("").trim().to_lowercase();
            let addr = addr.strip_prefix("mailto:").unwrap_or(&addr).to_string();
            (addr, param(p, "PARTSTAT").and_then(answer))
        })
        .collect();
    let updated = ["LAST-MODIFIED", "DTSTAMP"]
        .iter()
        .find_map(|n| match parse_time(get(n)?)? {
            EventTime::Utc { at } => Some(at),
            _ => None,
        })
        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH);

    Some(Raw {
        event: Event {
            id: EventId(format!("{}:{uid}", calendar.0)),
            calendar: calendar.clone(),
            uid,
            title,
            start,
            end,
            all_day,
            rrule,
            exdates,
            overrides: Vec::new(),
            status,
            my_response: None,
            attendees_count: u32::try_from(attendees.len()).unwrap_or(u32::MAX),
            location: text("LOCATION"),
            description: text("DESCRIPTION"),
            conference: text("X-GOOGLE-CONFERENCE")
                .or_else(|| text("X-MICROSOFT-SKYPETEAMSMEETINGURL"))
                .and_then(http_url),
            html_link: text("URL").and_then(http_url),
            updated,
        },
        recurrence_id,
        attendees,
    })
}

fn compact(t: &EventTime) -> String {
    match t {
        EventTime::Date { date } => date.format("%Y%m%d").to_string(),
        EventTime::Floating { local } | EventTime::Zoned { local, .. } => {
            local.format("%Y%m%dT%H%M%S").to_string()
        }
        EventTime::Utc { at } => at.format("%Y%m%dT%H%M%SZ").to_string(),
    }
}

/// Attach each RECURRENCE-ID VEVENT to the series with the same UID. One whose series is not
/// in the feed (Google sends these for single invited instances) stands alone under a UID that
/// carries the recurrence id, so UIDs stay unique per calendar.
fn fold_overrides(raw: Vec<Raw>) -> Vec<Event> {
    let (overrides, bases): (Vec<Raw>, Vec<Raw>) =
        raw.into_iter().partition(|r| r.recurrence_id.is_some());
    let mut events: Vec<Event> = bases.into_iter().map(|r| r.event).collect();
    let mut by_uid: HashMap<String, usize> = HashMap::new();
    for (i, e) in events.iter().enumerate() {
        by_uid.entry(e.uid.clone()).or_insert(i);
    }
    for r in overrides {
        let Some(recurrence_id) = r.recurrence_id else {
            continue;
        };
        let mut ev = r.event;
        match by_uid.get(&ev.uid).and_then(|&i| events.get_mut(i)) {
            Some(base) => {
                if ev.title.is_empty() {
                    ev.title = base.title.clone();
                }
                ev.my_response = ev.my_response.or(base.my_response);
                base.overrides.push(Override {
                    recurrence_id,
                    event: ev,
                });
            }
            None => {
                ev.uid = format!("{}#{}", ev.uid, compact(&recurrence_id));
                ev.id = EventId(format!("{}:{}", ev.calendar.0, ev.uid));
                events.push(ev);
            }
        }
    }
    events
}

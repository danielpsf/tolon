//! Google Calendar events (`events.list` items, `singleEvents=false`) to core `Event`s, mapped the
//! way `parse_ics` maps a VEVENT, and merged into what the calendar already holds.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use serde::Deserialize;
use tolon_core::{
    parse_recurrence, CalendarId, Event, EventId, EventStatus, EventTime, Override, Response,
};

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct Item {
    pub id: String,
    pub status: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    pub html_link: Option<String>,
    pub hangout_link: Option<String>,
    pub start: Option<Time>,
    pub end: Option<Time>,
    pub recurrence: Vec<String>,
    pub recurring_event_id: Option<String>,
    pub original_start_time: Option<Time>,
    pub attendees: Vec<Attendee>,
    pub conference_data: Option<Conference>,
    pub updated: Option<String>,
}

/// Id, title, description, location and links are not printed.
impl std::fmt::Debug for Item {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Item")
            .field("status", &self.status)
            .field("attendees", &self.attendees.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct Time {
    pub date: Option<String>,
    pub date_time: Option<String>,
    pub time_zone: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct Attendee {
    #[serde(rename = "self")]
    pub is_self: bool,
    pub response_status: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct Conference {
    pub entry_points: Vec<EntryPoint>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct EntryPoint {
    pub entry_point_type: Option<String>,
    pub uri: Option<String>,
}

impl Item {
    fn cancelled(&self) -> bool {
        self.status.as_deref() == Some("cancelled")
    }
}

/// `date` is an all-day date; `dateTime` with a known `timeZone` keeps wall-clock time in that zone
/// (so recurrence follows its DST), otherwise it is the instant.
fn time(t: &Time) -> Option<EventTime> {
    if let Some(d) = &t.date {
        return Some(EventTime::Date {
            date: NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()?,
        });
    }
    let at = DateTime::parse_from_rfc3339(t.date_time.as_deref()?).ok()?;
    Some(
        match t.time_zone.as_deref().and_then(|z| z.parse::<Tz>().ok()) {
            Some(tz) => EventTime::Zoned {
                tz,
                local: at.with_timezone(&tz).naive_local(),
            },
            None => EventTime::Utc {
                at: at.with_timezone(&Utc),
            },
        },
    )
}

fn https(s: &str) -> Option<String> {
    s.to_ascii_lowercase()
        .starts_with("https://")
        .then(|| s.to_owned())
}

/// One item to an `Event`; `None` when its start or RRULE is missing or invalid (as a VEVENT is skipped).
fn event(item: &Item, calendar: &CalendarId) -> Option<Event> {
    let start = time(item.start.as_ref()?)?;
    let all_day = matches!(start, EventTime::Date { .. });
    let end = item
        .end
        .as_ref()
        .and_then(time)
        .or_else(|| all_day.then(|| start.plus(Duration::days(1))).flatten())
        .unwrap_or_else(|| start.clone());
    let (rrule, exdates) = parse_recurrence(&item.recurrence, &start)?;
    let status = match item.status.as_deref() {
        Some("cancelled") => EventStatus::Cancelled,
        Some("tentative") => EventStatus::Tentative,
        _ => EventStatus::Confirmed,
    };
    // AC13.5: the owner's answer is the attendee Google marks `self`.
    let my_response = item.attendees.iter().find(|a| a.is_self).and_then(|a| {
        match a.response_status.as_deref()? {
            "accepted" => Some(Response::Accepted),
            "declined" => Some(Response::Declined),
            "tentative" => Some(Response::Tentative),
            "needsAction" => Some(Response::NeedsAction),
            _ => None,
        }
    });
    // AC13.5: the conference's video entry point, else the Meet link Google also puts in `hangoutLink`.
    let conference = item
        .conference_data
        .as_ref()
        .and_then(|c| {
            c.entry_points
                .iter()
                .find(|p| p.entry_point_type.as_deref() == Some("video"))
                .and_then(|p| p.uri.as_deref())
        })
        .or(item.hangout_link.as_deref())
        .and_then(https);
    let text = |s: &Option<String>| {
        s.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    Some(Event {
        id: EventId(format!("{}:{}", calendar.0, item.id)),
        calendar: calendar.clone(),
        uid: item.id.clone(),
        title: text(&item.summary).unwrap_or_default(),
        start,
        end,
        all_day,
        rrule,
        exdates,
        overrides: Vec::new(),
        status,
        my_response,
        attendees_count: u32::try_from(item.attendees.len()).unwrap_or(u32::MAX),
        location: text(&item.location),
        description: text(&item.description),
        conference,
        html_link: item.html_link.as_deref().and_then(https),
        updated: item
            .updated
            .as_deref()
            .and_then(|u| DateTime::parse_from_rfc3339(u).ok())
            .map_or(DateTime::<Utc>::UNIX_EPOCH, |u| u.with_timezone(&Utc)),
    })
}

/// Applies `items` (a full listing or a `syncToken` delta) to `stored`, keyed by Google event id.
/// A cancelled item deletes its event; a cancelled instance of a series becomes an EXDATE and a
/// changed one an override, as RECURRENCE-ID VEVENTs do in a feed. An instance whose series is
/// not in the calendar stands alone. Returns the events and how many items could not be read.
pub(crate) fn merge(
    stored: Vec<Event>,
    items: Vec<Item>,
    calendar: &CalendarId,
) -> (Vec<Event>, usize) {
    let mut events: BTreeMap<String, Event> =
        stored.into_iter().map(|e| (e.uid.clone(), e)).collect();
    let mut skipped = 0;
    // Series first, so an instance in the same listing finds its series.
    let (instances, series): (Vec<Item>, Vec<Item>) = items
        .into_iter()
        .partition(|i| i.recurring_event_id.is_some() && i.original_start_time.is_some());

    for item in series {
        if item.cancelled() {
            events.remove(&item.id);
            // Instances of a deleted series that stood alone go with it (their ids are `<series>_<start>`).
            let prefix = format!("{}_", item.id);
            events.retain(|uid, _| !uid.starts_with(&prefix));
            continue;
        }
        let Some(mut ev) = event(&item, calendar) else {
            skipped += 1;
            continue;
        };
        // A changed series is sent without its instances: keep the ones already applied.
        if let Some(old) = events.remove(&item.id) {
            ev.overrides = old.overrides;
            for x in old.exdates {
                if !ev.exdates.contains(&x) {
                    ev.exdates.push(x);
                }
            }
        }
        events.insert(item.id.clone(), ev);
    }

    for item in instances {
        let (Some(series_id), Some(rid)) = (
            item.recurring_event_id.as_deref(),
            item.original_start_time.as_ref().and_then(time),
        ) else {
            skipped += 1;
            continue;
        };
        if let Some(base) = events.get_mut(series_id) {
            base.overrides.retain(|o| o.recurrence_id != rid);
            base.exdates.retain(|x| x != &rid);
            if item.cancelled() {
                base.exdates.push(rid);
            } else if let Some(mut ev) = event(&item, calendar) {
                if ev.title.is_empty() {
                    ev.title = base.title.clone();
                }
                base.overrides.push(Override {
                    recurrence_id: rid,
                    event: ev,
                });
            } else {
                skipped += 1;
            }
            continue;
        }
        if item.cancelled() {
            events.remove(&item.id);
        } else if let Some(ev) = event(&item, calendar) {
            events.insert(item.id.clone(), ev);
        } else {
            skipped += 1;
        }
    }
    (events.into_values().collect(), skipped)
}

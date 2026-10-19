//! The M1 pipeline in plain functions: fetch, parse, upsert, list, today. No Tauri in here, so
//! the tests run without a window. Every URL that reaches a log or an error goes through
//! `redact_url`, or never appears at all.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Mutex, MutexGuard, PoisonError};

use anyhow::{anyhow, Context};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use serde::Serialize;
use tolon_core::{
    expand_window, parse_ics, wall_clock, CalendarId, Event, ExpandOptions, Occurrence,
    ParseOptions,
};
use tolon_store::{Calendar, FeedEvent, Store};
use tolon_sync::{normalize_url, redact_url, FetchError, FetchOutcome, IcsFetcher};

/// The store behind a mutex. A guard is never held across an await.
pub type Db = Mutex<Store>;

/// Settings keys for the ignore rules (AC8.2). The value is a JSON bool; a missing key means the default.
const IGNORE_ALL_DAY: &str = "ignore_all_day";
const IGNORE_DECLINED: &str = "ignore_declined";
const IGNORE_CANCELLED: &str = "ignore_cancelled";

/// What the UI sees of a calendar. `url` is shown on the settings page; it is never logged.
#[derive(Serialize)]
pub struct CalendarDto {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub url: Option<String>,
    pub last_sync_at: Option<String>,
    pub last_error: Option<String>,
}

impl From<Calendar> for CalendarDto {
    fn from(c: Calendar) -> Self {
        Self {
            id: c.id,
            name: c.name,
            kind: c.kind,
            enabled: c.enabled,
            url: c.url,
            last_sync_at: c.last_sync_at,
            last_error: c.last_error,
        }
    }
}

/// The one thing the pipeline needs from the network layer. Production is `IcsFetcher`; tests
/// pass a fake because `tolon-sync` only accepts plain http inside its own crate.
pub trait Fetch {
    fn fetch(
        &self,
        url: &str,
        etag: Option<&str>,
    ) -> impl Future<Output = Result<FetchOutcome, FetchError>> + Send;
}

impl Fetch for IcsFetcher {
    fn fetch(
        &self,
        url: &str,
        etag: Option<&str>,
    ) -> impl Future<Output = Result<FetchOutcome, FetchError>> + Send {
        IcsFetcher::fetch(self, url, etag)
    }
}

/// A panic elsewhere must not lock the person out of their calendars: SQLite keeps its own
/// consistency, so a poisoned lock is taken over.
fn lock(db: &Db) -> MutexGuard<'_, Store> {
    db.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn list(db: &Db) -> anyhow::Result<Vec<CalendarDto>> {
    let cals = lock(db).calendars().list()?;
    Ok(cals.into_iter().map(CalendarDto::from).collect())
}

pub fn calendars(db: &Db) -> anyhow::Result<Vec<Calendar>> {
    Ok(lock(db).calendars().list()?)
}

pub fn remove(db: &Db, id: &str) -> anyhow::Result<()> {
    lock(db).calendars().remove(id)?;
    Ok(())
}

pub fn set_enabled(db: &Db, id: &str, enabled: bool) -> anyhow::Result<()> {
    lock(db).calendars().set_enabled(id, enabled)?;
    Ok(())
}

/// S1.1, S1.2, S1.3: normalise, fetch at once, name the calendar from the feed, and keep it
/// listed when the fetch fails.
pub async fn add_ics(
    db: &Db,
    fetcher: &impl Fetch,
    raw_url: &str,
    now: DateTime<Utc>,
) -> anyhow::Result<CalendarDto> {
    let url = normalize_url(raw_url).map_err(|e| anyhow!(add_error_text(&e)))?;
    let result = fetcher.fetch(&url, None).await;
    // The repo cannot rename a calendar, and event ids embed the calendar id, so the name is
    // read from a first parse and the calendar is inserted afterwards; `apply` parses again with the real id.
    let feed_name = match &result {
        Ok(FetchOutcome::Modified { body, .. }) => {
            parse_ics(body, &CalendarId(String::new()), &ParseOptions::default()).name
        }
        _ => None,
    };
    let name = feed_name
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| host_of(&url));
    let cal = lock(db)
        .calendars()
        .insert("ics", &name, Some(&url), true, &now.to_rfc3339())?;
    apply(db, &cal.id, result, now)?;
    let fresh = lock(db)
        .calendars()
        .get(&cal.id)?
        .ok_or_else(|| anyhow!("Tolon could not find that calendar."))?;
    Ok(fresh.into())
}

/// Fetches one calendar with its stored etag and applies the answer.
pub async fn sync_one(
    db: &Db,
    fetcher: &impl Fetch,
    id: &str,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let cal = lock(db)
        .calendars()
        .get(id)?
        .ok_or_else(|| anyhow!("Tolon could not find that calendar."))?;
    let url = cal
        .url
        .as_deref()
        .ok_or_else(|| anyhow!("This calendar has no link to refresh."))?;
    let result = fetcher.fetch(url, cal.etag.as_deref()).await;
    apply(db, id, result, now)
}

/// Every enabled calendar, one after the other. A failure on one never stops the rest; the first
/// store error is returned at the end.
pub async fn sync_all(db: &Db, fetcher: &impl Fetch, now: DateTime<Utc>) -> anyhow::Result<()> {
    let mut first_err = None;
    for cal in calendars(db)?.into_iter().filter(|c| c.enabled) {
        if let Err(e) = sync_one(db, fetcher, &cal.id, now).await {
            tracing::warn!(calendar = %cal.id, error = %e, "calendar sync could not be saved");
            first_err.get_or_insert(e);
        }
    }
    first_err.map_or(Ok(()), Err)
}

/// Writes the outcome of one fetch. The error text and the log row never contain the URL.
fn apply(
    db: &Db,
    id: &str,
    result: Result<FetchOutcome, FetchError>,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let at = now.to_rfc3339();
    let store = lock(db);
    // Removed while the fetch was in flight: nothing to write.
    let Some(cal) = store.calendars().get(id)? else {
        return Ok(());
    };
    match result {
        Ok(FetchOutcome::Modified { body, etag }) => {
            let mut parsed =
                parse_ics(&body, &CalendarId(cal.id.clone()), &ParseOptions::default());
            let feed = feed_events(std::mem::take(&mut parsed.events))?;
            store.events().replace_for_calendar(id, &feed, &at)?;
            store.calendars().record_success(id, etag.as_deref(), &at)?;
            if let Some(summary) = parsed.skipped_summary() {
                store
                    .sync_log()
                    .append(Some(id), &at, "ok", Some(&summary))?;
            }
        }
        // A 304 is a healthy answer, so it clears a stale error (record_success, not
        // record_not_modified); the etag stays and the stored events are untouched (S1.5).
        Ok(FetchOutcome::NotModified) => {
            store
                .calendars()
                .record_success(id, cal.etag.as_deref(), &at)?;
        }
        Err(e) => {
            let text = error_text(&e);
            store.calendars().record_failure(id, &text)?;
            store
                .sync_log()
                .append(Some(id), &at, "error", Some(&text))?;
            let host = cal.url.as_deref().map(redact_url);
            tracing::warn!(calendar = %id, host = host.as_deref(), error = %text, "calendar sync failed");
        }
    }
    Ok(())
}

/// Duplicate UIDs keep the last event, then each event becomes its stored JSON payload.
fn feed_events(events: Vec<Event>) -> anyhow::Result<Vec<FeedEvent>> {
    let mut at: HashMap<String, usize> = HashMap::new();
    let mut kept: Vec<Event> = Vec::new();
    for e in events {
        match at.get(&e.uid).and_then(|&i| kept.get_mut(i)) {
            Some(slot) => *slot = e,
            None => {
                at.insert(e.uid.clone(), kept.len());
                kept.push(e);
            }
        }
    }
    kept.iter()
        .map(|e| {
            Ok(FeedEvent {
                uid: e.uid.clone(),
                payload: serde_json::to_string(e)
                    .context("Tolon could not save an event from that calendar.")?,
            })
        })
        .collect()
}

/// "Sync failed: 404" for an HTTP answer, the error kind's own text otherwise (S1.3).
fn error_text(e: &FetchError) -> String {
    match e.status() {
        Some(status) => format!("Sync failed: {status}"),
        None => e.to_string(),
    }
}

fn add_error_text(e: &FetchError) -> String {
    match e {
        FetchError::UnsupportedScheme => {
            "Tolon can only add calendar links that start with https:// or webcal://.".to_owned()
        }
        FetchError::InvalidUrl => {
            "That does not look like a calendar link. Check it and try again.".to_owned()
        }
        other => other.to_string(),
    }
}

/// Host of an already normalised https URL, through `redact_url` so nothing else leaks into a name.
fn host_of(url: &str) -> String {
    redact_url(url).trim_start_matches("https://").to_owned()
}

/// The machine's IANA zone, UTC when it cannot be read or is not a name `chrono-tz` knows.
pub fn local_zone() -> Tz {
    iana_time_zone::get_timezone()
        .ok()
        .and_then(|name| name.parse().ok())
        .unwrap_or(Tz::UTC)
}

/// `[local midnight, next local midnight)` around `now`, in UTC.
pub fn today_window(
    now: DateTime<Utc>,
    local: Tz,
) -> anyhow::Result<(DateTime<Utc>, DateTime<Utc>)> {
    let day = now.with_timezone(&local).date_naive();
    let next = day
        .checked_add_signed(Duration::days(1))
        .ok_or_else(|| anyhow!("Tolon could not work out today's date."))?;
    Ok((midnight(day, local)?, midnight(next, local)?))
}

fn midnight(day: NaiveDate, local: Tz) -> anyhow::Result<DateTime<Utc>> {
    day.and_hms_opt(0, 0, 0)
        .and_then(|t| wall_clock(t, local))
        .map(|t| t.with_timezone(&Utc))
        .ok_or_else(|| anyhow!("Tolon could not work out today's date."))
}

fn flag(store: &Store, key: &str, default: bool) -> anyhow::Result<bool> {
    Ok(store
        .settings()
        .get(key)?
        .and_then(|v| v.as_bool())
        .unwrap_or(default))
}

/// AC2.3, AC2.4, AC8.2: all-day off, declined on, cancelled on, unless a setting says otherwise.
fn expand_options(store: &Store) -> anyhow::Result<ExpandOptions> {
    Ok(ExpandOptions {
        ignore_cancelled: flag(store, IGNORE_CANCELLED, true)?,
        ignore_declined: flag(store, IGNORE_DECLINED, true)?,
        include_all_day: !flag(store, IGNORE_ALL_DAY, true)?,
    })
}

/// Today's occurrences from the enabled calendars, in the machine's zone, after the ignore rules.
pub fn today(db: &Db, now: DateTime<Utc>, local: Tz) -> anyhow::Result<Vec<Occurrence>> {
    let (from, to) = today_window(now, local)?;
    // The store guard is released before the (CPU-bound) expansion.
    let (opts, stored) = {
        let store = lock(db);
        (expand_options(&store)?, store.events().list_enabled()?)
    };
    let mut events = Vec::new();
    for stored in stored {
        match serde_json::from_str::<Event>(&stored.payload) {
            Ok(e) => events.push(e),
            // Titles are personal data and the error text can quote the payload: log the id and category only.
            Err(e) => {
                tracing::warn!(event = %stored.id, category = ?e.classify(), "stored event could not be read")
            }
        }
    }
    Ok(expand_window(&events, from, to, local, &opts))
}

#[cfg(test)]
mod tests;

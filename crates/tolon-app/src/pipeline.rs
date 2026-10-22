//! The M1 pipeline in plain functions: fetch, parse, upsert, list, today. No Tauri in here, so
//! the tests run without a window. Every URL that reaches a log or an error goes through
//! `redact_url`, or never appears at all.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, Duration, NaiveDate, SecondsFormat, Utc};
use chrono_tz::Tz;
use serde::Serialize;
use tolon_core::{
    judge_window, parse_ics, wall_clock, CalendarId, Event, Occurrence, ParseOptions, Verdict,
};
use tolon_store::{Calendar, FeedEvent, Store};
use tolon_sync::{normalize_url, redact_url, FetchError, FetchOutcome, IcsFetcher};

use crate::errors::{fetch_code, AppError, SyncErrorDto};
use crate::google;
use crate::interrupt::JoinDto;
use crate::settings;

/// The store behind a mutex. A guard is never held across an await.
pub type Db = Mutex<Store>;

/// Settings keys for the ignore rules (AC8.2). The value is a JSON bool; a missing key means the default.
pub(crate) const IGNORE_ALL_DAY: &str = "ignore_all_day";
pub(crate) const IGNORE_DECLINED: &str = "ignore_declined";
pub(crate) const IGNORE_CANCELLED: &str = "ignore_cancelled";

/// The longest custom calendar name, in characters (not bytes).
pub const MAX_NAME_CHARS: usize = 80;

/// What the UI sees of a calendar. The full URL never leaves the core (AC1.3): `display_url` is
/// scheme and host only, from `redact_url`.
#[derive(Serialize)]
pub struct CalendarDto {
    pub id: String,
    /// The name to show everywhere: the custom name, else the feed's.
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub display_url: Option<String>,
    pub last_sync_at: Option<String>,
    /// `{ kind, status? }`, for example `{ "kind": "http", "status": 404 }`; the UI localises it.
    pub last_error: Option<SyncErrorDto>,
    /// A Google calendar's account (its address); `None` for a feed.
    pub account: Option<String>,
}

impl From<Calendar> for CalendarDto {
    fn from(c: Calendar) -> Self {
        // A Google calendar's `url` is its Google calendar id, not a link: it is not shown.
        let google = c.kind == google::KIND;
        Self {
            name: c.display_name().to_owned(),
            id: c.id,
            enabled: c.enabled,
            display_url: c.url.as_deref().filter(|_| !google).map(redact_url),
            last_sync_at: c.last_sync_at,
            last_error: c.last_error.as_deref().map(SyncErrorDto::from_code),
            account: c.account.filter(|_| google),
            kind: c.kind,
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
pub(crate) fn lock(db: &Db) -> MutexGuard<'_, Store> {
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

/// What a typed name becomes: control characters dropped, trimmed, cut to 80 characters. `None`
/// (blank) clears the custom name.
pub fn clean_name(raw: &str) -> Option<String> {
    let name: String = raw.chars().filter(|c| !c.is_control()).collect();
    let name: String = name.trim().chars().take(MAX_NAME_CHARS).collect();
    let name = name.trim_end();
    (!name.is_empty()).then(|| name.to_owned())
}

/// Gives the calendar the typed name, or the feed's name again when `raw` is blank. An unknown id
/// is `not_found`. The name is kept on this computer only.
pub fn rename(db: &Db, id: &str, raw: &str) -> anyhow::Result<()> {
    lock(db)
        .calendars()
        .set_custom_name(id, clean_name(raw).as_deref())?;
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
    let url = normalize_url(raw_url).map_err(|e| match e {
        FetchError::UnsupportedScheme => AppError::UNSUPPORTED_SCHEME,
        _ => AppError::INVALID_URL,
    })?;
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
        .ok_or(AppError::NOT_FOUND)?;
    Ok(fresh.into())
}

/// Fetches one calendar with its stored etag and applies the answer.
pub async fn sync_one(
    db: &Db,
    fetcher: &impl Fetch,
    id: &str,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let cal = lock(db).calendars().get(id)?.ok_or(AppError::NOT_FOUND)?;
    let url = cal.url.as_deref().ok_or(AppError::NO_URL)?;
    let result = fetcher.fetch(url, cal.etag.as_deref()).await;
    apply(db, id, result, now)
}

/// Every enabled feed, one after the other (Google calendars: `google::sync_all`). A failure on
/// one never stops the rest; the first store error is returned at the end.
pub async fn sync_all(db: &Db, fetcher: &impl Fetch, now: DateTime<Utc>) -> anyhow::Result<()> {
    let mut first_err = None;
    for cal in calendars(db)?
        .into_iter()
        .filter(|c| c.enabled && c.kind != google::KIND)
    {
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
            // S2.3: the log records the skipped count as a code ("skipped:1"), not a sentence.
            if parsed.skipped_components > 0 {
                let detail = format!("skipped:{}", parsed.skipped_components);
                store
                    .sync_log()
                    .append(Some(id), &at, "ok", Some(&detail))?;
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
            let code = fetch_code(&e);
            store.calendars().record_failure(id, &code)?;
            store
                .sync_log()
                .append(Some(id), &at, "error", Some(&code))?;
            let host = cal.url.as_deref().map(redact_url);
            tracing::warn!(calendar = %id, host = host.as_deref(), code = %code, "calendar sync failed");
        }
    }
    Ok(())
}

/// Duplicate UIDs keep the last event, then each event becomes its stored JSON payload.
pub(crate) fn feed_events(events: Vec<Event>) -> anyhow::Result<Vec<FeedEvent>> {
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
                // Only the error category is logged: its text can quote the event.
                payload: serde_json::to_string(e).map_err(|err| {
                    tracing::warn!(category = ?err.classify(), "event could not be encoded");
                    AppError::INTERNAL
                })?,
            })
        })
        .collect()
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
        .ok_or(AppError::INTERNAL)?;
    Ok((midnight(day, local)?, midnight(next, local)?))
}

fn midnight(day: NaiveDate, local: Tz) -> anyhow::Result<DateTime<Utc>> {
    day.and_hms_opt(0, 0, 0)
        .and_then(|t| wall_clock(t, local))
        .map(|t| t.with_timezone(&Utc))
        .ok_or_else(|| AppError::INTERNAL.into())
}

pub(crate) fn flag(store: &Store, key: &str, default: bool) -> anyhow::Result<bool> {
    Ok(store
        .settings()
        .get(key)?
        .and_then(|v| v.as_bool())
        .unwrap_or(default))
}

/// The stored events as core events. A payload that no longer decodes is skipped.
pub(crate) fn decode_events(stored: Vec<tolon_store::Event>) -> Vec<Event> {
    let mut events = Vec::new();
    for item in stored {
        match serde_json::from_str::<Event>(&item.payload) {
            Ok(e) => events.push(e),
            // Titles are personal data and the error text can quote the payload: log the id and category only.
            Err(e) => {
                tracing::warn!(event = %item.id, category = ?e.classify(), "stored event could not be read")
            }
        }
    }
    events
}

/// One of today's occurrences as `occurrences_today` sends it (decision "All-day events, weekends
/// and window checks"). No URL: joining goes through `occurrences_join`. `ignored` says why it
/// gets no alarm, if it gets none (S8.1): `keyword`, `all_day`, `declined`, `cancelled` or
/// `no_attendees`.
#[derive(Serialize)]
pub struct ListedDto {
    /// `OccurrenceKey`'s text form, for `occurrences_join`.
    pub key: String,
    pub title: String,
    /// RFC 3339 with the machine's offset.
    pub start: String,
    pub end: String,
    pub all_day: bool,
    pub people: u32,
    pub ignored: Option<&'static str>,
    pub join: JoinDto,
}

/// The key (it holds the event id), the title and the join link are not printed.
impl std::fmt::Debug for ListedDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListedDto")
            .field("start", &self.start)
            .field("end", &self.end)
            .field("all_day", &self.all_day)
            .field("people", &self.people)
            .field("ignored", &self.ignored)
            .finish_non_exhaustive()
    }
}

impl ListedDto {
    fn new(o: &Occurrence, verdict: &Verdict) -> Self {
        Self {
            key: o.key.to_string(),
            title: o.title.clone(),
            start: o.start.to_rfc3339_opts(SecondsFormat::Secs, false),
            end: o.end.to_rfc3339_opts(SecondsFormat::Secs, false),
            all_day: o.all_day,
            people: o.people,
            ignored: match verdict {
                Verdict::Keep => None,
                Verdict::Ignored(reason) => Some(reason.code()),
            },
            join: JoinDto::of(&o.join),
        }
    }
}

/// Today's occurrences for the panel, as sent to the page.
pub fn today(db: &Db, now: DateTime<Utc>, local: Tz) -> anyhow::Result<Vec<ListedDto>> {
    Ok(today_judged(db, now, local)?
        .iter()
        .map(|(o, v)| ListedDto::new(o, v))
        .collect())
}

/// Today's occurrences from the enabled calendars, in the machine's zone, each with the ignore
/// rule that silences it (AC8.2, S8.1). The ignored ones stay listed so the panel can tag them.
pub fn today_judged(
    db: &Db,
    now: DateTime<Utc>,
    local: Tz,
) -> anyhow::Result<Vec<(Occurrence, Verdict)>> {
    let (from, to) = today_window(now, local)?;
    // The store guard is released before the (CPU-bound) expansion.
    let (settings, stored) = {
        let store = lock(db);
        (settings::read(&store)?, store.events().list_enabled()?)
    };
    Ok(judge_window(
        &decode_events(stored),
        from,
        to,
        local,
        &settings.expand_options(),
        &settings.rules(),
    ))
}

#[cfg(test)]
pub(crate) mod tests;

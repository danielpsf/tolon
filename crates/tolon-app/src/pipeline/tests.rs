use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Mutex};

use anyhow::anyhow;
use chrono::TimeZone;
use serde_json::json;

use super::*;
use crate::errors::{command_error, CommandError};

type TestResult = anyhow::Result<()>;

fn http(status: u16) -> SyncErrorDto {
    SyncErrorDto {
        kind: "http".to_owned(),
        status: Some(status),
    }
}

const SECRET_URL: &str = "https://calendar.example.com/ical/abc/private-123/basic.ics";

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 20, 8, 0, 0)
        .single()
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

fn db() -> anyhow::Result<Db> {
    Ok(Mutex::new(Store::open_in_memory()?))
}

/// Answers `fetch` from a queue and records what it was asked.
struct Fake {
    answers: Mutex<VecDeque<Result<FetchOutcome, FetchError>>>,
    calls: Mutex<Vec<(String, Option<String>)>>,
}

impl Fake {
    fn new(answers: Vec<Result<FetchOutcome, FetchError>>) -> Self {
        Self {
            answers: Mutex::new(answers.into()),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<(String, Option<String>)> {
        self.calls.lock().map(|c| c.clone()).unwrap_or_default()
    }
}

impl Fetch for Fake {
    fn fetch(
        &self,
        url: &str,
        etag: Option<&str>,
    ) -> impl Future<Output = Result<FetchOutcome, FetchError>> + Send {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push((url.to_owned(), etag.map(str::to_owned)));
        }
        let answer = self
            .answers
            .lock()
            .ok()
            .and_then(|mut q| q.pop_front())
            .unwrap_or(Err(FetchError::Network));
        async move { answer }
    }
}

fn modified(body: String, etag: Option<&str>) -> Result<FetchOutcome, FetchError> {
    Ok(FetchOutcome::Modified {
        body,
        etag: etag.map(str::to_owned),
    })
}

fn vevent(uid: &str, summary: &str, extra: &str) -> String {
    format!("BEGIN:VEVENT\r\nUID:{uid}\r\nSUMMARY:{summary}\r\n{extra}END:VEVENT\r\n")
}

fn feed(name: Option<&str>, events: &[String]) -> String {
    let name = name
        .map(|n| format!("X-WR-CALNAME:{n}\r\n"))
        .unwrap_or_default();
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{name}{}END:VCALENDAR\r\n",
        events.concat()
    )
}

/// Two events today (10:00 and 11:00 UTC) and one tomorrow.
fn basic_feed(name: Option<&str>) -> String {
    feed(
        name,
        &[
            vevent(
                "a",
                "Standup",
                "DTSTART:20261020T100000Z\r\nDTEND:20261020T103000Z\r\n",
            ),
            vevent(
                "b",
                "Review",
                "DTSTART:20261020T110000Z\r\nDTEND:20261020T120000Z\r\n",
            ),
            vevent(
                "c",
                "Tomorrow",
                "DTSTART:20261021T100000Z\r\nDTEND:20261021T110000Z\r\n",
            ),
        ],
    )
}

/// The titles of the meetings that get an alarm; the ignored ones are listed but left out here.
fn titles(listed: &[ListedDto]) -> Vec<&str> {
    listed
        .iter()
        .filter(|l| l.ignored.is_none())
        .map(|l| l.title.as_str())
        .collect()
}

fn stored_uids(db: &Db, id: &str) -> anyhow::Result<Vec<String>> {
    Ok(lock(db)
        .events()
        .list_by_calendar(id)?
        .into_iter()
        .map(|e| e.uid)
        .collect())
}

#[tokio::test]
async fn s1_1_add_names_calendar_from_x_wr_calname() -> TestResult {
    let db = db()?;
    let fake = Fake::new(vec![modified(
        basic_feed(Some("Team calendar")),
        Some("v1"),
    )]);
    let dto = add_ics(&db, &fake, SECRET_URL, now()).await?;

    assert_eq!(dto.name, "Team calendar");
    assert_eq!(dto.kind, "ics");
    assert!(dto.enabled);
    assert_eq!(
        dto.display_url.as_deref(),
        Some("https://calendar.example.com")
    );
    assert_eq!(
        dto.last_sync_at.as_deref(),
        Some(now().to_rfc3339().as_str())
    );
    assert_eq!(dto.last_error, None);
    assert_eq!(list(&db)?.len(), 1);
    assert_eq!(stored_uids(&db, &dto.id)?, ["a", "b", "c"]);
    // "today's events from that feed appear in the tray panel"
    let today = today(&db, now(), Tz::UTC)?;
    assert_eq!(titles(&today), ["Standup", "Review"]);
    Ok(())
}

#[tokio::test]
async fn l067_rename_shows_everywhere_and_clearing_restores_the_feed_name() -> TestResult {
    let db = db()?;
    let fake = Fake::new(vec![modified(basic_feed(Some("Team calendar")), None)]);
    let dto = add_ics(&db, &fake, SECRET_URL, now()).await?;

    rename(&db, &dto.id, "  Standups  ")?;
    assert_eq!(
        list(&db)?.first().map(|c| c.name.as_str()),
        Some("Standups")
    );
    // A sync keeps the custom name; the feed's name stays underneath it.
    let again = Fake::new(vec![modified(basic_feed(Some("Renamed upstream")), None)]);
    sync_one(&db, &again, &dto.id, now()).await?;
    assert_eq!(
        list(&db)?.first().map(|c| c.name.as_str()),
        Some("Standups")
    );
    assert_eq!(
        calendars(&db)?.first().map(|c| c.name.as_str()),
        Some("Team calendar")
    );

    rename(&db, &dto.id, "   ")?;
    assert_eq!(
        list(&db)?.first().map(|c| c.name.as_str()),
        Some("Team calendar")
    );
    Ok(())
}

#[test]
fn l067_rename_validates_the_id_and_the_name() -> TestResult {
    let db = db()?;
    let Err(e) = rename(&db, "no-such-calendar", "Work") else {
        return Err(anyhow!("an unknown id should be rejected"));
    };
    assert_eq!(command_error(&e), CommandError { code: "not_found" });

    assert_eq!(clean_name("  Work  "), Some("Work".to_owned()));
    assert_eq!(clean_name(" \t\n "), None);
    assert_eq!(clean_name(""), None);
    assert_eq!(clean_name("Wor\nk\u{0}"), Some("Work".to_owned()));
    // Cut by characters, never in the middle of one; a cut that ends on a space drops it.
    let long = "é".repeat(200);
    assert_eq!(
        clean_name(&long).map(|n| n.chars().count()),
        Some(MAX_NAME_CHARS)
    );
    let spaced = format!("{} tail", "a".repeat(MAX_NAME_CHARS - 1));
    assert_eq!(clean_name(&spaced), Some("a".repeat(MAX_NAME_CHARS - 1)));
    Ok(())
}

#[tokio::test]
async fn s1_1_add_falls_back_to_host() -> TestResult {
    let db = db()?;
    let fake = Fake::new(vec![modified(basic_feed(None), None)]);
    let dto = add_ics(&db, &fake, SECRET_URL, now()).await?;
    assert_eq!(dto.name, "calendar.example.com");
    Ok(())
}

#[tokio::test]
async fn s1_2_webcal_stored_as_https() -> TestResult {
    let db = db()?;
    let fake = Fake::new(vec![modified(basic_feed(None), None)]);
    let dto = add_ics(&db, &fake, "webcal://example.com/team.ics", now()).await?;

    assert_eq!(dto.display_url.as_deref(), Some("https://example.com"));
    assert_eq!(
        fake.calls(),
        [("https://example.com/team.ics".to_owned(), None)]
    );
    assert_eq!(dto.name, "example.com");
    Ok(())
}

#[tokio::test]
async fn s1_2_other_schemes_and_junk_are_rejected_with_a_code() -> TestResult {
    let db = db()?;
    let fake = Fake::new(vec![]);
    for (bad, code) in [
        ("http://example.com/a.ics", "unsupported_scheme"),
        ("ftp://example.com/a.ics", "unsupported_scheme"),
        ("not a link", "invalid_url"),
    ] {
        let Err(e) = add_ics(&db, &fake, bad, now()).await else {
            return Err(anyhow!("{bad} should be rejected"));
        };
        assert_eq!(command_error(&e), CommandError { code }, "{bad}");
        assert!(!e.to_string().contains(bad), "echoes the input");
    }
    assert!(
        fake.calls().is_empty(),
        "nothing is fetched for rejected input"
    );
    assert!(list(&db)?.is_empty());
    Ok(())
}

#[tokio::test]
async fn s1_3_unreachable_link_is_added_with_status() -> TestResult {
    let db = db()?;
    let fake = Fake::new(vec![Err(FetchError::Status(404))]);
    let dto = add_ics(&db, &fake, SECRET_URL, now()).await?;

    assert_eq!(dto.last_error, Some(http(404)));
    assert_eq!(dto.last_sync_at, None);
    assert_eq!(dto.name, "calendar.example.com");
    let listed = list(&db)?;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].last_error, Some(http(404)));
    // Stored as the stable code, not as a sentence.
    assert_eq!(
        lock(&db)
            .calendars()
            .get(&dto.id)?
            .and_then(|c| c.last_error),
        Some("http:404".to_owned())
    );
    let log = lock(&db).sync_log().list_recent(10)?;
    assert_eq!(log[0].detail.as_deref(), Some("http:404"));
    assert!(stored_uids(&db, &dto.id)?.is_empty());
    assert!(
        today(&db, now(), Tz::UTC)?.is_empty(),
        "no alarm source from it"
    );
    Ok(())
}

#[tokio::test]
async fn s1_3_non_http_failures_use_the_error_kind_code() -> TestResult {
    let cases = [
        (FetchError::Timeout, "timeout"),
        (FetchError::Connect, "connect"),
        (FetchError::NotCalendar, "not_calendar"),
        (FetchError::TooLarge { limit: 5 }, "too_large"),
        (FetchError::Network, "network"),
    ];
    for (err, kind) in cases {
        let db = db()?;
        let dto = add_ics(&db, &Fake::new(vec![Err(err)]), SECRET_URL, now()).await?;
        assert_eq!(
            dto.last_error,
            Some(SyncErrorDto {
                kind: kind.to_owned(),
                status: None
            })
        );
    }
    Ok(())
}

#[tokio::test]
async fn s1_5_not_modified_keeps_events_and_clears_error() -> TestResult {
    let db = db()?;
    let fake = Fake::new(vec![
        modified(basic_feed(Some("Team")), Some("v1")),
        Err(FetchError::Status(500)),
        Ok(FetchOutcome::NotModified),
    ]);
    let dto = add_ics(&db, &fake, SECRET_URL, now()).await?;
    let before = lock(&db).events().list_by_calendar(&dto.id)?;

    let later = now() + Duration::minutes(5);
    sync_one(&db, &fake, &dto.id, later).await?;
    assert_eq!(
        list(&db)?[0].last_error,
        Some(http(500)),
        "a failed refresh keeps the events and shows the error"
    );

    let latest = later + Duration::minutes(5);
    sync_one(&db, &fake, &dto.id, latest).await?;
    let after = list(&db)?;
    assert_eq!(
        after[0].last_error, None,
        "a 304 is healthy and clears the error"
    );
    assert_eq!(
        after[0].last_sync_at.as_deref(),
        Some(latest.to_rfc3339().as_str())
    );
    assert_eq!(lock(&db).events().list_by_calendar(&dto.id)?, before);
    let etag = lock(&db).calendars().get(&dto.id)?.and_then(|c| c.etag);
    assert_eq!(etag.as_deref(), Some("v1"));
    // The second and third refresh both carried the stored etag.
    let calls = fake.calls();
    assert_eq!(calls[1].1.as_deref(), Some("v1"));
    assert_eq!(calls[2].1.as_deref(), Some("v1"));
    Ok(())
}

#[tokio::test]
async fn s2_3_skipped_component_goes_to_sync_log() -> TestResult {
    let db = db()?;
    let body = feed(
        Some("Team"),
        &[
            vevent("ok", "Fine", "DTSTART:20261020T100000Z\r\n"),
            vevent("bad", "No start", ""),
        ],
    );
    let dto = add_ics(
        &db,
        &Fake::new(vec![modified(body, None)]),
        SECRET_URL,
        now(),
    )
    .await?;

    assert_eq!(stored_uids(&db, &dto.id)?, ["ok"]);
    let log = lock(&db).sync_log().list_recent(10)?;
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].detail.as_deref(), Some("skipped:1"));
    assert_eq!(log[0].calendar_id.as_deref(), Some(dto.id.as_str()));
    Ok(())
}

#[tokio::test]
async fn sync_keeps_the_last_of_duplicate_uids_and_drops_vanished_events() -> TestResult {
    let db = db()?;
    let first = feed(
        Some("Team"),
        &[
            vevent("dup", "First", "DTSTART:20261020T090000Z\r\n"),
            vevent("gone", "Gone", "DTSTART:20261020T100000Z\r\n"),
            vevent("dup", "Last", "DTSTART:20261020T110000Z\r\n"),
        ],
    );
    let second = feed(
        Some("Team"),
        &[vevent("dup", "Last", "DTSTART:20261020T110000Z\r\n")],
    );
    let fake = Fake::new(vec![
        modified(first, Some("v1")),
        modified(second, Some("v2")),
    ]);
    let dto = add_ics(&db, &fake, SECRET_URL, now()).await?;
    assert_eq!(stored_uids(&db, &dto.id)?, ["dup", "gone"]);
    assert_eq!(titles(&today(&db, now(), Tz::UTC)?), ["Gone", "Last"]);

    sync_one(&db, &fake, &dto.id, now() + Duration::minutes(5)).await?;
    assert_eq!(stored_uids(&db, &dto.id)?, ["dup"]);
    Ok(())
}

#[tokio::test]
async fn sync_all_skips_disabled_calendars() -> TestResult {
    let db = db()?;
    let fake = Fake::new(vec![
        modified(basic_feed(Some("One")), None),
        modified(basic_feed(Some("Two")), None),
        modified(basic_feed(Some("One")), None),
    ]);
    let one = add_ics(&db, &fake, "https://one.example.com/a.ics", now()).await?;
    let two = add_ics(&db, &fake, "https://two.example.com/a.ics", now()).await?;
    set_enabled(&db, &two.id, false)?;

    sync_all(&db, &fake, now() + Duration::minutes(5)).await?;
    let calls = fake.calls();
    assert_eq!(calls.len(), 3, "two adds and one refresh");
    assert_eq!(calls[2].0, "https://one.example.com/a.ics");
    assert_eq!(one.display_url.as_deref(), Some("https://one.example.com"));
    Ok(())
}

#[tokio::test]
async fn remove_and_set_enabled_affect_today() -> TestResult {
    let db = db()?;
    let fake = Fake::new(vec![
        modified(basic_feed(Some("One")), None),
        modified(
            feed(
                Some("Two"),
                &[vevent("z", "Other", "DTSTART:20261020T130000Z\r\n")],
            ),
            None,
        ),
    ]);
    let one = add_ics(&db, &fake, "https://one.example.com/a.ics", now()).await?;
    let two = add_ics(&db, &fake, "https://two.example.com/a.ics", now()).await?;
    assert_eq!(
        titles(&today(&db, now(), Tz::UTC)?),
        ["Standup", "Review", "Other"]
    );

    set_enabled(&db, &one.id, false)?;
    assert_eq!(titles(&today(&db, now(), Tz::UTC)?), ["Other"]);
    // Same created_at, so list order is by id; look the calendar up by id.
    assert!(list(&db)?.iter().any(|c| c.id == one.id && !c.enabled));
    assert_eq!(
        stored_uids(&db, &one.id)?.len(),
        3,
        "disabling keeps the events"
    );

    set_enabled(&db, &one.id, true)?;
    assert_eq!(titles(&today(&db, now(), Tz::UTC)?).len(), 3);

    remove(&db, &two.id)?;
    assert_eq!(titles(&today(&db, now(), Tz::UTC)?), ["Standup", "Review"]);
    assert_eq!(list(&db)?.len(), 1);
    assert!(
        stored_uids(&db, &two.id)?.is_empty(),
        "events go with the calendar"
    );
    Ok(())
}

/// One event per ignore rule, all today. The calendar name is an address, which is how the
/// parser recognises the owner for the declined rule.
fn rules_feed() -> String {
    feed(
        Some("me@example.com"),
        &[
            vevent("timed", "Timed", "DTSTART:20261020T100000Z\r\n"),
            vevent("allday", "All day", "DTSTART;VALUE=DATE:20261020\r\n"),
            vevent(
                "cancelled",
                "Cancelled",
                "DTSTART:20261020T110000Z\r\nSTATUS:CANCELLED\r\n",
            ),
            vevent(
                "declined",
                "Declined",
                "DTSTART:20261020T120000Z\r\nATTENDEE;PARTSTAT=DECLINED:mailto:me@example.com\r\n",
            ),
        ],
    )
}

#[tokio::test]
async fn ac8_2_today_ignores_all_day_declined_and_cancelled_by_default() -> TestResult {
    let db = db()?;
    let fake = Fake::new(vec![modified(rules_feed(), None)]);
    add_ics(&db, &fake, SECRET_URL, now()).await?;
    assert_eq!(titles(&today(&db, now(), Tz::UTC)?), ["Timed"]);
    Ok(())
}

#[tokio::test]
async fn s8_1_ignored_meetings_are_listed_with_their_reason() -> TestResult {
    let db = db()?;
    let feed_text = rules_feed().replace(
        "END:VCALENDAR",
        "BEGIN:VEVENT\r\nUID:focus\r\nSUMMARY:Focus time\r\nDTSTART:20261020T130000Z\r\nDTEND:20261020T140000Z\r\nEND:VEVENT\r\nEND:VCALENDAR",
    );
    let fake = Fake::new(vec![modified(feed_text, None)]);
    add_ics(&db, &fake, SECRET_URL, now()).await?;
    let listed = today(&db, now(), Tz::UTC)?;
    let tags: Vec<(&str, Option<&str>)> = listed
        .iter()
        .map(|l| (l.title.as_str(), l.ignored))
        .collect();
    assert_eq!(
        tags,
        [
            ("All day", Some("all_day")),
            ("Timed", None),
            ("Cancelled", Some("cancelled")),
            ("Declined", Some("declined")),
            ("Focus time", Some("keyword")),
        ]
    );
    // The DTO is the occurrence's fields plus `ignored`, as the panel reads it.
    let first = serde_json::to_value(&listed[1])?;
    assert_eq!(first["title"], json!("Timed"));
    assert_eq!(first["ignored"], serde_json::Value::Null);
    assert_eq!(
        serde_json::to_value(&listed[4])?["ignored"],
        json!("keyword")
    );
    Ok(())
}

#[tokio::test]
async fn ac8_2_settings_turn_each_rule_off() -> TestResult {
    let db = db()?;
    let fake = Fake::new(vec![modified(rules_feed(), None)]);
    add_ics(&db, &fake, SECRET_URL, now()).await?;

    let set = |key: &str, on: bool| -> TestResult {
        lock(&db).settings().set(key, &json!(on))?;
        Ok(())
    };
    set(IGNORE_ALL_DAY, false)?;
    assert_eq!(titles(&today(&db, now(), Tz::UTC)?), ["All day", "Timed"]);
    set(IGNORE_CANCELLED, false)?;
    assert_eq!(
        titles(&today(&db, now(), Tz::UTC)?),
        ["All day", "Timed", "Cancelled"]
    );
    set(IGNORE_DECLINED, false)?;
    assert_eq!(titles(&today(&db, now(), Tz::UTC)?).len(), 4);
    // A value that is not a bool counts as missing, so the default applies again.
    lock(&db).settings().set(IGNORE_ALL_DAY, &json!("yes"))?;
    assert_eq!(titles(&today(&db, now(), Tz::UTC)?).len(), 3);
    Ok(())
}

#[test]
fn today_window_follows_the_local_day() -> TestResult {
    let berlin: Tz = "Europe/Berlin".parse()?;
    // 23:30 UTC on 24 Oct is already 25 Oct in Berlin; clocks go back that night, so the day has 25 hours.
    let late = Utc
        .with_ymd_and_hms(2026, 10, 24, 23, 30, 0)
        .single()
        .ok_or_else(|| anyhow!("date"))?;
    let (from, to) = today_window(late, berlin)?;
    assert_eq!(
        from,
        Utc.with_ymd_and_hms(2026, 10, 24, 22, 0, 0)
            .single()
            .ok_or_else(|| anyhow!("date"))?
    );
    assert_eq!(to - from, Duration::hours(25));
    Ok(())
}

#[test]
fn local_zone_always_yields_a_zone() {
    // Whatever the machine says, there is a zone to expand in.
    let _ = local_zone();
}

/// Collects tracing output so a test can look at it.
#[derive(Clone, Default)]
pub(crate) struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Ok(mut v) = self.0.lock() {
            v.extend_from_slice(buf);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Sink {
    pub(crate) fn text(&self) -> String {
        self.0
            .lock()
            .map(|v| String::from_utf8_lossy(&v).into_owned())
            .unwrap_or_default()
    }

    pub(crate) fn capture(&self) -> tracing::subscriber::DefaultGuard {
        let sink = self.clone();
        tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .with_writer(move || sink.clone())
                .with_max_level(tracing::Level::TRACE)
                .finish(),
        )
    }
}

/// Everything a failed sync wrote, other than the stored URL itself, must be free of the secret.
fn assert_secret_free(db: &Db, logs: &Sink, secret: &str) -> TestResult {
    let out = logs.text();
    assert!(!out.is_empty(), "the failure was logged");
    assert!(!out.contains(secret), "log output leaked the secret: {out}");
    let rows = lock(db).sync_log().list_recent(100)?;
    assert!(!rows.is_empty());
    for row in rows {
        let text = format!("{} {} {:?}", row.at, row.outcome, row.detail);
        assert!(!text.contains(secret), "sync_log leaked the secret: {text}");
    }
    for cal in list(db)? {
        let err = format!("{:?}", cal.last_error);
        assert!(!err.contains(secret), "last_error leaked the secret: {err}");
    }
    Ok(())
}

#[tokio::test]
async fn ac1_3_secret_absent_from_logs_sync_log_and_last_error() -> TestResult {
    let logs = Sink::default();
    let _guard = logs.capture();
    let db = db()?;
    let fake = Fake::new(vec![Err(FetchError::Status(404)), Err(FetchError::Connect)]);

    let dto = add_ics(&db, &fake, SECRET_URL, now()).await?;
    sync_one(&db, &fake, &dto.id, now() + Duration::minutes(5)).await?;

    assert!(
        logs.text().contains("calendar.example.com"),
        "the redacted host is logged"
    );
    assert_secret_free(&db, &logs, "private-123")?;
    assert_eq!(
        dto.display_url.as_deref(),
        Some("https://calendar.example.com")
    );
    Ok(())
}

#[tokio::test]
async fn ac1_3_real_fetcher_failure_keeps_the_secret_out_too() -> TestResult {
    let logs = Sink::default();
    let _guard = logs.capture();
    let db = db()?;
    let fetcher = IcsFetcher::new()?;
    // Nothing listens on port 1 of localhost, so the connect fails at once.
    let url = "https://127.0.0.1:1/ical/private-123/basic.ics";

    let dto = add_ics(&db, &fetcher, url, now()).await?;
    assert!(dto.last_error.is_some());
    assert_secret_free(&db, &logs, "private-123")
}

#[tokio::test]
async fn unreadable_stored_events_are_skipped_and_their_text_is_not_logged() -> TestResult {
    let logs = Sink::default();
    let _guard = logs.capture();
    let db = db()?;
    let dto = add_ics(
        &db,
        &Fake::new(vec![modified(basic_feed(Some("Team")), None)]),
        SECRET_URL,
        now(),
    )
    .await?;
    lock(&db).events().replace_for_calendar(
        &dto.id,
        &[tolon_store::FeedEvent {
            uid: "bad".to_owned(),
            payload: r#"{"title":"SECRET-TITLE","start":"#.to_owned(),
        }],
        &now().to_rfc3339(),
    )?;

    assert!(today(&db, now(), Tz::UTC)?.is_empty());
    let out = logs.text();
    assert!(out.contains("stored event could not be read"), "{out}");
    assert!(
        !out.contains("SECRET-TITLE"),
        "payload text reached the log: {out}"
    );
    Ok(())
}

#[tokio::test]
async fn a_poisoned_store_lock_is_recovered() -> TestResult {
    let db = db()?;
    let _ = std::thread::scope(|s| {
        s.spawn(|| {
            let _held = db.lock();
            std::panic::resume_unwind(Box::new("poison the lock"));
        })
        .join()
    });
    assert!(db.is_poisoned());
    let fake = Fake::new(vec![modified(basic_feed(Some("Team")), None)]);
    add_ics(&db, &fake, SECRET_URL, now()).await?;
    assert_eq!(list(&db)?.len(), 1);
    Ok(())
}

fn calendar_row(url: &str) -> Calendar {
    Calendar {
        id: "c1".into(),
        name: "Work".into(),
        custom_name: None,
        kind: "ics".into(),
        enabled: true,
        url: Some(url.into()),
        account: None,
        color: None,
        etag: None,
        last_sync_at: None,
        last_error: None,
        created_at: "2026-10-20T08:00:00Z".into(),
    }
}

#[test]
fn ac1_3_dto_json_never_carries_the_secret() -> TestResult {
    let url = "https://user-private-123:pw-private-123@host.example.com:8443/private-123/feed.ics?token=private-123#private-123";
    let json = serde_json::to_string(&CalendarDto::from(calendar_row(url)))?;
    assert!(!json.contains("private-123"), "{json}");
    assert!(!json.contains("\"url\""), "{json}");
    assert!(
        json.contains("\"display_url\":\"https://host.example.com\""),
        "{json}"
    );
    Ok(())
}

#[test]
fn ac1_3_display_url_is_redact_url() {
    let dto = CalendarDto::from(calendar_row(SECRET_URL));
    assert_eq!(dto.display_url, Some(redact_url(SECRET_URL)));
}

#[test]
fn debug_output_of_a_listed_meeting_omits_its_title_key_and_join_link() {
    let listed = ListedDto {
        key: "event-SECRET@2026-10-22T08:00:00Z".into(),
        title: "Salary talk SECRET".into(),
        start: "2026-10-22T10:00:00+02:00".into(),
        end: "2026-10-22T10:30:00+02:00".into(),
        all_day: false,
        people: 2,
        ignored: None,
        join: crate::interrupt::JoinDto::Link {
            host: "meet-SECRET.example".into(),
            recognised: true,
        },
    };
    let text = format!("{listed:?}");
    assert!(!text.contains("SECRET"), "{text}");
    assert!(text.contains("people: 2"), "{text}");
}

#[tokio::test]
async fn occurrences_today_sends_no_url_for_any_join_kind() -> TestResult {
    let db = db()?;
    let text = feed(
        None,
        &[
            vevent(
                "call",
                "Call",
                "DTSTART:20261020T100000Z\r\nLOCATION:https://meet.google.com/SECRET-CALL\r\n",
            ),
            vevent(
                "page",
                "Page",
                "DTSTART:20261020T110000Z\r\nURL:https://calendar.example.com/event?eid=SECRET-PAGE\r\n",
            ),
            vevent("none", "Nothing", "DTSTART:20261020T120000Z\r\n"),
        ],
    );
    let fake = Fake::new(vec![modified(text, None)]);
    add_ics(&db, &fake, SECRET_URL, now()).await?;
    let json = serde_json::to_value(today(&db, now(), Tz::UTC)?)?;
    let joins: Vec<&serde_json::Value> = json
        .as_array()
        .map(|a| a.iter().map(|l| &l["join"]).collect())
        .unwrap_or_default();
    assert_eq!(
        joins,
        [
            &json!({ "kind": "link", "host": "meet.google.com", "recognised": true }),
            &json!({ "kind": "event_page" }),
            &json!({ "kind": "none" }),
        ]
    );
    let text = json.to_string();
    for secret in ["SECRET-CALL", "SECRET-PAGE", "https://", "url"] {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
    let first = &json[0];
    let mut fields: Vec<&str> = first
        .as_object()
        .map(|o| o.keys().map(String::as_str).collect())
        .unwrap_or_default();
    fields.sort_unstable();
    assert_eq!(
        fields,
        ["all_day", "end", "ignored", "join", "key", "people", "start", "title"]
    );
    Ok(())
}

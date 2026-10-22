//! L-072: the zip carries what is listed in `docs/help/diagnostics.md` and none of the secrets a
//! store and a log can hold.

use std::future::Future;
use std::sync::{Arc, Mutex};

use anyhow::anyhow;
use chrono::{TimeZone, Utc};
use chrono_tz::Tz;
use tolon_core::AlarmSettings;
use tolon_store::NewGoogleCalendar;
use tolon_sync::{FetchError, FetchOutcome};
use tracing_subscriber::layer::SubscriberExt;

use super::*;
use crate::logs::{file_layer, LogFiles};
use crate::pipeline::{self, Db, Fetch};
use crate::scheduler;

type TestResult = anyhow::Result<()>;

/// Every string that must never reach the zip: the address and its token, the feed's calendar
/// name, the event's title, description, location, attendee, UID and join link, the Google
/// account and calendar ids, the sync token, an OAuth access token, and an ignore word.
const SECRETS: [&str; 12] = [
    "private-SECRET",
    "SECRETTITLE",
    "SECRETDESCRIPTION",
    "SECRETLOCATION",
    "SECRETCALNAME",
    "a@b.example",
    "c@d.example",
    "SECRETETAG",
    "SECRET-SYNC-TOKEN",
    "ya29.SECRET-OAUTH-TOKEN",
    "SECRETKEYWORD",
    "meet.example",
];

const SECRET_URL: &str =
    "https://calendar.example/private-SECRET/basic.ics?access_token=ya29.SECRET-OAUTH-TOKEN";

/// One event a minute after `now` (2026-10-20 08:00 UTC), so the scheduler plans and fires it.
const FEED: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-WR-CALNAME:SECRETCALNAME\r\nBEGIN:VEVENT\r\nUID:uid-a@b.example\r\nDTSTAMP:20261020T000000Z\r\nDTSTART:20261020T080100Z\r\nDTEND:20261020T083000Z\r\nSUMMARY:SECRETTITLE\r\nDESCRIPTION:SECRETDESCRIPTION https://meet.example/private-SECRET\r\nLOCATION:SECRETLOCATION\r\nORGANIZER:mailto:c@d.example\r\nATTENDEE;CN=Someone:mailto:a@b.example\r\nATTENDEE:mailto:c@d.example\r\nURL:https://meet.example/private-SECRET\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

/// Answers every fetch from a queue.
struct Fake(Mutex<Vec<Result<FetchOutcome, FetchError>>>);

impl Fetch for Fake {
    fn fetch(
        &self,
        _url: &str,
        _etag: Option<&str>,
    ) -> impl Future<Output = Result<FetchOutcome, FetchError>> + Send {
        let next = self
            .0
            .lock()
            .ok()
            .and_then(|mut q| (!q.is_empty()).then(|| q.remove(0)));
        async move { next.unwrap_or(Err(FetchError::Network)) }
    }
}

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 20, 8, 0, 0)
        .single()
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// A store with an iCal calendar that synced, one that failed, and a Google calendar; the
/// scheduler has planned and fired the meeting; every step logged to the files in `logs`.
async fn seeded(logs: &Path) -> anyhow::Result<Db> {
    let files = Arc::new(LogFiles::open(logs)?);
    let subscriber = tracing_subscriber::registry().with(file_layer(files));
    let _guard = tracing::subscriber::set_default(subscriber);

    let db: Db = Mutex::new(Store::open_in_memory()?);
    let fake = Fake(Mutex::new(vec![
        Ok(FetchOutcome::Modified {
            body: FEED.to_owned(),
            etag: Some("SECRETETAG".to_owned()),
        }),
        Err(FetchError::Status(404)),
    ]));
    pipeline::add_ics(&db, &fake, SECRET_URL, now()).await?;
    pipeline::add_ics(
        &db,
        &fake,
        "webcal://calendar.example/private-SECRET/other.ics",
        now(),
    )
    .await?;
    {
        let store = pipeline::lock(&db);
        let id = tolon_store::CalendarsRepo::new_id();
        store.calendars().insert_google_many(
            &[NewGoogleCalendar {
                id: &id,
                name: "SECRETCALNAME",
                remote_id: "a@b.example",
            }],
            "a@b.example",
            "2026-10-19T08:00:00Z",
        )?;
        store
            .calendars()
            .record_success(&id, Some("SECRET-SYNC-TOKEN"), "2026-10-20T07:55:00Z")?;
        store.calendars().record_failure(&id, "auth")?;
        // A detail written as text by some older build is not copied.
        store.sync_log().append(
            Some(&id),
            "2026-10-20T07:56:00Z",
            "error",
            Some("token ya29.SECRET-OAUTH-TOKEN refused"),
        )?;
        store
            .sync_log()
            .append(Some(&id), "2026-10-20T07:57:00Z", "error", Some("auth"))?;
        store.settings().set(
            scheduler::IGNORE_KEYWORDS,
            &serde_json::json!("SECRETKEYWORD, Lunch"),
        )?;
    }
    let inputs = scheduler::load_inputs(&db, Tz::UTC, now())?;
    let occs = scheduler::occurrences(&inputs, now(), Tz::UTC);
    assert_eq!(occs.len(), 1, "the seeded meeting is planned");
    scheduler::plan_and_tick(&db, &occs, &AlarmSettings::default(), (None, None), now())?;
    tracing::info!(host = %redact_url(SECRET_URL), "a line with a redacted host");
    Ok(db)
}

fn facts() -> Facts {
    Facts {
        version: "0.1.0-beta.1".into(),
        os: "stub",
        os_version: "stub".into(),
        screens: vec![ScreenInfo {
            width: 3456,
            height: 2234,
            x: 0,
            y: 0,
            scale: 2.0,
        }],
        time_zone: "Europe/Berlin".into(),
        client: Client {
            locale: "en-US".into(),
            languages: vec!["en-US".into(), "pt-BR".into()],
            user_agent: "Mozilla/5.0 AppleWebKit/605.1.15".into(),
        },
    }
}

fn local() -> DateTime<FixedOffset> {
    now()
        .with_timezone(&chrono_tz::Europe::Berlin)
        .fixed_offset()
}

async fn zip_of_seeded_store() -> anyhow::Result<(tempfile::TempDir, Vec<u8>)> {
    let dir = tempfile::tempdir()?;
    let logs = dir.path().join("logs");
    let db = seeded(&logs).await?;
    let store = pipeline::lock(&db);
    // The secrets are really in the store, so their absence from the zip means something.
    let cals = store.calendars().list()?;
    assert!(cals.iter().any(|c| c.url.as_deref() == Some(SECRET_URL)));
    assert!(cals
        .iter()
        .any(|c| c.etag.as_deref() == Some("SECRET-SYNC-TOKEN")));
    assert!(cals
        .iter()
        .any(|c| c.account.as_deref() == Some("a@b.example")));
    assert!(cals.iter().any(|c| c.name == "SECRETCALNAME"));
    let payload: String = store
        .events()
        .list_enabled()?
        .into_iter()
        .map(|e| e.payload)
        .collect();
    for secret in [
        "SECRETTITLE",
        "SECRETDESCRIPTION",
        "SECRETLOCATION",
        "a@b.example",
    ] {
        assert!(payload.contains(secret), "{secret} not seeded");
    }
    let env = environment(facts(), &store, local());
    let files = store_entries(&store, now())?;
    let bytes = archive(&env, files, Some(&logs), local())?;
    Ok((dir, bytes))
}

#[tokio::test]
async fn l072_no_secret_title_or_address_reaches_the_zip() -> TestResult {
    let (dir, bytes) = zip_of_seeded_store().await?;
    let entries = zip_writer::read(&bytes)?;
    for secret in SECRETS {
        assert!(!contains(&bytes, secret), "{secret} in the zip bytes");
        for e in &entries {
            assert!(!contains(&e.data, secret), "{secret} in {}", e.name);
            assert!(!contains(e.name.as_bytes(), secret), "{secret} in a name");
        }
    }
    // The same through the system tools, on the file as it is saved.
    let path = target(&dir.path().join("Tolon-diagnostics").to_string_lossy())?;
    save(&path, &bytes)?;
    assert!(path.extension().is_some_and(|e| e == "zip"));
    if let Ok(out) = std::process::Command::new("unzip")
        .arg("-t")
        .arg(&path)
        .output()
    {
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
    }
    let into = dir.path().join("extracted");
    let extracted = std::process::Command::new("unzip")
        .arg("-q")
        .arg(&path)
        .arg("-d")
        .arg(&into)
        .status()
        .is_ok_and(|s| s.success());
    if extracted {
        let mut seen = 0;
        let mut stack = vec![into];
        while let Some(at) = stack.pop() {
            for entry in std::fs::read_dir(&at)? {
                let p = entry?.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                let data = std::fs::read(&p)?;
                for secret in SECRETS {
                    assert!(!contains(&data, secret), "{secret} in {}", p.display());
                }
                seen += 1;
            }
        }
        assert_eq!(seen, entries.len(), "every entry was extracted and read");
    }
    Ok(())
}

#[tokio::test]
async fn l072_the_zip_holds_the_listed_files_with_what_an_agent_needs() -> TestResult {
    let (_dir, bytes) = zip_of_seeded_store().await?;
    let entries = zip_writer::read(&bytes)?;
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names[..6],
        [
            "README.txt",
            "environment.json",
            "settings.json",
            "calendars.json",
            "sync-log.json",
            "alarms.json"
        ]
    );
    let file = |name: &str| -> anyhow::Result<String> {
        let e = entries
            .iter()
            .find(|e| e.name == name)
            .ok_or_else(|| anyhow!("{name} missing"))?;
        Ok(String::from_utf8(e.data.clone())?)
    };
    let json = |name: &str| -> anyhow::Result<Value> { Ok(serde_json::from_str(&file(name)?)?) };

    assert!(
        file("README.txt")?.ends_with(README),
        "README.txt is the doc"
    );

    let env = json("environment.json")?;
    assert_eq!(env["tolon_version"], "0.1.0-beta.1");
    assert_eq!(env["screen_count"], 1);
    assert_eq!(env["utc_offset"], "+02:00");
    assert_eq!(env["saved_at"], "2026-10-20T10:00:00+02:00");
    assert_eq!(env["arch"], std::env::consts::ARCH);
    assert!(env["webview"]
        .as_str()
        .is_some_and(|w| w.contains("WebKit")));

    let settings = json("settings.json")?;
    assert_eq!(settings["ignore"]["keyword_count"], 2);
    assert!(settings["ignore"].get("keywords").is_none());

    let calendars = json("calendars.json")?;
    let list = calendars.as_array().ok_or_else(|| anyhow!("not a list"))?;
    assert_eq!(list.len(), 3);
    let ical: Vec<&Value> = list.iter().filter(|c| c["kind"] == "ical").collect();
    assert!(ical.iter().all(|c| c["host"] == "calendar.example"));
    assert!(ical.iter().any(|c| c["sync_status"] == "ok"));
    assert!(ical
        .iter()
        .any(|c| c["last_error"] == serde_json::json!({ "kind": "http", "status": 404 })));
    let google = list
        .iter()
        .find(|c| c["kind"] == "google")
        .ok_or_else(|| anyhow!("no google calendar"))?;
    assert_eq!(google["host"], Value::Null);
    assert_eq!(google["last_error"]["kind"], "auth");
    assert!(list.iter().all(|c| c.get("name").is_none()));

    let log = json("sync-log.json")?;
    let details: Vec<&str> = log
        .as_array()
        .ok_or_else(|| anyhow!("not a list"))?
        .iter()
        .filter_map(|r| r["detail"].as_str())
        .collect();
    assert!(details.contains(&"auth") && details.contains(&"http:404"));
    assert!(details.contains(&"<redacted>"));

    let alarms = json("alarms.json")?;
    let first = &alarms[0];
    assert_eq!(first["meeting_start"], "2026-10-20T08:01:00Z");
    assert_eq!(first["state"], "firing");
    let alarm_ref = first["alarm"]
        .as_str()
        .ok_or_else(|| anyhow!("no ref"))?
        .to_owned();
    assert_eq!(alarm_ref.len(), 16);

    let log_name = names
        .iter()
        .find(|n| n.starts_with("logs/tolon.") && n.ends_with(".log"))
        .ok_or_else(|| anyhow!("no log file in {names:?}"))?;
    let lines = file(log_name)?;
    // Debug from Tolon's own code, with the alarm named as in alarms.json.
    assert!(lines.contains("DEBUG"), "{lines}");
    assert!(
        lines.contains("alarm planned") && lines.contains(&alarm_ref),
        "{lines}"
    );
    assert!(
        lines.contains("alarm transition") && lines.contains("to=\"firing\""),
        "{lines}"
    );
    assert!(
        lines.contains("calendar sync failed") && lines.contains("http:404"),
        "{lines}"
    );
    // Timestamps in UTC with their zone.
    assert!(
        lines.lines().all(|l| l
            .split_whitespace()
            .next()
            .is_some_and(|t| t.contains('T') && t.ends_with('Z'))),
        "{lines}"
    );
    Ok(())
}

#[test]
fn l072_the_default_name_and_the_target_path() -> TestResult {
    assert_eq!(
        file_name("0.1.0-beta.1", local()),
        "Tolon-diagnostics-0.1.0-beta.1-20261020-1000.zip"
    );
    let base = std::env::temp_dir();
    let named = base.join("x.ZIP");
    assert_eq!(target(&named.to_string_lossy())?, named);
    assert_eq!(
        target(&base.join("x").to_string_lossy())?,
        base.join("x.zip")
    );
    assert_eq!(target("relative.zip"), Err(AppError::DIAGNOSTICS_SAVE));
    assert_eq!(target(""), Err(AppError::DIAGNOSTICS_SAVE));
    Ok(())
}

#[test]
fn l072_a_failed_save_leaves_no_file() {
    let dir = std::env::temp_dir()
        .join("tolon-no-such-dir-l072")
        .join("deeper");
    let path = dir.join("d.zip");
    assert!(save(&path, b"x").is_err());
    assert!(!path.exists());
}

/// The doc (and so README.txt) names the same place to send the file as the About page.
#[test]
fn l072_the_doc_points_where_the_about_page_does() {
    assert!(README.contains(SUPPORT_URL));
    assert!(doc_url("0.1.0-beta.1").ends_with("/blob/v0.1.0-beta.1/docs/help/diagnostics.md"));
}

#[test]
fn l072_key_ref_is_stable_and_hides_the_key() {
    let key = "cal:uid-a@b.example@2026-10-20T08:01:00Z";
    assert_eq!(key_ref(key), key_ref(key));
    assert_ne!(key_ref(key), key_ref("cal:other@2026-10-20T08:01:00Z"));
    assert!(!key_ref(key).contains('@'));
}

/// A tracing call up to the `)` that closes it right after its message string.
fn up_to_message_end(call: &str) -> &str {
    let mut quote = None;
    for (i, c) in call.char_indices() {
        match c {
            '"' => quote = Some(i),
            ')' if quote.is_some() => return &call[..i],
            c if !c.is_whitespace() => quote = None,
            _ => {}
        }
    }
    call
}

/// Rule 4 for the log files: they keep Tolon's debug lines, so no tracing call in Tolon's crates
/// may name a title, description, attendee, token or full URL as a field.
#[test]
fn l072_no_log_line_names_personal_fields() -> TestResult {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut stack = vec![root.clone()];
    let mut checked = 0;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let p = entry?.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if p.extension().is_none_or(|e| e != "rs") || p.ends_with("tests.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&p)?;
            for (i, call) in text.split("tracing::").enumerate().skip(1) {
                if !["debug!", "info!", "warn!", "error!", "trace!"]
                    .iter()
                    .any(|m| call.starts_with(m))
                {
                    continue;
                }
                let args = up_to_message_end(call);
                // The message text may say "title bar"; only the fields and values count.
                let code: String = args.split('"').step_by(2).collect();
                for field in [
                    "title",
                    "description",
                    "attendee",
                    "summary",
                    "access_token",
                    "refresh_token",
                    "%url",
                    "?url",
                ] {
                    assert!(
                        !code.contains(field),
                        "{}: tracing call {i} names `{field}`: {args}",
                        p.display()
                    );
                }
                assert!(
                    !code.contains("url =") || code.contains("url = %redact_url("),
                    "{}: tracing call {i} logs a URL without redact_url: {args}",
                    p.display()
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 20, "the scan found the tracing calls ({checked})");
    Ok(())
}

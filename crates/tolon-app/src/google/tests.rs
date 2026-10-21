use std::collections::VecDeque;
use std::sync::Mutex;

use chrono::TimeZone;
use tolon_core::{EventStatus, EventTime};
use tolon_platform::secrets::{MemorySecrets, SecretError};
use tolon_store::Store;

use super::*;
use crate::pipeline::{list, CalendarDto};

type TestResult = anyhow::Result<()>;

const REFRESH: &str = "refresh-SECRET-1";
const REFRESH_2: &str = "refresh-SECRET-2";
const EMAIL: &str = "owner-SECRET@example.com";
const TEAM: &str = "team-SECRET@group.calendar.google.com";

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 22, 8, 0, 0)
        .single()
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

fn db() -> anyhow::Result<Db> {
    Ok(Mutex::new(Store::open_in_memory()?))
}

/// (refresh token, remote id, sync token, stored event uids) of one sync.
type SyncCall = (String, String, Option<String>, Vec<String>);

#[derive(Default)]
struct FakeGoogle {
    answers: Mutex<VecDeque<Result<Synced, GoogleError>>>,
    syncs: Mutex<Vec<SyncCall>>,
    revoked: Mutex<Vec<String>>,
}

impl FakeGoogle {
    fn new(answers: Vec<Result<Synced, GoogleError>>) -> Self {
        Self {
            answers: Mutex::new(answers.into()),
            ..Self::default()
        }
    }
}

impl GoogleApi for FakeGoogle {
    fn sync(
        &self,
        refresh: &str,
        remote_id: &str,
        sync_token: Option<&str>,
        stored: Vec<Event>,
        _calendar: &CalendarId,
    ) -> impl Future<Output = Result<Synced, GoogleError>> + Send {
        if let Ok(mut s) = self.syncs.lock() {
            s.push((
                refresh.to_owned(),
                remote_id.to_owned(),
                sync_token.map(str::to_owned),
                stored.into_iter().map(|e| e.uid).collect(),
            ));
        }
        let answer = self
            .answers
            .lock()
            .ok()
            .and_then(|mut a| a.pop_front())
            .unwrap_or(Err(GoogleError::Network));
        async move { answer }
    }

    fn revoke(&self, refresh: &str) -> impl Future<Output = Result<(), GoogleError>> + Send {
        if let Ok(mut r) = self.revoked.lock() {
            r.push(refresh.to_owned());
        }
        async { Ok(()) }
    }
}

/// A keychain that is locked or missing.
struct BrokenKeychain;

impl Secrets for BrokenKeychain {
    fn get(&self, _: &str) -> Result<Option<String>, SecretError> {
        Err(SecretError)
    }
    fn set(&self, _: &str, _: &str) -> Result<(), SecretError> {
        Err(SecretError)
    }
    fn delete(&self, _: &str) -> Result<(), SecretError> {
        Err(SecretError)
    }
}

fn sign_in(refresh: &str) -> SignIn {
    SignIn {
        account: EMAIL.into(),
        refresh_token: refresh.into(),
        calendars: vec![
            RemoteCalendar {
                id: EMAIL.into(),
                name: EMAIL.into(),
                primary: true,
            },
            RemoteCalendar {
                id: TEAM.into(),
                name: "Team".into(),
                primary: false,
            },
        ],
    }
}

fn event(uid: &str) -> Event {
    let at = now();
    Event {
        id: tolon_core::EventId(format!("x:{uid}")),
        calendar: CalendarId("x".into()),
        uid: uid.into(),
        title: format!("Meeting {uid}"),
        start: EventTime::Utc { at },
        end: EventTime::Utc { at },
        all_day: false,
        rrule: None,
        exdates: vec![],
        overrides: vec![],
        status: EventStatus::Confirmed,
        my_response: None,
        attendees_count: 2,
        location: None,
        description: None,
        conference: None,
        html_link: None,
        updated: at,
    }
}

fn synced(uids: &[&str], token: &str) -> Result<Synced, GoogleError> {
    Ok(Synced {
        events: uids.iter().map(|u| event(u)).collect(),
        sync_token: token.into(),
        skipped: 0,
    })
}

/// Signs in and adds both calendars of the account; returns their Tolon ids (primary first).
fn add_both(db: &Db, secrets: &dyn Secrets) -> anyhow::Result<Vec<String>> {
    let (_, pending, healed) = signed_in(db, secrets, sign_in(REFRESH))?;
    assert!(healed.is_empty());
    add(db, secrets, &pending, &[EMAIL.into(), TEAM.into()], now())
}

fn row(db: &Db, id: &str) -> anyhow::Result<CalendarDto> {
    list(db)?
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| anyhow::anyhow!("gone"))
}

#[test]
fn ac13_3_refresh_token_goes_to_the_keychain_never_to_sqlite() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("tolon.db");
    let secrets = MemorySecrets::default();
    {
        let db = Mutex::new(Store::open(&path)?);
        let ids = add_both(&db, &secrets)?;
        assert_eq!(ids.len(), 2);
        for id in &ids {
            assert_eq!(secrets.get(id)?.as_deref(), Some(REFRESH));
        }
        let team = row(&db, &ids[1])?;
        assert_eq!((team.kind.as_str(), team.name.as_str()), (KIND, "Team"));
        // The account is shown; the Google calendar id is not.
        assert_eq!(team.account.as_deref(), Some(EMAIL));
        assert_eq!(team.display_url, None);
    }
    // Every byte SQLite wrote, WAL included.
    for entry in std::fs::read_dir(dir.path())? {
        let bytes = std::fs::read(entry?.path())?;
        assert!(
            !String::from_utf8_lossy(&bytes).contains(REFRESH),
            "the refresh token reached the database files"
        );
    }
    Ok(())
}

#[test]
fn ac13_1_picker_marks_added_calendars_and_adds_each_once() -> TestResult {
    let db = db()?;
    let secrets = MemorySecrets::default();
    let (_, pending, _) = signed_in(&db, &secrets, sign_in(REFRESH))?;
    let ids = add(&db, &secrets, &pending, &[TEAM.into(), TEAM.into()], now())?;
    assert_eq!(ids.len(), 1);
    let (dto, pending, _) = signed_in(&db, &secrets, sign_in(REFRESH))?;
    let added: Vec<(&str, bool)> = dto
        .calendars
        .iter()
        .map(|c| (c.id.as_str(), c.added))
        .collect();
    assert_eq!(added, [(EMAIL, false), (TEAM, true)]);
    assert!(add(&db, &secrets, &pending, &[TEAM.into()], now())?.is_empty());
    // An id that was not offered is refused.
    assert!(add(&db, &secrets, &pending, &["other".into()], now()).is_err());
    Ok(())
}

#[tokio::test]
async fn ac13_4_sync_keeps_the_sync_token_and_sends_it_back_with_the_stored_events() -> TestResult {
    let db = db()?;
    let secrets = MemorySecrets::default();
    let ids = add_both(&db, &secrets)?;
    let google = FakeGoogle::new(vec![synced(&["a", "b"], "s1"), synced(&["a"], "s2")]);

    sync_one(&db, Some(&google), &secrets, &ids[1], now()).await?;
    let stored = lock(&db)
        .calendars()
        .get(&ids[1])?
        .ok_or(AppError::NOT_FOUND)?;
    assert_eq!(stored.etag.as_deref(), Some("s1"));
    assert_eq!(lock(&db).events().list_by_calendar(&ids[1])?.len(), 2);

    sync_one(&db, Some(&google), &secrets, &ids[1], now()).await?;
    assert_eq!(lock(&db).events().list_by_calendar(&ids[1])?.len(), 1);
    let calls = google.syncs.lock().map(|s| s.clone()).unwrap_or_default();
    assert_eq!(calls[0].0, REFRESH);
    assert_eq!(calls[0].1, TEAM);
    assert_eq!(calls[0].2, None);
    assert_eq!(calls[1].2.as_deref(), Some("s1"));
    assert_eq!(calls[1].3, ["a", "b"]);
    assert_eq!(row(&db, &ids[1])?.last_error, None);
    Ok(())
}

#[tokio::test]
async fn ac13_3_expired_sign_in_shows_auth_and_a_new_sign_in_heals_it() -> TestResult {
    let db = db()?;
    let secrets = MemorySecrets::default();
    let ids = add_both(&db, &secrets)?;
    let google = FakeGoogle::new(vec![Err(GoogleError::Auth), synced(&["a"], "s1")]);

    sync_one(&db, Some(&google), &secrets, &ids[0], now()).await?;
    let err = row(&db, &ids[0])?.last_error.ok_or(AppError::INTERNAL)?;
    assert_eq!((err.kind.as_str(), err.status), ("auth", None));

    // Signing in again stores the new token for every calendar of the account.
    let (_, _, healed) = signed_in(&db, &secrets, sign_in(REFRESH_2))?;
    let (mut healed, mut ids_sorted) = (healed, ids.clone());
    healed.sort();
    ids_sorted.sort();
    assert_eq!(healed, ids_sorted);
    assert_eq!(secrets.get(&ids[0])?.as_deref(), Some(REFRESH_2));
    sync_one(&db, Some(&google), &secrets, &ids[0], now()).await?;
    assert_eq!(row(&db, &ids[0])?.last_error, None);
    Ok(())
}

#[tokio::test]
async fn ac13_3_missing_token_broken_keychain_and_no_client_are_recorded() -> TestResult {
    let db = db()?;
    let secrets = MemorySecrets::default();
    let ids = add_both(&db, &secrets)?;
    let google = FakeGoogle::default();

    secrets.delete(&ids[0])?;
    sync_one(&db, Some(&google), &secrets, &ids[0], now()).await?;
    sync_one(&db, Some(&google), &BrokenKeychain, &ids[1], now()).await?;
    let kind = |id: &str| -> anyhow::Result<String> {
        Ok(row(&db, id)?.last_error.map(|e| e.kind).unwrap_or_default())
    };
    assert_eq!(kind(&ids[0])?, "auth");
    assert_eq!(kind(&ids[1])?, "keychain");
    sync_one(&db, None::<&FakeGoogle>, &secrets, &ids[1], now()).await?;
    assert_eq!(kind(&ids[1])?, "google");
    assert!(google.syncs.lock().map(|s| s.is_empty()).unwrap_or(false));

    // A keychain that refuses the token: nothing is added.
    let (_, pending, _) = signed_in(&db, &secrets, sign_in(REFRESH))?;
    let before = list(&db)?.len();
    lock(&db).calendars().remove(&ids[1])?;
    assert!(add(&db, &BrokenKeychain, &pending, &[TEAM.into()], now()).is_err());
    assert_eq!(list(&db)?.len(), before - 1);
    Ok(())
}

#[tokio::test]
async fn ac13_3_remove_deletes_the_token_and_revokes_with_the_last_calendar() -> TestResult {
    let db = db()?;
    let secrets = MemorySecrets::default();
    let ids = add_both(&db, &secrets)?;
    let google = FakeGoogle::default();

    remove(&db, Some(&google), &secrets, &ids[0]).await?;
    assert_eq!(secrets.get(&ids[0])?, None);
    assert!(google.revoked.lock().map(|r| r.is_empty()).unwrap_or(false));

    remove(&db, Some(&google), &secrets, &ids[1]).await?;
    assert_eq!(secrets.get(&ids[1])?, None);
    assert_eq!(
        google.revoked.lock().map(|r| r.clone()).unwrap_or_default(),
        [REFRESH]
    );
    assert!(list(&db)?.is_empty());
    Ok(())
}

#[tokio::test]
async fn ac13_3_a_sign_in_left_without_calendars_is_revoked() -> TestResult {
    let db = db()?;
    let secrets = MemorySecrets::default();
    let google = FakeGoogle::default();
    let (_, pending, _) = signed_in(&db, &secrets, sign_in(REFRESH))?;
    drop_pending(&db, Some(&google), pending).await?;
    assert_eq!(
        google.revoked.lock().map(|r| r.clone()).unwrap_or_default(),
        [REFRESH]
    );

    // With calendars of the account in Tolon, the grant is theirs: kept.
    let google = FakeGoogle::default();
    add_both(&db, &secrets)?;
    let (_, pending, _) = signed_in(&db, &secrets, sign_in(REFRESH))?;
    drop_pending(&db, Some(&google), pending).await?;
    assert!(google.revoked.lock().map(|r| r.is_empty()).unwrap_or(false));
    Ok(())
}

#[tokio::test]
async fn ac13_3_token_and_address_absent_from_logs_sync_log_and_errors() -> TestResult {
    let logs = crate::pipeline::tests::Sink::default();
    let _guard = logs.capture();
    let db = db()?;
    let secrets = MemorySecrets::default();
    let ids = add_both(&db, &secrets)?;
    let google = FakeGoogle::new(vec![Err(GoogleError::Auth), Err(GoogleError::Status(500))]);
    sync_one(&db, Some(&google), &secrets, &ids[0], now()).await?;
    sync_one(&db, Some(&google), &secrets, &ids[1], now()).await?;
    remove(&db, Some(&google), &BrokenKeychain, &ids[1]).await?;

    let out = logs.text();
    // Only this test reaches this callsite; "calendar sync failed" is shared with parallel tests
    // without a subscriber, whose cached interest can drop it from a per-thread capture.
    assert!(out.contains("token is still in the keychain"), "{out}");
    let rows = lock(&db).sync_log().list_recent(100)?;
    let text = format!(
        "{out} {:?} {:?}",
        rows.iter().map(|r| &r.detail).collect::<Vec<_>>(),
        list(&db)?.iter().map(|c| &c.last_error).collect::<Vec<_>>()
    );
    for secret in [REFRESH, "SECRET"] {
        assert!(!text.contains(secret), "{secret} leaked: {text}");
    }
    Ok(())
}

#[test]
fn sync_codes_match_the_ui_kinds() {
    assert_eq!(sync_code(&GoogleError::Auth), "auth");
    assert_eq!(sync_code(&GoogleError::Status(403)), "http:403");
    assert_eq!(sync_code(&GoogleError::Timeout), "timeout");
    assert_eq!(sync_code(&GoogleError::BadResponse), "google");
}

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

fn memory() -> SharedSecrets {
    Arc::new(MemorySecrets::default())
}

fn broken() -> SharedSecrets {
    Arc::new(BrokenKeychain)
}

/// A keychain that still has the token but cannot delete it.
struct StuckToken;

impl Secrets for StuckToken {
    fn get(&self, _: &str) -> Result<Option<String>, SecretError> {
        Ok(Some(REFRESH.into()))
    }
    fn set(&self, _: &str, _: &str) -> Result<(), SecretError> {
        Ok(())
    }
    fn delete(&self, _: &str) -> Result<(), SecretError> {
        Err(SecretError)
    }
}

fn stuck_token() -> SharedSecrets {
    Arc::new(StuckToken)
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
async fn add_both(db: &Db, secrets: &SharedSecrets) -> anyhow::Result<Vec<String>> {
    let (_, pending, healed) = signed_in(db, secrets, sign_in(REFRESH)).await?;
    assert!(healed.is_empty());
    add(db, secrets, &pending, &[EMAIL.into(), TEAM.into()], now()).await
}

fn row(db: &Db, id: &str) -> anyhow::Result<CalendarDto> {
    list(db)?
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| anyhow::anyhow!("gone"))
}

#[tokio::test]
async fn ac13_3_refresh_token_goes_to_the_keychain_never_to_sqlite() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("tolon.db");
    let secrets = memory();
    {
        let db = Mutex::new(Store::open(&path)?);
        let ids = add_both(&db, &secrets).await?;
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

#[tokio::test]
async fn ac13_1_picker_marks_added_calendars_and_adds_each_once() -> TestResult {
    let db = db()?;
    let secrets = memory();
    let (_, pending, _) = signed_in(&db, &secrets, sign_in(REFRESH)).await?;
    let ids = add(&db, &secrets, &pending, &[TEAM.into(), TEAM.into()], now()).await?;
    assert_eq!(ids.len(), 1);
    let (dto, pending, _) = signed_in(&db, &secrets, sign_in(REFRESH)).await?;
    let added: Vec<(&str, bool)> = dto
        .calendars
        .iter()
        .map(|c| (c.id.as_str(), c.added))
        .collect();
    assert_eq!(added, [(EMAIL, false), (TEAM, true)]);
    assert!(add(&db, &secrets, &pending, &[TEAM.into()], now())
        .await?
        .is_empty());
    // An id that was not offered is refused.
    assert!(add(&db, &secrets, &pending, &["other".into()], now())
        .await
        .is_err());
    Ok(())
}

#[tokio::test]
async fn ac13_4_sync_keeps_the_sync_token_and_sends_it_back_with_the_stored_events() -> TestResult {
    let db = db()?;
    let secrets = memory();
    let ids = add_both(&db, &secrets).await?;
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
    let secrets = memory();
    let ids = add_both(&db, &secrets).await?;
    let google = FakeGoogle::new(vec![Err(GoogleError::Auth), synced(&["a"], "s1")]);

    sync_one(&db, Some(&google), &secrets, &ids[0], now()).await?;
    let err = row(&db, &ids[0])?.last_error.ok_or(AppError::INTERNAL)?;
    assert_eq!((err.kind.as_str(), err.status), ("auth", None));

    // Signing in again stores the new token for every calendar of the account.
    let (_, _, healed) = signed_in(&db, &secrets, sign_in(REFRESH_2)).await?;
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
    let secrets = memory();
    let ids = add_both(&db, &secrets).await?;
    let google = FakeGoogle::default();

    secrets.delete(&ids[0])?;
    sync_one(&db, Some(&google), &secrets, &ids[0], now()).await?;
    sync_one(&db, Some(&google), &broken(), &ids[1], now()).await?;
    let kind = |id: &str| -> anyhow::Result<String> {
        Ok(row(&db, id)?.last_error.map(|e| e.kind).unwrap_or_default())
    };
    assert_eq!(kind(&ids[0])?, "auth");
    assert_eq!(kind(&ids[1])?, "keychain");
    sync_one(&db, None::<&FakeGoogle>, &secrets, &ids[1], now()).await?;
    assert_eq!(kind(&ids[1])?, "google");
    assert!(google.syncs.lock().map(|s| s.is_empty()).unwrap_or(false));

    // A keychain that refuses the token: nothing is added.
    let (_, pending, _) = signed_in(&db, &secrets, sign_in(REFRESH)).await?;
    let before = list(&db)?.len();
    lock(&db).calendars().remove(&ids[1])?;
    assert!(add(&db, &broken(), &pending, &[TEAM.into()], now())
        .await
        .is_err());
    assert_eq!(list(&db)?.len(), before - 1);
    Ok(())
}

#[tokio::test]
async fn ac13_3_remove_deletes_the_token_and_revokes_with_the_last_calendar() -> TestResult {
    let db = db()?;
    let secrets = memory();
    let ids = add_both(&db, &secrets).await?;
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
    let secrets = memory();
    let google = FakeGoogle::default();
    let (_, pending, _) = signed_in(&db, &secrets, sign_in(REFRESH)).await?;
    drop_pending(&db, Some(&google), pending).await?;
    assert_eq!(
        google.revoked.lock().map(|r| r.clone()).unwrap_or_default(),
        [REFRESH]
    );

    // With calendars of the account in Tolon, the grant is theirs: kept.
    let google = FakeGoogle::default();
    add_both(&db, &secrets).await?;
    let (_, pending, _) = signed_in(&db, &secrets, sign_in(REFRESH)).await?;
    drop_pending(&db, Some(&google), pending).await?;
    assert!(google.revoked.lock().map(|r| r.is_empty()).unwrap_or(false));
    Ok(())
}

#[tokio::test]
async fn ac13_3_token_and_address_absent_from_logs_sync_log_and_errors() -> TestResult {
    let logs = crate::pipeline::tests::Sink::default();
    let _guard = logs.capture();
    let db = db()?;
    let secrets = memory();
    let ids = add_both(&db, &secrets).await?;
    let google = FakeGoogle::new(vec![Err(GoogleError::Auth), Err(GoogleError::Status(500))]);
    sync_one(&db, Some(&google), &secrets, &ids[0], now()).await?;
    sync_one(&db, Some(&google), &secrets, &ids[1], now()).await?;
    remove(&db, Some(&google), &stuck_token(), &ids[1]).await?;

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

#[test]
fn debug_output_omits_the_address_and_calendar_titles() {
    let dto = AccountDto {
        account: EMAIL.into(),
        calendars: vec![RemoteCalendarDto {
            id: TEAM.into(),
            name: "Salary talks SECRET".into(),
            primary: true,
            added: false,
        }],
    };
    let text = format!("{dto:?} {:?}", dto.calendars[0]);
    assert!(!text.contains("SECRET"), "{text}");
    assert!(text.contains("AccountDto") && text.contains("RemoteCalendarDto"));
}

/// A keychain that takes its time and notes, on entry, whether the database lock was free.
struct SlowKeychain {
    db: Arc<Db>,
    db_free: Mutex<Vec<(&'static str, bool)>>,
}

impl SlowKeychain {
    fn visit(&self, call: &'static str) {
        let free = self.db.try_lock().is_ok();
        if let Ok(mut seen) = self.db_free.lock() {
            seen.push((call, free));
        }
        std::thread::sleep(Duration::from_millis(150));
    }

    fn calls(&self) -> Vec<(&'static str, bool)> {
        self.db_free.lock().map(|c| c.clone()).unwrap_or_default()
    }
}

impl Secrets for SlowKeychain {
    fn get(&self, _: &str) -> Result<Option<String>, SecretError> {
        self.visit("get");
        Ok(Some(REFRESH.into()))
    }
    fn set(&self, _: &str, _: &str) -> Result<(), SecretError> {
        self.visit("set");
        Ok(())
    }
    fn delete(&self, _: &str) -> Result<(), SecretError> {
        self.visit("delete");
        Ok(())
    }
}

/// Runs `work` and counts how often a 10 ms timer fired meanwhile on this one-thread runtime.
async fn with_ticks<T>(work: impl Future<Output = T>) -> (T, usize) {
    let mut ticks = 0;
    tokio::pin!(work);
    loop {
        tokio::select! {
            out = &mut work => return (out, ticks),
            () = tokio::time::sleep(Duration::from_millis(10)) => ticks += 1,
        }
    }
}

#[tokio::test]
async fn ac13_3_a_slow_keychain_blocks_neither_the_database_nor_the_async_worker() -> TestResult {
    let db = Arc::new(db()?);
    let slow = Arc::new(SlowKeychain {
        db: Arc::clone(&db),
        db_free: Mutex::default(),
    });
    let secrets: SharedSecrets = slow.clone();
    let google = FakeGoogle::new(vec![synced(&["a"], "s1")]);

    // Two calendars: two keychain writes of 150 ms each; the timer keeps firing meanwhile.
    let (_, pending, _) = signed_in(&db, &secrets, sign_in(REFRESH)).await?;
    let (ids, ticks) = with_ticks(add(
        &db,
        &secrets,
        &pending,
        &[EMAIL.into(), TEAM.into()],
        now(),
    ))
    .await;
    let ids = ids?;
    assert_eq!(ids.len(), 2);
    assert!(ticks >= 3, "the worker was blocked: {ticks} ticks");

    let (synced_one, ticks) =
        with_ticks(sync_one(&db, Some(&google), &secrets, &ids[0], now())).await;
    synced_one?;
    assert!(ticks >= 3, "the worker was blocked: {ticks} ticks");
    let (removed, ticks) = with_ticks(remove(&db, Some(&google), &secrets, &ids[0])).await;
    removed?;
    assert!(ticks >= 3, "the worker was blocked: {ticks} ticks");

    let calls = slow.calls();
    assert_eq!(
        calls.iter().map(|c| c.0).collect::<Vec<_>>(),
        ["set", "set", "get", "get", "delete"]
    );
    assert!(
        calls.iter().all(|c| c.1),
        "the database lock was held during a keychain call: {calls:?}"
    );
    Ok(())
}

fn command_code(e: &anyhow::Error) -> &'static str {
    crate::errors::command_error(e).code
}

#[tokio::test]
async fn ac13_3_when_a_token_cannot_be_stored_nothing_is_added_and_stored_tokens_go() -> TestResult
{
    /// The second write fails.
    struct SecondFails(Mutex<Vec<String>>);
    impl Secrets for SecondFails {
        fn get(&self, _: &str) -> Result<Option<String>, SecretError> {
            Ok(None)
        }
        fn set(&self, key: &str, _: &str) -> Result<(), SecretError> {
            let mut keys = self.0.lock().map_err(|_| SecretError)?;
            if keys.len() == 1 {
                return Err(SecretError);
            }
            keys.push(key.to_owned());
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<(), SecretError> {
            self.0.lock().map_err(|_| SecretError)?.retain(|k| k != key);
            Ok(())
        }
    }
    let db = db()?;
    let keys = Arc::new(SecondFails(Mutex::default()));
    let secrets: SharedSecrets = keys.clone();
    let (_, pending, _) = signed_in(&db, &secrets, sign_in(REFRESH)).await?;
    let added = add(&db, &secrets, &pending, &[EMAIL.into(), TEAM.into()], now()).await;
    assert_eq!(added.err().map(|e| command_code(&e)), Some("keychain"));
    assert!(list(&db)?.is_empty());
    assert!(keys.0.lock().map(|k| k.is_empty()).unwrap_or(false));
    Ok(())
}

#[tokio::test]
async fn remove_warns_with_the_calendar_id_only_when_the_keychain_cannot_be_read() -> TestResult {
    let logs = crate::pipeline::tests::Sink::default();
    let _guard = logs.capture();
    let db = db()?;
    let secrets = memory();
    let ids = add_both(&db, &secrets).await?;
    let google = FakeGoogle::default();

    // Not the account's last calendar: the read fails, the calendar is removed all the same.
    remove(&db, Some(&google), &broken(), &ids[0]).await?;
    assert_eq!(list(&db)?.len(), 1);

    let out = logs.text();
    let warn = out
        .lines()
        .find(|l| l.contains("could not be read from the keychain"))
        .ok_or_else(|| anyhow::anyhow!("no warning: {out}"))?;
    assert!(warn.contains("WARN"), "{warn}");
    assert!(warn.contains(&ids[0]), "{warn}");
    for secret in [REFRESH, "SECRET", TEAM, EMAIL] {
        assert!(!out.contains(secret), "{secret} leaked: {out}");
    }
    Ok(())
}

/// A Google whose revocation never answers.
struct Silent;

impl GoogleApi for Silent {
    fn sync(
        &self,
        _: &str,
        _: &str,
        _: Option<&str>,
        _: Vec<Event>,
        _: &CalendarId,
    ) -> impl Future<Output = Result<Synced, GoogleError>> + Send {
        std::future::ready(Err(GoogleError::Network))
    }

    fn revoke(&self, _: &str) -> impl Future<Output = Result<(), GoogleError>> + Send {
        std::future::pending()
    }
}

fn unpicked() -> Pending {
    Pending {
        account: EMAIL.into(),
        refresh_token: REFRESH.into(),
        calendars: vec![],
    }
}

fn revoked(google: &FakeGoogle) -> Vec<String> {
    google.revoked.lock().map(|r| r.clone()).unwrap_or_default()
}

#[test]
fn ac13_3_quitting_with_a_sign_in_nobody_picked_from_revokes_it_and_drops_the_token() -> TestResult
{
    let db = db()?;
    let google = FakeGoogle::default();
    let slot: Slot<Pending> = Mutex::new(Some(unpicked()));
    drop_pending_on_exit(&db, Some(&google), &slot);
    assert_eq!(revoked(&google), [REFRESH]);
    assert!(take(&slot).is_none(), "the token is gone from memory");
    // Nothing pending: nothing happens.
    drop_pending_on_exit(&db, Some(&google), &slot);
    assert_eq!(revoked(&google).len(), 1);

    // The account has calendars in Tolon: the grant is theirs, only the pending copy goes.
    let secrets = memory();
    let (_, pending, _) =
        tauri::async_runtime::block_on(signed_in(&db, &secrets, sign_in(REFRESH)))?;
    tauri::async_runtime::block_on(add(&db, &secrets, &pending, &[EMAIL.into()], now()))?;
    let google = FakeGoogle::default();
    let slot: Slot<Pending> = Mutex::new(Some(unpicked()));
    drop_pending_on_exit(&db, Some(&google), &slot);
    assert!(revoked(&google).is_empty());
    assert!(take(&slot).is_none());
    Ok(())
}

#[tokio::test]
async fn ac13_3_a_revocation_that_never_answers_is_given_up_on() -> TestResult {
    let db = db()?;
    let started = std::time::Instant::now();
    drop_pending_within(&db, Some(&Silent), unpicked(), Duration::from_millis(50)).await?;
    assert!(started.elapsed() < Duration::from_secs(5));
    // What the app really waits on a quit or a cancel is short.
    assert!(REVOKE_WAIT <= Duration::from_secs(5));
    Ok(())
}

#[tokio::test]
async fn ac13_1_cancel_ends_a_waiting_sign_in_and_drops_a_finished_one() -> TestResult {
    let db = db()?;
    let google = FakeGoogle::default();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let waiting: Slot<tokio::sync::oneshot::Sender<()>> = Mutex::new(Some(tx));
    let pending: Slot<Pending> = Mutex::new(Some(unpicked()));

    // The sign-in is still waiting for the browser; the select! in `until_cancelled` ends it.
    let silent = std::future::pending::<Result<(), AppError>>();
    let (ended, cancelled) = tokio::join!(until_cancelled(silent, rx), async {
        cancel(&db, Some(&google), &waiting, &pending).await
    });
    cancelled?;
    assert_eq!(ended, Err(AppError::GOOGLE_CANCELLED));
    assert_eq!(revoked(&google), [REFRESH]);
    assert!(take(&waiting).is_none() && take(&pending).is_none());

    // Cancel with nothing open does nothing.
    cancel(&db, Some(&google), &waiting, &pending).await?;
    assert_eq!(revoked(&google).len(), 1);
    Ok(())
}

#[tokio::test]
async fn ac13_1_a_sign_in_that_finishes_first_wins_and_a_replaced_one_ends() {
    let (_keep, rx) = tokio::sync::oneshot::channel::<()>();
    assert_eq!(until_cancelled(async { Ok(7) }, rx).await, Ok(7));
    // Its sender dropped (a newer sign-in replaced it): the old one ends as cancelled.
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    drop(tx);
    let silent = std::future::pending::<Result<(), AppError>>();
    assert_eq!(
        until_cancelled(silent, rx).await,
        Err(AppError::GOOGLE_CANCELLED)
    );
}

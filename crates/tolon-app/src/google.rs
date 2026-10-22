//! Google calendars (F13) in plain functions, as `pipeline` is for feeds. No Tauri in here.
//!
//! A Google calendar row keeps the Google calendar id in `url`, the `events.list` sync token in
//! `etag` and the account's address in `account`. Its refresh token is in the keychain under the
//! calendar id, never in SQLite (AC13.3). Logs name the calendar id only: never a token, the
//! account or the Google calendar id (which can be an address).
//!
//! The keychain can block (a locked login keychain asks the person for a password), so every call
//! to it runs on a blocking thread, and never while the database lock is held.

use std::fmt;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tolon_core::{CalendarId, Event};
use tolon_platform::secrets::{SecretError, Secrets};
use tolon_store::{Calendar, CalendarsRepo, NewGoogleCalendar};
use tolon_sync::google::{GoogleClient, GoogleConfig, GoogleError, RemoteCalendar, SignIn, Synced};

use crate::errors::AppError;
use crate::pipeline::{self, decode_events, feed_events, lock, Db};

pub const KIND: &str = "google";

/// The keychain, shared with the blocking threads that call it.
pub type SharedSecrets = Arc<dyn Secrets>;

/// How long an abandoned sign-in's grant gets to be revoked before Tolon goes on (or quits).
const REVOKE_WAIT: Duration = Duration::from_secs(3);

async fn blocking<R: Send + 'static>(
    secrets: &SharedSecrets,
    call: impl FnOnce(&dyn Secrets) -> Result<R, SecretError> + Send + 'static,
) -> Result<R, SecretError> {
    let secrets = Arc::clone(secrets);
    tauri::async_runtime::spawn_blocking(move || call(&*secrets))
        .await
        .map_err(|_| SecretError)?
}

async fn keychain_get(secrets: &SharedSecrets, key: &str) -> Result<Option<String>, SecretError> {
    let key = key.to_owned();
    blocking(secrets, move |s| s.get(&key)).await
}

async fn keychain_set(secrets: &SharedSecrets, key: &str, value: &str) -> Result<(), SecretError> {
    let (key, value) = (key.to_owned(), value.to_owned());
    blocking(secrets, move |s| s.set(&key, &value)).await
}

async fn keychain_delete(secrets: &SharedSecrets, key: &str) -> Result<(), SecretError> {
    let key = key.to_owned();
    blocking(secrets, move |s| s.delete(&key)).await
}

/// A one-value slot in the app state, taken or replaced as a whole.
pub type Slot<T> = Mutex<Option<T>>;

pub fn take<T>(slot: &Slot<T>) -> Option<T> {
    slot.lock().unwrap_or_else(PoisonError::into_inner).take()
}

pub fn put<T>(slot: &Slot<T>, value: T) {
    *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(value);
}

/// The OAuth client: `TOLON_GOOGLE_CLIENT_ID` and, for a Google "Desktop app" client,
/// `TOLON_GOOGLE_CLIENT_SECRET`, from the environment at run time or else baked in at build time.
/// No client ID, no Google sign-in. Google's installed-app guidance treats a desktop client's
/// secret as not confidential, so it is configuration like the ID (and never logged).
pub fn config() -> Option<GoogleConfig> {
    fn var(name: &str, built: Option<&'static str>) -> Option<String> {
        std::env::var(name)
            .ok()
            .or(built.map(str::to_owned))
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    }
    let id = var(
        "TOLON_GOOGLE_CLIENT_ID",
        option_env!("TOLON_GOOGLE_CLIENT_ID"),
    )?;
    let secret = var(
        "TOLON_GOOGLE_CLIENT_SECRET",
        option_env!("TOLON_GOOGLE_CLIENT_SECRET"),
    );
    Some(GoogleConfig::new(id, secret))
}

/// What the pipeline needs from Google. Production is `GoogleClient`; tests pass a fake.
pub trait GoogleApi {
    fn sync(
        &self,
        refresh: &str,
        remote_id: &str,
        sync_token: Option<&str>,
        stored: Vec<Event>,
        calendar: &CalendarId,
    ) -> impl Future<Output = Result<Synced, GoogleError>> + Send;
    fn revoke(&self, refresh: &str) -> impl Future<Output = Result<(), GoogleError>> + Send;
}

impl GoogleApi for GoogleClient {
    fn sync(
        &self,
        refresh: &str,
        remote_id: &str,
        sync_token: Option<&str>,
        stored: Vec<Event>,
        calendar: &CalendarId,
    ) -> impl Future<Output = Result<Synced, GoogleError>> + Send {
        GoogleClient::sync(self, refresh, remote_id, sync_token, stored, calendar)
    }

    fn revoke(&self, refresh: &str) -> impl Future<Output = Result<(), GoogleError>> + Send {
        GoogleClient::revoke(self, refresh)
    }
}

/// The stored code for a failed Google sync (`calendars.last_error`, `sync_log.detail`). `auth`
/// asks the person to sign in again; `keychain` says the token could not be read.
pub fn sync_code(e: &GoogleError) -> String {
    match e {
        GoogleError::Auth => "auth".to_owned(),
        GoogleError::Status(status) => format!("http:{status}"),
        GoogleError::Timeout => "timeout".to_owned(),
        GoogleError::Connect => "connect".to_owned(),
        GoogleError::Network => "network".to_owned(),
        GoogleError::Denied | GoogleError::BadResponse | GoogleError::Browser => {
            "google".to_owned()
        }
    }
}

/// A sign-in the person has not picked calendars from yet. Memory only.
pub struct Pending {
    pub account: String,
    pub refresh_token: String,
    pub calendars: Vec<RemoteCalendar>,
}

#[derive(Serialize)]
pub struct RemoteCalendarDto {
    /// Google's calendar id, sent back by `calendars_add_google`.
    pub id: String,
    pub name: String,
    pub primary: bool,
    /// Already in Tolon.
    pub added: bool,
}

/// The id can be an address and the name a title: neither is printed.
impl fmt::Debug for RemoteCalendarDto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteCalendarDto")
            .field("primary", &self.primary)
            .field("added", &self.added)
            .finish_non_exhaustive()
    }
}

/// The signed-in account and its calendars, for the picker. No token.
#[derive(Serialize)]
pub struct AccountDto {
    pub account: String,
    pub calendars: Vec<RemoteCalendarDto>,
}

/// The address is not printed.
impl fmt::Debug for AccountDto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccountDto")
            .field("calendars", &self.calendars.len())
            .finish_non_exhaustive()
    }
}

fn of_account<'a>(cals: &'a [Calendar], account: &'a str) -> impl Iterator<Item = &'a Calendar> {
    cals.iter()
        .filter(move |c| c.kind == KIND && c.account.as_deref() == Some(account))
}

/// After a sign-in: calendars of this account already in Tolon get the new refresh token (this
/// is how "sign in again" heals them), and the account's calendars become the pending pick.
/// Returns the picker's view and the ids of the healed calendars, to sync at once.
pub async fn signed_in(
    db: &Db,
    secrets: &SharedSecrets,
    signed: SignIn,
) -> anyhow::Result<(AccountDto, Pending, Vec<String>)> {
    let cals = lock(db).calendars().list()?;
    let mut healed = Vec::new();
    for c in of_account(&cals, &signed.account) {
        keychain_set(secrets, &c.id, &signed.refresh_token)
            .await
            .map_err(|_| AppError::KEYCHAIN)?;
        healed.push(c.id.clone());
    }
    let dto = AccountDto {
        account: signed.account.clone(),
        calendars: signed
            .calendars
            .iter()
            .map(|r| RemoteCalendarDto {
                id: r.id.clone(),
                name: r.name.clone(),
                primary: r.primary,
                added: of_account(&cals, &signed.account)
                    .any(|c| c.url.as_deref() == Some(r.id.as_str())),
            })
            .collect(),
    };
    let pending = Pending {
        account: signed.account,
        refresh_token: signed.refresh_token,
        calendars: signed.calendars,
    };
    Ok((dto, pending, healed))
}

/// Adds the picked calendars of `pending` (those not added yet) and keeps the refresh token in
/// the keychain for each. The tokens are stored first, with no database lock held, under ids made
/// up front; the rows follow in one transaction. When a token cannot be stored, or the rows cannot
/// be written, nothing is added and the tokens already stored are deleted again.
pub async fn add(
    db: &Db,
    secrets: &SharedSecrets,
    pending: &Pending,
    ids: &[String],
    now: DateTime<Utc>,
) -> anyhow::Result<Vec<String>> {
    let mut planned: Vec<(String, &RemoteCalendar)> = Vec::new();
    {
        let existing = lock(db).calendars().list()?;
        for id in ids {
            let remote = pending
                .calendars
                .iter()
                .find(|r| &r.id == id)
                .ok_or(AppError::NOT_FOUND)?;
            if planned.iter().any(|(_, r)| r.id == *id)
                || of_account(&existing, &pending.account)
                    .any(|c| c.url.as_deref() == Some(id.as_str()))
            {
                continue;
            }
            planned.push((CalendarsRepo::new_id(), remote));
        }
    }
    let mut stored: Vec<&str> = Vec::new();
    for (id, _) in &planned {
        if keychain_set(secrets, id, &pending.refresh_token)
            .await
            .is_err()
        {
            forget_tokens(secrets, &stored).await;
            return Err(AppError::KEYCHAIN.into());
        }
        stored.push(id);
    }
    let at = now.to_rfc3339();
    let rows: Vec<_> = planned
        .iter()
        .map(|(id, remote)| NewGoogleCalendar {
            id,
            name: &remote.name,
            remote_id: &remote.id,
        })
        .collect();
    let written = lock(db)
        .calendars()
        .insert_google_many(&rows, &pending.account, &at);
    if let Err(e) = written {
        forget_tokens(secrets, &stored).await;
        return Err(e.into());
    }
    Ok(planned.into_iter().map(|(id, _)| id).collect())
}

/// Takes tokens out of the keychain again after a failed add. Best effort.
async fn forget_tokens(secrets: &SharedSecrets, ids: &[&str]) {
    for id in ids {
        if keychain_delete(secrets, id).await.is_err() {
            tracing::warn!(calendar = %id, "a token that was not used is still in the keychain");
        }
    }
}

/// Fetches the changes since the stored sync token and writes them, as `pipeline::sync_one` does
/// for a feed. No client configured, a missing token or a keychain failure is recorded as the
/// calendar's error.
pub async fn sync_one(
    db: &Db,
    google: Option<&impl GoogleApi>,
    secrets: &SharedSecrets,
    id: &str,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let (cal, stored) = {
        let store = lock(db);
        let cal = store.calendars().get(id)?.ok_or(AppError::NOT_FOUND)?;
        let stored = decode_events(store.events().list_by_calendar(id)?);
        (cal, stored)
    };
    let result = match google {
        None => Err("google".to_owned()),
        Some(api) => match keychain_get(secrets, id).await {
            Err(_) => Err("keychain".to_owned()),
            Ok(None) => Err("auth".to_owned()),
            Ok(Some(refresh)) => {
                let remote = cal.url.as_deref().ok_or(AppError::NO_URL)?;
                api.sync(
                    &refresh,
                    remote,
                    cal.etag.as_deref(),
                    stored,
                    &CalendarId(cal.id.clone()),
                )
                .await
                .map_err(|e| sync_code(&e))
            }
        },
    };
    apply(db, id, result, now)
}

/// Every enabled Google calendar, one after the other; the first store error is returned at the end.
pub async fn sync_all(
    db: &Db,
    google: Option<&impl GoogleApi>,
    secrets: &SharedSecrets,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let mut first_err = None;
    let cals = lock(db).calendars().list()?;
    for cal in cals.into_iter().filter(|c| c.enabled && c.kind == KIND) {
        if let Err(e) = sync_one(db, google, secrets, &cal.id, now).await {
            tracing::warn!(calendar = %cal.id, error = %e, "calendar sync could not be saved");
            first_err.get_or_insert(e);
        }
    }
    first_err.map_or(Ok(()), Err)
}

fn apply(
    db: &Db,
    id: &str,
    result: Result<Synced, String>,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let at = now.to_rfc3339();
    let store = lock(db);
    // Removed while the sync was in flight: nothing to write.
    if store.calendars().get(id)?.is_none() {
        return Ok(());
    }
    match result {
        Ok(synced) => {
            let feed = feed_events(synced.events)?;
            store.events().replace_for_calendar(id, &feed, &at)?;
            // The sync token goes where a feed keeps its etag.
            store
                .calendars()
                .record_success(id, Some(&synced.sync_token), &at)?;
            if synced.skipped > 0 {
                let detail = format!("skipped:{}", synced.skipped);
                store
                    .sync_log()
                    .append(Some(id), &at, "ok", Some(&detail))?;
            }
        }
        Err(code) => {
            store.calendars().record_failure(id, &code)?;
            store
                .sync_log()
                .append(Some(id), &at, "error", Some(&code))?;
            tracing::warn!(calendar = %id, code = %code, "calendar sync failed");
        }
    }
    Ok(())
}

/// Removes a calendar. For a Google one, its keychain entry goes too, and when it was the
/// account's last calendar in Tolon the grant is revoked at Google (best effort).
pub async fn remove(
    db: &Db,
    google: Option<&impl GoogleApi>,
    secrets: &SharedSecrets,
    id: &str,
) -> anyhow::Result<()> {
    let cal = lock(db).calendars().get(id)?;
    pipeline::remove(db, id)?;
    let others = lock(db).calendars().list()?;
    let Some(cal) = cal.filter(|c| c.kind == KIND) else {
        return Ok(());
    };
    // Without the token there is nothing to revoke; the calendar is gone either way.
    let token = keychain_get(secrets, id).await.unwrap_or_else(|_| {
        tracing::warn!(calendar = %id, "removed calendar's token could not be read from the keychain");
        None
    });
    if keychain_delete(secrets, id).await.is_err() {
        tracing::warn!(calendar = %id, "removed calendar's token is still in the keychain");
    }
    let last = cal
        .account
        .as_deref()
        .is_none_or(|a| of_account(&others, a).next().is_none());
    if let (true, Some(api), Some(token)) = (last, google, token) {
        // Failure is logged by the client; the calendar is gone either way.
        let _ = api.revoke(&token).await;
    }
    Ok(())
}

/// The person left the picker without adding anything (or Tolon quits with one open): revoke the
/// new grant unless calendars of the account already use it. The token is dropped with `pending`.
pub async fn drop_pending(
    db: &Db,
    google: Option<&impl GoogleApi>,
    pending: Pending,
) -> anyhow::Result<()> {
    drop_pending_within(db, google, pending, REVOKE_WAIT).await
}

/// Best effort: a revocation that takes longer than `wait` is given up on (the grant stays until
/// the person revokes it in their Google account).
async fn drop_pending_within(
    db: &Db,
    google: Option<&impl GoogleApi>,
    pending: Pending,
    wait: Duration,
) -> anyhow::Result<()> {
    let cals = lock(db).calendars().list()?;
    if of_account(&cals, &pending.account).next().is_none() {
        if let Some(api) = google {
            if tokio::time::timeout(wait, api.revoke(&pending.refresh_token))
                .await
                .is_err()
            {
                tracing::warn!("revoking an abandoned sign-in timed out");
            }
        }
    }
    Ok(())
}

/// Tolon is quitting with a sign-in nobody picked calendars from: revoke it now, as on cancel.
/// It blocks, and runs on a thread of its own: `block_on` panics where a runtime already runs on
/// the calling thread, and the updater's hook reaches this from an async command (Windows).
pub fn drop_pending_on_exit(
    db: &Db,
    google: Option<&(impl GoogleApi + Sync)>,
    pending: &Slot<Pending>,
) {
    let Some(pending) = take(pending) else {
        return;
    };
    let done = std::thread::scope(|scope| {
        scope
            .spawn(|| tauri::async_runtime::block_on(drop_pending(db, google, pending)))
            .join()
    });
    match done {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "an abandoned sign-in could not be dropped at quit")
        }
        Err(_) => tracing::warn!("dropping an abandoned sign-in at quit stopped early"),
    }
}

/// What a sign-in compares to know a cancel came while it was finishing: the cancel count when
/// it began.
pub fn cancels_so_far(epoch: &AtomicU64) -> u64 {
    epoch.load(Ordering::SeqCst)
}

/// The sign-in that began at cancel count `began` has finished with `pending`. It becomes the
/// pending pick, unless a cancel came meanwhile: then its grant is revoked at once (as cancel
/// would have) and the sign-in ends as cancelled. The check and the put share the slot's lock, and
/// `cancel` counts before it takes the slot, so a cancel never slips between them.
pub async fn keep_pending(
    db: &Db,
    google: Option<&impl GoogleApi>,
    slot: &Slot<Pending>,
    epoch: &AtomicU64,
    began: u64,
    pending: Pending,
) -> Result<(), AppError> {
    let account = pending.account.clone();
    let (earlier, rejected) = {
        let mut slot = slot.lock().unwrap_or_else(PoisonError::into_inner);
        if cancels_so_far(epoch) == began {
            (slot.replace(pending), None)
        } else {
            (None, Some(pending))
        }
    };
    // Revoking a token ends the account's whole grant, so an earlier unpicked sign-in of the same
    // account is only forgotten: revoking it would end the one that just finished.
    if let Some(earlier) = earlier.filter(|e| e.account != account) {
        drop_pending(db, google, earlier)
            .await
            .map_err(|_| AppError::GOOGLE_SIGN_IN)?;
    }
    if let Some(rejected) = rejected {
        // Best effort: the sign-in is over either way.
        let _ = drop_pending(db, google, rejected).await;
        return Err(AppError::GOOGLE_CANCELLED);
    }
    Ok(())
}

/// Ends the sign-in waiting for the browser (its receiver sees the sender go) and drops a finished
/// one nobody picked calendars from. The count goes up first: a sign-in that is finishing sees it
/// in `keep_pending` and revokes its own grant.
pub async fn cancel(
    db: &Db,
    google: Option<&impl GoogleApi>,
    waiting: &Slot<tokio::sync::oneshot::Sender<()>>,
    pending: &Slot<Pending>,
    epoch: &AtomicU64,
) -> anyhow::Result<()> {
    epoch.fetch_add(1, Ordering::SeqCst);
    if let Some(cancel) = take(waiting) {
        let _ = cancel.send(());
    }
    match take(pending) {
        Some(pending) => drop_pending(db, google, pending).await,
        None => Ok(()),
    }
}

/// Runs the sign-in until it ends or `cancelled` fires or its sender goes. Dropping the sign-in
/// closes its loopback port.
pub async fn until_cancelled<T>(
    sign_in: impl Future<Output = Result<T, AppError>>,
    cancelled: tokio::sync::oneshot::Receiver<()>,
) -> Result<T, AppError> {
    tokio::select! {
        r = sign_in => r,
        _ = cancelled => Err(AppError::GOOGLE_CANCELLED),
    }
}

#[cfg(test)]
mod tests;

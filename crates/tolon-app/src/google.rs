//! Google calendars (F13) in plain functions, as `pipeline` is for feeds. No Tauri in here.
//!
//! A Google calendar row keeps the Google calendar id in `url`, the `events.list` sync token in
//! `etag` and the account's address in `account`. Its refresh token is in the keychain under the
//! calendar id, never in SQLite (AC13.3). Logs name the calendar id only: never a token, the
//! account or the Google calendar id (which can be an address).

use std::future::Future;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tolon_core::{CalendarId, Event};
use tolon_platform::secrets::Secrets;
use tolon_store::Calendar;
use tolon_sync::google::{GoogleClient, GoogleConfig, GoogleError, RemoteCalendar, SignIn, Synced};

use crate::errors::AppError;
use crate::pipeline::{self, decode_events, feed_events, lock, Db};

pub const KIND: &str = "google";

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

#[derive(Debug, Serialize)]
pub struct RemoteCalendarDto {
    /// Google's calendar id, sent back by `calendars_add_google`.
    pub id: String,
    pub name: String,
    pub primary: bool,
    /// Already in Tolon.
    pub added: bool,
}

/// The signed-in account and its calendars, for the picker. No token.
#[derive(Debug, Serialize)]
pub struct AccountDto {
    pub account: String,
    pub calendars: Vec<RemoteCalendarDto>,
}

fn of_account<'a>(cals: &'a [Calendar], account: &'a str) -> impl Iterator<Item = &'a Calendar> {
    cals.iter()
        .filter(move |c| c.kind == KIND && c.account.as_deref() == Some(account))
}

/// After a sign-in: calendars of this account already in Tolon get the new refresh token (this
/// is how "sign in again" heals them), and the account's calendars become the pending pick.
/// Returns the picker's view and the ids of the healed calendars, to sync at once.
pub fn signed_in(
    db: &Db,
    secrets: &dyn Secrets,
    signed: SignIn,
) -> anyhow::Result<(AccountDto, Pending, Vec<String>)> {
    let cals = lock(db).calendars().list()?;
    let mut healed = Vec::new();
    for c in of_account(&cals, &signed.account) {
        secrets
            .set(&c.id, &signed.refresh_token)
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
/// the keychain for each. A calendar whose token cannot be stored is taken out again.
pub fn add(
    db: &Db,
    secrets: &dyn Secrets,
    pending: &Pending,
    ids: &[String],
    now: DateTime<Utc>,
) -> anyhow::Result<Vec<String>> {
    let store = lock(db);
    let existing = store.calendars().list()?;
    let mut added = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for id in ids {
        let remote = pending
            .calendars
            .iter()
            .find(|r| &r.id == id)
            .ok_or(AppError::NOT_FOUND)?;
        if seen.contains(&id.as_str())
            || of_account(&existing, &pending.account)
                .any(|c| c.url.as_deref() == Some(id.as_str()))
        {
            continue;
        }
        seen.push(id);
        let cal = store.calendars().insert(
            KIND,
            &remote.name,
            Some(&remote.id),
            true,
            &now.to_rfc3339(),
        )?;
        store.calendars().set_account(&cal.id, &pending.account)?;
        if secrets.set(&cal.id, &pending.refresh_token).is_err() {
            store.calendars().remove(&cal.id)?;
            return Err(AppError::KEYCHAIN.into());
        }
        added.push(cal.id);
    }
    Ok(added)
}

/// Fetches the changes since the stored sync token and writes them, as `pipeline::sync_one` does
/// for a feed. No client configured, a missing token or a keychain failure is recorded as the
/// calendar's error.
pub async fn sync_one(
    db: &Db,
    google: Option<&impl GoogleApi>,
    secrets: &dyn Secrets,
    id: &str,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let (cal, stored) = {
        let store = lock(db);
        let cal = store.calendars().get(id)?.ok_or(AppError::NOT_FOUND)?;
        let stored = decode_events(store.events().list_by_calendar(id)?);
        (cal, stored)
    };
    let result = match (google, secrets.get(id)) {
        (None, _) => Err("google".to_owned()),
        (_, Err(_)) => Err("keychain".to_owned()),
        (_, Ok(None)) => Err("auth".to_owned()),
        (Some(api), Ok(Some(refresh))) => {
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
    };
    apply(db, id, result, now)
}

/// Every enabled Google calendar, one after the other; the first store error is returned at the end.
pub async fn sync_all(
    db: &Db,
    google: Option<&impl GoogleApi>,
    secrets: &dyn Secrets,
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
    secrets: &dyn Secrets,
    id: &str,
) -> anyhow::Result<()> {
    let cal = lock(db).calendars().get(id)?;
    pipeline::remove(db, id)?;
    let others = lock(db).calendars().list()?;
    let Some(cal) = cal.filter(|c| c.kind == KIND) else {
        return Ok(());
    };
    let token = secrets.get(id).ok().flatten();
    if secrets.delete(id).is_err() {
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

/// The person left the picker without adding anything: revoke the new grant unless calendars of
/// the account already use it.
pub async fn drop_pending(
    db: &Db,
    google: Option<&impl GoogleApi>,
    pending: Pending,
) -> anyhow::Result<()> {
    let cals = lock(db).calendars().list()?;
    if of_account(&cals, &pending.account).next().is_none() {
        if let Some(api) = google {
            let _ = api.revoke(&pending.refresh_token).await;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

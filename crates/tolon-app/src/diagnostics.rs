//! "Save diagnostics" (L-072, decision 2026-10-22 "Help inside Tolon, and a diagnostics zip"): one
//! zip with what an agent or a person needs to reproduce a problem, and nothing personal. Nothing
//! is sent anywhere (rule 8); the person sends the file.
//!
//! Never in the zip: calendar addresses (only the host of an iCal link), tokens, the Google
//! account address, event titles, descriptions, attendees, the ignore keywords' text. Alarms are
//! named by [`key_ref`], a hash, because their key holds the event's UID.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tolon_store::{Calendar, Store};
use tolon_sync::redact_url;

use crate::errors::{AppError, SyncErrorDto};
use crate::zip_writer::{self, Entry};
use crate::{google, logs, settings};

/// `docs/help/diagnostics.md` is the zip's `README.txt`, so the two always say the same thing.
pub const README: &str = include_str!("../../../docs/help/diagnostics.md");
/// Where people send the file, until Daniel names a support address (decision 2026-10-22). The
/// one place it is written; the About page shows what `diagnostics_plan` answers.
pub const SUPPORT_URL: &str = "https://github.com/danielpsf/tolon/issues";

/// `docs/help/diagnostics.md` on GitHub, at this version's tag.
pub fn doc_url(version: &str) -> String {
    format!("https://github.com/danielpsf/tolon/blob/v{version}/docs/help/diagnostics.md")
}

/// How many sync log rows go in, newest first.
const SYNC_LOG_ROWS: u32 = 500;
/// Alarms of meetings that started at most this long ago, and every later one.
const ALARM_DAYS: i64 = 7;

/// What the settings window adds: its language, the system's languages and the webview's user
/// agent (the webview version). Capped, since it comes from the page.
#[derive(Debug, Default, Deserialize)]
pub struct Client {
    pub locale: String,
    pub languages: Vec<String>,
    pub user_agent: String,
}

impl Client {
    fn capped(self) -> Self {
        let cut = |s: String, n: usize| s.chars().take(n).collect::<String>();
        Self {
            locale: cut(self.locale, 35),
            languages: self
                .languages
                .into_iter()
                .take(10)
                .map(|l| cut(l, 35))
                .collect(),
            user_agent: cut(self.user_agent, 300),
        }
    }
}

/// One display, in physical pixels.
#[derive(Debug, Serialize)]
pub struct ScreenInfo {
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
}

/// The machine facts `environment.json` reports; the command reads them from Tauri and the
/// platform layer, tests make them up.
pub struct Facts {
    pub version: String,
    pub os: &'static str,
    pub os_version: String,
    pub screens: Vec<ScreenInfo>,
    pub time_zone: String,
    pub client: Client,
}

/// A short, stable stand-in for an alarm's occurrence key (FNV-1a, 64 bits): the key holds the
/// event's UID, which can look like an email address. The scheduler's debug lines use the same
/// value, so `alarms.json` and the log files can be matched.
pub fn key_ref(key: &str) -> String {
    let hash = key.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

/// `Tolon-diagnostics-<version>-<YYYYMMDD-HHMM>.zip`, in the person's local time.
pub fn file_name(version: &str, local: DateTime<FixedOffset>) -> String {
    format!(
        "Tolon-diagnostics-{version}-{}.zip",
        local.format("%Y%m%d-%H%M")
    )
}

/// Where to write: an absolute path from the save dialog, ending in `.zip` (added when the
/// dialog left it off, as some Linux dialogs do).
pub fn target(path: &str) -> Result<PathBuf, AppError> {
    let p = PathBuf::from(path.trim());
    if !p.is_absolute() || p.file_name().is_none() {
        return Err(AppError::DIAGNOSTICS_SAVE);
    }
    let zip = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    Ok(if zip {
        p
    } else {
        let mut name = p.into_os_string();
        name.push(".zip");
        PathBuf::from(name)
    })
}

fn pretty(value: &Value) -> anyhow::Result<Vec<u8>> {
    let mut text = serde_json::to_vec_pretty(value)?;
    text.push(b'\n');
    Ok(text)
}

/// `environment.json`.
pub fn environment(facts: Facts, store: &Store, local: DateTime<FixedOffset>) -> Value {
    let client = facts.client.capped();
    json!({
        "tolon_version": facts.version,
        "build": if cfg!(debug_assertions) { "debug" } else { "release" },
        "tauri_version": tauri::VERSION,
        "os": facts.os,
        "os_version": facts.os_version,
        "arch": std::env::consts::ARCH,
        "locale": client.locale,
        "system_languages": client.languages,
        "webview": client.user_agent,
        "time_zone": facts.time_zone,
        "utc_offset": local.format("%:z").to_string(),
        "saved_at": local.to_rfc3339(),
        "screen_count": facts.screens.len(),
        "screens": facts.screens,
        "database_schema": store.schema_version().ok(),
        "log_files": "debug level for Tolon's own code, info for its libraries; times in UTC",
    })
}

/// `settings.json`: the settings contract, with the ignore keywords as a count (their words can
/// echo meeting titles), and the pause.
fn settings_json(store: &Store) -> anyhow::Result<Value> {
    let mut value = serde_json::to_value(settings::read(store)?)?;
    if let Some(ignore) = value.get_mut("ignore").and_then(Value::as_object_mut) {
        let count = ignore
            .remove("keywords")
            .and_then(|k| k.as_array().map(Vec::len))
            .unwrap_or(0);
        ignore.insert("keyword_count".into(), json!(count));
    }
    let paused = store.settings().get(settings::PAUSED_UNTIL)?;
    if let Some(map) = value.as_object_mut() {
        map.insert("paused_until".into(), paused.unwrap_or(Value::Null));
    }
    Ok(value)
}

/// One calendar without its address, token, account or name. An iCal link keeps its host; a
/// Google calendar's `url` is its Google id (often the address) and is left out.
/// shortcut: no custom name yet, because L-067 (local names) is not merged; add it when it is.
fn calendar_json(c: &Calendar) -> Value {
    let google = c.kind == google::KIND;
    let host = c
        .url
        .as_deref()
        .filter(|_| !google)
        .map(redact_url)
        .map(|u| u.split_once("://").map_or(u.clone(), |(_, h)| h.to_owned()));
    let status = match (&c.last_error, &c.last_sync_at) {
        (Some(_), _) => "error",
        (None, Some(_)) => "ok",
        (None, None) => "never_synced",
    };
    json!({
        "id": c.id,
        "kind": if google { "google" } else { "ical" },
        "host": host,
        "enabled": c.enabled,
        "sync_status": status,
        "last_error": c.last_error.as_deref().map(SyncErrorDto::from_code),
        "last_sync_at": c.last_sync_at,
        "added_at": c.created_at,
    })
}

/// A stored sync log detail is a code (`http:404`, `skipped:2`); anything else is replaced, in
/// case an older build stored text.
fn detail_code(detail: &str) -> String {
    let code = detail
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == ':');
    if code && detail.len() <= 40 {
        detail.to_owned()
    } else {
        "<redacted>".to_owned()
    }
}

/// The store's part of the zip, read under one lock: settings, calendars, sync log and alarms.
pub fn store_entries(store: &Store, now: DateTime<Utc>) -> anyhow::Result<Vec<Entry>> {
    let calendars: Vec<Value> = store
        .calendars()
        .list()?
        .iter()
        .map(calendar_json)
        .collect();
    let sync_log: Vec<Value> = store
        .sync_log()
        .list_recent(SYNC_LOG_ROWS)?
        .into_iter()
        .map(|e| {
            json!({
                "id": e.id,
                "calendar_id": e.calendar_id,
                "at": e.at,
                "outcome": e.outcome,
                "detail": e.detail.as_deref().map(detail_code),
            })
        })
        .collect();
    let since = now - Duration::days(ALARM_DAYS);
    let alarms: Vec<Value> = store
        .alarms()
        .list()?
        .into_iter()
        .filter(|a| {
            DateTime::parse_from_rfc3339(&a.start).map_or(true, |s| s.with_timezone(&Utc) >= since)
        })
        .map(|a| {
            json!({
                "alarm": key_ref(&a.occurrence_key),
                "kind": a.kind,
                "at": a.at,
                "meeting_start": a.start,
                "state": a.state,
                "state_at": a.state_at,
                "until": a.until,
            })
        })
        .collect();
    Ok(vec![
        Entry {
            name: "settings.json".into(),
            data: pretty(&settings_json(store)?)?,
        },
        Entry {
            name: "calendars.json".into(),
            data: pretty(&json!(calendars))?,
        },
        Entry {
            name: "sync-log.json".into(),
            data: pretty(&json!(sync_log))?,
        },
        Entry {
            name: "alarms.json".into(),
            data: pretty(&json!(alarms))?,
        },
    ])
}

/// The whole zip: `README.txt` and `environment.json` first, then the store's files and the log
/// files under `logs/`. A log file that cannot be read is skipped with a note in its place.
pub fn archive(
    environment: &Value,
    store_files: Vec<Entry>,
    log_dir: Option<&Path>,
    local: DateTime<FixedOffset>,
) -> anyhow::Result<Vec<u8>> {
    let readme = format!(
        "Tolon diagnostics, saved {}.\nThis file is docs/help/diagnostics.md of this version of Tolon.\n\n{README}",
        local.to_rfc3339()
    );
    let mut entries = vec![
        Entry {
            name: "README.txt".into(),
            data: readme.into_bytes(),
        },
        Entry {
            name: "environment.json".into(),
            data: pretty(environment)?,
        },
    ];
    entries.extend(store_files);
    let files = log_dir.map(logs::list).transpose()?.unwrap_or_default();
    for (path, _, _) in files {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let data = std::fs::read(&path).unwrap_or_else(|e| {
            format!("Tolon could not read this log file: {}\n", e.kind()).into_bytes()
        });
        entries.push(Entry {
            name: format!("logs/{name}"),
            data,
        });
    }
    Ok(zip_writer::write(&entries, local.naive_local())?)
}

/// Writes `bytes` next to `path` first and then moves it in place, so a failed write never
/// leaves half a zip under the chosen name.
pub fn save(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    std::fs::write(&part, bytes)
        .and_then(|()| std::fs::rename(&part, path))
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&part);
        })
}

#[cfg(test)]
mod tests;

//! "Save diagnostics" (L-072, decision 2026-10-22 "Help inside Tolon, and a diagnostics zip"): one
//! zip with what an agent or a person needs to reproduce a problem, and nothing personal. Nothing
//! is sent anywhere (rule 8); the person sends the file.
//!
//! Never in the zip: calendar addresses (only the host of an iCal link), tokens, the Google
//! account address, event titles, descriptions, attendees, the ignore keywords' text, the home
//! folder's path. Alarms are named by [`AlarmRefs`], a keyed hash, because their key holds the
//! event's UID.

use std::hash::Hasher;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use siphasher::sip::SipHasher24;
use tolon_store::{Calendar, Store};
use tolon_sync::redact_url;

use crate::errors::{AppError, SyncErrorDto};
use crate::zip_writer::{self, Entry};
use crate::{google, logs, settings};

/// `docs/help/diagnostics.md` is the zip's `README.txt`, so the two always say the same thing.
pub const README: &str = include_str!("../../../docs/help/diagnostics.md");
/// Where people send the file (decision 2026-10-22, Daniel). The one place it is written; the
/// About page shows what `diagnostics_plan` answers.
pub const SUPPORT_EMAIL: &str = "tolon@danielpsf.net";
/// The public alternative.
pub const ISSUES_URL: &str = "https://github.com/danielpsf/tolon/issues";
/// The email's body: what to write, and the reminder to attach the file. English, like the docs.
const MAIL_BODY: &str = "What happened:\n\n\nWhen did it happen (date, time and time zone):\n\n\nWhat did you expect, and what happened instead:\n\n\nPlease attach the diagnostics zip file that Tolon saved.\n";
/// The settings key of the per-install secret behind [`AlarmRefs`]. Outside the settings
/// contract, so `settings.json` never carries it.
const ALARM_SECRET: &str = "diagnostics_secret";

/// `docs/help/diagnostics.md` on GitHub, at this version's tag.
pub fn doc_url(version: &str) -> String {
    format!("https://github.com/danielpsf/tolon/blob/v{version}/docs/help/diagnostics.md")
}

/// RFC 3986 percent-encoding of everything but the unreserved characters.
fn percent(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// `mailto:` the support address, with the subject and the short template filled in.
pub fn support_mailto(version: &str) -> String {
    format!(
        "mailto:{SUPPORT_EMAIL}?subject={}&body={}",
        percent(&format!("Tolon diagnostics {version}")),
        percent(MAIL_BODY)
    )
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

/// Short, stable stand-ins for alarm keys: SipHash-2-4 under a 128-bit secret made once per
/// install and kept in the settings table, never exported. The key holds the event's UID, which
/// can look like an email address, and an unkeyed hash of a guessable UID could be reversed.
/// The scheduler's debug lines use the same codes, so `alarms.json` and the logs match.
pub struct AlarmRefs {
    k0: u64,
    k1: u64,
}

impl AlarmRefs {
    /// The install's secret, made on first use.
    pub fn load(store: &Store) -> anyhow::Result<AlarmRefs> {
        let stored = store
            .settings()
            .get(ALARM_SECRET)?
            .and_then(|v| v.as_str().and_then(|s| u128::from_str_radix(s, 16).ok()));
        let secret = match stored {
            Some(s) => s,
            None => {
                let mut bytes = [0u8; 16];
                getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
                let s = u128::from_le_bytes(bytes);
                store
                    .settings()
                    .set(ALARM_SECRET, &json!(format!("{s:032x}")))?;
                s
            }
        };
        // The two halves of the 128-bit secret.
        Ok(AlarmRefs {
            k0: (secret >> 64) as u64,
            k1: secret as u64,
        })
    }

    /// 16 hex digits for `key`.
    pub fn of(&self, key: &str) -> String {
        let mut h = SipHasher24::new_with_keys(self.k0, self.k1);
        h.write(key.as_bytes());
        format!("{:016x}", h.finish())
    }
}

/// `Tolon-diagnostics-<version>-<YYYYMMDD-HHMM>.zip`, in the person's local time.
pub fn file_name(version: &str, local: DateTime<FixedOffset>) -> String {
    format!(
        "Tolon-diagnostics-{version}-{}.zip",
        local.format("%Y%m%d-%H%M")
    )
}

/// Where to write: the save dialog's absolute path, ending in `.zip` (added when the dialog left
/// it off, as some Linux dialogs do). A symlink is refused: the file is written where it is shown.
pub fn target(path: &Path) -> Result<PathBuf, AppError> {
    let p = path.to_owned();
    if !p.is_absolute() || p.file_name().is_none() {
        return Err(AppError::DIAGNOSTICS_SAVE);
    }
    let zip = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    let p = if zip {
        p
    } else {
        let mut name = p.into_os_string();
        name.push(".zip");
        PathBuf::from(name)
    };
    if is_symlink(&p) {
        return Err(AppError::DIAGNOSTICS_SAVE);
    }
    Ok(p)
}

fn is_symlink(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink())
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
        "last_error": c.last_error.as_deref().map(|e| SyncErrorDto::from_code(&detail_code(e))),
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
    let refs = AlarmRefs::load(store)?;
    let alarms: Vec<Value> = store
        .alarms()
        .list()?
        .into_iter()
        .filter(|a| {
            DateTime::parse_from_rfc3339(&a.start).map_or(true, |s| s.with_timezone(&Utc) >= since)
        })
        .map(|a| {
            json!({
                "alarm": refs.of(&a.occurrence_key),
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

/// A log file as text with the home folder's path written as `~` (it can name the person).
fn log_text(path: &Path, home: Option<&Path>) -> Vec<u8> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            match home
                .map(|h| h.display().to_string())
                .filter(|h| h.len() > 1)
            {
                Some(h) => text.replace(&h, "~").into_bytes(),
                None => text.into_owned().into_bytes(),
            }
        }
        Err(e) => format!("Tolon could not read this log file: {}\n", e.kind()).into_bytes(),
    }
}

/// The whole zip: `README.txt` and `environment.json` first, then the store's files and the log
/// files under `logs/`, with `home` replaced by `~`. A log file that cannot be read gets a note.
pub fn archive(
    environment: &Value,
    store_files: Vec<Entry>,
    log_dir: Option<&Path>,
    home: Option<&Path>,
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
        entries.push(Entry {
            name: format!("logs/{name}"),
            data: log_text(&path, home),
        });
    }
    Ok(zip_writer::write(&entries, local.naive_local())?)
}

/// Writes `bytes` to a new `<path>.<random>.part` first (`create_new`: never through an existing
/// file or symlink) and then moves it in place, so a failed write never leaves half a zip under
/// the chosen name. A `path` that is a symlink is refused.
pub fn save(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if is_symlink(path) {
        return Err(std::io::Error::other("the chosen file is a symbolic link"));
    }
    let mut tag = [0u8; 4];
    getrandom::fill(&mut tag).map_err(|e| std::io::Error::other(e.to_string()))?;
    let mut part = path.as_os_str().to_owned();
    part.push(format!(".{:08x}.part", u32::from_le_bytes(tag)));
    let part = PathBuf::from(part);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part)?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| {
            drop(file);
            std::fs::rename(&part, path)
        })
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&part);
        })
}

#[cfg(test)]
mod tests;

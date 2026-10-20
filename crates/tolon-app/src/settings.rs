//! The settings contract of 2026-10-21 (docs/decisions.md, "Overnight M3 plan and the settings
//! contract"): `SettingsDto` out, `SettingsPatch` in, validated, saved at once and whole (AC9.1).
//! Plain functions over the store; the commands are in `commands.rs`, the scheduler reads the
//! same [`read`] every tick.
//!
//! Stored values are lenient: a key that is missing or holds the wrong type reads as its default.
//! Patches are strict: anything out of range is refused with `invalid:<field>`, nested fields as
//! `invalid:quiet.start`, a patch that is not an object or names an unknown field as
//! `invalid:patch`, and nothing is saved.

use chrono::NaiveTime;
use chrono_tz::Tz;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use tolon_core::{
    AlarmSettings, ExpandOptions, IgnoreRules, QuietHours, MAX_LEAD_MINUTES, MIN_LEAD_MINUTES,
};
use tolon_platform::Sound;
use tolon_store::Store;

use crate::errors::AppError;
use crate::pipeline::{self, flag, Db, IGNORE_ALL_DAY, IGNORE_CANCELLED, IGNORE_DECLINED};
use crate::scheduler::{
    AGAIN_AT_START, IGNORE_KEYWORDS, IGNORE_NO_ATTENDEES, LEAD_MINUTES, SOUND, SOUND_REPEAT,
};

pub const QUIET_ENABLED: &str = "quiet_enabled";
pub const QUIET_START: &str = "quiet_start";
pub const QUIET_END: &str = "quiet_end";
pub const QUIET_WEEKENDS: &str = "quiet_weekends";
pub const APPEARANCE: &str = "appearance";
pub const LOCALE: &str = "locale";
pub const START_AT_LOGIN: &str = "start_at_login";

/// The shipped UI catalogues (decisions "Locale list for beta 1" and "Hebrew joins").
pub const LOCALES: [&str; 12] = [
    "en-US", "en-GB", "pt-BR", "pt-PT", "de", "es", "ar", "tr", "hi", "zh-CN", "ja", "he",
];
const MAX_KEYWORDS: usize = 50;
const MAX_KEYWORD_CHARS: usize = 64;
const DEFAULT_QUIET: (&str, &str) = ("18:00", "09:00");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundChoice {
    Bell,
    Chime,
    None,
}

impl From<SoundChoice> for Sound {
    fn from(s: SoundChoice) -> Self {
        match s {
            SoundChoice::Bell => Sound::SoftBell,
            SoundChoice::Chime => Sound::Chime,
            SoundChoice::None => Sound::None,
        }
    }
}

impl From<Sound> for SoundChoice {
    fn from(s: Sound) -> Self {
        match s {
            Sound::SoftBell => SoundChoice::Bell,
            Sound::Chime => SoundChoice::Chime,
            Sound::None => SoundChoice::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    Auto,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QuietDto {
    pub enabled: bool,
    /// `HH:MM`, local time.
    pub start: String,
    pub end: String,
    pub weekends: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IgnoreDto {
    pub all_day: bool,
    pub declined: bool,
    pub cancelled: bool,
    pub no_attendees: bool,
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SettingsDto {
    pub lead_minutes: u32,
    pub again_at_start: bool,
    pub sound: SoundChoice,
    pub sound_repeat: bool,
    pub quiet: QuietDto,
    pub ignore: IgnoreDto,
    pub appearance: Appearance,
    /// `None`: follow the system.
    pub locale: Option<String>,
    pub start_at_login: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuietPatch {
    pub enabled: Option<bool>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub weekends: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IgnorePatch {
    pub all_day: Option<bool>,
    pub declined: Option<bool>,
    pub cancelled: Option<bool>,
    pub no_attendees: Option<bool>,
    pub keywords: Option<Vec<String>>,
}

/// Every field optional; nested ones too. `locale: null` sets "follow the system", a missing
/// `locale` leaves it alone.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsPatch {
    pub lead_minutes: Option<u32>,
    pub again_at_start: Option<bool>,
    pub sound: Option<SoundChoice>,
    pub sound_repeat: Option<bool>,
    pub quiet: Option<QuietPatch>,
    pub ignore: Option<IgnorePatch>,
    pub appearance: Option<Appearance>,
    #[serde(default, deserialize_with = "present")]
    pub locale: Option<Option<String>>,
    pub start_at_login: Option<bool>,
}

/// A present field, `null` included, is `Some`; serde's default would turn `null` into `None`.
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}

impl SettingsDto {
    pub fn alarm(&self) -> AlarmSettings {
        AlarmSettings {
            lead_minutes: self.lead_minutes,
            again_at_start: self.again_at_start,
        }
    }

    pub fn rules(&self) -> IgnoreRules {
        IgnoreRules {
            keywords: self.ignore.keywords.join(", "),
            without_other_attendees: self.ignore.no_attendees,
        }
    }

    pub fn expand_options(&self) -> ExpandOptions {
        ExpandOptions {
            ignore_cancelled: self.ignore.cancelled,
            ignore_declined: self.ignore.declined,
            include_all_day: !self.ignore.all_day,
        }
    }

    /// `None` while quiet hours are off. `tz` is the machine's zone: the window is local time.
    pub fn quiet_hours(&self, tz: Tz) -> Option<QuietHours> {
        let q = &self.quiet;
        if !q.enabled {
            return None;
        }
        Some(QuietHours {
            start: hhmm(&q.start)?,
            end: hhmm(&q.end)?,
            weekends: q.weekends,
            tz,
        })
    }
}

/// `HH:MM` exactly, 00:00 to 23:59.
fn hhmm(s: &str) -> Option<NaiveTime> {
    let shape = s.len() == 5
        && s.char_indices()
            .all(|(i, c)| if i == 2 { c == ':' } else { c.is_ascii_digit() });
    shape
        .then(|| NaiveTime::parse_from_str(s, "%H:%M").ok())
        .flatten()
}

fn keywords(stored: &str) -> Vec<String> {
    stored
        .split(',')
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The settings as stored, with the default for every key that is missing or unreadable.
pub fn read(store: &Store) -> anyhow::Result<SettingsDto> {
    let s = store.settings();
    let (alarm, rules) = (AlarmSettings::default(), IgnoreRules::default());
    let text = |key: &str| -> anyhow::Result<Option<String>> {
        Ok(s.get(key)?.and_then(|v| v.as_str().map(str::to_owned)))
    };
    let time = |key: &str, default: &str| -> anyhow::Result<String> {
        Ok(text(key)?
            .filter(|t| hhmm(t).is_some())
            .unwrap_or_else(|| default.to_owned()))
    };
    let lead = s
        .get(LEAD_MINUTES)?
        .and_then(|v| v.as_u64())
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(alarm.lead_minutes)
        .clamp(MIN_LEAD_MINUTES, MAX_LEAD_MINUTES);
    let sound: Sound = s
        .get(SOUND)?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or(Sound::SoftBell);
    Ok(SettingsDto {
        lead_minutes: lead,
        again_at_start: flag(store, AGAIN_AT_START, alarm.again_at_start)?,
        sound: sound.into(),
        sound_repeat: flag(store, SOUND_REPEAT, true)?,
        quiet: QuietDto {
            enabled: flag(store, QUIET_ENABLED, false)?,
            start: time(QUIET_START, DEFAULT_QUIET.0)?,
            end: time(QUIET_END, DEFAULT_QUIET.1)?,
            weekends: flag(store, QUIET_WEEKENDS, true)?,
        },
        ignore: IgnoreDto {
            all_day: flag(store, IGNORE_ALL_DAY, true)?,
            declined: flag(store, IGNORE_DECLINED, true)?,
            cancelled: flag(store, IGNORE_CANCELLED, true)?,
            no_attendees: flag(store, IGNORE_NO_ATTENDEES, rules.without_other_attendees)?,
            keywords: keywords(&text(IGNORE_KEYWORDS)?.unwrap_or(rules.keywords)),
        },
        appearance: s
            .get(APPEARANCE)?
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or(Appearance::Auto),
        locale: text(LOCALE)?.filter(|l| LOCALES.contains(&l.as_str())),
        // shortcut: the stored preference only, the OS login item is not read back yet (AC9.4); wired by L-041 follow-up.
        start_at_login: flag(store, START_AT_LOGIN, false)?,
    })
}

pub fn get(db: &Db) -> anyhow::Result<SettingsDto> {
    read(&pipeline::lock(db))
}

/// Validates `raw` as a [`SettingsPatch`], saves every field it names in one transaction, and
/// returns the settings as they now are. Nothing is saved when any field is refused.
pub fn apply(db: &Db, raw: &Value) -> anyhow::Result<SettingsDto> {
    let patch: SettingsPatch =
        serde_json::from_value(raw.clone()).map_err(|_| AppError(bad_field(raw)))?;
    let pairs = pairs(patch)?;
    let store = pipeline::lock(db);
    store.settings().set_all(&pairs)?;
    read(&store)
}

/// Every `invalid:` code a patch can get; the UI localises them.
const CODES: [&str; 19] = [
    "invalid:patch",
    "invalid:lead_minutes",
    "invalid:again_at_start",
    "invalid:sound",
    "invalid:sound_repeat",
    "invalid:quiet",
    "invalid:quiet.enabled",
    "invalid:quiet.start",
    "invalid:quiet.end",
    "invalid:quiet.weekends",
    "invalid:ignore",
    "invalid:ignore.all_day",
    "invalid:ignore.declined",
    "invalid:ignore.cancelled",
    "invalid:ignore.no_attendees",
    "invalid:ignore.keywords",
    "invalid:appearance",
    "invalid:locale",
    "invalid:start_at_login",
];

fn code(field: &str) -> &'static str {
    CODES
        .iter()
        .find(|c| c.strip_prefix("invalid:") == Some(field))
        .copied()
        .unwrap_or(CODES[0])
}

/// The first field of a patch that does not decode, found by decoding each field on its own.
fn bad_field(raw: &Value) -> &'static str {
    let Some(fields) = raw.as_object() else {
        return CODES[0];
    };
    let fails = |probe: Value| serde_json::from_value::<SettingsPatch>(probe).is_err();
    for (key, value) in fields {
        if let (Some(inner), "quiet" | "ignore") = (value.as_object(), key.as_str()) {
            for (k, v) in inner {
                if fails(json!({ key: { k: v } })) {
                    return code(&format!("{key}.{k}"));
                }
            }
        }
        if fails(json!({ key: value })) {
            return code(key);
        }
    }
    CODES[0]
}

fn invalid(field: &str) -> anyhow::Error {
    AppError(code(field)).into()
}

fn time_value(s: &str, field: &str) -> anyhow::Result<Value> {
    hhmm(s).ok_or_else(|| invalid(field))?;
    Ok(json!(s))
}

/// Trimmed, blanks dropped; at most 50 of up to 64 characters, no comma (the stored form is one
/// comma-separated string).
fn keywords_value(list: &[String]) -> anyhow::Result<Value> {
    let kept: Vec<&str> = list
        .iter()
        .map(|k| k.trim())
        .filter(|k| !k.is_empty())
        .collect();
    let ok = kept.len() <= MAX_KEYWORDS
        && kept
            .iter()
            .all(|k| k.chars().count() <= MAX_KEYWORD_CHARS && !k.contains(','));
    if !ok {
        return Err(invalid("ignore.keywords"));
    }
    Ok(json!(kept.join(", ")))
}

/// The store writes a patch turns into, or the first refused field.
fn pairs(p: SettingsPatch) -> anyhow::Result<Vec<(&'static str, Value)>> {
    let mut out: Vec<(&'static str, Value)> = Vec::new();
    if let Some(n) = p.lead_minutes {
        if !(MIN_LEAD_MINUTES..=MAX_LEAD_MINUTES).contains(&n) {
            return Err(invalid("lead_minutes"));
        }
        out.push((LEAD_MINUTES, json!(n)));
    }
    let flags = [
        (AGAIN_AT_START, p.again_at_start),
        (SOUND_REPEAT, p.sound_repeat),
        (START_AT_LOGIN, p.start_at_login),
    ];
    out.extend(flags.into_iter().filter_map(|(k, v)| Some((k, json!(v?)))));
    if let Some(sound) = p.sound {
        out.push((SOUND, serde_json::to_value(Sound::from(sound))?));
    }
    if let Some(q) = p.quiet {
        if let Some(s) = &q.start {
            out.push((QUIET_START, time_value(s, "quiet.start")?));
        }
        if let Some(e) = &q.end {
            out.push((QUIET_END, time_value(e, "quiet.end")?));
        }
        let flags = [(QUIET_ENABLED, q.enabled), (QUIET_WEEKENDS, q.weekends)];
        out.extend(flags.into_iter().filter_map(|(k, v)| Some((k, json!(v?)))));
    }
    if let Some(i) = p.ignore {
        if let Some(list) = &i.keywords {
            out.push((IGNORE_KEYWORDS, keywords_value(list)?));
        }
        let flags = [
            (IGNORE_ALL_DAY, i.all_day),
            (IGNORE_DECLINED, i.declined),
            (IGNORE_CANCELLED, i.cancelled),
            (IGNORE_NO_ATTENDEES, i.no_attendees),
        ];
        out.extend(flags.into_iter().filter_map(|(k, v)| Some((k, json!(v?)))));
    }
    if let Some(a) = p.appearance {
        out.push((APPEARANCE, serde_json::to_value(a)?));
    }
    if let Some(locale) = p.locale {
        if locale.as_deref().is_some_and(|l| !LOCALES.contains(&l)) {
            return Err(invalid("locale"));
        }
        out.push((LOCALE, json!(locale)));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;

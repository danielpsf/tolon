//! The words of the F4.5 fallback notification, in the person's language (L-038).
//!
//! Rust has no catalogues: the settings window builds a [`NotifyCopy`] from the active catalogue and
//! hands it over (`notify_copy_set`) when it loads and when the language changes. It is kept in
//! memory and in the settings table (`notify_copy`), so a notification still speaks the language
//! after a restart and while no window is open. Before the first bundle there is the built-in English.
//!
//! The bundle holds finished sentences, with two placeholders: `{title}` (the meeting title, in
//! `one_title`) and `{time}` (in `body`). Plurals are precomputed by the page: the title for 2 to
//! [`MANY_MAX`] meetings is its own string, and a bigger group gets `many_over`, which has no number.
//! The time is a table of the 1440 minutes of a day, formatted by `Intl` in the page, so Rust needs
//! no locale data (digits, 12 or 24 hours, "午後", "م" all come out as the page writes them).
//!
//! Everything that arrives is validated: the locale is a shipped one, lengths are bounded,
//! placeholders are exactly the expected ones, control and Unicode format characters are dropped
//! (the isolates U+2068 and U+2069 stay in `one_title`, where the page wraps `{title}` for Arabic and
//! Hebrew). The meeting title is cleaned the same way before it goes in, so a feed cannot reorder
//! the text around it.

use std::sync::{Arc, Mutex, PoisonError};

use chrono::{DateTime, Timelike};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tolon_store::Store;

use crate::errors::AppError;
use crate::interrupt::AlarmShow;
use crate::scheduler::{is_format_char, SNOOZE_MINUTES};
use crate::settings::LOCALES;

/// The settings key of the stored bundle. Outside the settings contract, like `paused_until`.
pub const KEY: &str = "notify_copy";
/// Titles for 2 to this many meetings are precomputed. Meetings that start in one minute: more is not real.
pub const MANY_MAX: usize = 20;
pub const MINUTES_PER_DAY: usize = 24 * 60;
const MAX_CHARS: usize = 200;
const MAX_TIME_CHARS: usize = 32;
const TITLE: &str = "{title}";
const TIME: &str = "{time}";
const ISOLATES: [char; 2] = ['\u{2068}', '\u{2069}'];

/// `invalid:notify_copy`: the page sent a bundle that is not valid. Nothing is stored.
const INVALID: AppError = AppError("invalid:notify_copy");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotifyCopy {
    /// The shipped catalogue the strings come from.
    pub locale: String,
    pub test_title: String,
    /// The title of one meeting, with `{title}`: `{title}`, or isolated for a right-to-left language.
    pub one_title: String,
    /// The title for 2, 3, ... [`MANY_MAX`] meetings; index 0 is 2.
    pub many_titles: Vec<String>,
    /// The title for a bigger group, without a number.
    pub many_over: String,
    /// With `{time}`.
    pub body: String,
    /// The Snooze button, already for [`SNOOZE_MINUTES`] minutes.
    pub snooze: String,
    pub join: String,
    /// The local time `HH:MM` as the person's locale writes it, minute 0 of the day first. Empty
    /// only in the built-in English, which shows the 24-hour clock as it is.
    pub times: Vec<String>,
}

impl NotifyCopy {
    /// What the notification said before it was translated, and what it says until a bundle arrives.
    pub fn english() -> Self {
        Self {
            locale: "en-US".to_owned(),
            test_title: "Test alarm".to_owned(),
            one_title: TITLE.to_owned(),
            many_titles: (2..=MANY_MAX)
                .map(|n| format!("{n} meetings are about to start"))
                .collect(),
            many_over: "Several meetings are about to start".to_owned(),
            body: format!("Starts at {TIME}. Tolon could not cover the screen."),
            snooze: format!("Snooze {SNOOZE_MINUTES} min"),
            join: "Join the call".to_owned(),
            times: Vec::new(),
        }
    }

    /// A bundle from the page, validated and cleaned, or `invalid:notify_copy`.
    pub fn from_page(value: &Value) -> Result<Self, AppError> {
        let raw: Self = serde_json::from_value(value.clone()).map_err(|_| INVALID)?;
        raw.checked().ok_or(INVALID)
    }

    fn checked(self) -> Option<Self> {
        let one_title = template(&self.one_title, &[TITLE], true)?;
        // The isolates are for `{title}` only: they come as a pair around it, or not at all.
        if one_title.contains(ISOLATES) && !isolated(&one_title) {
            return None;
        }
        let many_titles = self
            .many_titles
            .iter()
            .map(|t| template(t, &[], false))
            .collect::<Option<Vec<_>>>()?;
        let times = self
            .times
            .iter()
            .map(|t| clean(t, MAX_TIME_CHARS, false))
            .collect::<Option<Vec<_>>>()?;
        let valid = LOCALES.contains(&self.locale.as_str())
            && many_titles.len() == MANY_MAX - 1
            && times.len() == MINUTES_PER_DAY;
        valid.then_some(())?;
        Some(Self {
            locale: self.locale,
            test_title: template(&self.test_title, &[], false)?,
            one_title,
            many_titles,
            many_over: template(&self.many_over, &[], false)?,
            body: template(&self.body, &[TIME], false)?,
            snooze: template(&self.snooze, &[], false)?,
            join: template(&self.join, &[], false)?,
            times,
        })
    }

    /// Title and body of the notification for `show`; the time is the local one in `zone`.
    pub fn text(&self, show: &AlarmShow, zone: Tz) -> (String, String) {
        let first = show.meetings.first();
        let title = match (show.test, show.meetings.len()) {
            (true, _) => self.test_title.clone(),
            (false, 0 | 1) => {
                let meeting = first.map(|m| clean_title(&m.title)).unwrap_or_default();
                self.one_title.replace(TITLE, &meeting)
            }
            (false, n) => n
                .checked_sub(2)
                .and_then(|i| self.many_titles.get(i))
                .unwrap_or(&self.many_over)
                .clone(),
        };
        let local = first
            .and_then(|m| DateTime::parse_from_rfc3339(&m.start).ok())
            .map(|start| start.with_timezone(&zone));
        let clock = local.map_or_else(String::new, |t| {
            let minute = (t.hour() * 60 + t.minute()) as usize;
            self.times
                .get(minute)
                .cloned()
                .unwrap_or_else(|| format!("{:02}:{:02}", t.hour(), t.minute()))
        });
        (title, self.body.replace(TIME, &clock))
    }
}

/// A meeting title as the notification may show it: no control and no format characters.
pub fn clean_title(title: &str) -> String {
    title
        .chars()
        .filter(|c| !c.is_control() && !is_format_char(*c))
        .collect()
}

/// `text` without control characters and without format characters (the isolates stay when
/// `keep_isolates`), trimmed, not empty and at most `max` characters.
fn clean(text: &str, max: usize, keep_isolates: bool) -> Option<String> {
    // Bounds the work before cleaning; a longer string is refused, not cut.
    if text.len() > max * 4 {
        return None;
    }
    let cleaned: String = text
        .chars()
        .filter(|c| {
            !c.is_control() && (!is_format_char(*c) || (keep_isolates && ISOLATES.contains(c)))
        })
        .collect();
    let cleaned = cleaned.trim();
    (!cleaned.is_empty() && cleaned.chars().count() <= max).then(|| cleaned.to_owned())
}

/// A cleaned sentence whose `{placeholders}` are exactly `expected`, each once.
fn template(text: &str, expected: &[&str], keep_isolates: bool) -> Option<String> {
    let cleaned = clean(text, MAX_CHARS, keep_isolates)?;
    (placeholders(&cleaned)? == expected).then_some(cleaned)
}

/// The `{names}` in `text` in order; `None` for a stray or nested brace.
fn placeholders(text: &str) -> Option<Vec<String>> {
    let mut found = Vec::new();
    let mut open: Option<String> = None;
    for c in text.chars() {
        match (c, open.take()) {
            ('{', None) => open = Some("{".to_owned()),
            ('{', Some(_)) | ('}', None) => return None,
            ('}', Some(mut name)) => {
                name.push('}');
                found.push(name);
            }
            (c, Some(mut name)) => {
                name.push(c);
                open = Some(name);
            }
            (_, None) => {}
        }
    }
    open.is_none().then_some(found)
}

/// FSI before `{title}` and PDI right after it, and no isolate anywhere else.
fn isolated(text: &str) -> bool {
    text.matches(ISOLATES).count() == 2 && text.contains("\u{2068}{title}\u{2069}")
}

/// The bundle the notification uses now. Managed by Tauri from start.
pub struct NotifyCopyState(Mutex<Arc<NotifyCopy>>);

impl NotifyCopyState {
    pub fn new(copy: NotifyCopy) -> Self {
        Self(Mutex::new(Arc::new(copy)))
    }

    pub fn current(&self) -> Arc<NotifyCopy> {
        Arc::clone(&self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Swaps the bundle; false when it was the one in use already.
    pub fn replace(&self, copy: NotifyCopy) -> bool {
        let mut current = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let changed = **current != copy;
        if changed {
            *current = Arc::new(copy);
        }
        changed
    }
}

/// The stored bundle, or the built-in English when there is none or it no longer validates (an
/// older build wrote it, or the file was edited).
pub fn load(store: &Store) -> NotifyCopy {
    store
        .settings()
        .get(KEY)
        .ok()
        .flatten()
        .and_then(|v| NotifyCopy::from_page(&v).ok())
        .unwrap_or_else(NotifyCopy::english)
}

pub fn save(store: &Store, copy: &NotifyCopy) -> anyhow::Result<()> {
    store
        .settings()
        .set(KEY, &serde_json::to_value(copy)?)
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interrupt::{JoinDto, MeetingDto};
    use serde_json::json;

    type TestResult = anyhow::Result<()>;

    fn meeting(title: &str, start: &str) -> MeetingDto {
        MeetingDto {
            key: "k@2026-10-21T12:00:00Z".to_owned(),
            title: title.to_owned(),
            start: start.to_owned(),
            end: "2026-10-21T15:00:00+02:00".to_owned(),
            join: JoinDto::None,
            people: 2,
        }
    }

    fn show(titles: &[&str]) -> AlarmShow {
        AlarmShow {
            test: false,
            meetings: titles
                .iter()
                .map(|t| meeting(t, "2026-10-21T14:05:00+02:00"))
                .collect(),
        }
    }

    /// The 1440 minutes the way a locale writes them: `fmt(hour, minute)`.
    fn day(fmt: impl Fn(u32, u32) -> String) -> Vec<String> {
        (0..24)
            .flat_map(|h| (0..60).map(move |m| (h, m)))
            .map(|(h, m)| fmt(h, m))
            .collect()
    }

    /// A German-style bundle: 24 hours, two digits.
    fn german() -> NotifyCopy {
        NotifyCopy {
            locale: "de".to_owned(),
            test_title: "Testalarm".to_owned(),
            one_title: "{title}".to_owned(),
            many_titles: (2..=MANY_MAX)
                .map(|n| format!("{n} Termine beginnen gleich"))
                .collect(),
            many_over: "Mehrere Termine beginnen gleich".to_owned(),
            body: "Beginnt um {time} Uhr. Tolon konnte den Bildschirm nicht abdecken.".to_owned(),
            snooze: "5 Min. später erinnern".to_owned(),
            join: "An der Besprechung teilnehmen".to_owned(),
            times: day(|h, m| format!("{h:02}:{m:02}")),
        }
    }

    /// An Arabic-style bundle: 12 hours, Arabic-Indic digits, the title isolated.
    fn arabic() -> NotifyCopy {
        let digits = |n: u32| {
            n.to_string()
                .chars()
                .filter_map(|c| c.to_digit(10))
                .filter_map(|d| char::from_u32(0x0660 + d))
                .collect::<String>()
        };
        NotifyCopy {
            locale: "ar".to_owned(),
            test_title: "تنبيه تجريبي".to_owned(),
            one_title: "\u{2068}{title}\u{2069}".to_owned(),
            many_titles: (2..=MANY_MAX)
                .map(|n| format!("{n} اجتماع على وشك البدء"))
                .collect(),
            many_over: "عدة اجتماعات على وشك البدء".to_owned(),
            body: "يبدأ في الساعة {time}.".to_owned(),
            snooze: "تأجيل ٥ دقائق".to_owned(),
            join: "الانضمام إلى المكالمة".to_owned(),
            times: day(|h, m| {
                let (h12, mark) = (
                    if h % 12 == 0 { 12 } else { h % 12 },
                    if h < 12 { "ص" } else { "م" },
                );
                format!(
                    "{}:{}{} {mark}",
                    digits(h12),
                    digits(m / 10),
                    digits(m % 10)
                )
            }),
        }
    }

    /// A bundle whose day period comes before the number, as in some locales.
    fn day_period_first() -> NotifyCopy {
        NotifyCopy {
            locale: "ja".to_owned(),
            body: "開始時刻は{time}です。".to_owned(),
            times: day(|h, m| {
                format!(
                    "{}{}:{m:02}",
                    if h < 12 { "午前" } else { "午後" },
                    if h % 12 == 0 { 12 } else { h % 12 }
                )
            }),
            ..german()
        }
    }

    const BERLIN: Tz = Tz::Europe__Berlin;

    #[test]
    fn the_built_in_english_is_the_wording_from_before_l_038() {
        let english = NotifyCopy::english();
        let (title, body) = english.text(&show(&["Design review"]), BERLIN);
        assert_eq!(title, "Design review");
        assert_eq!(body, "Starts at 14:05. Tolon could not cover the screen.");
        let (many, _) = english.text(&show(&["a", "b", "c"]), BERLIN);
        assert_eq!(many, "3 meetings are about to start");
        let test = AlarmShow {
            test: true,
            ..show(&["x"])
        };
        assert_eq!(english.text(&test, BERLIN).0, "Test alarm");
        assert_eq!(english.snooze, format!("Snooze {SNOOZE_MINUTES} min"));
        assert_eq!(english.join, "Join the call");
    }

    #[test]
    fn the_snooze_minutes_are_the_ones_the_page_writes_into_its_label() {
        let page = include_str!("../../../ui/src/interrupt.ts");
        assert!(
            page.contains(&format!("SNOOZE_MINUTES = {SNOOZE_MINUTES};")),
            "ui/src/interrupt.ts and scheduler.rs must agree on the snooze minutes"
        );
    }

    #[test]
    fn the_time_is_the_locale_s_own_text_for_the_local_minute() -> TestResult {
        // 14:05 in Berlin, however the start is written.
        for start in ["2026-10-21T14:05:00+02:00", "2026-10-21T12:05:00Z"] {
            let one = AlarmShow {
                test: false,
                meetings: vec![meeting("x", start)],
            };
            assert_eq!(
                german().text(&one, BERLIN).1,
                "Beginnt um 14:05 Uhr. Tolon konnte den Bildschirm nicht abdecken.",
                "{start}"
            );
            assert_eq!(
                arabic().text(&one, BERLIN).1,
                "يبدأ في الساعة ٢:٠٥ م.",
                "{start}"
            );
            assert_eq!(
                day_period_first().text(&one, BERLIN).1,
                "開始時刻は午後2:05です。",
                "{start}"
            );
        }
        // The test alarm starts in UTC: the clock is still the local one.
        let sample =
            AlarmShow::sample(DateTime::parse_from_rfc3339("2026-10-21T12:03:00Z")?.to_utc());
        assert_eq!(
            german().text(&sample, BERLIN).1.split(' ').nth(2),
            Some("14:05")
        );
        Ok(())
    }

    #[test]
    fn midnight_and_the_last_minute_are_in_the_table() {
        let one = |start: &str| AlarmShow {
            test: false,
            meetings: vec![meeting("x", start)],
        };
        assert_eq!(
            day_period_first()
                .text(&one("2026-10-21T00:00:00+02:00"), BERLIN)
                .1,
            "開始時刻は午前12:00です。"
        );
        assert_eq!(
            german()
                .text(&one("2026-10-21T23:59:00+02:00"), BERLIN)
                .1
                .split(' ')
                .nth(2),
            Some("23:59")
        );
    }

    #[test]
    fn a_missing_table_shows_the_24_hour_clock() {
        let copy = NotifyCopy {
            times: Vec::new(),
            ..german()
        };
        assert!(copy.text(&show(&["x"]), BERLIN).1.contains("14:05"));
    }

    #[test]
    fn titles_follow_the_group_size() {
        let german = german();
        let titles = |n: usize| german.text(&show(&vec!["m"; n]), BERLIN).0;
        assert_eq!(titles(1), "m");
        assert_eq!(titles(2), "2 Termine beginnen gleich");
        assert_eq!(
            titles(MANY_MAX),
            format!("{MANY_MAX} Termine beginnen gleich")
        );
        assert_eq!(titles(MANY_MAX + 1), "Mehrere Termine beginnen gleich");
        assert_eq!(
            german
                .text(
                    &AlarmShow {
                        test: true,
                        ..show(&["m"])
                    },
                    BERLIN
                )
                .0,
            "Testalarm"
        );
        // No meetings and no test: an empty title, as before.
        assert_eq!(german.text(&show(&[]), BERLIN).0, "");
    }

    #[test]
    fn arabic_and_hebrew_titles_are_isolated_and_others_are_not() {
        let (title, _) = arabic().text(&show(&["Team (EU)"]), BERLIN);
        assert_eq!(title, "\u{2068}Team (EU)\u{2069}");
        assert_eq!(german().text(&show(&["Team (EU)"]), BERLIN).0, "Team (EU)");
    }

    #[test]
    fn a_feed_title_cannot_reorder_or_break_out_of_the_isolates() {
        let hostile = "a\u{202E}b\u{2069}c\u{200B}d\u{2068}e\nf\u{0}g";
        let (title, _) = arabic().text(&show(&[hostile]), BERLIN);
        assert_eq!(title, "\u{2068}abcdefg\u{2069}");
        // The braces of a title are not placeholders: it is substituted once, not scanned.
        assert_eq!(
            german().text(&show(&["{time} {title}"]), BERLIN).0,
            "{time} {title}"
        );
    }

    #[test]
    fn a_good_bundle_is_accepted_for_every_shape_and_comes_back_equal() -> TestResult {
        for copy in [german(), arabic(), day_period_first()] {
            let value = serde_json::to_value(&copy)?;
            assert_eq!(NotifyCopy::from_page(&value)?, copy, "{}", copy.locale);
        }
        Ok(())
    }

    fn refused(edit: impl FnOnce(&mut Value)) -> bool {
        let mut value = serde_json::to_value(german()).unwrap_or_default();
        edit(&mut value);
        NotifyCopy::from_page(&value) == Err(INVALID)
    }

    #[test]
    fn a_bundle_that_is_not_shaped_right_is_refused() {
        assert!(refused(|v| *v = json!({})));
        assert!(refused(|v| *v = json!("text")));
        assert!(refused(|v| v["extra"] = json!(1)));
        assert!(refused(|v| v["locale"] = json!("xx")));
        assert!(refused(|v| v["locale"] = json!("de-DE")));
        assert!(refused(|v| v["times"] = json!([])));
        assert!(refused(|v| {
            v["times"] = json!(day(|h, m| format!("{h:02}:{m:02}"))
                .into_iter()
                .skip(1)
                .collect::<Vec<_>>());
        }));
        assert!(refused(|v| {
            v["times"] = json!(day(|h, m| format!("{h:02}:{m:02}"))
                .into_iter()
                .chain(["00:00".to_owned()])
                .collect::<Vec<_>>());
        }));
        assert!(refused(|v| {
            v["many_titles"] = json!(vec!["x"; MANY_MAX - 2]);
        }));
        assert!(refused(|v| v["many_titles"][3] = json!(7)));
        assert!(refused(|v| v["snooze"] = Value::Null));
    }

    #[test]
    fn lengths_are_bounded_and_blank_strings_refused() {
        assert!(refused(|v| v["join"] = json!("")));
        assert!(refused(|v| v["join"] = json!("  \u{200B}\n ")));
        assert!(refused(|v| v["join"] = json!("x".repeat(MAX_CHARS + 1))));
        assert!(!refused(|v| v["join"] = json!("x".repeat(MAX_CHARS))));
        assert!(refused(
            |v| v["times"][7] = json!("x".repeat(MAX_TIME_CHARS + 1))
        ));
        assert!(refused(|v| v["times"][7] = json!("")));
        // Far over the bound: refused before any work.
        assert!(refused(|v| v["join"] = json!("x".repeat(1_000_000))));
    }

    #[test]
    fn placeholders_are_exactly_the_expected_ones() {
        assert!(refused(|v| v["one_title"] = json!("title")));
        assert!(refused(|v| v["one_title"] = json!("{time}")));
        assert!(refused(|v| v["one_title"] = json!("{title} {title}")));
        assert!(refused(|v| v["one_title"] = json!("{title} {x}")));
        assert!(refused(|v| v["body"] = json!("Beginnt um 14:00")));
        assert!(refused(|v| v["body"] = json!("{title} {time}")));
        assert!(refused(|v| v["body"] = json!("{time} {time}")));
        assert!(refused(|v| v["body"] = json!("{time")));
        assert!(refused(|v| v["body"] = json!("time} {time}")));
        assert!(refused(|v| v["body"] = json!("{{time}}")));
        assert!(refused(|v| v["snooze"] = json!("{minutes} Min.")));
        assert!(refused(|v| v["join"] = json!("{")));
        assert!(refused(|v| v["many_titles"][0] = json!("{count} Termine")));
        assert!(refused(|v| v["test_title"] = json!("}")));
    }

    #[test]
    fn control_and_format_characters_are_stripped_and_the_isolates_kept_for_the_title_only(
    ) -> TestResult {
        let mut value = serde_json::to_value(german())?;
        value["join"] = json!("A\u{202E}B\u{200B}C\u{0007}D\u{2068}E\u{2069}F\n");
        value["body"] = json!("\u{FEFF}Beginnt\u{0} um {time}");
        value["times"][0] = json!("\u{200F}00:00\u{202C}");
        value["one_title"] = json!("\u{202E}\u{2068}{title}\u{2069}");
        let copy = NotifyCopy::from_page(&value)?;
        assert_eq!(copy.join, "ABCDEF");
        assert_eq!(copy.body, "Beginnt um {time}");
        assert_eq!(copy.times.first().map(String::as_str), Some("00:00"));
        assert_eq!(copy.one_title, "\u{2068}{title}\u{2069}");
        Ok(())
    }

    #[test]
    fn the_isolates_come_as_a_pair_around_the_title_or_not_at_all() {
        assert!(!refused(
            |v| v["one_title"] = json!("\u{2068}{title}\u{2069}")
        ));
        assert!(refused(|v| v["one_title"] = json!("\u{2068}{title}")));
        assert!(refused(|v| v["one_title"] = json!("{title}\u{2069}")));
        assert!(refused(
            |v| v["one_title"] = json!("\u{2069}{title}\u{2068}")
        ));
        assert!(refused(
            |v| v["one_title"] = json!("\u{2068}\u{2068}{title}\u{2069}\u{2069}")
        ));
    }

    #[test]
    fn the_catalogue_s_every_locale_is_one_a_bundle_may_name() -> TestResult {
        for locale in LOCALES {
            let mut value = serde_json::to_value(german())?;
            value["locale"] = json!(locale);
            assert!(NotifyCopy::from_page(&value).is_ok(), "{locale}");
        }
        Ok(())
    }

    #[test]
    fn the_state_swaps_the_bundle_and_says_whether_it_changed() {
        let state = NotifyCopyState::new(NotifyCopy::english());
        assert_eq!(*state.current(), NotifyCopy::english());
        assert!(state.replace(german()));
        assert!(!state.replace(german()), "the same bundle is no change");
        assert_eq!(state.current().locale, "de");
    }

    #[test]
    fn the_bundle_survives_a_restart() -> TestResult {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("tolon.db");
        {
            let store = Store::open(&file)?;
            assert_eq!(load(&store), NotifyCopy::english(), "nothing stored yet");
            save(&store, &arabic())?;
        }
        let reopened = Store::open(&file)?;
        let copy = load(&reopened);
        assert_eq!(copy, arabic());
        assert_eq!(
            copy.text(&show(&["Team"]), BERLIN).0,
            "\u{2068}Team\u{2069}"
        );
        Ok(())
    }

    #[test]
    fn a_stored_bundle_that_no_longer_validates_falls_back_to_english() -> TestResult {
        let store = Store::open_in_memory()?;
        let mut value = serde_json::to_value(german())?;
        value["locale"] = json!("klingon");
        store.settings().set(KEY, &value)?;
        assert_eq!(load(&store), NotifyCopy::english());
        store.settings().set(KEY, &json!("garbage"))?;
        assert_eq!(load(&store), NotifyCopy::english());
        Ok(())
    }
}

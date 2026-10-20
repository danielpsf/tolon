//! Ignore rules that `expand` does not apply (AC8.2). Cancelled, declined and all-day events
//! are dropped inside `expand` through `ExpandOptions`; the two rules here need the title and the
//! attendee count of an occurrence that survived it. [`judge_window`] runs both and keeps every
//! occurrence with its verdict, for the panel's muted "ignored" tag (S8.1).

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::expand::{expand_judged, ExpandOptions};
use crate::model::{Event, Occurrence};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IgnoreRules {
    /// Comma-separated, case-insensitive, matched anywhere in the title. Blank entries count as nothing.
    pub keywords: String,
    /// Drop events with no attendee besides the owner. Default off.
    pub without_other_attendees: bool,
}

impl Default for IgnoreRules {
    fn default() -> Self {
        Self {
            keywords: "Focus, Lunch, OOO".to_string(),
            without_other_attendees: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IgnoreReason {
    /// The keyword that matched, as the person typed it (trimmed).
    Keyword(String),
    NoOtherAttendees,
    AllDay,
    Declined,
    Cancelled,
}

impl IgnoreReason {
    /// The code the UI turns into the muted tag: `keyword`, `no_attendees`, `all_day`, `declined`
    /// or `cancelled`. The keyword itself stays out: it is close to the title.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Keyword(_) => "keyword",
            Self::NoOtherAttendees => "no_attendees",
            Self::AllDay => "all_day",
            Self::Declined => "declined",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Keep,
    Ignored(IgnoreReason),
}

/// Judge each occurrence, keeping the input order. The panel lists the ignored ones with a muted
/// tag (S8.1); only the `Keep` ones go to `plan`.
///
/// `people` counts ATTENDEE lines only, never the ORGANIZER (parse.rs). Google leaves ATTENDEE out of a
/// solo event and Outlook lists the organizer separately, so "no other attendees" is `people == 0`.
/// shortcut: a feed that lists the owner as an ATTENDEE on a solo event is not ignored; add the
/// account addresses (as the declined rule has) if that bites.
pub fn apply_rules(
    occurrences: Vec<Occurrence>,
    rules: &IgnoreRules,
) -> Vec<(Occurrence, Verdict)> {
    judge(occurrences.into_iter().map(|occ| (occ, None)), rules)
}

/// Every occurrence of `[from, to)` with its verdict: hidden by `opts` (cancelled, declined,
/// all-day) or by `rules` (keyword, no other attendees), or kept. Same order as `expand_window`;
/// its `Keep` ones are exactly what `apply_rules(expand_window(..))` keeps.
pub fn judge_window(
    events: &[Event],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    local: Tz,
    opts: &ExpandOptions,
    rules: &IgnoreRules,
) -> Vec<(Occurrence, Verdict)> {
    judge(
        expand_judged(events, from, to, local, opts).into_iter(),
        rules,
    )
}

fn judge(
    occurrences: impl Iterator<Item = (Occurrence, Option<IgnoreReason>)>,
    rules: &IgnoreRules,
) -> Vec<(Occurrence, Verdict)> {
    let keywords: Vec<&str> = rules
        .keywords
        .split(',')
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .collect();
    occurrences
        .map(|(occ, hidden)| {
            let title = occ.title.to_lowercase();
            let verdict = if let Some(reason) = hidden {
                Verdict::Ignored(reason)
            } else if let Some(k) = keywords.iter().find(|k| title.contains(&k.to_lowercase())) {
                Verdict::Ignored(IgnoreReason::Keyword((*k).to_string()))
            } else if rules.without_other_attendees && occ.people == 0 {
                Verdict::Ignored(IgnoreReason::NoOtherAttendees)
            } else {
                Verdict::Keep
            };
            (occ, verdict)
        })
        .collect()
}

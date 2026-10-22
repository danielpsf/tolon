//! The five-minute refresh. `due` is pure; `run` is the thin loop around it.

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tauri::AppHandle;
use tolon_store::Calendar;

use crate::commands::{state, sync_and_notify};
use crate::pipeline;

/// Time between two syncs of one calendar (AC1.4).
pub const INTERVAL: chrono::Duration = chrono::Duration::minutes(5);
/// First syncs after launch spread over this many seconds; each calendar then keeps its own offset.
const FIRST_SPREAD_SECS: u32 = 60;
const TICK: Duration = Duration::from_secs(15);

/// A fixed offset in `0..FIRST_SPREAD_SECS` for a calendar id (FNV-1a, so it is stable across runs).
pub fn stagger_secs(id: &str) -> i64 {
    let hash = id.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    i64::from(hash % FIRST_SPREAD_SECS)
}

/// Ids of the enabled calendars that should sync at `now`.
/// - Synced before: due `INTERVAL` after the last attempt.
/// - Created since launch: it was synced when added, so due `INTERVAL` after `created_at`.
/// - Otherwise: due at `started` plus its stagger offset.
pub fn due(
    calendars: &[Calendar],
    last_attempt: &HashMap<String, DateTime<Utc>>,
    started: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Vec<String> {
    calendars
        .iter()
        .filter(|c| c.enabled)
        .filter(|c| {
            let created = DateTime::parse_from_rfc3339(&c.created_at)
                .ok()
                .map(|t| t.with_timezone(&Utc))
                .filter(|t| *t >= started);
            let next = match (last_attempt.get(&c.id), created) {
                (Some(t), _) => *t + INTERVAL,
                (None, Some(t)) => t + INTERVAL,
                (None, None) => started + chrono::Duration::seconds(stagger_secs(&c.id)),
            };
            now >= next
        })
        .map(|c| c.id.clone())
        .collect()
}

/// Runs for the life of the app on Tauri's async runtime.
pub async fn run(app: AppHandle) {
    let started = Utc::now();
    let mut last_attempt: HashMap<String, DateTime<Utc>> = HashMap::new();
    loop {
        tokio::time::sleep(TICK).await;
        let now = Utc::now();
        let calendars = match state(&app).and_then(|s| pipeline::calendars(&s.db)) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, "refresh could not list calendars");
                continue;
            }
        };
        for id in due(&calendars, &last_attempt, started, now) {
            last_attempt.insert(id.clone(), now);
            if let Err(e) = sync_and_notify(&app, Some(&id)).await {
                tracing::warn!(calendar = %id, error = %e, "refresh could not save a sync");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cal(id: &str, enabled: bool, created_at: &str) -> Calendar {
        Calendar {
            id: id.into(),
            kind: "ics".into(),
            name: id.into(),
            custom_name: None,
            enabled,
            url: Some("https://example.com/a.ics".into()),
            account: None,
            color: None,
            etag: None,
            last_sync_at: None,
            last_error: None,
            created_at: created_at.into(),
        }
    }

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s)
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or(DateTime::<Utc>::MIN_UTC)
    }

    #[test]
    fn ac1_4_refresh_stagger_spreads_ids_inside_the_first_minute() {
        let offsets: Vec<i64> = (0..40).map(|i| stagger_secs(&format!("cal-{i}"))).collect();
        assert!(offsets.iter().all(|s| (0..60).contains(s)));
        let mut distinct = offsets.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert!(
            distinct.len() > 10,
            "40 ids should not collapse to a few offsets"
        );
        assert_eq!(stagger_secs("cal-1"), stagger_secs("cal-1"));
    }

    #[test]
    fn ac1_4_refresh_first_run_waits_for_each_calendars_offset() {
        let started = at("2026-10-20T08:00:00Z");
        let cals = [cal("a", true, "2026-10-01T00:00:00Z")];
        let offset = chrono::Duration::seconds(stagger_secs("a"));
        let none = HashMap::new();
        if offset > chrono::Duration::zero() {
            assert!(due(&cals, &none, started, started).is_empty());
        }
        assert_eq!(due(&cals, &none, started, started + offset), ["a"]);
    }

    #[test]
    fn ac1_4_refresh_repeats_every_five_minutes_after_the_last_attempt() {
        let started = at("2026-10-20T08:00:00Z");
        let cals = [cal("a", true, "2026-10-01T00:00:00Z")];
        let last = HashMap::from([("a".to_owned(), at("2026-10-20T08:00:30Z"))]);
        assert!(due(&cals, &last, started, at("2026-10-20T08:05:29Z")).is_empty());
        assert_eq!(
            due(&cals, &last, started, at("2026-10-20T08:05:30Z")),
            ["a"]
        );
    }

    #[test]
    fn ac1_4_refresh_skips_disabled_and_waits_after_a_fresh_add() {
        let started = at("2026-10-20T08:00:00Z");
        let cals = [
            cal("off", false, "2026-10-01T00:00:00Z"),
            cal("new", true, "2026-10-20T08:10:00Z"),
        ];
        let none = HashMap::new();
        // Both are overdue by the clock, but the disabled one never runs and the new one was just synced.
        assert!(due(&cals, &none, started, at("2026-10-20T08:12:00Z")).is_empty());
        assert_eq!(
            due(&cals, &none, started, at("2026-10-20T08:15:00Z")),
            ["new"]
        );
    }
}

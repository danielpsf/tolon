//! The scheduler against a fake clock and a fake host: no Tauri, no window, no sound.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex as StdMutex, PoisonError};

use anyhow::anyhow;
use chrono::TimeZone;
use tolon_core::{
    countdown, parse_ics, CalendarId, Countdown, ParseOptions, EXPIRY_MINUTES, MAX_LEAD_MINUTES,
    MIN_LEAD_MINUTES,
};
use tolon_store::{AlarmRow, Store, TERMINAL_STATES};

use super::*;
use crate::pipeline::feed_events;

type TestResult = anyhow::Result<()>;

fn at(h: u32, m: u32, s: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 20, h, m, s)
        .single()
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

/// Records every call the scheduler makes, in order, and can be told to misbehave.
struct Fake {
    now: StdMutex<DateTime<Utc>>,
    calls: StdMutex<Vec<String>>,
    shows: StdMutex<Vec<AlarmShow>>,
    urls: StdMutex<Vec<String>>,
    notices: StdMutex<Vec<i32>>,
    /// Every `Host::paused` call, oldest first; kept out of `calls`.
    pauses: StdMutex<Vec<Option<DateTime<Utc>>>>,
    focused: AtomicBool,
    degraded: Option<&'static str>,
    fail_open: AtomicBool,
    fail_url: AtomicBool,
}

impl Fake {
    fn new(now: DateTime<Utc>) -> Self {
        Self {
            now: StdMutex::new(now),
            calls: StdMutex::default(),
            shows: StdMutex::default(),
            urls: StdMutex::default(),
            notices: StdMutex::default(),
            pauses: StdMutex::default(),
            focused: AtomicBool::new(true),
            degraded: None,
            fail_open: AtomicBool::new(false),
            fail_url: AtomicBool::new(false),
        }
    }

    fn log(&self, call: &str) {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(call.to_owned());
    }

    fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn count(&self, call: &str) -> usize {
        self.calls().iter().filter(|c| *c == call).count()
    }

    fn shows(&self) -> Vec<AlarmShow> {
        self.shows
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn urls(&self) -> Vec<String> {
        self.urls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The identifiers of every notification sent, oldest first.
    fn notices(&self) -> Vec<i32> {
        self.notices
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn pauses(&self) -> Vec<Option<DateTime<Utc>>> {
        self.pauses
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set_now(&self, t: DateTime<Utc>) {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner) = t;
    }
}

impl Host for Fake {
    fn now(&self) -> DateTime<Utc> {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn zone(&self) -> Tz {
        Tz::UTC
    }

    fn open(&self, show: &AlarmShow) -> impl Future<Output = anyhow::Result<Opened>> + Send {
        self.log("open");
        self.shows
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(show.clone());
        let answer = if self.fail_open.load(Ordering::SeqCst) {
            Err(anyhow!("no window"))
        } else {
            Ok(Opened {
                degraded: self.degraded,
            })
        };
        async move { answer }
    }

    fn focused(&self) -> impl Future<Output = bool> + Send {
        self.log("focused");
        let focused = self.focused.load(Ordering::SeqCst);
        async move { focused }
    }

    fn close(&self) {
        self.log("close");
    }

    fn play(&self, _sound: Sound) {
        self.log("play");
    }

    fn stop_sound(&self) {
        self.log("stop");
    }

    fn open_url(&self, url: &str) -> anyhow::Result<()> {
        if self.fail_url.load(Ordering::SeqCst) {
            return Err(anyhow!("no browser"));
        }
        self.log("url");
        self.urls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(url.to_owned());
        Ok(())
    }

    fn paused(&self, until: Option<DateTime<Utc>>) {
        self.pauses
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(until);
    }

    fn notify(&self, _show: &AlarmShow, id: i32) {
        self.log("notify");
        self.notices
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(id);
    }
}

/// One event: uid, title, start `HHMMSS` today (UTC), and extra property lines.
type Seed<'a> = (&'a str, &'a str, &'a str, &'a str);

fn ics(events: &[Seed<'_>]) -> String {
    let body: String = events
        .iter()
        .map(|(uid, title, start, extra)| {
            format!(
                "BEGIN:VEVENT\r\nUID:{uid}\r\nSUMMARY:{title}\r\nDTSTART:20261020T{start}Z\r\nDTEND:20261020T235900Z\r\n{extra}END:VEVENT\r\n"
            )
        })
        .collect();
    format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{body}END:VCALENDAR\r\n")
}

struct Rig {
    db: Db,
    sched: Scheduler<Fake>,
}

impl Rig {
    fn new(events: &[Seed<'_>], now: DateTime<Utc>) -> anyhow::Result<Self> {
        Self::with_host(events, Fake::new(now))
    }

    fn with_host(events: &[Seed<'_>], host: Fake) -> anyhow::Result<Self> {
        let db: Db = StdMutex::new(Store::open_in_memory()?);
        {
            let store = pipeline::lock(&db);
            let cal = store.calendars().insert(
                "ics",
                "Test",
                Some("https://example.com/a.ics"),
                true,
                "t",
            )?;
            let parsed = parse_ics(
                &ics(events),
                &CalendarId(cal.id.clone()),
                &ParseOptions::default(),
            );
            store
                .events()
                .replace_for_calendar(&cal.id, &feed_events(parsed.events)?, "t")?;
        }
        Ok(Self {
            db,
            sched: Scheduler::new(host),
        })
    }

    fn host(&self) -> &Fake {
        &self.sched.host
    }

    fn setting(&self, key: &str, value: serde_json::Value) -> TestResult {
        pipeline::lock(&self.db).settings().set(key, &value)?;
        Ok(())
    }

    /// A full tick at `t`.
    async fn tick_at(&self, t: DateTime<Utc>) -> StepReport {
        self.host().set_now(t);
        self.sched.step(&self.db, true).await
    }

    /// Housekeeping only (focus re-check, sound repeat) at `t`.
    async fn housekeep_at(&self, t: DateTime<Utc>) {
        self.host().set_now(t);
        self.sched.step(&self.db, false).await;
    }

    async fn act_on_notice(&self, t: DateTime<Utc>, notice: i32, act: Act) -> anyhow::Result<()> {
        self.host().set_now(t);
        self.sched.act_on_notice(&self.db, notice, act).await
    }

    async fn act(&self, t: DateTime<Utc>, key: &str, act: Act) -> anyhow::Result<()> {
        self.host().set_now(t);
        self.sched.act(&self.db, key, act).await
    }

    /// `(uid, kind, state)` of every stored alarm.
    fn states(&self) -> anyhow::Result<Vec<(String, String, String)>> {
        let rows = pipeline::lock(&self.db).alarms().list()?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let uid = r
                    .occurrence_key
                    .split_once(':')
                    .and_then(|(_, rest)| rest.split_once('@'))
                    .map(|(uid, _)| uid.to_owned())
                    .unwrap_or_default();
                (uid, r.kind, r.state)
            })
            .collect())
    }

    fn state_of(&self, uid: &str, kind: &str) -> anyhow::Result<Option<String>> {
        Ok(self
            .states()?
            .into_iter()
            .find(|(u, k, _)| u == uid && k == kind)
            .map(|(_, _, s)| s))
    }

    fn until_of(&self, uid: &str) -> anyhow::Result<Option<String>> {
        let rows = pipeline::lock(&self.db).alarms().list()?;
        Ok(rows
            .into_iter()
            .find(|r| r.occurrence_key.contains(&format!(":{uid}@")) && r.kind == "lead")
            .and_then(|r| r.until))
    }

    fn last_show(&self) -> anyhow::Result<AlarmShow> {
        self.host()
            .shows()
            .pop()
            .ok_or_else(|| anyhow!("nothing was shown"))
    }

    fn key(&self, index: usize) -> anyhow::Result<String> {
        self.last_show()?
            .meetings
            .get(index)
            .map(|m| m.key.clone())
            .ok_or_else(|| anyhow!("no meeting {index} in the interrupt"))
    }
}

/// A daily series at 14:00 for two days, with its first instance moved to `moved_to` (`HHMMSS`).
fn series(moved_to: Option<&str>) -> String {
    let moved = moved_to
        .map(|start| {
            format!(
                "BEGIN:VEVENT\r\nUID:series\r\nSUMMARY:Series\r\nRECURRENCE-ID:20261020T140000Z\r\nDTSTART:20261020T{start}Z\r\nDTEND:20261020T235900Z\r\nEND:VEVENT\r\n"
            )
        })
        .unwrap_or_default();
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:series\r\nSUMMARY:Series\r\nDTSTART:20261020T140000Z\r\nDTEND:20261020T145900Z\r\nRRULE:FREQ=DAILY;COUNT=2\r\nEND:VEVENT\r\n{moved}END:VCALENDAR\r\n"
    )
}

impl Rig {
    /// A rig over a raw feed instead of the `Seed` list.
    fn from_feed(feed: &str, now: DateTime<Utc>) -> anyhow::Result<Self> {
        let rig = Self::new(&[], now)?;
        rig.replace_feed(feed)?;
        Ok(rig)
    }

    /// What the calendar server sends next: the stored events follow the feed.
    fn replace_feed(&self, feed: &str) -> TestResult {
        let store = pipeline::lock(&self.db);
        let cal = store
            .calendars()
            .list()?
            .pop()
            .ok_or_else(|| anyhow!("no calendar"))?;
        let parsed = parse_ics(feed, &CalendarId(cal.id.clone()), &ParseOptions::default());
        store
            .events()
            .replace_for_calendar(&cal.id, &feed_events(parsed.events)?, "t2")?;
        Ok(())
    }
}

const STANDUP: Seed<'static> = ("standup", "Standup", "140000", "");
const MEET: &str = "https://meet.google.com/abc-defg-hij";
const MEET_LOCATION: &str = "LOCATION:https://meet.google.com/abc-defg-hij\r\n";

fn with_link<'a>(uid: &'a str, start: &'a str, location: &'a str) -> Seed<'a> {
    (uid, uid, start, location)
}

// ---- F3 ----------------------------------------------------------------------------------------

#[tokio::test]
async fn s3_1_fires_within_15_s_of_the_lead_time() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    // Lead time 2 minutes, meeting at 14:00: the alarm is due at 13:58:00.
    assert_eq!(rig.tick_at(at(13, 57, 50)).await.fired, 0);
    assert_eq!(rig.host().count("open"), 0);
    // The next tick is 15 s later at the latest.
    let report = rig.tick_at(at(13, 58, 5)).await;
    assert_eq!(report.fired, 1);
    assert_eq!(rig.host().count("open"), 1);
    assert_eq!(rig.last_show()?.meetings[0].title, "Standup");
    assert_eq!(rig.state_of("standup", "lead")?.as_deref(), Some("firing"));
    Ok(())
}

#[tokio::test]
async fn lead_time_comes_from_the_setting() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.setting(LEAD_MINUTES, serde_json::json!(5))?;
    assert_eq!(rig.tick_at(at(13, 54, 50)).await.fired, 0);
    assert_eq!(rig.tick_at(at(13, 55, 5)).await.fired, 1);
    Ok(())
}

#[tokio::test]
async fn s3_2_snooze_returns() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.tick_at(at(13, 58, 5)).await;
    let key = rig.key(0)?;

    rig.act(at(13, 58, 10), &key, Act::Snooze(5)).await?;
    assert_eq!(rig.host().count("close"), 1, "the interrupt hides");
    assert_eq!(rig.state_of("standup", "lead")?.as_deref(), Some("snoozed"));
    assert_eq!(
        rig.until_of("standup")?.as_deref(),
        Some("2026-10-20T14:03:10Z")
    );

    // Not yet at 14:03:05, back at the next tick after 14:03:10.
    assert_eq!(rig.tick_at(at(14, 3, 5)).await.fired, 0);
    assert_eq!(rig.host().count("open"), 1);
    assert_eq!(rig.tick_at(at(14, 3, 20)).await.fired, 1);
    assert_eq!(rig.host().count("open"), 2);
    Ok(())
}

#[tokio::test]
async fn s3_3_dismiss_survives_a_restart() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.tick_at(at(13, 58, 5)).await;
    let key = rig.key(0)?;
    rig.act(at(13, 58, 10), &key, Act::Dismiss).await?;
    assert_eq!(
        rig.state_of("standup", "lead")?.as_deref(),
        Some("dismissed")
    );

    // A new process: the scheduler is rebuilt, only the store is left.
    let restarted = Scheduler::new(Fake::new(at(13, 58, 30)));
    restarted.host.set_now(at(13, 59, 0));
    let report = restarted.step(&rig.db, true).await;
    assert_eq!(report.fired, 0);
    assert!(
        restarted.host.calls().is_empty(),
        "{:?}",
        restarted.host.calls()
    );
    restarted.host.set_now(at(14, 0, 15));
    assert_eq!(restarted.step(&rig.db, true).await.fired, 0);
    Ok(())
}

/// An instance at 14:00 is handled at 13:58, then moves to 14:30; Tolon sees the move at `seen`.
/// The handled alarm must not come back (S3.3, decision "Alarm behaviour": only Expired re-arms).
async fn moved_meeting_stays_quiet(act: Act, seen: DateTime<Utc>, state: &str) -> TestResult {
    let rig = Rig::from_feed(&series(None), at(13, 50, 0))?;
    rig.tick_at(at(13, 58, 5)).await;
    let key = rig.key(0)?;
    rig.act(at(13, 58, 10), &key, act).await?;
    assert_eq!(rig.state_of("series", "lead")?.as_deref(), Some(state));
    assert_eq!(rig.host().count("open"), 1);

    rig.replace_feed(&series(Some("143000")))?;
    rig.tick_at(seen).await;
    assert_eq!(
        rig.state_of("series", "lead")?.as_deref(),
        Some(state),
        "the move must not re-arm it"
    );
    // The new alarm time, 14:28, comes and goes without an interrupt.
    assert_eq!(rig.tick_at(at(14, 28, 5)).await.fired, 0);
    assert_eq!(rig.tick_at(at(14, 28, 20)).await.fired, 0);
    assert_eq!(rig.host().count("open"), 1);
    Ok(())
}

#[tokio::test]
async fn s3_3_a_dismissed_alarm_stays_dismissed_when_a_move_is_seen_after_the_old_window(
) -> TestResult {
    // 14:11 is past the old start + 10 min: the pruning must not read the old start.
    moved_meeting_stays_quiet(Act::Dismiss, at(14, 11, 0), "dismissed").await
}

#[tokio::test]
async fn s3_3_a_joined_alarm_stays_joined_when_a_move_is_seen_after_the_old_window() -> TestResult {
    moved_meeting_stays_quiet(Act::AlreadyThere, at(14, 11, 0), "joined").await
}

#[tokio::test]
async fn s3_3_a_dismissed_alarm_stays_dismissed_when_a_move_is_seen_before_the_old_window_closes(
) -> TestResult {
    moved_meeting_stays_quiet(Act::Dismiss, at(14, 5, 0), "dismissed").await
}

#[tokio::test]
async fn a_handled_alarm_of_a_finished_meeting_is_pruned() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.tick_at(at(13, 58, 5)).await;
    let key = rig.key(0)?;
    rig.act(at(13, 58, 10), &key, Act::Dismiss).await?;
    rig.tick_at(at(14, 9, 59)).await;
    assert_eq!(rig.states()?.len(), 1, "kept until start + 10 min");
    rig.tick_at(at(14, 10, 1)).await;
    assert!(rig.states()?.is_empty(), "{:?}", rig.states()?);
    Ok(())
}

#[tokio::test]
async fn ac3_3_a_snoozed_alarm_fires_at_its_time_after_a_restart() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.tick_at(at(13, 58, 5)).await;
    let key = rig.key(0)?;
    rig.act(at(13, 58, 10), &key, Act::Snooze(5)).await?;

    let restarted = Scheduler::new(Fake::new(at(14, 0, 0)));
    assert_eq!(restarted.step(&rig.db, true).await.fired, 0);
    restarted.host.set_now(at(14, 3, 20));
    assert_eq!(restarted.step(&rig.db, true).await.fired, 1);
    assert_eq!(restarted.host.count("open"), 1);
    Ok(())
}

#[tokio::test]
async fn s3_4_wake_inside_the_window_fires_with_the_started_minutes() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    assert!(!rig.tick_at(at(13, 50, 0)).await.woke);
    // Asleep from 13:50 to 14:03.
    let report = rig.tick_at(at(14, 3, 0)).await;
    assert!(report.woke);
    assert_eq!(report.fired, 1);
    assert_eq!(rig.host().count("open"), 1);
    let start =
        DateTime::parse_from_rfc3339(&rig.last_show()?.meetings[0].start)?.with_timezone(&Utc);
    assert_eq!(
        countdown(start, at(14, 3, 0)),
        Countdown::StartedAgo { minutes: 3 }
    );
    Ok(())
}

#[tokio::test]
async fn s3_5_wake_outside_the_window_expires() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.tick_at(at(13, 50, 0)).await;
    assert_eq!(
        rig.state_of("standup", "lead")?.as_deref(),
        Some("scheduled")
    );
    // Asleep from 13:50 to 14:30, 20 minutes past the start.
    let report = rig.tick_at(at(14, 30, 0)).await;
    assert!(report.woke);
    assert_eq!(report.fired, 0);
    assert_eq!(report.expired, 1);
    assert!(rig.host().calls().is_empty(), "{:?}", rig.host().calls());
    assert_eq!(rig.state_of("standup", "lead")?.as_deref(), Some("expired"));
    // The next tick prunes it: the meeting is long over.
    rig.tick_at(at(14, 30, 15)).await;
    assert!(rig.states()?.is_empty());
    Ok(())
}

#[tokio::test]
async fn s3_6_two_meetings_in_the_same_minute_make_one_group() -> TestResult {
    let rig = Rig::new(
        &[("a", "Alpha", "140000", ""), ("b", "Beta", "140000", "")],
        at(13, 50, 0),
    )?;
    let report = rig.tick_at(at(13, 58, 5)).await;
    assert_eq!(report.fired, 2);
    assert_eq!(rig.host().count("open"), 1, "one interrupt");
    let titles: Vec<String> = rig
        .last_show()?
        .meetings
        .into_iter()
        .map(|m| m.title)
        .collect();
    assert_eq!(titles, ["Alpha", "Beta"]);
    Ok(())
}

#[tokio::test]
async fn ac3_5_lead_alarms_on_different_ticks_merge_into_the_open_group() -> TestResult {
    // Same start minute, 30 s apart: their lead alarms are due 30 s apart and land on two ticks.
    let rig = Rig::new(
        &[("a", "Alpha", "140000", ""), ("b", "Beta", "140030", "")],
        at(13, 50, 0),
    )?;
    rig.tick_at(at(13, 58, 5)).await;
    assert_eq!(rig.last_show()?.meetings.len(), 1);
    assert_eq!(rig.host().count("play"), 1);

    let report = rig.tick_at(at(13, 58, 35)).await;
    assert_eq!(report.fired, 1);
    let show = rig.last_show()?;
    let titles: Vec<&str> = show.meetings.iter().map(|m| m.title.as_str()).collect();
    assert_eq!(
        titles,
        ["Alpha", "Beta"],
        "the second meeting joined the open interrupt"
    );
    assert_eq!(rig.host().count("close"), 0, "never closed and reopened");
    assert_eq!(rig.host().count("play"), 2, "the new meeting is heard");
    Ok(())
}

#[tokio::test]
async fn another_start_minute_waits_for_the_open_interrupt() -> TestResult {
    let rig = Rig::new(
        &[("a", "Alpha", "140000", ""), ("b", "Beta", "140500", "")],
        at(13, 50, 0),
    )?;
    rig.tick_at(at(13, 58, 5)).await;
    let key = rig.key(0)?;
    // Beta is due at 14:03 while Alpha is still up: Alpha's interrupt is not touched.
    let report = rig.tick_at(at(14, 3, 5)).await;
    assert_eq!(report.fired, 1);
    assert_eq!(rig.host().count("open"), 1);
    assert_eq!(rig.state_of("b", "lead")?.as_deref(), Some("firing"));

    // Dismissing Alpha hands the screen to Beta at once.
    rig.act(at(14, 3, 10), &key, Act::Dismiss).await?;
    assert_eq!(rig.host().count("open"), 2);
    assert_eq!(rig.last_show()?.meetings[0].title, "Beta");
    Ok(())
}

#[tokio::test]
async fn ac3_2_an_interrupt_closes_when_its_meeting_expires() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.tick_at(at(13, 58, 5)).await;
    rig.tick_at(at(14, 9, 50)).await;
    assert_eq!(rig.host().count("close"), 0);
    // Ten minutes after the start with no action (AC3.2).
    let report = rig.tick_at(at(14, 10, 5)).await;
    assert_eq!(report.expired, 1);
    assert_eq!(rig.host().count("close"), 1);
    assert_eq!(rig.host().count("stop"), 1);
    Ok(())
}

#[tokio::test]
async fn a_rule_from_the_settings_keeps_a_meeting_from_firing() -> TestResult {
    let rig = Rig::new(&[("f", "Focus time", "140000", "")], at(13, 50, 0))?;
    assert_eq!(
        rig.tick_at(at(13, 58, 5)).await.fired,
        0,
        "the default keywords include Focus"
    );
    assert!(rig.states()?.is_empty());
    rig.setting(IGNORE_KEYWORDS, serde_json::json!(""))?;
    assert_eq!(rig.tick_at(at(13, 58, 20)).await.fired, 1);
    Ok(())
}

// ---- F4, F5 ------------------------------------------------------------------------------------

#[tokio::test]
async fn s4_3_join_opens_the_validated_url_and_already_there_opens_nothing() -> TestResult {
    let events = [with_link("call", "140000", MEET_LOCATION)];
    let rig = Rig::new(&events, at(13, 50, 0))?;
    rig.tick_at(at(13, 58, 5)).await;
    let key = rig.key(0)?;
    rig.act(at(13, 58, 10), &key, Act::Join).await?;
    assert_eq!(rig.host().urls(), [MEET]);
    assert_eq!(rig.state_of("call", "lead")?.as_deref(), Some("joined"));
    assert_eq!(rig.host().count("close"), 1);

    let rig = Rig::new(&events, at(13, 50, 0))?;
    rig.tick_at(at(13, 58, 5)).await;
    let key = rig.key(0)?;
    rig.act(at(13, 58, 10), &key, Act::AlreadyThere).await?;
    assert!(rig.host().urls().is_empty());
    assert_eq!(rig.state_of("call", "lead")?.as_deref(), Some("joined"));
    assert_eq!(rig.host().count("close"), 1);
    Ok(())
}

#[tokio::test]
async fn s4_3_the_event_page_opens_when_there_is_no_call_link() -> TestResult {
    let page = "https://calendar.example.com/event?eid=1";
    let rig = Rig::new(
        &[with_link("page", "140000", &format!("URL:{page}\r\n"))],
        at(13, 50, 0),
    )?;
    rig.tick_at(at(13, 58, 5)).await;
    let key = rig.key(0)?;
    rig.act(at(13, 58, 10), &key, Act::Join).await?;
    assert_eq!(rig.host().urls(), [page]);
    Ok(())
}

#[tokio::test]
async fn s4_3_an_event_page_that_is_not_https_is_never_opened() -> TestResult {
    let rig = Rig::new(
        &[with_link("page", "140000", "URL:file:///etc/passwd\r\n")],
        at(13, 50, 0),
    )?;
    rig.tick_at(at(13, 58, 5)).await;
    let key = rig.key(0)?;
    rig.act(at(13, 58, 10), &key, Act::Join).await?;
    assert!(rig.host().urls().is_empty());
    assert_eq!(rig.state_of("page", "lead")?.as_deref(), Some("joined"));
    Ok(())
}

#[test]
fn only_plain_https_links_are_openable() {
    for good in [
        "https://meet.google.com/abc-defg-hij",
        "https://us02web.zoom.us/j/123456789?pwd=x",
        "https://example.com:8443/path#frag",
        // Other non-ASCII text is fine: only the format characters are refused.
        "https://example.com/caf\u{e9}",
    ] {
        assert!(openable(good), "{good}");
    }
    for bad in [
        "http://example.com/a",
        "file:///etc/passwd",
        "javascript:alert(1)",
        "https://",
        "https:///path",
        "https://user@example.com/a",
        "https://example.com\\@evil.com/a",
        "https://exa mple.com/a",
        "https://example.com/a\nb",
        "https://[::1]/a",
        "HTTPS://example.com/a",
        // Format characters hide where a link goes: zero-width, bidi override, BOM, soft hyphen, tags.
        "https://example.com/a\u{200B}b",
        "https://example.com/gpj.exe\u{202E}fdp",
        "https://example.com/?next=\u{2066}evil",
        "https://example.com/\u{FEFF}",
        "https://example.com/\u{00AD}",
        "https://example.com/\u{E0041}",
        "https://exa\u{200D}mple.com/",
    ] {
        assert!(!openable(bad), "{bad}");
    }
    assert!(!openable(&format!(
        "https://example.com/{}",
        "a".repeat(9000)
    )));
}

#[tokio::test]
async fn a_link_that_cannot_open_leaves_the_interrupt_and_the_alarm_alone() -> TestResult {
    let events = [with_link("call", "140000", MEET_LOCATION)];
    let rig = Rig::new(&events, at(13, 50, 0))?;
    rig.tick_at(at(13, 58, 5)).await;
    let key = rig.key(0)?;
    rig.host().fail_url.store(true, Ordering::SeqCst);
    let Err(e) = rig.act(at(13, 58, 10), &key, Act::Join).await else {
        anyhow::bail!("a link that cannot open must fail the command");
    };
    assert_eq!(e.downcast_ref::<AppError>(), Some(&AppError::INTERNAL));
    assert_eq!(rig.host().count("close"), 0);
    assert_eq!(rig.state_of("call", "lead")?.as_deref(), Some("firing"));
    Ok(())
}

#[tokio::test]
async fn snooze_minutes_are_clamped_to_1_through_60() -> TestResult {
    for (asked, minutes) in [(0, 1), (1, 1), (5, 5), (60, 60), (61, 60), (u32::MAX, 60)] {
        let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
        rig.tick_at(at(13, 58, 5)).await;
        let key = rig.key(0)?;
        rig.act(at(13, 59, 0), &key, Act::Snooze(asked)).await?;
        let until = DateTime::parse_from_rfc3339(
            &rig.until_of("standup")?
                .ok_or_else(|| anyhow!("not snoozed"))?,
        )?
        .with_timezone(&Utc);
        assert_eq!(
            until,
            at(13, 59, 0) + Duration::minutes(i64::from(minutes)),
            "asked for {asked}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn snooze_and_dismiss_cover_every_meeting_of_the_group() -> TestResult {
    for act in [Act::Snooze(5), Act::Dismiss] {
        let rig = Rig::new(
            &[("a", "Alpha", "140000", ""), ("b", "Beta", "140000", "")],
            at(13, 50, 0),
        )?;
        rig.tick_at(at(13, 58, 5)).await;
        // The key of either meeting stands for the group.
        let key = rig.key(1)?;
        rig.act(at(13, 58, 10), &key, act).await?;
        let want = if matches!(act, Act::Dismiss) {
            "dismissed"
        } else {
            "snoozed"
        };
        for uid in ["a", "b"] {
            assert_eq!(rig.state_of(uid, "lead")?.as_deref(), Some(want), "{uid}");
        }
        assert_eq!(rig.host().count("close"), 1);
    }
    Ok(())
}

#[tokio::test]
async fn a_dismissed_lead_alarm_still_lets_the_start_alarm_fire() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.setting(AGAIN_AT_START, serde_json::json!(true))?;
    rig.tick_at(at(13, 58, 5)).await;
    let key = rig.key(0)?;
    rig.act(at(13, 58, 10), &key, Act::Dismiss).await?;
    assert_eq!(
        rig.state_of("standup", "start")?.as_deref(),
        Some("scheduled")
    );
    assert_eq!(rig.tick_at(at(14, 0, 5)).await.fired, 1);
    assert_eq!(rig.host().count("open"), 2);
    Ok(())
}

#[tokio::test]
async fn an_unknown_key_gives_not_found() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    // Nothing is open yet.
    let Err(e) = rig
        .act(at(13, 50, 0), "x@2026-10-20T14:00:00Z", Act::Dismiss)
        .await
    else {
        anyhow::bail!("no interrupt is open");
    };
    assert_eq!(e.downcast_ref::<AppError>(), Some(&AppError::NOT_FOUND));

    rig.tick_at(at(13, 58, 5)).await;
    for key in ["x@2026-10-20T14:00:00Z", "not a key", ""] {
        for act in [Act::Snooze(5), Act::Dismiss, Act::Join, Act::AlreadyThere] {
            let Err(e) = rig.act(at(13, 58, 10), key, act).await else {
                anyhow::bail!("{key:?} is not in the open group");
            };
            assert_eq!(
                e.downcast_ref::<AppError>(),
                Some(&AppError::NOT_FOUND),
                "{key:?}"
            );
        }
    }
    assert_eq!(
        rig.host().count("close"),
        0,
        "a refused key changes nothing"
    );
    assert_eq!(rig.state_of("standup", "lead")?.as_deref(), Some("firing"));
    Ok(())
}

#[tokio::test]
async fn a_partial_join_reopens_the_interrupt_for_the_meetings_left() -> TestResult {
    let rig = Rig::new(
        &[
            with_link("a", "140000", MEET_LOCATION),
            ("b", "Beta", "140000", ""),
        ],
        at(13, 50, 0),
    )?;
    rig.tick_at(at(13, 58, 5)).await;
    let alpha = rig.key(0)?;
    let beta = rig.key(1)?;
    assert_eq!(rig.host().count("open"), 1);

    rig.act(at(13, 58, 10), &alpha, Act::Join).await?;
    assert_eq!(rig.host().urls(), [MEET]);
    assert_eq!(rig.host().count("close"), 0, "Beta is still firing");
    assert_eq!(rig.host().count("open"), 2, "re-opened");
    let show = rig.last_show()?;
    assert_eq!(show.meetings.len(), 1);
    assert_eq!(show.meetings[0].key, beta);
    assert_eq!(rig.state_of("a", "lead")?.as_deref(), Some("joined"));
    assert_eq!(rig.state_of("b", "lead")?.as_deref(), Some("firing"));
    // The joined key is no longer in the group.
    assert!(rig.act(at(13, 58, 11), &alpha, Act::Dismiss).await.is_err());

    rig.act(at(13, 58, 12), &beta, Act::AlreadyThere).await?;
    assert_eq!(rig.host().count("close"), 1);
    assert_eq!(rig.host().count("stop"), 1);
    Ok(())
}

#[tokio::test]
async fn joining_closes_the_start_alarm_of_the_same_meeting() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.setting(AGAIN_AT_START, serde_json::json!(true))?;
    rig.tick_at(at(13, 58, 5)).await;
    let key = rig.key(0)?;
    rig.act(at(13, 58, 10), &key, Act::AlreadyThere).await?;
    assert_eq!(rig.state_of("standup", "lead")?.as_deref(), Some("joined"));
    assert_eq!(rig.state_of("standup", "start")?.as_deref(), Some("joined"));
    assert_eq!(rig.tick_at(at(14, 0, 5)).await.fired, 0);
    Ok(())
}

// ---- F4.5 --------------------------------------------------------------------------------------

#[tokio::test]
async fn focus_no_triggers_the_fallback() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.host().focused.store(false, Ordering::SeqCst);
    rig.tick_at(at(13, 58, 5)).await;
    assert_eq!(rig.host().count("notify"), 0, "not before the re-check");
    // Too early: 400 ms after the open.
    rig.host()
        .set_now(at(13, 58, 5) + Duration::milliseconds(400));
    rig.sched.step(&rig.db, false).await;
    assert_eq!(rig.host().count("notify"), 0);
    rig.housekeep_at(at(13, 58, 5) + Duration::milliseconds(500))
        .await;
    assert_eq!(rig.host().count("notify"), 1);
    // Once per interrupt.
    rig.housekeep_at(at(13, 58, 7)).await;
    assert_eq!(rig.host().count("notify"), 1);
    Ok(())
}

#[tokio::test]
async fn a_window_that_has_focus_needs_no_fallback() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.tick_at(at(13, 58, 5)).await;
    rig.housekeep_at(at(13, 58, 6)).await;
    assert_eq!(rig.host().count("focused"), 1, "the check ran");
    assert_eq!(rig.host().count("notify"), 0);
    Ok(())
}

#[tokio::test]
async fn degraded_takeover_triggers_the_fallback_with_the_windows_and_the_sound() -> TestResult {
    let mut host = Fake::new(at(13, 50, 0));
    host.degraded = Some("wayland_no_takeover");
    let rig = Rig::with_host(&[STANDUP], host)?;
    rig.tick_at(at(13, 58, 5)).await;
    assert_eq!(rig.host().calls(), ["open", "notify", "play"]);
    // The re-check finds nothing left to do.
    rig.housekeep_at(at(13, 58, 6)).await;
    assert_eq!(rig.host().count("notify"), 1);
    Ok(())
}

#[tokio::test]
async fn windows_that_cannot_open_leave_a_notification_whose_actions_work() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.host().fail_open.store(true, Ordering::SeqCst);
    rig.tick_at(at(13, 58, 5)).await;
    assert_eq!(rig.host().calls(), ["open", "notify", "play"]);
    // The notification's Join and Snooze act on the first meeting.
    let notice = *rig
        .host()
        .notices()
        .last()
        .ok_or_else(|| anyhow!("the notification is the interrupt"))?;
    rig.act_on_notice(at(13, 58, 10), notice, Act::Snooze(SNOOZE_MINUTES))
        .await?;
    assert_eq!(rig.state_of("standup", "lead")?.as_deref(), Some("snoozed"));
    assert_eq!(rig.host().count("stop"), 1);
    Ok(())
}

fn degraded_host() -> Fake {
    let mut host = Fake::new(at(13, 50, 0));
    host.degraded = Some("wayland_no_takeover");
    host
}

#[tokio::test]
async fn s4_4_a_notification_action_acts_on_the_meeting_it_announced() -> TestResult {
    let rig = Rig::with_host(&[STANDUP], degraded_host())?;
    rig.tick_at(at(13, 58, 5)).await;
    let [notice] = rig.host().notices()[..] else {
        anyhow::bail!("one notification expected: {:?}", rig.host().notices());
    };
    rig.act_on_notice(at(13, 58, 10), notice, Act::Snooze(SNOOZE_MINUTES))
        .await?;
    assert_eq!(rig.state_of("standup", "lead")?.as_deref(), Some("snoozed"));
    assert_eq!(rig.host().count("close"), 1);
    Ok(())
}

#[tokio::test]
async fn s4_4_a_stale_notification_action_is_a_no_op() -> TestResult {
    let rig = Rig::with_host(
        &[("a", "Alpha", "140000", ""), ("b", "Beta", "140500", "")],
        degraded_host(),
    )?;
    rig.tick_at(at(13, 58, 5)).await;
    let old = rig.host().notices()[0];
    let alpha = rig.key(0)?;
    rig.act(at(13, 58, 10), &alpha, Act::Dismiss).await?;
    // Beta is next on screen, with a notification of its own.
    rig.tick_at(at(14, 3, 5)).await;
    let current = *rig.host().notices().last().unwrap_or(&0);
    assert_ne!(current, old);
    assert_eq!(rig.state_of("b", "lead")?.as_deref(), Some("firing"));

    // Alpha's notification, still on screen, must not touch Beta; neither must an unknown id.
    for stale in [old, current + 1, i32::MIN] {
        rig.act_on_notice(at(14, 3, 10), stale, Act::Join).await?;
        rig.act_on_notice(at(14, 3, 10), stale, Act::Snooze(5))
            .await?;
    }
    assert_eq!(rig.state_of("b", "lead")?.as_deref(), Some("firing"));
    assert_eq!(rig.host().count("close"), 1, "only Alpha's dismiss closed");
    assert!(rig.host().urls().is_empty());

    rig.act_on_notice(at(14, 3, 20), current, Act::Join).await?;
    assert_eq!(rig.state_of("b", "lead")?.as_deref(), Some("joined"));
    Ok(())
}

#[tokio::test]
async fn ac3_5_a_degraded_system_announces_a_meeting_that_joins_the_open_minute() -> TestResult {
    let rig = Rig::with_host(
        &[("a", "Alpha", "140000", ""), ("b", "Beta", "140030", "")],
        degraded_host(),
    )?;
    rig.tick_at(at(13, 58, 5)).await;
    assert_eq!(rig.host().count("notify"), 1);
    rig.tick_at(at(13, 58, 35)).await;
    assert_eq!(
        rig.host().count("notify"),
        2,
        "the notification lists both now"
    );
    let [first, second] = rig.host().notices()[..] else {
        anyhow::bail!("two notifications expected");
    };
    // The first one is stale: its meetings changed.
    rig.act_on_notice(at(13, 58, 40), first, Act::Snooze(5))
        .await?;
    assert_eq!(rig.host().count("close"), 0);
    rig.act_on_notice(at(13, 58, 41), second, Act::Snooze(5))
        .await?;
    assert_eq!(rig.host().count("close"), 1);
    Ok(())
}

#[tokio::test]
async fn s4_4_a_stale_notification_action_after_a_regroup_does_not_touch_the_larger_group(
) -> TestResult {
    let rig = Rig::with_host(
        &[("a", "Alpha", "140000", ""), ("b", "Beta", "140030", "")],
        degraded_host(),
    )?;
    rig.tick_at(at(13, 58, 5)).await;
    let first = rig.host().notices()[0];
    // Beta joins the open minute: the group is larger and the first notification is stale.
    rig.tick_at(at(13, 58, 35)).await;
    assert_eq!(rig.last_show()?.meetings.len(), 2);
    rig.act_on_notice(at(13, 58, 40), first, Act::Snooze(5))
        .await?;
    for uid in ["a", "b"] {
        assert_eq!(
            rig.state_of(uid, "lead")?.as_deref(),
            Some("firing"),
            "{uid}"
        );
    }
    assert_eq!(rig.host().count("close"), 0);
    Ok(())
}

#[tokio::test]
async fn notification_ids_differ_between_runs() -> TestResult {
    // Two starts of the app, a minute apart: the first notification of each gets another id.
    let mut first_ids = Vec::new();
    for start in [at(13, 40, 0), at(13, 41, 0)] {
        let mut host = Fake::new(start);
        host.degraded = Some("wayland_no_takeover");
        let rig = Rig::with_host(&[STANDUP], host)?;
        rig.tick_at(at(13, 58, 5)).await;
        first_ids.extend(rig.host().notices());
    }
    let [one, two] = first_ids[..] else {
        anyhow::bail!("two notifications expected: {first_ids:?}");
    };
    assert_ne!(one, two);
    assert!(one > 0 && two > 0);
    assert_eq!(two, one + 60, "seeded from the start time in seconds");
    Ok(())
}

#[tokio::test]
async fn a_regroup_checks_focus_again() -> TestResult {
    let rig = Rig::new(
        &[("a", "Alpha", "140000", ""), ("b", "Beta", "140030", "")],
        at(13, 50, 0),
    )?;
    rig.tick_at(at(13, 58, 5)).await;
    rig.housekeep_at(at(13, 58, 6)).await;
    assert_eq!(rig.host().count("focused"), 1);
    // Beta joins; the windows are updated, and the check starts over.
    rig.tick_at(at(13, 58, 35)).await;
    rig.host().focused.store(false, Ordering::SeqCst);
    rig.housekeep_at(at(13, 58, 35) + Duration::milliseconds(499))
        .await;
    assert_eq!(rig.host().count("notify"), 0);
    rig.housekeep_at(at(13, 58, 35) + Duration::milliseconds(500))
        .await;
    assert_eq!(rig.host().count("focused"), 2);
    assert_eq!(rig.host().count("notify"), 1);
    Ok(())
}

#[tokio::test]
async fn a_partial_join_checks_focus_again() -> TestResult {
    let rig = Rig::new(
        &[("a", "Alpha", "140000", ""), ("b", "Beta", "140000", "")],
        at(13, 50, 0),
    )?;
    rig.tick_at(at(13, 58, 5)).await;
    rig.housekeep_at(at(13, 58, 6)).await;
    let alpha = rig.key(0)?;
    rig.act(at(13, 58, 10), &alpha, Act::AlreadyThere).await?;
    rig.host().focused.store(false, Ordering::SeqCst);
    assert_eq!(
        rig.sched.deadline().await,
        Some(at(13, 58, 10) + FOCUS_RECHECK)
    );
    rig.housekeep_at(at(13, 58, 10) + FOCUS_RECHECK).await;
    assert_eq!(rig.host().count("notify"), 1);
    Ok(())
}

// ---- F6 ----------------------------------------------------------------------------------------

#[tokio::test]
async fn ac6_1_the_sound_plays_on_open_repeats_every_30_s_and_stops_on_close() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.tick_at(at(13, 58, 0)).await;
    assert_eq!(rig.host().count("play"), 1, "once when the interrupt opens");
    rig.housekeep_at(at(13, 58, 29)).await;
    assert_eq!(rig.host().count("play"), 1);
    rig.housekeep_at(at(13, 58, 30)).await;
    assert_eq!(rig.host().count("play"), 2);
    rig.housekeep_at(at(13, 58, 59)).await;
    assert_eq!(rig.host().count("play"), 2);
    rig.housekeep_at(at(13, 59, 0)).await;
    assert_eq!(rig.host().count("play"), 3);

    let key = rig.key(0)?;
    rig.act(at(13, 59, 1), &key, Act::Dismiss).await?;
    assert_eq!(rig.host().count("stop"), 1, "stopped on close");
    rig.housekeep_at(at(13, 59, 40)).await;
    assert_eq!(rig.host().count("play"), 3, "no repeat after the close");
    Ok(())
}

#[tokio::test]
async fn ac6_1_the_repeat_can_be_turned_off() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.setting(SOUND_REPEAT, serde_json::json!(false))?;
    rig.tick_at(at(13, 58, 0)).await;
    rig.housekeep_at(at(14, 0, 0)).await;
    assert_eq!(rig.host().count("play"), 1);
    Ok(())
}

/// True when a `wake` is waiting for the loop (the permit is consumed).
async fn woken(sched: &Scheduler<Fake>) -> bool {
    tokio::time::timeout(std::time::Duration::from_millis(20), sched.wake.notified())
        .await
        .is_ok()
}

#[tokio::test]
async fn the_test_alarm_and_every_action_nudge_the_loop_for_their_new_deadlines() -> TestResult {
    let rig = Rig::new(
        &[("a", "Alpha", "140000", ""), ("b", "Beta", "140500", "")],
        at(13, 50, 0),
    )?;
    assert!(!woken(&rig.sched).await);
    rig.sched.show_test().await?;
    assert!(
        woken(&rig.sched).await,
        "the test alarm has a focus check to run"
    );
    let key = rig.key(0)?;
    rig.act(at(13, 50, 5), &key, Act::Dismiss).await?;
    assert!(woken(&rig.sched).await);

    // A waiting minute opened by a close needs its own check on time.
    rig.tick_at(at(13, 58, 5)).await;
    rig.tick_at(at(14, 3, 5)).await;
    assert!(!woken(&rig.sched).await, "a tick does not nudge itself");
    let alpha = rig.key(0)?;
    rig.act(at(14, 3, 10), &alpha, Act::Dismiss).await?;
    assert!(woken(&rig.sched).await);
    assert_eq!(
        rig.sched.deadline().await,
        Some(at(14, 3, 10) + FOCUS_RECHECK)
    );
    Ok(())
}

#[tokio::test]
async fn the_loop_wakes_for_the_focus_check_and_the_next_repeat() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    assert_eq!(rig.sched.deadline().await, None, "nothing is open");
    rig.tick_at(at(13, 58, 0)).await;
    assert_eq!(
        rig.sched.deadline().await,
        Some(at(13, 58, 0) + FOCUS_RECHECK)
    );
    rig.housekeep_at(at(13, 58, 1)).await;
    assert_eq!(rig.sched.deadline().await, Some(at(13, 58, 30)));
    Ok(())
}

// ---- F10 ---------------------------------------------------------------------------------------

#[tokio::test]
async fn the_test_alarm_opens_a_sample_that_every_action_can_close() -> TestResult {
    let rig = Rig::new(&[], at(13, 50, 0))?;
    rig.sched.show_test().await?;
    assert!(rig.last_show()?.test);
    assert_eq!(rig.host().count("play"), 1);
    let key = rig.key(0)?;
    rig.act(at(13, 50, 5), &key, Act::Join).await?;
    assert!(rig.host().urls().is_empty(), "the sample has no link");
    assert_eq!(rig.host().count("close"), 1);
    assert!(rig.states()?.is_empty(), "no alarm row behind it");
    Ok(())
}

#[tokio::test]
async fn ac10_1_a_second_test_alarm_while_one_is_up_opens_nothing() -> TestResult {
    let rig = Rig::new(&[], at(13, 50, 0))?;
    rig.sched.show_test().await?;
    rig.sched.show_test().await?;
    assert_eq!(rig.host().count("open"), 1);
    assert_eq!(rig.host().count("play"), 1);
    // Once it is closed, the next one opens.
    let key = rig.key(0)?;
    rig.act(at(13, 50, 5), &key, Act::Dismiss).await?;
    rig.sched.show_test().await?;
    assert_eq!(rig.host().count("open"), 2);
    Ok(())
}

#[tokio::test]
async fn a_real_interrupt_is_not_replaced_by_the_test_alarm_but_replaces_it() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.tick_at(at(13, 58, 5)).await;
    rig.sched.show_test().await?;
    assert_eq!(rig.host().count("open"), 1, "the test alarm stood back");

    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.host().set_now(at(13, 57, 0));
    rig.sched.show_test().await?;
    rig.tick_at(at(13, 58, 5)).await;
    assert_eq!(rig.last_show()?.meetings[0].title, "Standup");
    Ok(())
}

// ---- Persistence, pins -------------------------------------------------------------------------

#[test]
fn alarm_rows_round_trip_through_every_state() -> TestResult {
    let occurrence: OccurrenceKey = "cal:evt@2026-10-20T14:00:00Z".parse()?;
    for state in [
        AlarmState::Scheduled,
        AlarmState::Firing {
            since: at(13, 58, 0),
        },
        AlarmState::Snoozed {
            until: at(14, 3, 0),
        },
        AlarmState::Dismissed,
        AlarmState::Joined,
        AlarmState::Expired,
    ] {
        let alarm = Alarm {
            occurrence: occurrence.clone(),
            at: at(13, 58, 0),
            start: at(14, 0, 0),
            kind: AlarmKind::Lead,
            state,
        };
        assert_eq!(from_row(&to_row(&alarm, at(13, 59, 0))), Some(alarm));
    }
    let broken = AlarmRow {
        occurrence_key: "no key".into(),
        kind: "lead".into(),
        at: "x".into(),
        start: "x".into(),
        state: "firing".into(),
        state_at: "x".into(),
        until: None,
    };
    assert_eq!(from_row(&broken), None);
    Ok(())
}

#[test]
fn the_stores_terminal_states_are_the_cores_final_states() {
    let core: Vec<&str> = [
        AlarmState::Dismissed,
        AlarmState::Joined,
        AlarmState::Expired,
    ]
    .iter()
    .map(AlarmState::name)
    .collect();
    assert_eq!(core, TERMINAL_STATES);
}

#[test]
fn ac3_2_the_stores_prune_uses_the_cores_expiry_minutes() -> TestResult {
    let store = Store::open_in_memory()?;
    let start = at(14, 0, 0);
    let row = |key: &str| AlarmRow {
        occurrence_key: key.into(),
        kind: "lead".into(),
        at: ts(start - Duration::minutes(2)),
        start: ts(start),
        state: "dismissed".into(),
        state_at: ts(start),
        until: None,
    };
    store.alarms().upsert(&row("k"))?;
    let expiry = start + Duration::minutes(EXPIRY_MINUTES);
    // One second before the core's expiry the row stays; one second after, it goes.
    assert_eq!(
        store
            .alarms()
            .prune_before(&ts(expiry - Duration::seconds(1)))?,
        0
    );
    assert_eq!(
        store.alarms().prune_before(&ts(expiry))?,
        0,
        "the cutoff is exclusive"
    );
    assert_eq!(
        store
            .alarms()
            .prune_before(&ts(expiry + Duration::seconds(1)))?,
        1
    );
    Ok(())
}

#[test]
fn lead_time_limits_are_the_cores() {
    assert_eq!((MIN_LEAD_MINUTES, MAX_LEAD_MINUTES), (1, 60));
}

// ---- Capabilities ------------------------------------------------------------------------------

#[test]
fn no_webview_capability_grants_opener_or_notification_commands() -> TestResult {
    for (name, text) in [
        ("default", include_str!("../../capabilities/default.json")),
        (
            "interrupt",
            include_str!("../../capabilities/interrupt.json"),
        ),
    ] {
        let json: serde_json::Value = serde_json::from_str(text)?;
        let permissions: Vec<&str> = json["permissions"]
            .as_array()
            .ok_or_else(|| anyhow!("{name} has no permissions"))?
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        assert_eq!(
            permissions,
            ["core:event:allow-listen", "core:event:allow-unlisten"],
            "{name}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn a_meeting_that_leaves_the_feed_takes_its_alarm_and_the_interrupt_with_it() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.tick_at(at(13, 58, 5)).await;
    assert_eq!(rig.host().count("open"), 1);
    {
        let store = pipeline::lock(&rig.db);
        let cal = store
            .calendars()
            .list()?
            .pop()
            .ok_or_else(|| anyhow!("no calendar"))?;
        store.events().replace_for_calendar(&cal.id, &[], "t2")?;
    }
    rig.tick_at(at(13, 58, 20)).await;
    assert!(rig.states()?.is_empty(), "{:?}", rig.states()?);
    assert_eq!(rig.host().count("close"), 1);
    Ok(())
}

#[test]
fn the_opener_plugin_is_never_registered() -> TestResult {
    // `open_url` is called from Rust; registering the plugin would add commands to the webview.
    let main = include_str!("../main.rs");
    assert!(
        !main.contains("tauri_plugin_opener"),
        "register nothing from the opener plugin"
    );
    Ok(())
}

// ---- F8 quiet hours and rules, F6 preview (L-016) ------------------------------------------------

/// An all-day event today, 20 Oct 2026. With the machine on UTC and quiet hours off, its one
/// alarm is at the 09:00 anchor and its window stays open until 09:10.
const ALL_DAY_TODAY: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:holiday\r\nSUMMARY:Holiday\r\nDTSTART;VALUE=DATE:20261020\r\nDTEND;VALUE=DATE:20261021\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

/// Runs the real loop, lets its first tick pass, saves `patch` the way `settings_set` does (save,
/// then wake), and waits for `uid`'s lead alarm to reach `state`. The fake clock stands still and
/// the loop's next timed tick is 15 s of real time away, so only the wake can re-plan in time.
async fn after_a_settings_change(
    rig: &Rig,
    patch: serde_json::Value,
    uid: &str,
    state: &str,
) -> anyhow::Result<bool> {
    let changed = async {
        tokio::time::sleep(StdDuration::from_millis(200)).await;
        settings::change(&rig.db, &patch, |_| {}, || rig.sched.wake())?;
        for _ in 0..50 {
            if rig.state_of(uid, "lead")?.as_deref() == Some(state) {
                return Ok(true);
            }
            tokio::time::sleep(StdDuration::from_millis(50)).await;
        }
        Ok(false)
    };
    tokio::select! {
        () = rig.sched.run_loop(&rig.db) => Err(anyhow!("the loop ended")),
        seen = changed => seen,
    }
}

#[tokio::test]
async fn s8_2_turning_ignore_all_day_off_makes_its_alarm_appear_within_15_s() -> TestResult {
    // At any hour before the anchor the alarm appears planned; at the anchor it fires.
    for hour in [0, 6, 8] {
        let rig = Rig::from_feed(ALL_DAY_TODAY, at(hour, 59, 0))?;
        assert_eq!(
            rig.tick_at(at(hour, 59, 0)).await.fired,
            0,
            "all-day is ignored by default"
        );
        assert!(rig.states()?.is_empty());
        let patch = serde_json::json!({ "ignore": { "all_day": false } });
        assert!(
            after_a_settings_change(&rig, patch, "holiday", "scheduled").await?,
            "{hour}:59"
        );
        assert_eq!(rig.host().count("open"), 0, "nothing before the anchor");
        assert_eq!(rig.tick_at(at(9, 0, 5)).await.fired, 1);
    }
    Ok(())
}

#[tokio::test]
async fn all_day_alarms_anchor_at_the_end_of_quiet_hours() -> TestResult {
    let rig = Rig::from_feed(ALL_DAY_TODAY, at(0, 0, 0))?;
    settings::apply(
        &rig.db,
        &serde_json::json!({ "ignore": { "all_day": false },
                             "quiet": { "enabled": true, "start": "20:00", "end": "07:30" } }),
    )?;
    assert_eq!(rig.tick_at(at(7, 29, 50)).await.fired, 0);
    assert_eq!(rig.tick_at(at(7, 30, 5)).await.fired, 1);
    Ok(())
}

#[tokio::test]
async fn ac8_3_a_keyword_change_replans_without_a_restart() -> TestResult {
    let rig = Rig::new(&[("f", "Focus time", "140000", "")], at(13, 59, 0))?;
    assert_eq!(rig.tick_at(at(13, 59, 0)).await.fired, 0);
    let patch = serde_json::json!({ "ignore": { "keywords": ["Lunch", "OOO"] } });
    assert!(after_a_settings_change(&rig, patch, "f", "firing").await?);
    Ok(())
}

#[tokio::test]
async fn ac8_3_turning_a_rule_on_drops_a_planned_alarm() -> TestResult {
    let rig = Rig::new(&[("solo", "Write-up", "140000", "")], at(13, 0, 0))?;
    rig.tick_at(at(13, 0, 0)).await;
    assert_eq!(rig.state_of("solo", "lead")?.as_deref(), Some("scheduled"));
    settings::apply(
        &rig.db,
        &serde_json::json!({ "ignore": { "no_attendees": true } }),
    )?;
    rig.tick_at(at(13, 0, 15)).await;
    assert!(rig.states()?.is_empty());
    Ok(())
}

#[tokio::test]
async fn ac8_1_quiet_hours_hold_everything_and_keep_the_plan() -> TestResult {
    let rig = Rig::new(
        &[
            ("inside", "Review", "140000", ""),
            ("after", "Retro", "150000", ""),
        ],
        at(13, 0, 0),
    )?;
    settings::apply(
        &rig.db,
        &serde_json::json!({ "quiet": { "enabled": true, "start": "13:00", "end": "15:00" } }),
    )?;
    // 14:00 starts inside quiet hours: no interrupt, no notification, no sound; still planned.
    assert_eq!(rig.tick_at(at(13, 58, 5)).await.fired, 0);
    assert_eq!(
        rig.state_of("inside", "lead")?.as_deref(),
        Some("scheduled")
    );
    // 15:00 starts after them: its lead time 14:58 is quiet, so it fires at the start.
    assert_eq!(rig.tick_at(at(14, 58, 5)).await.fired, 0);
    assert_eq!(rig.state_of("after", "lead")?.as_deref(), Some("scheduled"));
    assert_eq!(rig.state_of("inside", "lead")?.as_deref(), Some("expired"));
    for call in ["open", "notify", "play"] {
        assert_eq!(rig.host().count(call), 0, "{call}");
    }
    assert_eq!(rig.tick_at(at(15, 0, 5)).await.fired, 1);
    assert_eq!(rig.state_of("after", "lead")?.as_deref(), Some("firing"));
    assert_eq!(rig.host().count("open"), 1);
    Ok(())
}

#[tokio::test]
async fn ac8_1_quiet_hours_are_off_by_default() -> TestResult {
    let rig = Rig::new(&[("night", "Late call", "220000", "")], at(21, 0, 0))?;
    assert_eq!(rig.tick_at(at(21, 58, 5)).await.fired, 1);
    Ok(())
}

#[tokio::test]
async fn ac6_3_the_preview_plays_once_and_opens_nothing() -> TestResult {
    let rig = Rig::new(&[], at(13, 0, 0))?;
    assert!(rig.sched.preview(Sound::Chime).await);
    assert_eq!(rig.host().calls(), ["play"]);
    Ok(())
}

/// A preview at `t` on the rig's clock.
async fn preview_at(rig: &Rig, t: DateTime<Utc>, sound: Sound) -> bool {
    rig.host().set_now(t);
    rig.sched.preview(sound).await
}

#[tokio::test]
async fn ac6_3_a_preview_is_dropped_while_one_plays_and_within_300_ms() -> TestResult {
    let rig = Rig::new(&[], at(13, 0, 0))?;
    let t = at(13, 0, 0);
    let ms = Duration::milliseconds;
    assert!(preview_at(&rig, t, Sound::SoftBell).await);
    // The soft bell plays 1.5 s: anything before that is dropped, whatever sound it asks for.
    assert!(!preview_at(&rig, t + ms(299), Sound::Chime).await);
    assert!(!preview_at(&rig, t + ms(1499), Sound::None).await);
    assert!(preview_at(&rig, t + ms(1500), Sound::None).await);
    // "None" plays nothing, so only the 300 ms gap holds the next one back.
    assert!(!preview_at(&rig, t + ms(1799), Sound::Chime).await);
    assert!(preview_at(&rig, t + ms(1800), Sound::Chime).await);
    assert!(!preview_at(&rig, t + ms(2799), Sound::Chime).await);
    assert_eq!(rig.host().count("play"), 3);
    Ok(())
}

#[tokio::test]
async fn ac6_3_a_preview_is_ignored_while_an_interrupt_is_open() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.tick_at(at(13, 58, 5)).await;
    assert_eq!(rig.host().count("open"), 1);
    let plays = rig.host().count("play");
    assert!(!preview_at(&rig, at(13, 58, 10), Sound::Chime).await);
    assert_eq!(rig.host().count("play"), plays);
    Ok(())
}

// ---- occurrences_join (decision "All-day events, weekends and window checks") -----------------

/// The key of today's meeting `uid` as `occurrences_today` lists it.
fn listed_key(rig: &Rig, uid: &str) -> anyhow::Result<String> {
    pipeline::today(&rig.db, rig.host().now(), Tz::UTC)?
        .into_iter()
        .find(|l| l.key.contains(&format!(":{uid}@")))
        .map(|l| l.key)
        .ok_or_else(|| anyhow!("{uid} is not listed today"))
}

#[tokio::test]
async fn occurrences_join_opens_the_link_of_a_listed_meeting_and_changes_no_alarm() -> TestResult {
    let rig = Rig::new(&[with_link("call", "140000", MEET_LOCATION)], at(9, 0, 0))?;
    rig.tick_at(at(9, 0, 0)).await;
    let key = listed_key(&rig, "call")?;
    rig.sched.join_listed(&rig.db, &key)?;
    assert_eq!(rig.host().urls(), [MEET]);
    assert_eq!(rig.state_of("call", "lead")?.as_deref(), Some("scheduled"));
    Ok(())
}

#[tokio::test]
async fn occurrences_join_with_an_unknown_key_does_nothing() -> TestResult {
    let rig = Rig::new(&[with_link("call", "140000", MEET_LOCATION)], at(9, 0, 0))?;
    let key = listed_key(&rig, "call")?;
    let other_day = key.replace("2026-10-20", "2026-10-21");
    for bad in [
        other_day.as_str(),
        "not a key",
        "x@2026-10-20T14:00:00Z",
        "",
    ] {
        let e = rig
            .sched
            .join_listed(&rig.db, bad)
            .err()
            .ok_or_else(|| anyhow!("{bad:?} joined"))?;
        assert_eq!(
            crate::errors::command_error(&e).code,
            "not_found",
            "{bad:?}"
        );
    }
    assert!(rig.host().urls().is_empty());
    assert!(rig.host().calls().is_empty());
    Ok(())
}

#[tokio::test]
async fn occurrences_join_re_checks_the_link_before_opening() -> TestResult {
    let rig = Rig::new(
        &[
            with_link("file", "100000", "URL:file:///etc/passwd\r\n"),
            with_link("zw", "110000", "URL:https://exa\u{200B}mple.com/e\r\n"),
            with_link(
                "page",
                "120000",
                "URL:https://calendar.example.com/e?eid=1\r\n",
            ),
            ("none", "Nothing", "130000", ""),
        ],
        at(9, 0, 0),
    )?;
    // The core keeps no file: link at all; the zero-width one reaches the re-check and fails it.
    for (uid, code) in [("file", "not_found"), ("zw", "internal")] {
        let e = rig
            .sched
            .join_listed(&rig.db, &listed_key(&rig, uid)?)
            .err()
            .ok_or_else(|| anyhow!("{uid} opened"))?;
        assert_eq!(crate::errors::command_error(&e).code, code, "{uid}");
    }
    let e = rig
        .sched
        .join_listed(&rig.db, &listed_key(&rig, "none")?)
        .err()
        .ok_or_else(|| anyhow!("nothing opened"))?;
    assert_eq!(crate::errors::command_error(&e).code, "not_found");
    assert!(rig.host().urls().is_empty());
    rig.sched.join_listed(&rig.db, &listed_key(&rig, "page")?)?;
    assert_eq!(rig.host().urls(), ["https://calendar.example.com/e?eid=1"]);
    Ok(())
}

// ---- settings::change: announce under the lock, then wake ----------------------------------------

#[tokio::test]
async fn ac9_1_a_change_announces_then_wakes_and_a_refused_one_does_neither() -> TestResult {
    let rig = Rig::new(&[], at(13, 0, 0))?;
    let steps = StdMutex::new(Vec::new());
    let push = |s: String| steps.lock().unwrap_or_else(PoisonError::into_inner).push(s);
    let dto = settings::change(
        &rig.db,
        &serde_json::json!({ "lead_minutes": 7 }),
        |dto| push(format!("announce {}", dto.lead_minutes)),
        || push("wake".to_owned()),
    )?;
    assert_eq!(dto.lead_minutes, 7);
    let refused = settings::change(
        &rig.db,
        &serde_json::json!({ "lead_minutes": 0 }),
        |_| push("announce refused".to_owned()),
        || push("wake refused".to_owned()),
    );
    assert!(refused.is_err());
    assert_eq!(
        *steps.lock().unwrap_or_else(PoisonError::into_inner),
        ["announce 7", "wake"]
    );
    Ok(())
}

#[test]
fn ac9_1_concurrent_changes_are_announced_in_the_order_they_were_saved() -> TestResult {
    for _ in 0..20 {
        let db: Db = StdMutex::new(Store::open_in_memory()?);
        let announced = StdMutex::new(Vec::new());
        std::thread::scope(|scope| {
            for lead in 1..=8u32 {
                let (db, announced) = (&db, &announced);
                scope.spawn(move || {
                    let _ = settings::change(
                        db,
                        &serde_json::json!({ "lead_minutes": lead }),
                        |dto| {
                            announced
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .push(dto.lead_minutes);
                        },
                        || {},
                    );
                });
            }
        });
        let announced = announced
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner);
        assert_eq!(announced.len(), 8);
        // The last announcement is what the store holds: no older change was announced late.
        assert_eq!(
            announced.last().copied(),
            Some(settings::get(&db)?.lead_minutes)
        );
    }
    Ok(())
}

// ---- F7 tray: pause, panel actions, quit (L-015) ----------------------------------------------

/// Pauses alarms from `t` for `minutes`, as `app_pause` does.
fn pause_at(rig: &Rig, t: DateTime<Utc>, minutes: u32) -> anyhow::Result<DateTime<Utc>> {
    settings::pause(&rig.db, minutes, t)
}

#[tokio::test]
async fn s7_1_pause_for_an_hour_keeps_a_meeting_in_20_minutes_quiet() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 40, 0))?;
    rig.tick_at(at(13, 40, 0)).await;
    let until = pause_at(&rig, at(13, 40, 0), 60)?;
    assert_eq!(until, at(14, 40, 0));
    let mut t = at(13, 40, 15);
    while t <= at(14, 45, 0) {
        assert_eq!(rig.tick_at(t).await.fired, 0, "fired at {t}");
        // The meeting's window closed at 14:10, inside the pause: it expired silently.
        if t == at(14, 10, 0) {
            assert_eq!(rig.state_of("standup", "lead")?.as_deref(), Some("expired"));
        }
        t += Duration::seconds(15);
    }
    assert_eq!(rig.host().count("open"), 0);
    // The tray heard: not paused at the first tick, paused, then resumed at 14:40 by itself.
    assert_eq!(rig.host().pauses(), [None, Some(until), None]);
    Ok(())
}

#[tokio::test]
async fn ac7_3_an_alarm_held_by_the_pause_fires_when_it_ends_inside_the_window() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    pause_at(&rig, at(13, 50, 0), 13)?;
    // Due at 13:58, held back until 14:03.
    assert_eq!(rig.tick_at(at(13, 58, 5)).await.fired, 0);
    assert_eq!(rig.tick_at(at(14, 2, 55)).await.fired, 0);
    assert_eq!(rig.tick_at(at(14, 3, 0)).await.fired, 1);
    assert_eq!(rig.state_of("standup", "lead")?.as_deref(), Some("firing"));
    Ok(())
}

#[tokio::test]
async fn ac7_3_resume_lets_a_held_alarm_fire_at_once_and_a_restart_keeps_the_pause() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    pause_at(&rig, at(13, 50, 0), 60)?;
    assert_eq!(rig.tick_at(at(13, 58, 5)).await.fired, 0);
    // A new process reads the pause from the store.
    let restarted = Scheduler::new(Fake::new(at(13, 59, 0)));
    assert_eq!(restarted.step(&rig.db, true).await.fired, 0);
    assert_eq!(restarted.host.pauses(), [Some(at(14, 50, 0))]);
    settings::resume(&rig.db)?;
    restarted.host.set_now(at(13, 59, 10));
    assert_eq!(restarted.step(&rig.db, true).await.fired, 1);
    assert_eq!(restarted.host.pauses(), [Some(at(14, 50, 0)), None]);
    Ok(())
}

#[tokio::test]
async fn ac7_2_panel_snooze_ahead_moves_the_alarm_later() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 40, 0))?;
    rig.tick_at(at(13, 40, 0)).await;
    let key = listed_key(&rig, "standup")?;
    rig.host().set_now(at(13, 45, 0));
    rig.sched.act_listed(&rig.db, &key, Act::Snooze(5)).await?;
    assert_eq!(rig.state_of("standup", "lead")?.as_deref(), Some("snoozed"));
    // Due at 13:58, so it comes at 14:03, not at 13:50.
    assert_eq!(
        rig.until_of("standup")?.as_deref(),
        Some("2026-10-20T14:03:00Z")
    );
    assert_eq!(rig.tick_at(at(13, 58, 5)).await.fired, 0);
    assert_eq!(rig.tick_at(at(14, 3, 5)).await.fired, 1);
    Ok(())
}

#[tokio::test]
async fn ac7_2_panel_dismiss_ahead_silences_every_alarm_of_the_meeting() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 40, 0))?;
    rig.setting(AGAIN_AT_START, serde_json::json!(true))?;
    rig.tick_at(at(13, 40, 0)).await;
    let key = listed_key(&rig, "standup")?;
    rig.sched.act_listed(&rig.db, &key, Act::Dismiss).await?;
    for kind in ["lead", "start"] {
        assert_eq!(
            rig.state_of("standup", kind)?.as_deref(),
            Some("dismissed"),
            "{kind}"
        );
    }
    for t in [at(13, 58, 5), at(14, 0, 5)] {
        assert_eq!(rig.tick_at(t).await.fired, 0);
    }
    Ok(())
}

#[tokio::test]
async fn ac7_2_panel_actions_on_the_meeting_on_screen_are_the_interrupt_buttons() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.tick_at(at(13, 58, 5)).await;
    let key = listed_key(&rig, "standup")?;
    rig.host().set_now(at(13, 58, 10));
    rig.sched.act_listed(&rig.db, &key, Act::Snooze(5)).await?;
    assert_eq!(rig.host().count("close"), 1, "the interrupt hides");
    assert_eq!(
        rig.until_of("standup")?.as_deref(),
        Some("2026-10-20T14:03:10Z")
    );
    Ok(())
}

#[tokio::test]
async fn ac7_2_panel_actions_refuse_keys_that_are_not_listed_today() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 40, 0))?;
    rig.tick_at(at(13, 40, 0)).await;
    let key = listed_key(&rig, "standup")?;
    let other_day = key.replace("2026-10-20", "2026-10-21");
    for (bad, act) in [
        (other_day.as_str(), Act::Dismiss),
        ("not a key", Act::Dismiss),
        (key.as_str(), Act::Join),
        (key.as_str(), Act::AlreadyThere),
    ] {
        let err = rig.sched.act_listed(&rig.db, bad, act).await.err();
        let code = err.map(|e| crate::errors::command_error(&e).code);
        assert_eq!(code, Some("not_found"), "{bad} {act:?}");
    }
    assert_eq!(
        rig.state_of("standup", "lead")?.as_deref(),
        Some("scheduled")
    );
    Ok(())
}

#[tokio::test]
async fn ac7_4_quit_stops_the_scheduler_and_closes_the_interrupt() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.tick_at(at(13, 58, 5)).await;
    rig.sched.shutdown(&rig.db).await;
    assert_eq!(rig.host().count("close"), 1);
    // Nothing runs after Quit, and the loop returns at once.
    let opens = rig.host().count("open");
    rig.tick_at(at(14, 3, 0)).await;
    assert_eq!(rig.host().count("open"), opens);
    tokio::time::timeout(StdDuration::from_secs(1), rig.sched.run_loop(&rig.db)).await?;
    Ok(())
}

/// S7.2: the scheduler does not depend on the settings window. With no window at all (closing
/// it only hides it, `tray::hides_on_close`), an alarm still fires on time.
#[tokio::test]
async fn s7_2_alarms_keep_firing_without_the_settings_window() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    assert!(crate::tray::hides_on_close("main"));
    assert_eq!(rig.tick_at(at(13, 58, 5)).await.fired, 1);
    assert_eq!(rig.host().count("open"), 1);
    Ok(())
}

// ---- L-015 review: pause holds every interrupt, snooze ahead edges ---------------------------

/// `until` of the stored alarm `uid`/`kind`.
fn until_of_kind(rig: &Rig, uid: &str, kind: &str) -> anyhow::Result<Option<String>> {
    let rows = pipeline::lock(&rig.db).alarms().list()?;
    Ok(rows
        .into_iter()
        .find(|r| r.occurrence_key.contains(&format!(":{uid}@")) && r.kind == kind)
        .and_then(|r| r.until))
}

#[test]
fn ac7_3_the_loop_wakes_at_the_pause_end_not_a_tick_later() {
    let now = at(14, 2, 55);
    assert_eq!(next_full_tick(now, None), at(14, 3, 10));
    assert_eq!(next_full_tick(now, Some(at(14, 3, 0))), at(14, 3, 0));
    assert_eq!(next_full_tick(now, Some(at(15, 0, 0))), at(14, 3, 10));
    // A pause end already passed is no deadline.
    assert_eq!(next_full_tick(now, Some(at(14, 0, 0))), at(14, 3, 10));
}

#[tokio::test]
async fn ac7_3_a_pause_and_quiet_hours_together_fire_after_the_later_of_both() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    // 13:58 is quiet, 14:00 is not: alone, the alarm would wait for the start (decision "Quiet
    // hours and settings details"). Paused until 14:03, it waits for that too.
    settings::apply(
        &rig.db,
        &serde_json::json!({ "quiet": { "enabled": true, "start": "13:55", "end": "14:00" } }),
    )?;
    pause_at(&rig, at(13, 50, 0), 13)?;
    for t in [at(13, 58, 5), at(14, 0, 5), at(14, 2, 55)] {
        assert_eq!(rig.tick_at(t).await.fired, 0, "fired at {t}");
    }
    assert_eq!(rig.tick_at(at(14, 3, 0)).await.fired, 1);
    assert_eq!(rig.host().count("open"), 1);
    Ok(())
}

#[tokio::test]
async fn ac7_3_an_open_interrupt_stays_during_a_pause_and_nothing_else_opens() -> TestResult {
    let rig = Rig::new(
        &[STANDUP, ("review", "Review", "140500", "")],
        at(13, 50, 0),
    )?;
    rig.tick_at(at(13, 58, 5)).await;
    let standup = rig.key(0)?;
    // The review's lead alarm fires too; it waits behind the open interrupt (another minute).
    assert_eq!(rig.tick_at(at(14, 3, 5)).await.fired, 1);
    assert_eq!(rig.host().count("open"), 1);
    pause_at(&rig, at(14, 3, 10), 5)?;
    rig.tick_at(at(14, 3, 15)).await;
    assert_eq!(rig.host().count("close"), 0, "the open interrupt stays");
    // Handling it would hand the screen to the waiting review: not while paused.
    rig.act(at(14, 3, 20), &standup, Act::Dismiss).await?;
    assert_eq!(rig.host().count("close"), 1);
    assert_eq!(rig.host().count("open"), 1);
    rig.tick_at(at(14, 8, 0)).await;
    assert_eq!(rig.host().count("open"), 1, "still paused at 14:08:00");
    // The pause ends at 14:08:10, inside the review's window (14:15): it opens then.
    rig.tick_at(at(14, 8, 10)).await;
    assert_eq!(rig.host().count("open"), 2);
    assert_eq!(rig.last_show()?.meetings[0].title, "Review");
    Ok(())
}

#[tokio::test]
async fn ac7_3_a_restart_during_a_pause_opens_no_interrupt_until_it_ends() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.tick_at(at(13, 58, 5)).await;
    assert_eq!(rig.state_of("standup", "lead")?.as_deref(), Some("firing"));
    pause_at(&rig, at(13, 58, 10), 5)?;
    // A new process: the alarm is Firing in the store, but the first tick is paused.
    let restarted = Scheduler::new(Fake::new(at(13, 59, 0)));
    restarted.step(&rig.db, true).await;
    assert_eq!(restarted.host.count("open"), 0);
    assert_eq!(restarted.host.pauses(), [Some(at(14, 3, 10))]);
    restarted.host.set_now(at(14, 3, 10));
    restarted.step(&rig.db, true).await;
    assert_eq!(restarted.host.count("open"), 1);
    Ok(())
}

#[tokio::test]
async fn ac7_2_panel_snooze_ahead_moves_the_start_alarm_once_the_lead_is_done() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 50, 0))?;
    rig.setting(AGAIN_AT_START, serde_json::json!(true))?;
    rig.tick_at(at(13, 58, 5)).await;
    rig.act(at(13, 58, 10), &rig.key(0)?, Act::Dismiss).await?;
    // The lead alarm is dismissed; the start alarm (14:00) is the one the panel moves.
    let key = listed_key(&rig, "standup")?;
    rig.host().set_now(at(13, 59, 0));
    assert!(!rig.sched.act_listed(&rig.db, &key, Act::Snooze(5)).await?);
    assert_eq!(
        rig.state_of("standup", "lead")?.as_deref(),
        Some("dismissed")
    );
    assert_eq!(
        rig.state_of("standup", "start")?.as_deref(),
        Some("snoozed")
    );
    assert_eq!(
        until_of_kind(&rig, "standup", "start")?.as_deref(),
        Some("2026-10-20T14:05:00Z")
    );
    assert_eq!(rig.tick_at(at(14, 0, 5)).await.fired, 0);
    assert_eq!(rig.tick_at(at(14, 5, 5)).await.fired, 1);
    Ok(())
}

#[tokio::test]
async fn ac7_2_a_panel_snooze_past_the_window_ends_the_reminders_and_says_so() -> TestResult {
    let rig = Rig::new(&[STANDUP], at(13, 40, 0))?;
    rig.tick_at(at(13, 40, 0)).await;
    let key = listed_key(&rig, "standup")?;
    rig.host().set_now(at(13, 45, 0));
    // 13:58 to 14:03, then 14:08: both still inside the window that closes at 14:10.
    for _ in 0..2 {
        assert!(!rig.sched.act_listed(&rig.db, &key, Act::Snooze(5)).await?);
    }
    // 14:13 would be after it: the reminder ends and the panel is told.
    assert!(rig.sched.act_listed(&rig.db, &key, Act::Snooze(5)).await?);
    assert_eq!(rig.state_of("standup", "lead")?.as_deref(), Some("expired"));
    assert_eq!(rig.tick_at(at(14, 8, 5)).await.fired, 0);
    // A Dismiss never reports an end.
    assert!(!rig.sched.act_listed(&rig.db, &key, Act::Dismiss).await?);
    Ok(())
}

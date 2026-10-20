//! The scheduler (F3) with the focus fallback (F4.5) and the repeating sound (F6).
//!
//! One loop, [`Scheduler::run_loop`], on Tauri's async runtime. Every 15 s, and at once after a
//! sync or settings change ([`Scheduler::wake`]), a full tick runs:
//!
//! 1. Prune final alarms whose meeting is over, load the settings and the stored events, expand
//!    the next 7 days (and the last day, so an alarm that fell due during sleep still finds its
//!    meeting, S3.4 and S3.5), drop what the ignore rules drop.
//! 2. `plan`, then `tick`, then persist: one store lock for all of it, so a command (snooze,
//!    dismiss) can never be overwritten by a tick that started before it.
//! 3. Show every Firing meeting: meetings that start in the same minute share one interrupt, also
//!    when their lead alarms land on different ticks (AC3.5); another minute waits for the open
//!    one to close.
//!
//! A gap of more than 60 s between two full ticks is a wake from sleep (AC3.4). The tick is a pure
//! function of `now`, so a wake needs no special code besides the log line: an overdue alarm fires,
//! one past its window expires.
//!
//! Between full ticks the loop only does housekeeping, on its own deadlines: the focus re-check
//! 500 ms after an interrupt opened and the sound repeat every 30 s while it stays open.
//!
//! Everything outside the process (windows, sound, notifications, the browser, the clock) is behind
//! [`Host`], so the unit tests run the whole loop without Tauri. The Tauri side is `tauri_host.rs`.
//!
//! Pause and quiet hours have no settings yet (M3, L-016): `tick` gets `None` for both. They hold
//! firing back only, so adding them later is two arguments.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::str::FromStr;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use chrono_tz::Tz;
use tokio::sync::{Mutex, Notify};
use tolon_core::{
    apply_rules, apply_transitions, expand_window, group_firing, plan, tick, Alarm, AlarmKind,
    AlarmSettings, AlarmState, Event, ExpandOptions, IgnoreRules, InterruptGroup, JoinTarget,
    Occurrence, OccurrenceKey, Transition, Verdict, MAX_LEAD_MINUTES, MIN_LEAD_MINUTES,
    WINDOW_DAYS,
};
use tolon_platform::Sound;
use tolon_store::AlarmRow;

use crate::errors::AppError;
use crate::interrupt::{self, AlarmShow};
use crate::pipeline::{self, Db};

mod tauri_host;
pub use tauri_host::{setup_fallback_actions, TauriHost};

/// Time between two full ticks (AC3.4).
pub const TICK: StdDuration = StdDuration::from_secs(15);
/// A gap this long between two full ticks means the machine slept.
pub const WAKE_GAP: Duration = Duration::seconds(60);
/// The sound repeats this often while an interrupt stays open (AC6.1).
pub const REPEAT_EVERY: Duration = Duration::seconds(30);
/// Focus is checked this long after an interrupt opened (decision "Platform contract after L-012").
pub const FOCUS_RECHECK: Duration = Duration::milliseconds(500);
/// The Snooze button, the Esc key and the notification's Snooze action (the UI has the same 5).
pub const SNOOZE_MINUTES: u32 = 5;
/// How far back occurrences are expanded: a meeting whose alarm fell due while the machine slept
/// is still found, so its alarm is marked Expired instead of silently dropped (S3.5).
const LOOKBACK: Duration = Duration::days(1);

/// Settings keys read every tick. JSON values; a missing key means the default.
pub const LEAD_MINUTES: &str = "lead_minutes";
pub const AGAIN_AT_START: &str = "again_at_start";
pub const IGNORE_KEYWORDS: &str = "ignore_keywords";
pub const IGNORE_NO_ATTENDEES: &str = "ignore_no_attendees";
/// `"soft_bell"`, `"chime"` or `"none"`.
pub const SOUND: &str = "sound";
/// Repeat the sound every 30 s until the interrupt closes. Default on (AC6.1).
pub const SOUND_REPEAT: &str = "sound_repeat";

/// What the scheduler needs from the outside world. Tauri implements it; the tests use a fake.
pub trait Host: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
    fn zone(&self) -> Tz;
    /// Opens the interrupt on every screen with `show`, or updates it when it is already open.
    fn open(&self, show: &AlarmShow) -> impl Future<Output = anyhow::Result<Opened>> + Send;
    /// False when the interrupt has no keyboard focus, or the platform said it refused it.
    fn focused(&self) -> impl Future<Output = bool> + Send;
    /// Closes every interrupt window and tells the pages.
    fn close(&self);
    /// One shot of the alarm sound; failures are logged and ignored.
    fn play(&self, sound: Sound);
    fn stop_sound(&self);
    /// Opens an https link in the default browser. The error never carries the URL.
    fn open_url(&self, url: &str) -> anyhow::Result<()>;
    /// The F4.5 fallback: a system notification with Join and Snooze.
    fn notify(&self, show: &AlarmShow);
}

/// What `Host::open` found out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Opened {
    /// Why the system cannot cover the screen (`Takeover::Degraded`), as the platform's code.
    pub degraded: Option<&'static str>,
}

/// A person's action on an interrupt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// Every meeting of the interrupt; the minutes are clamped to 1..=60.
    Snooze(u32),
    /// Every meeting of the interrupt.
    Dismiss,
    /// The one meeting; opens its join link or event page.
    Join,
    /// The one meeting; opens nothing.
    AlreadyThere,
}

/// What one full tick did, for the tests and the log.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct StepReport {
    pub woke: bool,
    pub fired: usize,
    pub expired: usize,
}

// ---- Alarm rows ----------------------------------------------------------------------------

fn ts(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// `state_at` holds `Firing.since`; for every other state it is when the row was written.
pub(crate) fn to_row(a: &Alarm, now: DateTime<Utc>) -> AlarmRow {
    AlarmRow {
        occurrence_key: a.occurrence.to_string(),
        kind: a.kind.as_str().to_owned(),
        at: ts(a.at),
        start: ts(a.start),
        state: a.state.name().to_owned(),
        state_at: ts(a.state.since().unwrap_or(now)),
        until: a.state.until().map(ts),
    }
}

/// `None` for a row that does not decode; the caller drops it with a log line.
pub(crate) fn from_row(r: &AlarmRow) -> Option<Alarm> {
    Some(Alarm {
        occurrence: OccurrenceKey::from_str(&r.occurrence_key).ok()?,
        at: parse_ts(&r.at)?,
        start: parse_ts(&r.start)?,
        kind: AlarmKind::parse(&r.kind)?,
        state: AlarmState::from_parts(
            &r.state,
            parse_ts(&r.state_at),
            r.until.as_deref().and_then(parse_ts),
        )?,
    })
}

fn stored_alarms(store: &tolon_store::Store) -> anyhow::Result<Vec<Alarm>> {
    let mut alarms = Vec::new();
    for row in store.alarms().list()? {
        match from_row(&row) {
            Some(a) => alarms.push(a),
            // The key embeds the event id; log the shape of the row, not its content.
            None => {
                tracing::warn!(kind = %row.kind, state = %row.state, "dropping an alarm row that does not decode")
            }
        }
    }
    Ok(alarms)
}

// ---- One tick, without the outside world ---------------------------------------------------

/// Everything one tick reads from the store, taken under one lock.
pub struct Inputs {
    opts: ExpandOptions,
    events: Vec<Event>,
    alarm: AlarmSettings,
    rules: IgnoreRules,
    sound: Sound,
    repeat: bool,
}

pub fn load_inputs(db: &Db) -> anyhow::Result<Inputs> {
    let store = pipeline::lock(db);
    let settings = store.settings();
    let (alarm_default, rules_default) = (AlarmSettings::default(), IgnoreRules::default());
    let lead = settings
        .get(LEAD_MINUTES)?
        .and_then(|v| v.as_u64())
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(alarm_default.lead_minutes)
        .clamp(MIN_LEAD_MINUTES, MAX_LEAD_MINUTES);
    let alarm = AlarmSettings {
        lead_minutes: lead,
        again_at_start: pipeline::flag(&store, AGAIN_AT_START, alarm_default.again_at_start)?,
    };
    let rules = IgnoreRules {
        keywords: settings
            .get(IGNORE_KEYWORDS)?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or(rules_default.keywords),
        without_other_attendees: pipeline::flag(
            &store,
            IGNORE_NO_ATTENDEES,
            rules_default.without_other_attendees,
        )?,
    };
    let sound = settings
        .get(SOUND)?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or(Sound::SoftBell);
    Ok(Inputs {
        opts: pipeline::expand_options(&store)?,
        events: pipeline::decode_events(store.events().list_enabled()?),
        alarm,
        rules,
        sound,
        repeat: pipeline::flag(&store, SOUND_REPEAT, true)?,
    })
}

/// The occurrences the alarms are planned from: the last day and the next 7, minus what the
/// ignore rules drop (AC8.2).
pub fn occurrences(inputs: &Inputs, now: DateTime<Utc>, zone: Tz) -> Vec<Occurrence> {
    let to = now
        .checked_add_signed(Duration::days(WINDOW_DAYS))
        .unwrap_or(DateTime::<Utc>::MAX_UTC);
    let all = expand_window(&inputs.events, now - LOOKBACK, to, zone, &inputs.opts);
    apply_rules(all, &inputs.rules)
        .into_iter()
        .filter_map(|(occ, verdict)| (verdict == Verdict::Keep).then_some(occ))
        .collect()
}

/// The result of planning and ticking: the transitions of this tick, and every alarm that is Firing now.
pub struct Ticked {
    pub transitions: Vec<Transition>,
    pub firing: Vec<Alarm>,
}

/// Plan, prune, tick, persist, in one store lock.
///
/// - `plan` drops non-final alarms of occurrences that are gone or ignored; they are deleted too.
/// - The plan is written first, so every stored `start` follows the occurrence, and only then are
///   final alarms whose meeting ended (new start + 10 min before `now`) pruned. Pruning on the old
///   start would delete a Dismissed alarm of a meeting that moved later and was seen after the old
///   window closed, and `plan` would then re-create it as Scheduled (S3.3, decision "Alarm behaviour").
/// - The transitions are written after the prune, so an alarm that expired in this tick is still
///   stored (and visible) until the next one.
/// - Pause and quiet hours are `None`: no setting exists yet.
pub fn plan_and_tick(
    db: &Db,
    occurrences: &[Occurrence],
    settings: &AlarmSettings,
    now: DateTime<Utc>,
) -> anyhow::Result<Ticked> {
    let store = pipeline::lock(db);
    let stored = stored_alarms(&store)?;
    let mut alarms = plan(occurrences, settings, &stored, now);
    let planned: Vec<AlarmRow> = alarms.iter().map(|a| to_row(a, now)).collect();
    store.alarms().upsert_all(&planned)?;
    store.alarms().prune_before(&ts(now))?;
    let transitions = tick(&alarms, now, None, None);
    apply_transitions(&mut alarms, &transitions);
    // Only the alarms that changed: a pruned row must not come back.
    let changed: Vec<AlarmRow> = transitions
        .iter()
        .filter_map(|t| {
            alarms
                .iter()
                .find(|a| a.occurrence == t.occurrence && a.kind == t.kind)
        })
        .map(|a| to_row(a, now))
        .collect();
    store.alarms().upsert_all(&changed)?;
    let keep: Vec<(String, String)> = planned
        .iter()
        .map(|r| (r.occurrence_key.clone(), r.kind.clone()))
        .collect();
    store.alarms().delete_missing(&keep)?;
    let firing = alarms
        .into_iter()
        .filter(|a| a.state.since().is_some())
        .collect();
    Ok(Ticked {
        transitions,
        firing,
    })
}

/// Applies a person's action to the stored alarms of `keys`, in one store lock.
/// Snooze and Dismiss touch the alarms that are Firing (the ones on screen), so a Dismiss of the
/// lead alarm still lets the start alarm fire (decision "Alarm behaviour"). Join and
/// "already there" close every open alarm of the meeting.
fn apply_action(db: &Db, keys: &[String], act: Act, now: DateTime<Utc>) -> anyhow::Result<()> {
    let store = pipeline::lock(db);
    let mut rows = Vec::new();
    for key in keys {
        for kind in [AlarmKind::Lead, AlarmKind::Start] {
            let Some(mut alarm) = store
                .alarms()
                .get(key, kind.as_str())?
                .and_then(|r| from_row(&r))
            else {
                continue;
            };
            let firing = alarm.state.since().is_some();
            let changed = match act {
                Act::Snooze(minutes) => {
                    let minutes = minutes.clamp(MIN_LEAD_MINUTES, MAX_LEAD_MINUTES);
                    firing && tolon_core::snooze(&mut alarm, minutes, now)
                }
                Act::Dismiss => firing && tolon_core::dismiss(&mut alarm),
                Act::Join => tolon_core::join(&mut alarm),
                Act::AlreadyThere => tolon_core::already_there(&mut alarm),
            };
            if changed {
                rows.push(to_row(&alarm, now));
            }
        }
    }
    store.alarms().upsert_all(&rows)?;
    Ok(())
}

/// Alarms in the store that are Firing now, for when an interrupt closes and another minute waits.
fn stored_firing(db: &Db) -> anyhow::Result<Vec<Alarm>> {
    let store = pipeline::lock(db);
    Ok(stored_alarms(&store)?
        .into_iter()
        .filter(|a| a.state.since().is_some())
        .collect())
}

// ---- Join links ----------------------------------------------------------------------------

/// A link Tolon may open: `https://`, at most 8 KB, no whitespace or control characters, and an
/// authority of plain host characters (no userinfo, no backslash). The core validated a `Link`;
/// an event page comes straight from the feed's `URL` property, so every link is checked again
/// right before it is opened (decision "Join links are treated as untrusted").
pub(crate) fn openable(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    url.len() <= 8 * 1024
        && !url.chars().any(|c| c.is_control() || c.is_whitespace())
        && !authority.is_empty()
        && authority
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'))
}

/// The link a meeting's join button opens, or `None` when there is none or it fails the check.
fn link_of(occ: &Occurrence) -> Option<String> {
    let url = match &occ.join {
        JoinTarget::Link { url, .. } | JoinTarget::EventPage(url) => url,
        JoinTarget::None => return None,
    };
    openable(url).then(|| url.clone())
}

fn links_of(group: &InterruptGroup) -> HashMap<String, String> {
    group
        .occurrences
        .iter()
        .filter_map(|o| link_of(o).map(|url| (o.key.to_string(), url)))
        .collect()
}

// ---- The loop ------------------------------------------------------------------------------

/// The interrupt that is up. With `test` it is F10's sample: no alarm behind it.
struct Open {
    show: AlarmShow,
    /// The start minute the meetings share (AC3.5); a later firing of that minute joins this interrupt.
    minute: DateTime<Utc>,
    /// Meeting key to link. The URLs stay here; the page never sees them.
    links: HashMap<String, String>,
    test: bool,
    last_sound: DateTime<Utc>,
    recheck_at: Option<DateTime<Utc>>,
    /// The F4.5 notification went out for this interrupt.
    notified: bool,
}

fn new_open(
    show: AlarmShow,
    minute: DateTime<Utc>,
    links: HashMap<String, String>,
    test: bool,
    now: DateTime<Utc>,
) -> Open {
    Open {
        show,
        minute,
        links,
        test,
        last_sound: now,
        recheck_at: Some(now + FOCUS_RECHECK),
        notified: false,
    }
}

struct Inner {
    open: Option<Open>,
    last_tick: Option<DateTime<Utc>>,
    sound: Sound,
    repeat: bool,
    degraded_logged: bool,
}

pub struct Scheduler<H> {
    host: H,
    inner: Mutex<Inner>,
    wake: Notify,
}

impl<H: Host> Scheduler<H> {
    pub fn new(host: H) -> Self {
        Self {
            host,
            inner: Mutex::new(Inner {
                open: None,
                last_tick: None,
                sound: Sound::SoftBell,
                repeat: true,
                degraded_logged: false,
            }),
            wake: Notify::new(),
        }
    }

    /// Runs the next tick now: a sync finished or a setting changed (AC8.3). Calls collapse into one.
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    /// For the life of the app. The first tick runs at once: an alarm that was Firing when the app
    /// last ran is shown again, and one that fell due while it was closed fires or expires.
    pub async fn run_loop(&self, db: &Db) {
        let mut changed = true;
        let mut next_full = self.host.now();
        loop {
            let full = changed || self.host.now() >= next_full;
            self.step(db, full).await;
            let now = self.host.now();
            if full {
                next_full = now + Duration::from_std(TICK).unwrap_or(Duration::seconds(15));
            }
            let due = self
                .deadline()
                .await
                .map_or(next_full, |d| d.min(next_full));
            let wait = (due - now)
                .to_std()
                .unwrap_or_default()
                .max(StdDuration::from_millis(100));
            changed = tokio::select! {
                () = tokio::time::sleep(wait) => false,
                () = self.wake.notified() => true,
            };
        }
    }

    /// The next housekeeping moment: the focus re-check or the sound repeat.
    async fn deadline(&self) -> Option<DateTime<Utc>> {
        let inner = self.inner.lock().await;
        let open = inner.open.as_ref()?;
        let repeat = inner.repeat.then_some(open.last_sound + REPEAT_EVERY);
        [open.recheck_at, repeat].into_iter().flatten().min()
    }

    /// One loop iteration. A full one plans and ticks; every one does the housekeeping.
    pub async fn step(&self, db: &Db, full: bool) -> StepReport {
        let now = self.host.now();
        let mut inner = self.inner.lock().await;
        let mut report = StepReport::default();
        if full {
            report.woke = inner.last_tick.is_some_and(|t| now - t > WAKE_GAP);
            if report.woke {
                tracing::info!("wake from sleep detected; alarms that fell due are handled now");
            }
            inner.last_tick = Some(now);
            let evaluated = load_inputs(db).and_then(|inputs| {
                let occs = occurrences(&inputs, now, self.host.zone());
                let ticked = plan_and_tick(db, &occs, &inputs.alarm, now)?;
                Ok((inputs, occs, ticked))
            });
            match evaluated {
                Ok((inputs, occs, ticked)) => {
                    inner.sound = inputs.sound;
                    inner.repeat = inputs.repeat;
                    report.fired = ticked.transitions.iter().filter(|t| t.fires()).count();
                    report.expired = ticked
                        .transitions
                        .iter()
                        .filter(|t| t.to == AlarmState::Expired)
                        .count();
                    self.drop_stale_meetings(&mut inner, &ticked.firing).await;
                    self.present(&mut inner, &ticked.firing, &occs, now).await;
                }
                // The next tick tries again; nothing fires from a half-read store.
                Err(e) => tracing::warn!(error = %e, "scheduler tick failed"),
            }
        }
        self.housekeeping(&mut inner, now).await;
        report
    }

    /// Shows what is Firing. An open interrupt takes the meetings of its own start minute (AC3.5);
    /// another minute waits for it to close. A test interrupt gives way to a real one.
    async fn present(
        &self,
        inner: &mut Inner,
        firing: &[Alarm],
        occs: &[Occurrence],
        now: DateTime<Utc>,
    ) {
        let groups = group_firing(firing, occs);
        if let Some(open) = inner.open.as_ref().filter(|o| !o.test) {
            let grown = groups.iter().find(|g| {
                g.minute == open.minute
                    && g.occurrences.iter().any(|o| {
                        !open
                            .show
                            .meetings
                            .iter()
                            .any(|m| m.key == o.key.to_string())
                    })
            });
            if let Some(group) = grown {
                self.regroup(inner, group, now).await;
            }
        } else if let Some(group) = groups.first() {
            self.open_group(inner, group, now).await;
        }
    }

    /// AC3.5: a meeting of the open minute fired on a later tick; the interrupt lists it too.
    async fn regroup(&self, inner: &mut Inner, group: &InterruptGroup, now: DateTime<Utc>) {
        let show = AlarmShow::from_group(group);
        if let Some(open) = inner.open.as_mut() {
            open.links = links_of(group);
            open.show = show.clone();
        }
        if let Err(e) = self.host.open(&show).await {
            tracing::warn!(error = %e, "could not update the interrupt with a new meeting");
        }
        self.sound(inner, now);
    }

    async fn open_group(&self, inner: &mut Inner, group: &InterruptGroup, now: DateTime<Utc>) {
        let show = AlarmShow::from_group(group);
        let mut open = new_open(show.clone(), group.minute, links_of(group), false, now);
        match self.host.open(&show).await {
            Ok(opened) => {
                if let Some(reason) = opened.degraded {
                    if !inner.degraded_logged {
                        inner.degraded_logged = true;
                        tracing::warn!(
                            reason,
                            "the system cannot cover the screen; using notifications"
                        );
                    }
                    open.notified = true;
                    self.host.notify(&show);
                }
            }
            // No window at all: the notification is the interrupt, and its actions still work.
            Err(e) => {
                tracing::warn!(error = %e, "could not open the interrupt window; using a notification");
                open.recheck_at = None;
                open.notified = true;
                self.host.notify(&show);
            }
        }
        inner.open = Some(open);
        self.sound(inner, now);
    }

    /// A meeting in the open interrupt whose alarm is no longer Firing (expired, or the meeting
    /// was deleted or ignored) leaves it; an interrupt with no meeting left closes.
    async fn drop_stale_meetings(&self, inner: &mut Inner, firing: &[Alarm]) {
        let live: HashSet<String> = firing.iter().map(|a| a.occurrence.to_string()).collect();
        let Some(open) = inner.open.as_mut().filter(|o| !o.test) else {
            return;
        };
        let before = open.show.meetings.len();
        open.show.meetings.retain(|m| live.contains(&m.key));
        open.links.retain(|key, _| live.contains(key));
        if open.show.meetings.is_empty() {
            self.close(inner);
        } else if open.show.meetings.len() != before {
            let show = open.show.clone();
            if let Err(e) = self.host.open(&show).await {
                tracing::warn!(error = %e, "could not update the interrupt");
            }
        }
    }

    /// The focus re-check and the sound repeat.
    async fn housekeeping(&self, inner: &mut Inner, now: DateTime<Utc>) {
        let Some(open) = inner.open.as_mut() else {
            return;
        };
        if open.recheck_at.is_some_and(|at| now >= at) {
            open.recheck_at = None;
            if !open.notified && !self.host.focused().await {
                open.notified = true;
                tracing::warn!("the interrupt has no keyboard focus; using a notification too");
                self.host.notify(&open.show);
            }
        }
        if inner.repeat && open.last_sound + REPEAT_EVERY <= now {
            self.sound(inner, now);
        }
    }

    fn sound(&self, inner: &mut Inner, now: DateTime<Utc>) {
        if let Some(open) = inner.open.as_mut() {
            open.last_sound = now;
        }
        self.host.play(inner.sound);
    }

    fn close(&self, inner: &mut Inner) {
        inner.open = None;
        self.host.close();
        self.host.stop_sound();
    }

    /// Shows F10's test alarm. A real interrupt that is up is left alone.
    pub async fn show_test(&self) -> anyhow::Result<()> {
        let now = self.host.now();
        let mut inner = self.inner.lock().await;
        if inner.open.as_ref().is_some_and(|o| !o.test) {
            return Ok(());
        }
        let show = AlarmShow::sample(now);
        let opened = self.host.open(&show).await?;
        let mut open = new_open(show, now, HashMap::new(), true, now);
        if opened.degraded.is_some() {
            open.notified = true;
            self.host.notify(&open.show);
        }
        inner.open = Some(open);
        self.sound(&mut inner, now);
        Ok(())
    }

    /// The first meeting of the open interrupt: what the notification's Join action joins.
    pub async fn first_key(&self) -> Option<String> {
        let inner = self.inner.lock().await;
        let open = inner.open.as_ref()?;
        open.show.meetings.first().map(|m| m.key.clone())
    }

    /// A person's action. `key` must be a meeting of the open interrupt, else `not_found`.
    ///
    /// - The link of a join opens before any state changes: a link that cannot open leaves the
    ///   interrupt up, so the person can try again or press "I am already there".
    /// - Snooze and Dismiss cover every meeting of the interrupt (one Snooze, one Dismiss).
    /// - Join and "already there" cover one meeting. Meetings still on screen keep the interrupt
    ///   open, re-opened for them; with none left it closes.
    /// - A closed interrupt hands the screen to a waiting minute, if one is Firing.
    pub async fn act(&self, db: &Db, key: &str, act: Act) -> anyhow::Result<()> {
        interrupt::parse_key(key)?;
        let now = self.host.now();
        let mut inner = self.inner.lock().await;
        let open = inner
            .open
            .as_ref()
            .filter(|o| o.show.meetings.iter().any(|m| m.key == key))
            .ok_or(AppError::NOT_FOUND)?;
        let (targets, link) = match act {
            Act::Snooze(_) | Act::Dismiss => (
                open.show.meetings.iter().map(|m| m.key.clone()).collect(),
                None,
            ),
            Act::Join => (vec![key.to_owned()], open.links.get(key).cloned()),
            Act::AlreadyThere => (vec![key.to_owned()], None),
        };
        let rest: Vec<_> = match act {
            Act::Join | Act::AlreadyThere => open
                .show
                .meetings
                .iter()
                .filter(|m| m.key != key)
                .cloned()
                .collect(),
            Act::Snooze(_) | Act::Dismiss => Vec::new(),
        };
        let test = open.test;
        if let Some(url) = &link {
            self.host.open_url(url).map_err(|_| AppError::INTERNAL)?;
        }
        if !test {
            apply_action(db, &targets, act, now)?;
        }
        if rest.is_empty() {
            self.close(&mut inner);
            self.present_waiting(&mut inner, db, now).await;
            return Ok(());
        }
        let Some(open) = inner.open.as_mut() else {
            return Ok(());
        };
        open.show.meetings = rest;
        open.links.remove(key);
        let show = open.show.clone();
        if let Err(e) = self.host.open(&show).await {
            tracing::warn!(error = %e, "could not re-open the interrupt for the remaining meetings");
        }
        Ok(())
    }

    /// After a close: a Firing meeting of another minute was waiting for the screen.
    async fn present_waiting(&self, inner: &mut Inner, db: &Db, now: DateTime<Utc>) {
        let waiting = stored_firing(db).and_then(|firing| {
            if firing.is_empty() {
                return Ok(None);
            }
            let inputs = load_inputs(db)?;
            Ok(Some((firing, occurrences(&inputs, now, self.host.zone()))))
        });
        match waiting {
            Ok(Some((firing, occs))) => self.present(inner, &firing, &occs, now).await,
            Ok(None) => {}
            Err(e) => tracing::warn!(error = %e, "could not look for waiting meetings"),
        }
    }
}

/// Runs for the life of the app on Tauri's async runtime (same shape as `refresh::run`).
pub async fn run(app: tauri::AppHandle) {
    use tauri::Manager;
    let (Some(scheduler), Ok(state)) = (
        app.try_state::<crate::commands::AppScheduler>(),
        crate::commands::state(&app),
    ) else {
        tracing::error!("the scheduler has no state to run on");
        return;
    };
    scheduler.run_loop(&state.db).await;
}

#[cfg(test)]
mod tests;

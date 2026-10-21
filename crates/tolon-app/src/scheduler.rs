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
//! Quiet hours come from the settings (AC8.1, `settings::read`), the pause from
//! `settings::paused_until` (AC7.3): `tick` holds firing back while either applies, and an alarm
//! that fell due meanwhile fires afterwards if its meeting is still within its window (decision
//! "Alarm behaviour"). A change of the pause state (paused, resumed, or the pause ran out) goes to
//! [`Host::paused`] for the tray icon and the panel; a pause wakes the loop when it ends.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::str::FromStr;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use chrono_tz::Tz;
use tokio::sync::{Mutex, Notify};
use tolon_core::{
    apply_transitions, group_firing, judge_window, plan, tick, Alarm, AlarmKind, AlarmSettings,
    AlarmState, Event, ExpandOptions, IgnoreRules, InterruptGroup, JoinTarget, Occurrence,
    OccurrenceKey, QuietHours, Transition, Verdict, MAX_LEAD_MINUTES, MIN_LEAD_MINUTES,
    WINDOW_DAYS,
};
use tolon_platform::Sound;
use tolon_store::AlarmRow;

use crate::errors::AppError;
use crate::interrupt::{self, AlarmShow};
use crate::pipeline::{self, Db};
use crate::settings;

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
    /// The F4.5 fallback: a system notification with Join and Snooze. `id` is the notification's
    /// identifier; the host hands it back with the action (see `Scheduler::act_on_notice`).
    fn notify(&self, show: &AlarmShow, id: i32);
    /// Alarms are paused until `until`, or no longer paused (`None`): the tray icon and the panel
    /// follow (AC7.3). Called on the first tick and on every change.
    fn paused(&self, until: Option<DateTime<Utc>>);
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
    quiet: Option<QuietHours>,
    /// When the pause ends; `None` while not paused.
    pause: Option<DateTime<Utc>>,
}

/// `zone` is the machine's: quiet hours are local time.
pub fn load_inputs(db: &Db, zone: Tz, now: DateTime<Utc>) -> anyhow::Result<Inputs> {
    let store = pipeline::lock(db);
    let s = settings::read(&store)?;
    let pause = settings::paused_until(&store, now)?;
    Ok(Inputs {
        opts: s.expand_options(),
        events: pipeline::decode_events(store.events().list_enabled()?),
        alarm: s.alarm(),
        rules: s.rules(),
        sound: s.sound.into(),
        repeat: s.sound_repeat,
        quiet: s.quiet_hours(zone),
        pause,
    })
}

/// The occurrences the alarms are planned from: the last day and the next 7, minus what the
/// ignore rules drop (AC8.2).
pub fn occurrences(inputs: &Inputs, now: DateTime<Utc>, zone: Tz) -> Vec<Occurrence> {
    let to = now
        .checked_add_signed(Duration::days(WINDOW_DAYS))
        .unwrap_or(DateTime::<Utc>::MAX_UTC);
    judge_window(
        &inputs.events,
        now - LOOKBACK,
        to,
        zone,
        &inputs.opts,
        &inputs.rules,
    )
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
/// - Pause and quiet hours hold firing back only (AC7.3, AC8.1): the plan is written as usual, so
///   the tray still lists what is coming.
pub fn plan_and_tick(
    db: &Db,
    occurrences: &[Occurrence],
    settings: &AlarmSettings,
    (pause, quiet): (Option<DateTime<Utc>>, Option<&QuietHours>),
    now: DateTime<Utc>,
) -> anyhow::Result<Ticked> {
    let store = pipeline::lock(db);
    let stored = stored_alarms(&store)?;
    let mut alarms = plan(occurrences, settings, &stored, now);
    let planned: Vec<AlarmRow> = alarms.iter().map(|a| to_row(a, now)).collect();
    store.alarms().upsert_all(&planned)?;
    store.alarms().prune_before(&ts(now))?;
    let transitions = tick(&alarms, now, pause, quiet);
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

/// The panel's Snooze and Dismiss for a meeting whose interrupt is not on screen (AC7.2): every
/// alarm of the meeting that is not final. Snooze moves each one `minutes` later than it was due
/// (`tolon_core::snooze_later`); Dismiss makes each one Dismissed, so neither the lead alarm nor
/// "again at the start" takes over the screen. A meeting without alarms (ignored, or past) changes
/// nothing. `true` when a Snooze ended the reminders instead: every alarm it moved would have come
/// after the meeting's window, so each one expired (decision "Tray panel, pause and window
/// rights"; the panel says so).
fn apply_ahead(db: &Db, key: &str, act: Act, now: DateTime<Utc>) -> anyhow::Result<bool> {
    let store = pipeline::lock(db);
    let mut rows = Vec::new();
    let mut ended = None;
    for kind in [AlarmKind::Lead, AlarmKind::Start] {
        let Some(mut alarm) = store
            .alarms()
            .get(key, kind.as_str())?
            .and_then(|r| from_row(&r))
        else {
            continue;
        };
        let changed = match act {
            Act::Snooze(minutes) => tolon_core::snooze_later(
                &mut alarm,
                minutes.clamp(MIN_LEAD_MINUTES, MAX_LEAD_MINUTES),
                now,
            ),
            Act::Dismiss => tolon_core::dismiss(&mut alarm),
            Act::Join | Act::AlreadyThere => false,
        };
        if changed {
            if matches!(act, Act::Snooze(_)) {
                let expired = alarm.state == AlarmState::Expired;
                ended = Some(ended.unwrap_or(true) && expired);
            }
            rows.push(to_row(&alarm, now));
        }
    }
    store.alarms().upsert_all(&rows)?;
    Ok(ended.unwrap_or(false))
}

/// When the next full tick is due after one at `now`: one tick later, or the end of a pause if
/// that comes first, so the pause ends on time (AC7.3) and not up to 15 s late.
pub fn next_full_tick(now: DateTime<Utc>, pause_end: Option<DateTime<Utc>>) -> DateTime<Utc> {
    let tick = now + Duration::from_std(TICK).unwrap_or(Duration::seconds(15));
    pause_end
        .filter(|end| *end > now)
        .map_or(tick, |end| end.min(tick))
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

/// A link Tolon may open: `https://`, at most 8 KB, no whitespace, control or Unicode format
/// characters (zero-width and bidi overrides hide where a link goes), and an
/// authority of plain host characters (no userinfo, no backslash). The core validated a `Link`;
/// an event page comes straight from the feed's `URL` property, so every link is checked again
/// right before it is opened (decision "Join links are treated as untrusted").
/// Unicode general category Cf (format), as of Unicode 16: zero-width and joiner characters, bidi
/// marks and overrides, the BOM, tag characters. `std` has no category table, so they are listed.
pub(crate) fn is_format_char(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

pub(crate) fn openable(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    url.len() <= 8 * 1024
        && !url
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || is_format_char(c))
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

/// Where the notification identifiers start: the low 30 bits of the start time in unix seconds.
fn notice_seed(now: DateTime<Utc>) -> i32 {
    i32::try_from(now.timestamp().rem_euclid(1 << 30)).unwrap_or(0)
}

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
    /// The F4.5 notification went out for the interrupt as it is now.
    notified: bool,
    /// Identifier of that notification. Its Join and Snooze act only while it is this one: a
    /// notification of an earlier interrupt, or of the same interrupt before its meetings changed,
    /// is stale and does nothing.
    notice: Option<i32>,
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
        notice: None,
    }
}

struct Inner {
    open: Option<Open>,
    last_tick: Option<DateTime<Utc>>,
    sound: Sound,
    repeat: bool,
    degraded_logged: bool,
    /// The last notification identifier handed out. It counts up from a start-time seed, so a stale
    /// id never matches within a run, and a notification left over from an earlier run (the OS may
    /// keep it on screen) is unlikely to match either.
    last_notice: i32,
    /// When the last preview started, and which sound, for the preview throttle.
    last_preview: Option<(DateTime<Utc>, Sound)>,
    /// The pause state last handed to `Host::paused`; `None` before the first tick.
    paused: Option<Option<DateTime<Utc>>>,
    /// Set by `shutdown`: the loop ends and nothing opens again.
    stopped: bool,
}

/// Previews closer together than this are dropped, so a held key or fast clicks do not stack sounds.
pub const PREVIEW_GAP: Duration = Duration::milliseconds(300);

/// How long a preview of `sound` plays. shortcut: the lengths of the bundled sounds as decision
/// "Alarm sound" gives them; read them from tolon-platform if the sounds change.
fn preview_length(sound: Sound) -> Duration {
    match sound {
        Sound::SoftBell => Duration::milliseconds(1500),
        Sound::Chime => Duration::seconds(1),
        Sound::None => Duration::zero(),
    }
}

pub struct Scheduler<H> {
    host: H,
    inner: Mutex<Inner>,
    wake: Notify,
}

impl<H: Host> Scheduler<H> {
    pub fn new(host: H) -> Self {
        let now = host.now();
        Self {
            host,
            inner: Mutex::new(Inner {
                open: None,
                last_tick: None,
                sound: Sound::SoftBell,
                repeat: true,
                degraded_logged: false,
                last_notice: notice_seed(now),
                last_preview: None,
                paused: None,
                stopped: false,
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
            let (stopped, pause_end) = {
                let inner = self.inner.lock().await;
                (inner.stopped, inner.paused.flatten())
            };
            if stopped {
                return;
            }
            if full {
                next_full = next_full_tick(now, pause_end);
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
        if inner.stopped {
            return report;
        }
        if full {
            report.woke = inner.last_tick.is_some_and(|t| now - t > WAKE_GAP);
            if report.woke {
                tracing::info!("wake from sleep detected; alarms that fell due are handled now");
            }
            inner.last_tick = Some(now);
            let zone = self.host.zone();
            let evaluated = load_inputs(db, zone, now).and_then(|inputs| {
                let occs = occurrences(&inputs, now, zone);
                let held = (inputs.pause, inputs.quiet.as_ref());
                let ticked = plan_and_tick(db, &occs, &inputs.alarm, held, now)?;
                Ok((inputs, occs, ticked))
            });
            match evaluated {
                Ok((inputs, occs, ticked)) => {
                    inner.sound = inputs.sound;
                    inner.repeat = inputs.repeat;
                    if inner.paused != Some(inputs.pause) {
                        inner.paused = Some(inputs.pause);
                        self.host.paused(inputs.pause);
                    }
                    report.fired = ticked.transitions.iter().filter(|t| t.fires()).count();
                    report.expired = ticked
                        .transitions
                        .iter()
                        .filter(|t| t.to == AlarmState::Expired)
                        .count();
                    self.drop_stale_meetings(&mut inner, &ticked.firing, now)
                        .await;
                    let paused = inputs.pause.is_some();
                    self.present(&mut inner, &ticked.firing, &occs, now, paused)
                        .await;
                }
                // The next tick tries again; nothing fires from a half-read store.
                Err(e) => tracing::warn!(error = %e, "scheduler tick failed"),
            }
        }
        self.housekeeping(&mut inner, now).await;
        report
    }

    /// Shows what is Firing. An open interrupt takes the meetings of its own start minute (AC3.5);
    /// another minute waits for it to close. A test interrupt gives way to a real one. While
    /// `paused`, nothing opens or grows, not even an alarm already Firing in the store (the first
    /// tick after a restart, a close that hands over to a waiting minute): it waits for the pause
    /// to end. An interrupt already on screen when the pause began stays until it is handled.
    async fn present(
        &self,
        inner: &mut Inner,
        firing: &[Alarm],
        occs: &[Occurrence],
        now: DateTime<Utc>,
        paused: bool,
    ) {
        if paused {
            return;
        }
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
        if let Some(open) = inner.open.as_mut() {
            open.links = links_of(group);
            open.show = AlarmShow::from_group(group);
        }
        self.show_open(inner, now).await;
        self.sound(inner, now);
    }

    async fn open_group(&self, inner: &mut Inner, group: &InterruptGroup, now: DateTime<Utc>) {
        let show = AlarmShow::from_group(group);
        inner.open = Some(new_open(show, group.minute, links_of(group), false, now));
        self.show_open(inner, now).await;
        self.sound(inner, now);
    }

    /// Opens the windows, or updates them, with what the open interrupt lists now, and applies the
    /// F4.5 rules. A changed list starts a new focus check and drops the old notification: its
    /// buttons no longer match what is on screen. A degraded system announces the new list at once;
    /// windows that cannot open leave the notification as the interrupt.
    async fn show_open(&self, inner: &mut Inner, now: DateTime<Utc>) {
        let Some(open) = inner.open.as_mut() else {
            return;
        };
        open.recheck_at = Some(now + FOCUS_RECHECK);
        open.notified = false;
        open.notice = None;
        let show = open.show.clone();
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
                    self.announce(inner);
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "could not open the interrupt window; using a notification");
                if let Some(open) = inner.open.as_mut() {
                    open.recheck_at = None;
                }
                self.announce(inner);
            }
        }
    }

    /// The F4.5 notification for the open interrupt, under a new identifier.
    fn announce(&self, inner: &mut Inner) {
        inner.last_notice = inner.last_notice.wrapping_add(1);
        let id = inner.last_notice;
        if let Some(open) = inner.open.as_mut() {
            open.notified = true;
            open.notice = Some(id);
            self.host.notify(&open.show, id);
        }
    }

    /// A meeting in the open interrupt whose alarm is no longer Firing (expired, or the meeting
    /// was deleted or ignored) leaves it; an interrupt with no meeting left closes.
    async fn drop_stale_meetings(&self, inner: &mut Inner, firing: &[Alarm], now: DateTime<Utc>) {
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
            self.show_open(inner, now).await;
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
                tracing::warn!("the interrupt has no keyboard focus; using a notification too");
                self.announce(inner);
            }
        }
        if inner.repeat
            && inner
                .open
                .as_ref()
                .is_some_and(|o| o.last_sound + REPEAT_EVERY <= now)
        {
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

    /// The panel's Join (`occurrences_join`): opens the link or event page of one of today's
    /// meetings, resolved from `key` in Rust and re-checked like `alarms_join`. Alarm states do not
    /// change. A key that is not one of today's meetings, or a meeting with nothing to open, is
    /// `not_found`; a link that fails the re-check is `internal`. Either way nothing opens.
    pub fn join_listed(&self, db: &Db, key: &str) -> anyhow::Result<()> {
        interrupt::parse_key(key)?;
        let today = pipeline::today_judged(db, self.host.now(), self.host.zone())?;
        let occ = today
            .iter()
            .map(|(o, _)| o)
            .find(|o| o.key.to_string() == key)
            .ok_or(AppError::NOT_FOUND)?;
        let url = match occ.join {
            JoinTarget::None => return Err(AppError::NOT_FOUND.into()),
            _ => link_of(occ).ok_or(AppError::INTERNAL)?,
        };
        self.host
            .open_url(&url)
            .map_err(|_| AppError::INTERNAL.into())
    }

    /// The panel's Snooze and Dismiss for the next meeting (AC7.2). `key` must be one of today's
    /// meetings, else `not_found`. When its interrupt is on screen this is exactly the interrupt's
    /// button ([`Scheduler::act`]); otherwise it acts ahead on the stored alarms (`apply_ahead`).
    /// Join goes through [`Scheduler::join_listed`] instead, and "already there" is not offered.
    ///
    /// `true` when a Snooze ended the reminders: it would have come after the meeting's window.
    pub async fn act_listed(&self, db: &Db, key: &str, act: Act) -> anyhow::Result<bool> {
        if matches!(act, Act::Join | Act::AlreadyThere) {
            return Err(AppError::NOT_FOUND.into());
        }
        interrupt::parse_key(key)?;
        let now = self.host.now();
        let today = pipeline::today_judged(db, now, self.host.zone())?;
        if !today.iter().any(|(o, _)| o.key.to_string() == key) {
            return Err(AppError::NOT_FOUND.into());
        }
        let mut inner = self.inner.lock().await;
        let on_screen = inner
            .open
            .as_ref()
            .is_some_and(|o| !o.test && o.show.meetings.iter().any(|m| m.key == key));
        if on_screen {
            return self
                .act_locked(&mut inner, db, key, act)
                .await
                .map(|()| false);
        }
        let ended = apply_ahead(db, key, act, now)?;
        drop(inner);
        self.wake();
        Ok(ended)
    }

    /// Quit (AC7.4): closes any interrupt, stops the sound and the loop, and waits for the store
    /// lock, so no tick is half written when the process exits. Later steps do nothing.
    pub async fn shutdown(&self, db: &Db) {
        let mut inner = self.inner.lock().await;
        inner.stopped = true;
        if inner.open.is_some() {
            self.close(&mut inner);
        }
        self.host.stop_sound();
        drop(pipeline::lock(db));
        self.wake();
    }

    /// One shot of `sound`, for the settings page's Play button (AC6.3).
    /// Dropped while an interrupt is open (its own sound is playing), while the last preview
    /// still plays, and within 300 ms of it. `true` when it played.
    pub async fn preview(&self, sound: Sound) -> bool {
        let now = self.host.now();
        let mut inner = self.inner.lock().await;
        let busy = inner
            .last_preview
            .is_some_and(|(at, last)| now < at + preview_length(last).max(PREVIEW_GAP));
        if inner.open.is_some() || busy {
            return false;
        }
        inner.last_preview = Some((now, sound));
        self.host.play(sound);
        true
    }

    /// Shows F10's test alarm. Any interrupt that is up, real or another test alarm (a second click
    /// on "Try a test alarm"), is left alone.
    pub async fn show_test(&self) -> anyhow::Result<()> {
        let now = self.host.now();
        let mut inner = self.inner.lock().await;
        if inner.open.is_some() {
            return Ok(());
        }
        let show = AlarmShow::sample(now);
        let opened = self.host.open(&show).await?;
        inner.open = Some(new_open(show, now, HashMap::new(), true, now));
        if opened.degraded.is_some() {
            self.announce(&mut inner);
        }
        self.sound(&mut inner, now);
        // The loop must know the new deadlines (focus re-check, sound repeat): it sleeps for the
        // ones it knew when it last looked.
        self.wake();
        Ok(())
    }

    /// What a button of the F4.5 notification does: Join joins, and Snooze snoozes, the first
    /// meeting of the interrupt that notification announced. A notification that is not the current
    /// one (an earlier interrupt, or the same one before its meetings changed) does nothing.
    pub async fn act_on_notice(&self, db: &Db, notice: i32, act: Act) -> anyhow::Result<()> {
        // One lock from the check to the action: a regroup cannot land in between and hand a stale
        // button a larger group.
        let mut inner = self.inner.lock().await;
        let key = inner
            .open
            .as_ref()
            .filter(|o| o.notice == Some(notice))
            .and_then(|o| o.show.meetings.first().map(|m| m.key.clone()));
        match key {
            Some(key) => self.act_locked(&mut inner, db, &key, act).await,
            None => {
                tracing::debug!("a notification action for an interrupt that is gone");
                Ok(())
            }
        }
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
        let mut inner = self.inner.lock().await;
        self.act_locked(&mut inner, db, key, act).await
    }

    async fn act_locked(
        &self,
        inner: &mut Inner,
        db: &Db,
        key: &str,
        act: Act,
    ) -> anyhow::Result<()> {
        interrupt::parse_key(key)?;
        let now = self.host.now();
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
        // A close may open a waiting minute and a partial join re-opens: new deadlines either way.
        self.wake();
        if rest.is_empty() {
            self.close(inner);
            self.present_waiting(inner, db, now).await;
            return Ok(());
        }
        let Some(open) = inner.open.as_mut() else {
            return Ok(());
        };
        open.show.meetings = rest;
        open.links.remove(key);
        self.show_open(inner, now).await;
        Ok(())
    }

    /// After a close: a Firing meeting of another minute was waiting for the screen.
    async fn present_waiting(&self, inner: &mut Inner, db: &Db, now: DateTime<Utc>) {
        let waiting = stored_firing(db).and_then(|firing| {
            if firing.is_empty() {
                return Ok(None);
            }
            let inputs = load_inputs(db, self.host.zone(), now)?;
            let paused = inputs.pause.is_some();
            Ok(Some((
                firing,
                occurrences(&inputs, now, self.host.zone()),
                paused,
            )))
        });
        match waiting {
            Ok(Some((firing, occs, paused))) => {
                self.present(inner, &firing, &occs, now, paused).await
            }
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

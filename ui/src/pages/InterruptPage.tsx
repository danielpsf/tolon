import { useEffect, useRef, useState, type FocusEvent } from "react";
import { api, onAlarmHide, onAlarmShow } from "../api";
import type { AlarmShowDto, MeetingDto } from "../api-types";
import { Bell } from "../Bell";
import { copy } from "../copy";
import {
  countdownText,
  detailsText,
  handlesKey,
  hintText,
  keyAction,
  nextFocusIndex,
  perform,
  primaryAction,
  primaryLabel,
  SNOOZE_MINUTES,
  startClock,
  titleSize,
  type Action,
} from "../interrupt";
import { errorText } from "../view";

/** The current time, ticking every second (AC4.7). */
function useNow(): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => startClock(setNow), []);
  return now;
}

/** The interrupt for this window: pulled once on load, then replaced by `alarm:show` and cleared by `alarm:hide`. */
function useAlarm(): AlarmShowDto | null {
  const [alarm, setAlarm] = useState<AlarmShowDto | null>(null);
  useEffect(() => {
    let stopped = false;
    const offs: (() => void)[] = [];
    const keep = (off: () => void) => (stopped ? off() : offs.push(off));
    // Listen first, then ask: an `alarm:show` sent while this window loaded must not be missed.
    // No IPC (plain browser, tests) leaves the page empty.
    onAlarmShow(setAlarm).then(keep, () => undefined);
    onAlarmHide(() => setAlarm(null)).then(keep, () => undefined);
    api.alarmsCurrent().then(
      (current) => !stopped && setAlarm((shown) => shown ?? current),
      () => undefined,
    );
    return () => {
      stopped = true;
      offs.forEach((off) => off());
    };
  }, []);
  return alarm;
}

/** Every button the focus trap cycles through, in tab order. */
const CONTROLS = "[data-action]";

/** What the button holding the focus does, or null when focus is on the page itself. */
const actionOf = (el: Element | null): Action | null =>
  (el?.closest<HTMLElement>(CONTROLS)?.dataset.action as Action | undefined) ?? null;

/** The page: waits for an interrupt, then shows it. A new interrupt is a new view, so no state carries over. */
export function InterruptPage() {
  const alarm = useAlarm();
  const screen = new URLSearchParams(window.location.search).get("screen");
  const { interrupt: t } = copy().t;
  const first = alarm?.meetings[0];
  if (!alarm || !first) return <main className="tl-int" aria-label={t.region} data-screen={screen} />;
  return <InterruptView key={first.key} alarm={alarm} first={first} screen={screen} />;
}

function InterruptView({ alarm, first, screen }: { alarm: AlarmShowDto; first: MeetingDto; screen: string | null }) {
  const c = copy();
  const { interrupt: t } = c.t;
  const now = useNow();
  const root = useRef<HTMLElement>(null);
  const busy = useRef(false);
  const [error, setError] = useState<string | null>(null);
  const [focused, setFocused] = useState<Action | null>(null);

  // The interrupt opens on its primary button (AC4.4).
  useEffect(() => {
    root.current?.querySelector<HTMLElement>(CONTROLS)?.focus();
  }, []);

  /** One action, once: the window closes on success, so a second press must not send a second command. */
  const run = async (action: Action, meeting: MeetingDto) => {
    if (busy.current) return;
    busy.current = true;
    setError(null);
    try {
      await perform(action, meeting.key);
    } catch (e) {
      busy.current = false;
      setError(errorText(e, c));
    }
  };

  /** The meeting whose controls hold the focus, else the first one. */
  const meetingAt = (el: Element | null): MeetingDto =>
    alarm.meetings.find((m) => m.key === el?.closest<HTMLElement>("[data-key]")?.dataset.key) ?? first;

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Tab") {
      // The focus trap: Tab and Shift+Tab cycle through the controls and never leave them.
      e.preventDefault();
      const all = [...(root.current?.querySelectorAll<HTMLElement>(CONTROLS) ?? [])];
      const next = nextFocusIndex(all.indexOf(document.activeElement as HTMLElement), all.length, e.shiftKey);
      all[next]?.focus();
      return;
    }
    const wanted = keyAction(e);
    // Space on Snooze or Dismiss is left to the button: it must not mark the meeting as joined.
    if (!wanted || !handlesKey(wanted, actionOf(document.activeElement))) return;
    // A handled key never reaches the focused button, so Space cannot also click it.
    e.preventDefault();
    const meeting = meetingAt(document.activeElement);
    if (wanted !== "enter") return void run(wanted, meeting);
    // Enter does what the focused button says; with no button focused it takes the primary action.
    const button = document.activeElement?.closest<HTMLElement>(CONTROLS);
    if (button) button.click();
    else void run(primaryAction(meeting.join), meeting);
  };

  // The listeners are added once and call the latest handler, so a new payload needs no re-subscribe.
  const latest = useRef(onKeyDown);
  useEffect(() => {
    latest.current = onKeyDown;
  });
  useEffect(() => {
    const down = (e: KeyboardEvent) => latest.current(e);
    // Some webviews click a button on Space key-up; the key-down above already took the key,
    // unless focus is on Snooze or Dismiss, where that click is the point.
    const up = (e: KeyboardEvent) => {
      const space = e.key === " " || e.key === "Spacebar";
      if (space && handlesKey("already_there", actionOf(document.activeElement))) e.preventDefault();
    };
    window.addEventListener("keydown", down, true);
    window.addEventListener("keyup", up, true);
    return () => {
      window.removeEventListener("keydown", down, true);
      window.removeEventListener("keyup", up, true);
    };
  }, []);

  // Focus never rests outside the controls (a click on the backdrop, the window regaining focus).
  const keepFocus = (e: FocusEvent) => {
    if (!root.current?.contains(e.relatedTarget as Node | null)) {
      queueMicrotask(() => root.current?.querySelector<HTMLElement>(CONTROLS)?.focus());
    }
  };

  const single = alarm.meetings.length === 1;
  const titleOf = (m: MeetingDto) => (alarm.test ? t.testTitle : m.title);
  const start = Date.parse(first.start);

  const joinButton = (m: MeetingDto, size: "xl" | "lg") => (
    <button
      type="button"
      className={`sm-btn tl-primary ${size}`}
      data-action={primaryAction(m.join)}
      onClick={() => void run(primaryAction(m.join), m)}
    >
      {primaryLabel(c, m)}
    </button>
  );

  return (
    <main
      ref={root}
      className="tl-int"
      role="alertdialog"
      aria-modal="true"
      // One meeting: the dialog is announced by its title and details. Several: by its name.
      aria-label={single ? undefined : t.region}
      aria-labelledby={single ? "tl-int-title" : undefined}
      aria-describedby={single ? "tl-int-details" : undefined}
      data-screen={screen}
      onFocus={(e) => setFocused(actionOf(e.target))}
      onBlur={keepFocus}
    >
      <header className="tl-int-top">
        <span>{c.formatTime(now)}</span>
        <span className="tl-int-count">
          <span>{countdownText(c, start, now)}</span>
          <Bell />
        </span>
      </header>

      <section className="tl-int-body">
        {single ? (
          <div className="tl-int-head">
            <h1 id="tl-int-title" className="sm-display-xl tl-int-title" data-len={titleSize(titleOf(first))}>
              {/* <bdi>: a feed's title keeps its own direction inside a right-to-left page. */}
              <bdi>{titleOf(first)}</bdi>
            </h1>
            <p id="tl-int-details" className="tl-int-details">
              {detailsText(c, first)}
            </p>
          </div>
        ) : (
          <ul className="tl-int-list">
            {alarm.meetings.map((m) => (
              <li key={m.key} className="tl-int-item" data-key={m.key}>
                <h2 className="sm-display-l">
                  <bdi>{titleOf(m)}</bdi>
                </h2>
                <p className="tl-int-details">{detailsText(c, m)}</p>
                {joinButton(m, "lg")}
              </li>
            ))}
          </ul>
        )}
        <p className="tl-int-hint">{hintText(c, primaryAction(first.join), handlesKey("already_there", focused))}</p>
      </section>

      <footer className="tl-int-actions">
        {single && joinButton(first, "xl")}
        <button type="button" className="sm-btn xl" data-action="snooze" onClick={() => void run("snooze", first)}>
          {t.snooze(SNOOZE_MINUTES)}
        </button>
        <button type="button" className="sm-btn xl" data-action="dismiss" onClick={() => void run("dismiss", first)}>
          {t.dismiss}
        </button>
        <p className="tl-int-consequence">{t.dismissConsequence}</p>
      </footer>
      {error && (
        <p role="alert" className="tl-int-error">
          {error}
        </p>
      )}
    </main>
  );
}

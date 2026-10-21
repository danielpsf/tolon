import { useEffect, useReducer, useRef, useState, type FocusEvent } from "react";
import { api, onAlarmHide, onAlarmShow } from "../api";
import type { AlarmShowDto, InterruptColor, MeetingDto } from "../api-types";
import { PeekMark } from "../PeekMark";
import { copy } from "../copy";
import {
  countdownText,
  detailsText,
  handlesKey,
  hintText,
  keyAction,
  nextAlarmView,
  nextFocusIndex,
  NO_ALARM,
  perform,
  primaryAction,
  primaryParts,
  SNOOZE_MINUTES,
  startClock,
  titleSize,
  type Action,
  type AlarmView,
} from "../interrupt";
import { colorAttr, PREVIEW_TOKENS, tokensFor } from "../reminder-colors";
import { useSettings } from "../store";
import { errorText } from "../view";
import { PeekScene, peekColor } from "./PeekScene";

/** The current time, ticking every second (AC4.7). */
function useNow(): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => startClock(setNow), []);
  return now;
}

/**
 * The interrupt for this window: pulled once on load, then replaced by `alarm:show` and cleared by
 * `alarm:hide`. The window stays alive between alarms (it is only hidden), so the page keeps listening
 * for the next one; `shown` says which alarm this is (see `nextAlarmView`).
 */
function useAlarm(): AlarmView {
  const [view, dispatch] = useReducer(nextAlarmView, NO_ALARM);
  useEffect(() => {
    let stopped = false;
    const offs: (() => void)[] = [];
    const keep = (off: () => void) => (stopped ? off() : offs.push(off));
    // Listen first, then ask: an `alarm:show` sent while this window loaded must not be missed.
    // No IPC (plain browser, tests) leaves the page empty.
    onAlarmShow((alarm) => dispatch({ type: "show", alarm })).then(keep, () => undefined);
    onAlarmHide(() => dispatch({ type: "hide" })).then(keep, () => undefined);
    api.alarmsCurrent().then(
      (alarm) => !stopped && dispatch({ type: "current", alarm }),
      () => undefined,
    );
    return () => {
      stopped = true;
      offs.forEach((off) => off());
    };
  }, []);
  return view;
}

/** Every button the focus trap cycles through, in tab order. */
const CONTROLS = "[data-action]";

/** What the button holding the focus does, or null when focus is on the page itself. */
const actionOf = (el: Element | null): Action | null =>
  (el?.closest<HTMLElement>(CONTROLS)?.dataset.action as Action | undefined) ?? null;

/** The page: waits for an interrupt, then shows it. A new interrupt is a new view, so no state carries over. */
export function InterruptPage() {
  const { alarm, shown } = useAlarm();
  const screen = new URLSearchParams(window.location.search).get("screen");
  // The reminder colour (L-043) from `settings_get`, and live from `settings:changed` while it is open.
  const color = colorAttr(useSettings((s) => s.settings?.interrupt_color));
  // Peek (L-050) keeps the page in the window's appearance and pins only its hill and Join.
  const peek = useSettings((s) => s.settings?.style) === "peek";
  const { interrupt: t } = copy().t;
  const first = alarm?.meetings[0];
  if (!alarm || !first)
    return (
      <main className="tl-int" aria-label={t.region} data-screen={screen} data-color={color} data-tokens={peek ? undefined : tokensFor(color)}>
        <style>{PREVIEW_TOKENS}</style>
      </main>
    );
  // Keyed by the alarm's number, not its meeting: the window is reused, and a snoozed meeting that fires again has the same key.
  // An update of the alarm on screen keeps the number, so the person's focus stays where it is.
  return <InterruptView key={shown} alarm={alarm} first={first} screen={screen} color={color} peek={peek} />;
}

export function InterruptView({
  alarm,
  first,
  screen,
  color,
  peek,
}: {
  alarm: AlarmShowDto;
  first: MeetingDto;
  screen: string | null;
  color: InterruptColor;
  peek: boolean;
}) {
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
    // This view is rendered while its reused window is still hidden, where a webview may not take
    // focus yet: when the window is shown and gets focus, the primary button takes it again.
    const onWindowFocus = () => {
      if (!root.current?.contains(document.activeElement)) {
        root.current?.querySelector<HTMLElement>(CONTROLS)?.focus();
      }
    };
    window.addEventListener("focus", onWindowFocus);
    return () => window.removeEventListener("focus", onWindowFocus);
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

  const joinButton = (m: MeetingDto, size: "xl" | "lg") => {
    const [before, host, after] = primaryParts(c, m);
    return (
      <button
        type="button"
        className={`sm-btn tl-primary ${size}`}
        data-action={primaryAction(m.join)}
        data-color={peek ? peekColor(color) : undefined}
        data-tokens={peek ? "light" : undefined}
        onClick={() => void run(primaryAction(m.join), m)}
      >
        {/* One flex item: sm-btn's 8 px gap must not open between the words and the host. */}
        <span>
          {before}
          {host && (
            // D3: a long host loses its start to an ellipsis; its end, the site, stays readable.
            <span className="tl-host">
              <bdi dir="ltr">{host}</bdi>
            </span>
          )}
          {after}
        </span>
        {/* Peek's key chip: the key hint below says the same, so screen readers skip it. */}
        {peek && size === "xl" && (
          <kbd className="tl-peek-key" aria-hidden="true">
            Enter
          </kbd>
        )}
      </button>
    );
  };

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
      data-color={color}
      // Every choice but Default is pinned to Swiss Moss's light values, in a dark window too (L-043).
      data-tokens={peek ? undefined : tokensFor(color)}
      onFocus={(e) => setFocused(actionOf(e.target))}
      onBlur={keepFocus}
    >
      <style>{PREVIEW_TOKENS}</style>
      <header className="tl-int-top">
        <span>{c.formatTime(now)}</span>
        <span className="tl-int-count">
          <span>{countdownText(c, start, now)}</span>
          <PeekMark />
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
      {peek && <PeekScene color={color} say={t.peekSay} />}
      {error && (
        <p role="alert" className="tl-int-error">
          {error}
        </p>
      )}
    </main>
  );
}

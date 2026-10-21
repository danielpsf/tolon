import { useEffect, useRef, useState } from "react";
import { api, onAlarmHide } from "../api";
import type { TrayPlace } from "../api-types";
import { Bell } from "../Bell";
import { copy, type Messages } from "../copy";
import { firstCalendarAdded, hideAndEndFirstRun } from "../first-run";
import { SNOOZE_MINUTES } from "../interrupt";
import { errorText } from "../view";
import { AddCalendarForm } from "./CalendarsPage";

const MINUTE = 60_000;

/** A still picture of the interrupt (mockup 05): a sample meeting at 14:00 today, seen at 13:58. Not interactive. */
function Preview() {
  const c = copy();
  const { firstRun: t, interrupt: i } = c.t;
  const start = new Date();
  start.setHours(14, 0, 0, 0);
  const now = start.getTime() - 2 * MINUTE;
  const range = i.timeRange(c.formatTime(start), c.formatTime(start.getTime() + 45 * MINUTE));
  return (
    <figure className="tl-first-preview">
      <div className="tl-preview" aria-hidden="true">
        <div className="tl-preview-top">
          <span>{c.formatTime(now)}</span>
          <span>{c.startsIn(start, now)}</span>
        </div>
        <p className="sm-title tl-preview-title">{t.sampleTitle}</p>
        <p className="tl-preview-details">{i.details(range, "Google Meet", 0)}</p>
        <div className="tl-preview-actions">
          <span className="sm-btn sm tl-primary">{i.joinCall}</span>
          <span className="sm-btn sm">{i.snooze(SNOOZE_MINUTES)}</span>
          <span className="sm-btn sm">{i.dismiss}</span>
        </div>
      </div>
      <figcaption className="sm-caption">{t.previewLabel}</figcaption>
    </figure>
  );
}

/** The tray step's title for where the icon lives, or null while Rust has not answered. */
export function trayTitle(t: Messages["firstRun"], place: TrayPlace | null): string | null {
  if (place === null) return null;
  return place === "menu_bar" ? t.trayMenuBar : t.traySystemTray;
}

/** Where the tray icon lives on this OS, from Rust; "tray" when it cannot say. Null while asking. */
function useTrayPlace(): TrayPlace | null {
  const [place, setPlace] = useState<TrayPlace | null>(null);
  useEffect(() => {
    let live = true;
    api.appTrayPlace().then(
      (p) => live && setPlace(p),
      () => live && setPlace("tray"),
    );
    return () => {
      live = false;
    };
  }, []);
  return place;
}

/** True from a "Try a test alarm" click until its interrupt closes (`alarm:hide`), so a second click does nothing. */
function useTestAlarmOpen(): [boolean, (open: boolean) => void] {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    let stopped = false;
    let unlisten: (() => void) | undefined;
    onAlarmHide(() => setOpen(false)).then(
      (off) => {
        if (stopped) off();
        else unlisten = off;
      },
      () => undefined,
    );
    return () => {
      stopped = true;
      unlisten?.();
    };
  }, []);
  return [open, setOpen];
}

/**
 * First run (F10, mockup 05; decision "First run (L-018)"): add a calendar, try a test alarm, and where
 * Tolon lives from now on. A first calendar that syncs hides the window (AC10.2); one that fails moves to
 * the Calendars page with its error. The close button and "Skip for now" hide it before that.
 */
export function FirstRunPage() {
  const c = copy();
  const { firstRun: t } = c.t;
  const place = useTrayPlace();
  const title = trayTitle(t, place);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [testing, setTesting] = useTestAlarmOpen();
  const paste = useRef<HTMLButtonElement>(null);

  async function testAlarm() {
    if (testing) return;
    setTesting(true);
    setError(null);
    try {
      // A real interrupt with a sample meeting, from this window (AC10.1).
      await api.alarmsTest();
    } catch (e) {
      setTesting(false);
      setError(errorText(e, c));
    }
  }

  return (
    <main className="tl-first" aria-labelledby="first-run-title">
      <div className="tl-first-text">
        <span className="tl-first-bell">
          <Bell />
        </span>
        <header className="tl-head">
          <h1 className="sm-display-l" id="first-run-title">
            {t.title}
          </h1>
          <p className="sm-lead sm-muted">{t.lead}</p>
        </header>
        {/* role="list": WebKit drops the list role of an <ol> whose markers are styled away. */}
        <ol className="sm-steps tl-first-steps" role="list">
          <li>
            <div>
              <h2 className="sm-heading">{t.addTitle}</h2>
              <div className="sm-body">{t.addBody}</div>
            </div>
          </li>
          <li>
            <div>
              <h2 className="sm-heading">{t.tryTitle}</h2>
              <div className="sm-body">{t.tryBody}</div>
            </div>
          </li>
          <li>
            <div>
              {title ? (
                <h2 className="sm-heading">{title}</h2>
              ) : (
                // Holds the heading's line until Rust answers, so nothing jumps; no empty heading meanwhile.
                <div className="sm-heading" aria-hidden="true">
                  {"\u00a0"}
                </div>
              )}
              <div className="sm-body">{t.trayBody}</div>
            </div>
          </li>
        </ol>
        {error && (
          <p className="sm-alert signal" role="alert">
            {error}
          </p>
        )}
        <div className="sm-cluster">
          <button
            ref={paste}
            type="button"
            className="sm-btn primary"
            autoFocus
            aria-expanded={adding}
            aria-controls="add-calendar"
            onClick={() => setAdding(true)}
          >
            {t.pasteIcal}
          </button>
          <button
            type="button"
            className="sm-btn quiet"
            data-action="test-alarm"
            aria-disabled={testing}
            onClick={() => void testAlarm()}
          >
            {t.tryAlarm}
          </button>
          <button
            type="button"
            className="sm-btn quiet"
            data-action="skip"
            aria-describedby="skip-consequence"
            onClick={() => void hideAndEndFirstRun()}
          >
            {t.skip}
          </button>
        </div>
        <p className="sm-caption" id="skip-consequence">
          {t.skipConsequence}
        </p>
        {adding && (
          <AddCalendarForm
            onAdded={(calendar) => void firstCalendarAdded(calendar)}
            onCancel={() => {
              setAdding(false);
              paste.current?.focus();
            }}
          />
        )}
        <p className="sm-caption">{t.footer}</p>
      </div>
      <Preview />
    </main>
  );
}

import { useEffect, useState } from "react";
import { api } from "../api";
import type { TrayPlace } from "../api-types";
import { Bell } from "../Bell";
import { copy, type Messages } from "../copy";
import { finishFirstRun } from "../first-run";
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

/** The tray step's title for where the icon lives; a blank line while Rust has not answered, so nothing jumps. */
export function trayTitle(t: Messages["firstRun"], place: TrayPlace | null): string {
  if (place === null) return " ";
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

/**
 * First run (F10, mockup 05): add a calendar, try a test alarm, and where Tolon lives from now on.
 * The first calendar hides the window (AC10.2); closing it before that is the skip.
 */
export function FirstRunPage() {
  const c = copy();
  const { firstRun: t } = c.t;
  const place = useTrayPlace();
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function testAlarm() {
    setError(null);
    try {
      // A real interrupt with a sample meeting, from this window (AC10.1).
      await api.alarmsTest();
    } catch (e) {
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
        <ol className="sm-steps tl-first-steps">
          <li>
            <span>
              <h2 className="sm-heading">{t.addTitle}</h2>
              <span className="sm-body">{t.addBody}</span>
            </span>
          </li>
          <li>
            <span>
              <h2 className="sm-heading">{t.tryTitle}</h2>
              <span className="sm-body">{t.tryBody}</span>
            </span>
          </li>
          <li>
            <span>
              <h2 className="sm-heading">{trayTitle(t, place)}</h2>
              <span className="sm-body">{t.trayBody}</span>
            </span>
          </li>
        </ol>
        {error && (
          <p className="sm-alert signal" role="alert">
            {error}
          </p>
        )}
        <div className="sm-cluster">
          <button
            type="button"
            className="sm-btn primary"
            aria-expanded={adding}
            aria-controls="add-calendar"
            onClick={() => setAdding(true)}
          >
            {t.pasteIcal}
          </button>
          <button type="button" className="sm-btn quiet" data-action="test-alarm" onClick={() => void testAlarm()}>
            {t.tryAlarm}
          </button>
        </div>
        {adding && <AddCalendarForm onAdded={() => void finishFirstRun()} onCancel={() => setAdding(false)} />}
        <p className="sm-caption">{t.footer}</p>
      </div>
      <Preview />
    </main>
  );
}

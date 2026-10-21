import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { TrayPlace } from "../api-types";
import { PeekMark } from "../PeekMark";
import { copy, type Messages } from "../copy";
import { firstCalendarAdded, focusAtTop, hideAndEndFirstRun } from "../first-run";
import { colorAttr } from "../reminder-colors";
import { useSettings } from "../store";
import { errorText } from "../view";
import { AlarmPreview, useTestAlarmOpen } from "./AlarmPreview";
import { AddCalendarForm, GoogleSignIn, useGoogleAvailable } from "./CalendarsPage";

/** The tray step's title for where the icon lives, or null while Rust has not answered. */
export function trayTitle(t: Messages["firstRun"], place: TrayPlace | null): string | null {
  if (place === null) return null;
  return place === "menu_bar" ? t.trayMenuBar : t.traySystemTray;
}

/** How to bring this window back when the tray icon is hidden: Finder or Spotlight on macOS, the app list elsewhere. */
export function trayHidden(t: Messages["firstRun"], place: TrayPlace): string {
  return place === "menu_bar" ? t.trayHiddenMenuBar : t.trayHiddenSystemTray;
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
  const color = colorAttr(useSettings((s) => s.settings?.interrupt_color));
  const google = useGoogleAvailable();

  // Focus on the primary button, never "Try a test alarm" (Enter would take over the screen), and
  // without the scroll that autoFocus does: the window opens at the top (D6, D8).
  useEffect(() => focusAtTop(paste.current), []);

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
        <span className="tl-first-mark">
          <PeekMark />
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
              <div className="sm-body">{google ? t.addBodyGoogle : t.addBody}</div>
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
              {/* The way back when the icon is hidden (decision "Reaching Tolon without the tray"). */}
              {place && <div className="sm-body">{trayHidden(t, place)}</div>}
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
        {google && <GoogleSignIn onAdded={(calendar) => void firstCalendarAdded(calendar)} />}
        <p className="sm-caption">{t.footer}</p>
      </div>
      {/* In the reminder colour the person chose (L-043), as the test alarm will be. */}
      <AlarmPreview color={color} />
    </main>
  );
}

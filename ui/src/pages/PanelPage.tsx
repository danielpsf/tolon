// The tray panel (F7, mockup 02): the next meeting with Join, Snooze and Dismiss, the rest of
// today, Pause for 1 hour, Settings and Quit. Every action is a command; the panel computes nothing
// about alarms.
import { useCallback, useEffect, useRef, useState } from "react";
import { api, onOccurrencesChanged, onPaused } from "../api";
import type { ListedDto, PauseDto } from "../api-types";
import { PeekMark } from "../PeekMark";
import { copy } from "../copy";
import { detailsText, primaryLabel, SNOOZE_MINUTES } from "../interrupt";
import {
  firstLine,
  focusFirstAction,
  hidesPanel,
  nextOf,
  pausedText,
  remaining,
  rowTime,
  rowWhere,
  summaryText,
} from "../panel";
import { useSettings } from "../store";
import { useUpdate } from "../update";
import { errorText } from "../view";

/** Pause for 1 hour (AC7.2). */
const PAUSE_MINUTES = 60;

/** Today's meetings and the pause, reloaded on every change and whenever the panel shows. */
function usePanelData() {
  const [items, setItems] = useState<ListedDto[]>([]);
  const [pause, setPause] = useState<PauseDto>({ paused_until: null });
  const [now, setNow] = useState(Date.now);
  const reload = useCallback(() => {
    api.occurrencesToday().then((list) => {
      setItems(list);
      setNow(Date.now());
    }, () => undefined);
    api.appStatus().then(setPause, () => undefined);
  }, []);
  useEffect(() => {
    let stopped = false;
    const offs: (() => void)[] = [];
    const keep = (off: () => void) => (stopped ? off() : offs.push(off));
    // No IPC (plain browser, tests) leaves the panel empty.
    onOccurrencesChanged(reload).then(keep, () => undefined);
    onPaused(setPause).then(keep, () => undefined);
    // The panel window stays loaded while hidden: showing it focuses it, and the list catches up.
    window.addEventListener("focus", reload);
    const timer = setInterval(() => setNow(Date.now()), 30_000);
    reload();
    return () => {
      stopped = true;
      offs.forEach((off) => off());
      window.removeEventListener("focus", reload);
      clearInterval(timer);
    };
  }, [reload]);
  return { items, pause, setPause, now };
}

export function PanelPage() {
  const c = copy();
  const { panel: t, interrupt: ti } = c.t;
  const settings = useSettings((s) => s.settings);
  const { items, pause, setPause, now } = usePanelData();
  const [update] = useUpdate();
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const root = useRef<HTMLElement>(null);

  // Esc hides the panel; each time it shows (the window takes focus) focus starts on the first action.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!hidesPanel(e)) return;
      e.preventDefault();
      api.appHidePanel().catch(() => undefined);
    };
    const onFocus = () => focusFirstAction(root.current);
    window.addEventListener("keydown", onKey);
    window.addEventListener("focus", onFocus);
    focusFirstAction(root.current);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("focus", onFocus);
    };
  }, []);

  const run = async (action: () => Promise<unknown>) => {
    setError(null);
    setNotice(null);
    try {
      await action();
    } catch (e) {
      setError(errorText(e, c));
    }
  };

  const snooze = (key: string) =>
    run(async () => {
      const { ended } = await api.occurrencesSnooze(key, SNOOZE_MINUTES);
      if (ended) setNotice(t.snoozeEnded);
    });

  const next = nextOf(items, now);
  const later = remaining(items, now).filter((m) => m.key !== next?.key);

  return (
    <main ref={root} className="tl-panel" aria-label={t.label}>
      <header className="tl-panel-top">
        <span className="tl-brand">
          <PeekMark />
          <span>{c.t.app.windowTitle}</span>
        </span>
        {pause.paused_until && (
          <span className="tl-panel-paused" role="status">
            {pausedText(c, pause.paused_until)}
          </span>
        )}
      </header>

      {next ? (
        <section className="tl-panel-next" aria-labelledby="tl-panel-title">
          <p className="sm-caption sm-muted">{firstLine(c, next, now)}</p>
          <h1 id="tl-panel-title" className="sm-display-m tl-panel-title">
            <bdi>{next.title}</bdi>
          </h1>
          <p className="sm-body sm-muted">{detailsText(c, next)}</p>
          <div className="tl-panel-actions">
            {next.join.kind !== "none" && (
              <button type="button" className="sm-btn primary" onClick={() => void run(() => api.occurrencesJoin(next.key))}>
                {primaryLabel(c, next)}
              </button>
            )}
            <button type="button" className="sm-btn" onClick={() => void snooze(next.key)}>
              {ti.snooze(SNOOZE_MINUTES)}
            </button>
            <button
              type="button"
              className="sm-btn quiet"
              aria-describedby="tl-panel-dismiss"
              onClick={() => void run(() => api.occurrencesDismiss(next.key))}
            >
              {ti.dismiss}
            </button>
          </div>
          <p id="tl-panel-dismiss" className="sm-caption sm-muted">
            {t.dismissConsequence}
          </p>
        </section>
      ) : (
        <p className="sm-body sm-muted tl-panel-next">{t.noMore}</p>
      )}

      {later.length > 0 && (
        <ul className="sm-list tl-panel-list" aria-label={t.later}>
          {later.map((m) => (
            <li key={m.key} className={m.ignored ? "tl-panel-ignored" : undefined}>
              <span className="tl-panel-time">{rowTime(c, m)}</span>
              <span className="tl-panel-name">
                <bdi>{m.title}</bdi>
              </span>
              {m.ignored ? <span className="sm-tag">{t.ignored}</span> : <span className="sm-caption sm-muted">{rowWhere(c, m)}</span>}
            </li>
          ))}
        </ul>
      )}

      <footer className="tl-panel-foot">
        {error && (
          <p role="alert" className="sm-caption tl-panel-error">
            {error}
          </p>
        )}
        {notice && (
          <p role="status" className="sm-caption">
            {notice}
          </p>
        )}
        {/* F11: one quiet line while an update waits; the buttons are on the About page. */}
        {update?.available && !update.installing && (
          <p role="status" className="sm-caption" data-update>
            {t.update(update.available, c.t.nav.label, c.t.nav.about)}
          </p>
        )}
        <div className="tl-panel-actions">
          {pause.paused_until ? (
            <button type="button" className="sm-btn" onClick={() => void run(async () => setPause(await api.appResume()))}>
              {t.resume}
            </button>
          ) : (
            <button
              type="button"
              className="sm-btn"
              onClick={() => void run(async () => setPause(await api.appPause(PAUSE_MINUTES)))}
            >
              {t.pause}
            </button>
          )}
          <button type="button" className="sm-btn" onClick={() => void run(() => api.appShowSettings())}>
            {t.settings}
          </button>
          <button type="button" className="sm-btn quiet" onClick={() => void run(() => api.appQuit())}>
            {t.quit}
          </button>
        </div>
        {settings && <p className="sm-caption sm-muted">{summaryText(c, settings)}</p>}
      </footer>
    </main>
  );
}

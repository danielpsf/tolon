// The tray panel (F7, mockup 02): the next meeting with Join, Snooze and Dismiss, the rest of
// today, Pause for 1 hour, Settings and Quit. Every action is a command; the panel computes nothing
// about alarms.
import { useCallback, useEffect, useState } from "react";
import { api, onOccurrencesChanged, onPaused } from "../api";
import type { ListedDto, PauseDto } from "../api-types";
import { Bell } from "../Bell";
import { copy } from "../copy";
import { detailsText, primaryLabel, SNOOZE_MINUTES } from "../interrupt";
import { firstLine, nextOf, pausedText, remaining, rowTime, rowWhere, summaryText } from "../panel";
import { useSettings } from "../store";
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
  const [error, setError] = useState<string | null>(null);

  const run = async (action: () => Promise<unknown>) => {
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(errorText(e, c));
    }
  };

  const next = nextOf(items, now);
  const later = remaining(items, now).filter((m) => m.key !== next?.key);

  return (
    <main className="tl-panel" aria-label={t.label}>
      <header className="tl-panel-top">
        <span className="tl-brand">
          <Bell />
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
            <button
              type="button"
              className="sm-btn"
              onClick={() => void run(() => api.occurrencesSnooze(next.key, SNOOZE_MINUTES))}
            >
              {ti.snooze(SNOOZE_MINUTES)}
            </button>
            <button type="button" className="sm-btn quiet" onClick={() => void run(() => api.occurrencesDismiss(next.key))}>
              {ti.dismiss}
            </button>
          </div>
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

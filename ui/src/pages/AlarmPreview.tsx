import { useEffect, useState } from "react";
import { onAlarmHide } from "../api";
import type { InterruptColor } from "../api-types";
import { copy } from "../copy";
import { SNOOZE_MINUTES, startClock } from "../interrupt";
import { PREVIEW_TOKENS, tokensFor } from "../reminder-colors";
import { peekColor, PeekScene } from "./PeekScene";

const MINUTE = 60_000;

/**
 * The preview's clock (L-062). `now` is the first render's time, `tick` calls back every second, so a test
 * hands in a fixed instant and a live page shows the real time. The sample meeting is the test alarm's: it
 * starts 2 minutes from `now`, as `alarms_test` does (`MeetingDto::sample`).
 */
export interface PreviewClock {
  now: () => number;
  tick: (onTick: (now: number) => void) => () => void;
}

export const liveClock: PreviewClock = { now: () => Date.now(), tick: (onTick) => startClock(onTick) };

/** The clock to the minute: the preview shows minutes, so a second's tick changes nothing and renders nothing. */
function usePreviewNow(clock: PreviewClock): number {
  const [now, setNow] = useState(() => Math.floor(clock.now() / MINUTE) * MINUTE);
  useEffect(() => clock.tick((t) => setNow(Math.floor(t / MINUTE) * MINUTE)), [clock]);
  return now;
}

/**
 * A still picture of the interrupt (mockup 05): a sample meeting 2 minutes from now, in the reminder colour
 * `color` (L-043). Default draws in `tokens` (the window's appearance when unset); every other colour is
 * pinned to light, as on the interrupt. With `peek` (L-062) it is the small Peek card: the page follows
 * `tokens`, and the hill, Join and the bubble take the reminder colour pinned to light, as the Peek reminder
 * does. Not interactive.
 */
export function AlarmPreview({
  color,
  tokens,
  peek = false,
  clock = liveClock,
}: {
  color: InterruptColor;
  tokens?: "light" | "dark";
  peek?: boolean;
  clock?: PreviewClock;
}) {
  const c = copy();
  const { firstRun: t, interrupt: i } = c.t;
  const now = usePreviewNow(clock);
  const start = now + 2 * MINUTE;
  const range = i.timeRange(c.formatTime(start), c.formatTime(start + 45 * MINUTE));
  const top = (
    <div className="tl-preview-top">
      <span>{c.formatTime(now)}</span>
      <span>{c.startsIn(start, now)}</span>
    </div>
  );
  const title = <p className="sm-title tl-preview-title">{t.sampleTitle}</p>;
  const details = <p className="tl-preview-details">{i.details(range, "Google Meet", 0)}</p>;
  const pair = (
    // Snooze and Dismiss wrap as a pair, so a long locale never leaves Dismiss alone on a row (B3).
    <span className="tl-preview-pair">
      <span className="sm-btn sm">{i.snooze(SNOOZE_MINUTES)}</span>
      <span className="sm-btn sm">{i.dismiss}</span>
    </span>
  );
  return (
    <figure className="tl-first-preview">
      <style>{PREVIEW_TOKENS}</style>
      {peek ? (
        <div className="tl-preview tl-preview-peek" aria-hidden="true" data-tokens={tokens}>
          <div className="tl-preview-page">
            {top}
            {title}
            {details}
            <div className="tl-preview-actions">
              <span className="sm-btn sm tl-primary" data-color={peekColor(color)} data-tokens="light">
                {t.previewJoin}
              </span>
              {pair}
            </div>
          </div>
          <PeekScene color={color} say={i.peekSay} card />
        </div>
      ) : (
        <div className="tl-preview" aria-hidden="true" data-color={color} data-tokens={tokensFor(color, tokens)}>
          {top}
          {title}
          {details}
          <div className="tl-preview-actions">
            <span className="sm-btn sm tl-primary">{t.previewJoin}</span>
            {pair}
          </div>
        </div>
      )}
      <figcaption className="sm-caption">{t.previewLabel}</figcaption>
    </figure>
  );
}

/** True from a test alarm click until its interrupt closes (`alarm:hide`), so a second click does nothing. */
export function useTestAlarmOpen(): [boolean, (open: boolean) => void] {
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

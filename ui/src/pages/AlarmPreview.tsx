import { useEffect, useState } from "react";
import { onAlarmHide } from "../api";
import type { InterruptColor } from "../api-types";
import { copy } from "../copy";
import { SNOOZE_MINUTES } from "../interrupt";
import { PREVIEW_TOKENS, tokensFor } from "../reminder-colors";

const MINUTE = 60_000;

/**
 * A still picture of the interrupt (mockup 05): a sample meeting at 14:00 today, seen at 13:58, in the
 * reminder colour `color` (L-043). Default draws in `tokens` (the window's appearance when unset); every
 * other colour is pinned to light, as on the interrupt. Not interactive.
 */
export function AlarmPreview({ color, tokens }: { color: InterruptColor; tokens?: "light" | "dark" }) {
  const c = copy();
  const { firstRun: t, interrupt: i } = c.t;
  const start = new Date();
  start.setHours(14, 0, 0, 0);
  const now = start.getTime() - 2 * MINUTE;
  const range = i.timeRange(c.formatTime(start), c.formatTime(start.getTime() + 45 * MINUTE));
  return (
    <figure className="tl-first-preview">
      <style>{PREVIEW_TOKENS}</style>
      <div className="tl-preview" aria-hidden="true" data-color={color} data-tokens={tokensFor(color, tokens)}>
        <div className="tl-preview-top">
          <span>{c.formatTime(now)}</span>
          <span>{c.startsIn(start, now)}</span>
        </div>
        <p className="sm-title tl-preview-title">{t.sampleTitle}</p>
        <p className="tl-preview-details">{i.details(range, "Google Meet", 0)}</p>
        <div className="tl-preview-actions">
          <span className="sm-btn sm tl-primary">{t.previewJoin}</span>
          <span className="sm-btn sm">{i.snooze(SNOOZE_MINUTES)}</span>
          <span className="sm-btn sm">{i.dismiss}</span>
        </div>
      </div>
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

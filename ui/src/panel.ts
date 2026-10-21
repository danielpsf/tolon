// View logic for the tray panel and the tray tooltip (F7, mockup 02): what to show, from what
// `occurrences_today` and `app_status` answer. The alarms themselves stay in Rust.
import { api, onOccurrencesChanged, onPaused, onSettingsChanged, onTrayHover } from "./api";
import type { ListedDto, SettingsDto } from "./api-types";
import { copy, type Copy } from "./copy";
import { providerOf } from "./interrupt";

/** Today's meetings that have not ended, earliest first; the ignored ones too (S8.1). */
export function remaining(items: readonly ListedDto[], now: number): ListedDto[] {
  return items
    .filter((m) => Date.parse(m.end) > now)
    .sort((a, b) => Date.parse(a.start) - Date.parse(b.start) || a.key.localeCompare(b.key));
}

/**
 * The meeting the panel puts first, with Join, Snooze and Dismiss: the first remaining one that
 * gets an alarm (not ignored, not all day). A meeting already running counts until it ends.
 */
export function nextOf(items: readonly ListedDto[], now: number): ListedDto | null {
  return remaining(items, now).find((m) => !m.ignored && !m.all_day) ?? null;
}

/** "Next, in 12 min", or "Started 3 min ago" once it runs. */
export function firstLine(c: Copy, next: ListedDto, now: number): string {
  const start = Date.parse(next.start);
  if (start <= now) return c.startedAgo(start, now);
  const span = c.until(start, now);
  return span ? c.t.panel.nextIn(span) : c.t.relative.startsNow;
}

/** "Paused until 16:00" in the locale's clock (AC7.3). */
export function pausedText(c: Copy, until: string): string {
  return c.t.panel.pausedUntil(c.formatTime(Date.parse(until)));
}

/** The time column of a row: the start, or "All day". */
export function rowTime(c: Copy, m: ListedDto): string {
  return m.all_day ? c.t.panel.allDay : c.formatTime(Date.parse(m.start));
}

/** The end column of a row: the provider ("Zoom"), the host of another link, "Event page" or "No link". */
export function rowWhere(c: Copy, m: ListedDto): string {
  const { panel } = c.t;
  if (m.join.kind === "link") return providerOf(m.join.host) ?? m.join.host;
  return m.join.kind === "event_page" ? panel.eventPage : panel.noLink;
}

/** "Alarms 2 min before. Quiet hours off." */
export function summaryText(c: Copy, s: SettingsDto): string {
  const quiet = s.quiet.enabled ? c.t.interrupt.timeRange(s.quiet.start, s.quiet.end) : null;
  return c.t.panel.summary(s.lead_minutes, quiet);
}

/** Unicode format characters (zero-width, bidi overrides and isolates): a feed title must not reorder the text around it. */
const FORMAT = /\p{Cf}/gu;

/** A meeting title for a sentence: its format characters dropped, then wrapped in FSI and PDI, so it keeps its own direction in every locale. */
export function isolate(title: string): string {
  return `\u2068${title.replace(FORMAT, "")}\u2069`;
}

/** Esc hides the panel (AC7.2); every other key is the page's. */
export function hidesPanel(e: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "altKey" | "isComposing">): boolean {
  return e.key === "Escape" && !e.ctrlKey && !e.metaKey && !e.altKey && !e.isComposing;
}

/** Puts the focus on the panel's first action, the way the interrupt opens on its primary button. */
export function focusFirstAction(root: Pick<ParentNode, "querySelector"> | null): boolean {
  const first = root?.querySelector<HTMLElement>(".tl-panel-actions button");
  first?.focus();
  return Boolean(first);
}

/** The tray tooltip (AC7.1): the pause, else the next meeting and the minutes until it, else "no more". */
export function tooltipText(c: Copy, items: readonly ListedDto[], pausedUntil: string | null, now: number): string {
  const { panel } = c.t;
  const next = nextOf(items, now);
  const meeting = next
    ? (() => {
        const span = c.until(Date.parse(next.start), now);
        return span ? panel.tooltip(isolate(next.title), span) : panel.tooltipNow(isolate(next.title));
      })()
    : panel.noMore;
  // A line break is the one separator every language reads the same way.
  return pausedUntil ? `${pausedText(c, pausedUntil)}\n${meeting}` : meeting;
}

/**
 * Keeps the tray tooltip current from the settings window, which has the catalogues: on start,
 * when the pointer reaches the icon, when meetings, settings or the pause change, and every
 * minute. Returns the cleanup. Failures are silent: the tooltip then keeps its last text.
 */
export function startTrayTooltip(intervalMs = 60_000): () => void {
  let stopped = false;
  const update = async () => {
    try {
      const [items, status] = await Promise.all([api.occurrencesToday(), api.appStatus()]);
      if (!stopped) await api.trayTooltip(tooltipText(copy(), items, status.paused_until, Date.now()));
    } catch {
      // No IPC (plain browser, tests) or no tray: nothing to update.
    }
  };
  const offs: (() => void)[] = [];
  const keep = (off: () => void) => (stopped ? off() : offs.push(off));
  for (const listen of [onTrayHover, onOccurrencesChanged, onSettingsChanged]) {
    listen(() => void update()).then(keep, () => undefined);
  }
  onPaused(() => void update()).then(keep, () => undefined);
  const timer = setInterval(() => void update(), intervalMs);
  void update();
  return () => {
    stopped = true;
    clearInterval(timer);
    offs.forEach((off) => off());
  };
}

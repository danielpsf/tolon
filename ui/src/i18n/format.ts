import type { CommandError, SyncError } from "../api-types";
import type { Messages } from "./en-US";
import type { Locale } from "./locale";

type Instant = Date | number;

const MINUTE = 60_000;

/** Wall-clock time in the locale's own style: 14:05 in en-GB and pt, 2:05 PM in en-US. */
export function formatTime(locale: Locale, at: Instant): string {
  return new Intl.DateTimeFormat(locale, { hour: "numeric", minute: "2-digit" }).format(at);
}

/** "2 min", "3 hr", "2 days": Intl picks the unit label and its plural. */
function formatSpan(locale: Locale, minutes: number): string {
  const [value, unit] =
    minutes < 60
      ? [minutes, "minute"]
      : minutes < 24 * 60
        ? [Math.floor(minutes / 60), "hour"]
        : [Math.floor(minutes / (24 * 60)), "day"];
  return new Intl.NumberFormat(locale, { style: "unit", unit, unitDisplay: "short" }).format(value);
}

const ms = (at: Instant) => (typeof at === "number" ? at : at.getTime());

/** "Synced 1 min ago". Rounds down; under a minute reads "just now". */
export function syncedAgo(locale: Locale, m: Messages, then: Instant, now: Instant = Date.now()): string {
  const minutes = Math.floor((ms(now) - ms(then)) / MINUTE);
  return minutes < 1 ? m.relative.syncedJustNow : m.relative.syncedAgo(formatSpan(locale, minutes));
}

/** "Started 3 min ago". Rounds down; under a minute reads "just now". */
export function startedAgo(locale: Locale, m: Messages, start: Instant, now: Instant = Date.now()): string {
  const minutes = Math.floor((ms(now) - ms(start)) / MINUTE);
  return minutes < 1 ? m.relative.startedJustNow : m.relative.startedAgo(formatSpan(locale, minutes));
}

/** "Starts in 2 min". Rounds up, so the copy never says "0 min" before the start. */
export function startsIn(locale: Locale, m: Messages, start: Instant, now: Instant = Date.now()): string {
  const minutes = Math.ceil((ms(start) - ms(now)) / MINUTE);
  return minutes < 1 ? m.relative.startsNow : m.relative.startsIn(formatSpan(locale, minutes));
}

function httpHint(m: Messages, status: number): string | null {
  const hints = m.errors.httpHints;
  if (status === 401) return hints.unauthorized;
  if (status === 403) return hints.forbidden;
  if (status === 404) return hints.notFound;
  if (status === 429) return hints.tooManyRequests;
  if (status >= 500) return hints.server;
  return null;
}

/** `CalendarDto.last_error` to copy. */
export function syncErrorText(m: Messages, error: SyncError): string {
  if (error.kind !== "http") return m.errors.sync[error.kind];
  const hint = httpHint(m, error.status);
  return hint ? m.errors.sync.httpHint(error.status, hint) : m.errors.sync.http(error.status);
}

/** A rejected command to copy. A code from a newer core falls back to the internal-error copy. */
export function commandErrorText(m: Messages, error: Pick<CommandError, "code">): string {
  return (m.errors.command as Record<string, string>)[error.code] ?? m.errors.command.internal;
}

/** `sync_log.detail` to copy: `skipped:N` is the one coded detail today. Anything else is not for the user. */
export function syncLogDetailText(m: Messages, detail: string): string | null {
  const skipped = /^skipped:(\d+)$/.exec(detail);
  return skipped ? m.syncLog.skipped(Number(skipped[1])) : null;
}

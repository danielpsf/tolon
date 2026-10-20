import { SYNC_ERROR_KINDS, type CommandError, type SyncError } from "../api-types";
import type { Messages } from "./en-US";
import type { Locale } from "./locale";

type Instant = Date | number;

const MINUTE = 60_000;

/**
 * Wall-clock time in the locale's own style: 2:05 PM in en-US, 09:05 and 14:05 in 24-hour locales.
 * Every 12-hour locale (en-US, ar, hi) drops the leading zero as en-US does. Digits follow Intl.
 */
export function formatTime(locale: Locale, at: Instant): string {
  const hour = new Intl.DateTimeFormat(locale, { hour: "numeric" }).resolvedOptions().hour12 ? "numeric" : "2-digit";
  return new Intl.DateTimeFormat(locale, { hour, minute: "2-digit" }).format(at);
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
  if (status === 429) return hints.tooManyRequests;
  if (status >= 500) return hints.server;
  return null;
}

/**
 * `CalendarDto.last_error` to copy. The core sends `kind` as a plain string, so a kind this
 * build does not know, or `http` without a status, gets the generic sentence, never `undefined`.
 * 404 has no hint on purpose: S1.3 quotes "Sync failed: 404".
 */
export function syncErrorText(m: Messages, error: SyncError): string {
  const { kind } = error;
  if (kind === "http") {
    const status = (error as { status?: unknown }).status;
    if (typeof status !== "number" || !Number.isFinite(status)) return m.errors.sync.unknown;
    const hint = httpHint(m, status);
    return hint ? m.errors.sync.httpHint(status, hint) : m.errors.sync.http(status);
  }
  return (SYNC_ERROR_KINDS as readonly string[]).includes(kind) ? m.errors.sync[kind] : m.errors.sync.unknown;
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

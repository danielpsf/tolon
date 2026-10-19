// View logic for the Calendars page: what to show, never what to compute (docs/agents/ui.md).
import { asCommandError } from "./api";
import type { CalendarDto } from "./api-types";
import type { Copy } from "./copy";

/**
 * A subscription URL for display: scheme and host only, as `redact_url()` in tolon-sync does.
 * Path, query, fragment, user info and port never show: private feeds put their token in any of
 * them (Google in the middle of the path, iCloud and Nextcloud in the last segment).
 * A string that is not a URL shows nothing.
 */
export function displayUrl(raw: string | null): string {
  if (!raw) return "";
  try {
    const { protocol, hostname } = new URL(raw);
    return hostname ? `${protocol}//${hostname}` : "";
  } catch {
    return "";
  }
}

export interface Status {
  text: string;
  /** True when the row shows a failed sync: signal colour, and the dot follows. */
  failed: boolean;
}

/** A failure wins over the last good sync; a calendar that never synced says so. */
export function statusOf(calendar: CalendarDto, c: Copy, now: number = Date.now()): Status {
  if (calendar.last_error) return { text: c.syncError(calendar.last_error), failed: true };
  const at = calendar.last_sync_at ? Date.parse(calendar.last_sync_at) : NaN;
  return { text: Number.isNaN(at) ? c.t.relative.neverSynced : c.syncedAgo(at, now), failed: false };
}

/** "Work, iCal subscription" for a feed (the only kind in beta 1), the bare name for anything else. */
export function titleOf(calendar: CalendarDto, c: Copy): string {
  const { calendars } = c.t;
  return calendar.kind === "ics" ? calendars.nameWithKind(calendar.name, calendars.kindIcs) : calendar.name;
}

/** Any rejection from a command, as the sentence to show. */
export function errorText(e: unknown, c: Copy): string {
  return c.commandError(asCommandError(e));
}

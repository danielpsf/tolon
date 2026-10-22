// View logic for the settings pages: what to show, never what to compute (docs/agents/ui.md).
import { asCommandError } from "./api";
import type { CalendarDto } from "./api-types";
import type { Copy } from "./copy";

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

/** "Work, iCal subscription" for a feed, "Team, Google Calendar, signed in" for Google, the bare name for anything else. */
export function titleOf(calendar: CalendarDto, c: Copy): string {
  const { calendars } = c.t;
  if (calendar.kind === "ics") return calendars.nameWithKind(calendar.name, calendars.kindIcs);
  if (calendar.kind === "google") return calendars.nameWithKind(calendar.name, calendars.kindGoogle);
  return calendar.name;
}

/** The longest custom calendar name; Rust cuts at 80 characters too. */
export const MAX_NAME_LENGTH = 80;

/** What to send to `calendars_rename` for a typed name: the trimmed text, or null while it is the name already shown. A blank string clears the custom name. */
export function renameValue(draft: string, shown: string): string | null {
  const name = draft.trim();
  return name === shown ? null : name;
}

/** Enter saves through the form; Esc cancels, unless an input method is composing. */
export function cancelsRename(e: Pick<KeyboardEvent, "key" | "isComposing">): boolean {
  return e.key === "Escape" && !e.isComposing;
}

/** Any rejection from a command, as the sentence to show. */
export function errorText(e: unknown, c: Copy): string {
  return c.commandError(asCommandError(e));
}

/** The lead time field to minutes, or null while it is not a whole number from 1 to 60 (AC9.5). */
export function parseLead(raw: string): number | null {
  const minutes = /^\s*\d+\s*$/.test(raw) ? Number(raw) : NaN;
  return minutes >= 1 && minutes <= 60 ? minutes : null;
}

// The contract's limits for ignore keywords (docs/decisions.md, settings contract).
export const MAX_KEYWORDS = 50;
export const MAX_KEYWORD_LENGTH = 64;

/** Any comma: Latin, Arabic, full-width and ideographic. */
const SEPARATORS = /[,،，、]/;

/** The list with the typed words added: trimmed, cut to 64 code points (not UTF-16 units), blanks and case-insensitive repeats dropped, at most 50. */
export function addKeywords(list: readonly string[], typed: string): string[] {
  const next = [...list];
  for (const word of typed.split(SEPARATORS).map((w) => Array.from(w.trim()).slice(0, MAX_KEYWORD_LENGTH).join(""))) {
    if (!word || next.length >= MAX_KEYWORDS) continue;
    if (!next.some((k) => k.toLowerCase() === word.toLowerCase())) next.push(word);
  }
  return next;
}

/** Text in the keyword box: the words before the last comma are done, the rest is still being typed. */
export function splitTyped(typed: string): { done: string; rest: string } {
  const parts = typed.split(SEPARATORS);
  const rest = parts.pop() ?? "";
  return { done: parts.join(","), rest };
}

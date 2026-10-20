// All user-facing copy goes through here (docs/agents/ui.md). Catalogues live in ./i18n.
// Logs stay English and never come from a catalogue.
import type { CommandError, SyncError } from "./api-types";
import * as f from "./i18n/format";
import { catalogues, resolveLocale, type Locale } from "./i18n/locale";

export type { Messages } from "./i18n/en-US";
export type { Locale } from "./i18n/locale";
export { LOCALES, resolveLocale } from "./i18n/locale";

type Instant = Date | number;

export function createCopy(locale: Locale) {
  const t = catalogues[locale];
  return {
    locale,
    /** The catalogue: `t.calendars.title`, `t.calendars.addCalendar`, ... */
    t,
    formatTime: (at: Instant) => f.formatTime(locale, at),
    syncedAgo: (then: Instant, now?: Instant) => f.syncedAgo(locale, t, then, now),
    startsIn: (start: Instant, now?: Instant) => f.startsIn(locale, t, start, now),
    startedAgo: (start: Instant, now?: Instant) => f.startedAgo(locale, t, start, now),
    syncError: (error: SyncError) => f.syncErrorText(t, error),
    commandError: (error: Pick<CommandError, "code">) => f.commandErrorText(t, error),
    syncLogDetail: (detail: string) => f.syncLogDetailText(t, detail),
    /** "Focus, Lunch, OOO" in the locale's list style. */
    list: (items: readonly string[]) =>
      new Intl.ListFormat(locale, { style: "long", type: "unit" }).format(items),
  };
}

export type Copy = ReturnType<typeof createCopy>;

let current: Copy | undefined;

/**
 * Resolves the locale once: `override` (for example the `lang` query parameter) first, then
 * `navigator.languages`. Sets `<html lang>` and `<html dir>` (rtl for Arabic). Call it at startup
 * before the first render.
 */
export function initCopy(override?: string | null): Copy {
  const systemLanguages = typeof navigator === "undefined" ? [] : navigator.languages;
  current = createCopy(resolveLocale(systemLanguages, override));
  if (typeof document !== "undefined") {
    document.documentElement.lang = current.locale;
    document.documentElement.dir = current.locale === "ar" ? "rtl" : "ltr";
  }
  return current;
}

/** The active copy. Initialises from the system locale on first use. */
export function copy(): Copy {
  return current ?? initCopy();
}

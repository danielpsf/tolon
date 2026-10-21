// The fallback notification's wording (F4.5, L-038). Rust shows the notification and has no
// catalogues, so the settings window builds this bundle from the active one and hands it over; Rust
// validates, stores and uses it. Nothing here decides what the notification says: it only fills
// the placeholders Rust fills at show time (`{title}`, `{time}`).
import { api } from "./api";
import type { NotifyCopyDto } from "./api-types";
import { copy, type Copy } from "./copy";
import { timeFormatter } from "./i18n/format";
import { RTL_LOCALES } from "./i18n/locale";
import { SNOOZE_MINUTES } from "./interrupt";

/** Titles for 2 to this many meetings are written out; a bigger group gets the title without a number (`MANY_MAX` in Rust). */
export const NOTIFY_MANY_MAX = 20;

const MINUTES_PER_DAY = 24 * 60;
const TIME = "{time}";
const TITLE = "{title}";

/** The 1440 minutes of a day in the locale's clock. UTC, so no local day can lack a minute (daylight saving). */
function dayOfTimes(c: Copy): string[] {
  const format = timeFormatter(c.locale, "UTC");
  return Array.from({ length: MINUTES_PER_DAY }, (_, minute) => format(Date.UTC(2026, 0, 1, 0, minute)));
}

export function buildNotifyCopy(c: Copy): NotifyCopyDto {
  const { t } = c;
  return {
    locale: c.locale,
    test_title: t.interrupt.testTitle,
    // First-strong isolates keep a Latin title in its own order inside a right-to-left notification.
    one_title: RTL_LOCALES.has(c.locale) ? `⁨${TITLE}⁩` : TITLE,
    many_titles: Array.from({ length: NOTIFY_MANY_MAX - 1 }, (_, i) => t.notification.many(i + 2)),
    many_over: t.notification.manyOver,
    body: t.notification.body(TIME),
    snooze: t.interrupt.snooze(SNOOZE_MINUTES),
    join: t.interrupt.joinCall,
    untitled: t.notification.untitled,
    times: dayOfTimes(c),
  };
}

/**
 * Hands the bundle for the active language to Rust. Failures are silent: the notification then keeps
 * the bundle it has (or the built-in English), and the next load or language change tries again.
 */
export function syncNotifyCopy(c: Copy = copy()): void {
  void api.notifyCopySet(buildNotifyCopy(c)).catch(() => undefined);
}

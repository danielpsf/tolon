// First run (F10, mockup 05; decision "First run (L-018)"): when the settings window opens on it, and
// when it ends. Rust shows the window at start only while there is no calendar (`tray::shows_at_start`);
// the page makes the same call.
import { create } from "zustand";
import { api, onSettingsHidden } from "./api";
import type { CalendarDto } from "./api-types";
import { useCalendars } from "./store";

/** A route in the URL asks for a settings page (a reload, the WebDriver tests): Tolon opens the window on none. */
/**
 * Focus for a window that opens (decision "Reaching Tolon without the tray, and first-run focus"): on
 * `el` without scrolling it into view, with the page at its top, so a page taller than the window never
 * opens scrolled down to its buttons.
 */
export function focusAtTop(el: Pick<HTMLElement, "focus"> | null | undefined, page: Pick<Window, "scrollTo"> = window): void {
  el?.focus({ preventScroll: true });
  page.scrollTo(0, 0);
}

export const asksForPage = (hash: string): boolean => hash.replace(/^#\/?/, "") !== "";

/**
 * Whether the window shows first run: null until the first calendar list is in, then true with no
 * calendar (AC10.1). A list that failed shows the Calendars page with its error instead. Decided once
 * and only ends: when the window hides (the close button or "Skip for now"; after the first calendar
 * the page hides it, AC10.2), or when the first calendar fails to sync. Removing the last calendar
 * later does not bring it back in this session; the next launch decides again.
 */
export const useFirstRun = create<{ shows: boolean | null }>(() => ({
  shows: typeof window !== "undefined" && asksForPage(window.location.hash) ? false : null,
}));

/** First run is over for this window. */
export const endFirstRun = () => useFirstRun.setState({ shows: false });

/** Hides the window (AC10.2, or the skip), then leaves first run, so the settings pages never flash on screen. */
export function hideAndEndFirstRun(): Promise<void> {
  return api.appHideSettings().then(endFirstRun, endFirstRun);
}

/**
 * The first calendar is in. A sync that failed keeps the window and moves to the Calendars page, so
 * the person sees the error row and can fix the link; one without an error hides the window (AC10.2).
 */
export function firstCalendarAdded(calendar: CalendarDto): Promise<void> {
  if (calendar.last_error) {
    endFirstRun();
    return Promise.resolve();
  }
  return hideAndEndFirstRun();
}

/**
 * Ends first run on `settings:hidden`. Called in main.tsx before the first load, so a close that comes
 * before the page has rendered is not missed. Returns the cleanup.
 */
export function listenForHide(): () => void {
  let stopped = false;
  let unlisten: (() => void) | undefined;
  onSettingsHidden(endFirstRun).then(
    (off) => {
      if (stopped) off();
      else unlisten = off;
    },
    // No IPC (plain browser, tests): the page decides from the list alone.
    () => undefined,
  );
  return () => {
    stopped = true;
    unlisten?.();
  };
}

/** Decides on the first calendar list. Returns the cleanup. */
export function startFirstRunWatch(): () => void {
  const decide = ({ loaded, calendars, loadError }: ReturnType<typeof useCalendars.getState>) => {
    if (loaded && useFirstRun.getState().shows === null) {
      useFirstRun.setState({ shows: loadError === null && calendars.length === 0 });
    }
  };
  decide(useCalendars.getState());
  return useCalendars.subscribe(decide);
}

// First run (F10, mockup 05): when the settings window opens on it, and when it ends. Rust shows the
// window at start only while there is no calendar (`tray::shows_at_start`); the page makes the same call.
import { create } from "zustand";
import { api, onSettingsHidden } from "./api";
import { useCalendars } from "./store";

/** A route in the URL asks for a settings page (a reload, the WebDriver tests): Tolon opens the window on none. */
export const asksForPage = (hash: string): boolean => hash.replace(/^#\/?/, "") !== "";

/**
 * Whether the window shows first run: null until the first calendar list is in, then true with no
 * calendar (AC10.1). It is decided once and only ends, when the window hides: the close button is
 * the skip, and after the first calendar the page hides it (AC10.2). Removing the last calendar later
 * does not bring it back in this session; the next launch decides again.
 */
export const useFirstRun = create<{ shows: boolean | null }>(() => ({
  shows: typeof window !== "undefined" && asksForPage(window.location.hash) ? false : null,
}));

/** First run is over for this window. */
export const endFirstRun = () => useFirstRun.setState({ shows: false });

/** AC10.2: the first calendar is in; the window hides, then the page leaves first run (no flash of the settings pages). */
export function finishFirstRun(): Promise<void> {
  return api.appHideSettings().then(endFirstRun, endFirstRun);
}

/** Decides on the first calendar list and ends on `settings:hidden`. Returns the cleanup. */
export function startFirstRunWatch(): () => void {
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
  const decide = ({ loaded, calendars }: ReturnType<typeof useCalendars.getState>) => {
    if (loaded && useFirstRun.getState().shows === null) useFirstRun.setState({ shows: calendars.length === 0 });
  };
  decide(useCalendars.getState());
  const unsubscribe = useCalendars.subscribe(decide);
  return () => {
    stopped = true;
    unlisten?.();
    unsubscribe();
  };
}

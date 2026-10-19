import { create } from "zustand";
import { api, asCommandError, onCalendarsChanged } from "./api";
import type { CalendarDto, CommandError } from "./api-types";

interface CalendarsState {
  calendars: CalendarDto[];
  loaded: boolean;
  /** The last failed `calendars_list`, cleared by the next good one. */
  loadError: CommandError | null;
  refresh: () => Promise<void>;
}

// A slow answer must not overwrite a newer one.
let latest = 0;

export const useCalendars = create<CalendarsState>((set) => ({
  calendars: [],
  loaded: false,
  loadError: null,
  refresh: async () => {
    const mine = ++latest;
    try {
      const calendars = await api.calendarsList();
      if (mine === latest) set({ calendars, loaded: true, loadError: null });
    } catch (e) {
      if (mine === latest) set({ loaded: true, loadError: asCommandError(e) });
    }
  },
}));

/** Loads the list and reloads it on every `calendars:changed`. Returns the cleanup. */
export function startCalendarsFeed(): () => void {
  const { refresh } = useCalendars.getState();
  let stopped = false;
  let unlisten: (() => void) | undefined;
  onCalendarsChanged(() => void refresh()).then(
    (off) => {
      if (stopped) off();
      else unlisten = off;
    },
    // No IPC (plain browser, tests): the page still shows what the first fetch returned.
    () => undefined,
  );
  void refresh();
  return () => {
    stopped = true;
    unlisten?.();
  };
}

import { create } from "zustand";
import { api, asCommandError, onCalendarsChanged, onSettingsChanged } from "./api";
import type { CalendarDto, CommandError, SettingsDto, SettingsPatch } from "./api-types";
import { initCopy } from "./copy";
import { syncNotifyCopy } from "./notify-copy";
import { applyAppearance, applyStyle } from "./theme";

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

interface SettingsState {
  /** What the page shows: the stored settings plus any change still on its way (AC9.1). */
  settings: SettingsDto | null;
  /** The last failed load, save or sound preview, cleared by the next change. */
  error: CommandError | null;
  load: () => Promise<void>;
  /** Shows the change at once, saves it, and rolls back to the stored settings when the save fails. True when saved. */
  save: (patch: SettingsPatch) => Promise<boolean>;
}

/** The settings with a patch applied; `quiet` and `ignore` merge field by field. */
export function withPatch(settings: SettingsDto, patch: SettingsPatch): SettingsDto {
  return {
    ...settings,
    ...patch,
    quiet: { ...settings.quiet, ...patch.quiet },
    ignore: { ...settings.ignore, ...patch.ignore },
  };
}

// The settings as the backend last answered, how many saves are on their way, the number of the
// newest save, and the newest save whose answer is in `stored`. Answers can arrive out of order.
let stored: SettingsDto | null = null;
let saving = 0;
let newest = 0;
let answered = 0;

export const useSettings = create<SettingsState>((set, get) => ({
  settings: null,
  error: null,
  load: async () => {
    try {
      const answer = await api.settingsGet();
      // A save on its way answers with newer settings: keep its answer, and do not flash the older ones.
      if (saving > 0) return;
      stored = answer;
      set({ settings: stored });
    } catch (e) {
      set({ error: asCommandError(e) });
    }
  },
  save: async (patch) => {
    const shown = get().settings;
    if (!shown) return false;
    const mine = ++newest;
    saving++;
    set({ settings: withPatch(shown, patch), error: null });
    let error: CommandError | null = null;
    try {
      const answer = await api.settingsSet(patch);
      if (mine > answered) {
        answered = mine;
        stored = answer;
      }
    } catch (e) {
      error = asCommandError(e);
    }
    saving--;
    // While a newer change is on its way, the page keeps showing it (it holds this one too); the newest
    // or the last save to settle puts the stored settings back, which also rolls back a failed change.
    if (mine === newest || saving === 0) set({ settings: stored });
    if (error) set({ error });
    return error === null;
  },
}));

/** Test hook: forget the stored settings between tests. */
export function resetSettings(): void {
  stored = null;
  saving = 0;
  newest = 0;
  answered = 0;
  useSettings.setState({ settings: null, error: null });
}

/**
 * Loads the settings, reloads them on every `settings:changed`, and applies the appearance, the style and
 * the language to this window whenever they change. `lang` (the query parameter) stands in when the
 * setting follows the system. Resolves once the first load is done, so the first render is right.
 * `notification` (the settings window only) also hands Rust the notification's wording for the language shown.
 */
export function startSettingsFeed(lang: string | null, notification = false): Promise<void> {
  const { load } = useSettings.getState();
  initCopy(lang);
  applyAppearance("auto");
  let sent = false;
  useSettings.subscribe(({ settings }, previous) => {
    if (!settings) return;
    if (settings.appearance !== previous.settings?.appearance) applyAppearance(settings.appearance);
    if (settings.style !== previous.settings?.style) applyStyle(settings.style);
    if (settings.locale !== previous.settings?.locale) {
      const c = initCopy(settings.locale ?? lang);
      if (notification) {
        syncNotifyCopy(c);
        sent = true;
      }
    }
  });
  // No IPC (plain browser, tests): the window keeps the system language and theme.
  onSettingsChanged(() => void load()).catch(() => undefined);
  // The notification (shown by Rust) speaks the language this window shows: sent when the first load
  // fixes it (the subscription above, which also covers every later change), or after a failed load.
  return load().finally(() => {
    if (notification && !sent) syncNotifyCopy();
  });
}

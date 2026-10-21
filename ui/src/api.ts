// The only file that talks to Tauri: commands in, events out (docs/architecture.md, "Tauri commands").
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AlarmShowDto,
  CalendarDto,
  CommandError,
  ListedDto,
  NotifyCopyDto,
  PauseDto,
  SettingsDto,
  SnoozeDto,
  SettingsPatch,
  Sound,
} from "./api-types";

export const api = {
  calendarsList: () => invoke<CalendarDto[]>("calendars_list"),
  calendarsAddIcs: (url: string) => invoke<CalendarDto>("calendars_add_ics", { url }),
  calendarsRemove: (id: string) => invoke<void>("calendars_remove", { id }),
  calendarsSetEnabled: (id: string, enabled: boolean) => invoke<void>("calendars_set_enabled", { id, enabled }),
  /** The interrupt on screen, or null: a window that loads after `alarm:show` still gets it. */
  alarmsCurrent: () => invoke<AlarmShowDto | null>("alarms_current"),
  alarmsSnooze: (key: string, minutes: number) => invoke<void>("alarms_snooze", { key, minutes }),
  alarmsDismiss: (key: string) => invoke<void>("alarms_dismiss", { key }),
  alarmsJoin: (key: string) => invoke<void>("alarms_join", { key }),
  alarmsAlreadyThere: (key: string) => invoke<void>("alarms_already_there", { key }),
  settingsGet: () => invoke<SettingsDto>("settings_get"),
  /** Saves at once (AC9.1) and answers with every setting as stored. */
  settingsSet: (patch: SettingsPatch) => invoke<SettingsDto>("settings_set", { patch }),
  /** Plays the sound once (AC6.3). */
  soundPreview: (sound: Sound) => invoke<void>("sound_preview", { sound }),
  // The tray panel and the tray tooltip (F7).
  occurrencesToday: () => invoke<ListedDto[]>("occurrences_today"),
  occurrencesJoin: (key: string) => invoke<void>("occurrences_join", { key }),
  occurrencesSnooze: (key: string, minutes: number) => invoke<SnoozeDto>("occurrences_snooze", { key, minutes }),
  occurrencesDismiss: (key: string) => invoke<void>("occurrences_dismiss", { key }),
  appStatus: () => invoke<PauseDto>("app_status"),
  appPause: (minutes: number) => invoke<PauseDto>("app_pause", { minutes }),
  appResume: () => invoke<PauseDto>("app_resume"),
  appShowSettings: () => invoke<void>("app_show_settings"),
  /** Esc in the panel: it hides, as on a click away. */
  appHidePanel: () => invoke<void>("app_hide_panel"),
  appQuit: () => invoke<void>("app_quit"),
  trayTooltip: (text: string) => invoke<void>("tray_tooltip", { text }),
  /** The fallback notification's wording in the active language (L-038). Settings window only. */
  notifyCopySet: (copy: NotifyCopyDto) => invoke<void>("notify_copy_set", { copy }),
};

export const onSettingsChanged = (handler: () => void): Promise<UnlistenFn> => listen("settings:changed", handler);

export const onCalendarsChanged = (handler: () => void): Promise<UnlistenFn> =>
  listen("calendars:changed", handler);

export const onAlarmShow = (handler: (alarm: AlarmShowDto) => void): Promise<UnlistenFn> =>
  listen<AlarmShowDto>("alarm:show", (e) => handler(e.payload));

export const onAlarmHide = (handler: () => void): Promise<UnlistenFn> => listen("alarm:hide", handler);

export const onOccurrencesChanged = (handler: () => void): Promise<UnlistenFn> =>
  listen("occurrences:changed", handler);

export const onPaused = (handler: (pause: PauseDto) => void): Promise<UnlistenFn> =>
  listen<PauseDto>("app:paused", (e) => handler(e.payload));

/** The tray icon is under the pointer: its tooltip is about to show (settings window only). */
export const onTrayHover = (handler: () => void): Promise<UnlistenFn> => listen("tray:hover", handler);

/** A rejected command carries `{ code }`; anything else (no IPC, a bug) reads as `internal`. */
export function asCommandError(e: unknown): CommandError {
  const code = typeof e === "object" && e !== null ? (e as { code?: unknown }).code : undefined;
  return { code: typeof code === "string" ? (code as CommandError["code"]) : "internal" };
}

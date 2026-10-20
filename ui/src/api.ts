// The only file that talks to Tauri: commands in, events out (docs/architecture.md, "Tauri commands").
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AlarmShowDto, CalendarDto, CommandError } from "./api-types";

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
};

export const onCalendarsChanged = (handler: () => void): Promise<UnlistenFn> =>
  listen("calendars:changed", handler);

export const onAlarmShow = (handler: (alarm: AlarmShowDto) => void): Promise<UnlistenFn> =>
  listen<AlarmShowDto>("alarm:show", (e) => handler(e.payload));

export const onAlarmHide = (handler: () => void): Promise<UnlistenFn> => listen("alarm:hide", handler);

/** A rejected command carries `{ code }`; anything else (no IPC, a bug) reads as `internal`. */
export function asCommandError(e: unknown): CommandError {
  const code = typeof e === "object" && e !== null ? (e as { code?: unknown }).code : undefined;
  return { code: typeof code === "string" ? (code as CommandError["code"]) : "internal" };
}

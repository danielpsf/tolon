// Mirrors what `tolon-app` sends: `CalendarDto` in crates/tolon-app/src/pipeline.rs,
// `SyncErrorDto` and `CommandError` in crates/tolon-app/src/errors.rs.

export const SYNC_ERROR_KINDS = [
  "timeout",
  "connect",
  "not_calendar",
  "too_large",
  "network",
  "unsupported_scheme",
  "invalid_url",
] as const;

export type SyncErrorKind = (typeof SYNC_ERROR_KINDS)[number];

/** `last_error` from a stored code: `http:404` is `{ kind: "http", status: 404 }`. */
export type SyncError = { kind: "http"; status: number } | { kind: SyncErrorKind };

export const COMMAND_ERROR_CODES = [
  "unsupported_scheme",
  "invalid_url",
  "not_found",
  "no_url",
  "not_ready",
  "db_error",
  "internal",
  "login_item",
] as const;

export type CommandErrorCode = (typeof COMMAND_ERROR_CODES)[number];

/** What a rejected command carries. `settings_set` refuses a value with `invalid:<field>`. */
export interface CommandError {
  code: CommandErrorCode | `invalid:${string}`;
}

export interface CalendarDto {
  id: string;
  name: string;
  kind: string;
  enabled: boolean;
  /** Scheme and host only, already redacted by the core: the full URL never reaches the webview (AC1.3). */
  display_url: string | null;
  /** RFC 3339 timestamp, or null when the calendar never synced. */
  last_sync_at: string | null;
  last_error: SyncError | null;
}

/** Where a meeting's join button leads. The URL stays in Rust; the page only gets the host (AC1.3). */
export type JoinDto =
  | { kind: "link"; host: string; recognised: boolean }
  | { kind: "event_page" }
  | { kind: "none" };

/** One meeting of an interrupt: `MeetingDto` in crates/tolon-app/src/interrupt.rs. */
export interface MeetingDto {
  /** The occurrence key, sent back by the alarm commands. */
  key: string;
  title: string;
  /** RFC 3339 with an offset. */
  start: string;
  end: string;
  join: JoinDto;
  /** Attendee count; names never arrive. */
  people: number;
}

/** The `alarm:show` payload and the answer to `alarms_current`: every meeting of one start minute (AC3.5). */
export interface AlarmShowDto {
  /** The test alarm: the page shows its own title instead of `title`. */
  test: boolean;
  meetings: MeetingDto[];
}

// The settings contract (docs/decisions.md, "Overnight M3 plan and the settings contract"):
// `settings_get`, `settings_set` and `sound_preview` in tolon-app.

export const SOUNDS = ["bell", "chime", "none"] as const;
export type Sound = (typeof SOUNDS)[number];

export const APPEARANCES = ["auto", "light", "dark"] as const;
export type Appearance = (typeof APPEARANCES)[number];

export interface QuietHoursDto {
  enabled: boolean;
  /** "HH:MM", local time. */
  start: string;
  end: string;
  /** Saturday and Sunday stay quiet all day. */
  weekends: boolean;
}

export interface IgnoreDto {
  all_day: boolean;
  declined: boolean;
  cancelled: boolean;
  no_attendees: boolean;
  /** Trimmed, at most 50 of up to 64 characters each; matched case-insensitively. */
  keywords: string[];
}

export interface SettingsDto {
  /** 1 to 60 (AC9.5). */
  lead_minutes: number;
  again_at_start: boolean;
  sound: Sound;
  sound_repeat: boolean;
  quiet: QuietHoursDto;
  ignore: IgnoreDto;
  appearance: Appearance;
  /** A shipped catalogue, or null to follow the system. */
  locale: string | null;
  /** On only when the OS will start Tolon (AC9.4); the settings window reads it from the OS. */
  start_at_login: boolean;
  /** What the OS said, settings window only; null in other windows and in `settings:changed`. */
  start_at_login_status: "enabled" | "disabled" | "requires_approval" | "unavailable" | null;
}

/** Every field optional, `quiet` and `ignore` field by field too: send only what changed. */
export type SettingsPatch = Partial<Omit<SettingsDto, "quiet" | "ignore" | "start_at_login_status">> & {
  quiet?: Partial<QuietHoursDto>;
  ignore?: Partial<IgnoreDto>;
};

// The tray panel (F7): `ListedDto` in crates/tolon-app/src/pipeline.rs, `PauseDto` in commands.rs.

export type IgnoredReason = "keyword" | "all_day" | "declined" | "cancelled" | "no_attendees";

/** One of today's meetings from `occurrences_today`. No URL: Join goes through `occurrences_join`. */
export interface ListedDto {
  key: string;
  title: string;
  /** RFC 3339 with the machine's offset. */
  start: string;
  end: string;
  all_day: boolean;
  people: number;
  /** Why it gets no alarm (S8.1), or null. */
  ignored: IgnoredReason | null;
  join: JoinDto;
}

/** `occurrences_snooze`: `ended` when the snooze would have come after the meeting's window, so its reminders ended instead. */
export interface SnoozeDto {
  ended: boolean;
}

/** `app_status`, `app_pause`, `app_resume` and the `app:paused` event. */
export interface PauseDto {
  /** RFC 3339 UTC, or null while alarms are not paused. */
  paused_until: string | null;
}

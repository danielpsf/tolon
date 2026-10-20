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
] as const;

export type CommandErrorCode = (typeof COMMAND_ERROR_CODES)[number];

/** What a rejected command carries. */
export interface CommandError {
  code: CommandErrorCode;
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

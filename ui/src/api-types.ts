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

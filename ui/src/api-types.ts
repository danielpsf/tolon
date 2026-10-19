// Error shapes from L-029 (tolon-app). Mirror them here until that branch merges.

export type SyncErrorKind =
  | "timeout"
  | "connect"
  | "not_calendar"
  | "too_large"
  | "network"
  | "unsupported_scheme"
  | "invalid_url";

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

/** Only the field the i18n layer reads; L-029 and L-008 own the rest of the shape. */
export interface CalendarDto {
  last_error: SyncError | null;
}

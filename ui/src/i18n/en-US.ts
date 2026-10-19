import type { CommandErrorCode, SyncErrorKind } from "../api-types";

const plural = new Intl.PluralRules("en-US");
const number = new Intl.NumberFormat("en-US");

// Source catalogue. Writing rules: sentence case, no exclamation marks, "Tolon" never "the app".
export const enUS = {
  app: { windowTitle: "Tolon" },
  nav: {
    label: "Settings",
    calendars: "Calendars",
    alarms: "Alarms",
    quietHours: "Quiet hours",
    appearance: "Appearance",
    startup: "Startup",
    about: "About",
    comingSoon: "Coming in a later version.",
  },
  calendars: {
    title: "Calendars",
    subtitle: "Tolon reads your calendars and never writes to them.",
    googleName: (account: string) => `Google: ${account}`,
    kindGoogle: "Google Calendar, signed in",
    kindIcs: "iCal subscription",
    kindOutlook: "Published Outlook calendar",
    toggleLabel: (name: string) => `Enable calendar ${name}`,
    addCalendar: "Add calendar",
    pasteIcalUrl: "Paste an iCal URL",
    ignoreHeading: "Ignore",
    ignoreAllDay: "All-day events",
    ignoreDeclined: "Events I declined",
    ignoreSolo: "Events without other people",
    ignoreMatching: (keywords: string) => `Events matching: ${keywords}`,
    changesSave: "Changes save as you make them.",
    nameWithKind: (name: string, kind: string) => `${name}, ${kind}`,
    removeLabel: (name: string) => `Remove calendar ${name}`,
    removeTitle: (name: string) => `Remove ${name}?`,
    removeConsequence: "Tolon stops reading this calendar. The calendar itself is not changed.",
    remove: "Remove",
    removeConfirm: "Remove calendar",
    cancel: "Cancel",
    urlPlaceholder: "https://calendar.example.com/team.ics",
    urlHint: "Tolon keeps the link on this computer and only reads from it.",
    adding: "Adding calendar",
    empty: "No calendars yet. Paste an iCal URL to add one.",
  },
  relative: {
    neverSynced: "Not synced yet",
    syncedJustNow: "Synced just now",
    syncedAgo: (duration: string) => `Synced ${duration} ago`,
    startsNow: "Starts now",
    startsIn: (duration: string) => `Starts in ${duration}`,
    startedJustNow: "Started just now",
    startedAgo: (duration: string) => `Started ${duration} ago`,
  },
  syncLog: {
    skipped: (count: number) =>
      `${number.format(count)} ${plural.select(count) === "one" ? "component" : "components"} skipped`,
  },
  errors: {
    sync: {
      http: (status: number) => `Sync failed: ${status}`,
      httpHint: (status: number, hint: string) => `Sync failed: ${status}, ${hint}`,
      timeout: "Sync failed: the calendar took too long to answer.",
      connect: "Sync failed: Tolon could not connect to the server.",
      not_calendar: "Sync failed: that link is not an iCal calendar.",
      too_large: "Sync failed: the calendar is too large to read.",
      network: "Sync failed: there is no network connection.",
      unsupported_scheme: "Sync failed: only https and webcal links work.",
      invalid_url: "Sync failed: that is not a valid URL.",
      unknown: "Sync failed.",
    },
    httpHints: {
      unauthorized: "link expired",
      forbidden: "access denied",
      tooManyRequests: "too many requests",
      server: "server error",
    },
    command: {
      unsupported_scheme: "Tolon can only add https or webcal links.",
      invalid_url: "That is not a valid calendar URL.",
      not_found: "That calendar no longer exists.",
      no_url: "This calendar has no link to sync.",
      not_ready: "Tolon is still starting. Try again in a moment.",
      db_error: "Tolon could not save that change. Try again.",
      internal: "Something went wrong inside Tolon. Try again.",
    },
  },
} as const satisfies {
  [section: string]: unknown;
  errors: {
    sync: Record<SyncErrorKind, string> & {
      http: (status: number) => string;
      httpHint: (status: number, hint: string) => string;
      unknown: string;
    };
    command: Record<CommandErrorCode, string>;
    httpHints: Record<"unauthorized" | "forbidden" | "tooManyRequests" | "server", string>;
  };
};

type Widen<T> = T extends string
  ? string
  : T extends (...args: infer A) => string
    ? (...args: A) => string
    : { -readonly [K in keyof T]: Widen<T[K]> };

/** Every catalogue is typed `Messages`, so a missing or extra key fails `tsc`. */
export type Messages = Widen<typeof enUS>;

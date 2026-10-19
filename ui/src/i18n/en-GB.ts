import type { Messages } from "./en-US";

const plural = new Intl.PluralRules("en-GB");
const number = new Intl.NumberFormat("en-GB");

// British English. Most strings match en-US; times and units differ through Intl.
export const enGB: Messages = {
  app: { windowTitle: "Tolon" },
  nav: {
    label: "Settings",
    calendars: "Calendars",
    alarms: "Alarms",
    quietHours: "Quiet hours",
    appearance: "Appearance",
    startup: "Startup",
    about: "About",
    comingSoon: "This page is not built yet.",
  },
  calendars: {
    title: "Calendars",
    subtitle: "Tolon reads your calendars and never writes to them.",
    googleName: (account) => `Google: ${account}`,
    kindGoogle: "Google Calendar, signed in",
    kindIcs: "iCal subscription",
    kindOutlook: "Published Outlook calendar",
    toggleLabel: (name) => `Enable calendar ${name}`,
    addCalendar: "Add calendar",
    pasteIcalUrl: "Paste an iCal URL",
    ignoreHeading: "Ignore",
    ignoreAllDay: "All-day events",
    ignoreDeclined: "Events I declined",
    ignoreSolo: "Events without other people",
    ignoreMatching: (keywords) => `Events matching: ${keywords}`,
    changesSave: "Changes save as you make them.",
    nameWithKind: (name, kind) => `${name}, ${kind}`,
    removeLabel: (name) => `Remove calendar ${name}`,
    removeTitle: (name) => `Remove ${name}?`,
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
    syncedAgo: (duration) => `Synced ${duration} ago`,
    startsNow: "Starts now",
    startsIn: (duration) => `Starts in ${duration}`,
    startedJustNow: "Started just now",
    startedAgo: (duration) => `Started ${duration} ago`,
  },
  syncLog: {
    skipped: (count) =>
      `${number.format(count)} ${plural.select(count) === "one" ? "component" : "components"} skipped`,
  },
  errors: {
    sync: {
      http: (status) => `Sync failed: ${status}`,
      httpHint: (status, hint) => `Sync failed: ${status}, ${hint}`,
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
};

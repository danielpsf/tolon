import { describe, expect, it } from "vitest";
import { asCommandError } from "./api";
import type { CalendarDto } from "./api-types";
import { createCopy } from "./copy";
import {
  cancelsRename,
  errorText,
  titleParts,
  MAX_NAME_LENGTH,
  renameValue,
  statusHelp,
  statusOf,
  titleOf,
} from "./view";

const en = createCopy("en-US");
const now = Date.parse("2026-10-20T10:00:00Z");

const calendar = (over: Partial<CalendarDto> = {}): CalendarDto => ({
  id: "c1",
  name: "Work",
  kind: "ics",
  enabled: true,
  display_url: "https://calendar.google.com",
  last_sync_at: "2026-10-20T09:59:00Z",
  last_error: null,
  account: null,
  ...over,
});

describe("statusHelp", () => {
  it("links a not-public calendar to the Google steps of the iCal guide, inside Tolon", () => {
    const failed = calendar({ last_error: { kind: "not_public" } });
    expect(statusHelp(failed, en)).toEqual({
      href: "#/help/ical-links/google-calendar",
      text: "How to find your secret address.",
    });
    expect(statusHelp(failed, createCopy("de"))?.text).toBe("So findest du deine geheime Adresse.");
  });

  it("offers no link for any other status", () => {
    expect(statusHelp(calendar(), en)).toBeNull();
    expect(statusHelp(calendar({ last_error: { kind: "http", status: 404 } }), en)).toBeNull();
    expect(statusHelp(calendar({ last_error: { kind: "too_large" } }), en)).toBeNull();
  });
});

describe("statusOf", () => {
  it("reads 'Synced 1 min ago' for a good sync, as in mockup 03", () => {
    expect(statusOf(calendar(), en, now)).toEqual({ text: "Synced 1 min ago", failed: false });
  });

  it("reads 'Synced just now' inside the first minute", () => {
    const fresh = calendar({ last_sync_at: "2026-10-20T09:59:45Z" });
    expect(statusOf(fresh, en, now).text).toBe("Synced just now");
  });

  it("reads 'Sync failed: 404' for an HTTP 404 (S1.3)", () => {
    const failed = calendar({ last_sync_at: null, last_error: { kind: "http", status: 404 } });
    expect(statusOf(failed, en, now)).toEqual({ text: "Sync failed: 404", failed: true });
  });

  it("adds the hint for an expired link, as in mockup 03", () => {
    const expired = calendar({ last_error: { kind: "http", status: 401 } });
    expect(statusOf(expired, en, now).text).toBe("Sync failed: 401, link expired");
  });

  it("lets a failure win over an earlier good sync", () => {
    const failed = calendar({ last_error: { kind: "timeout" } });
    expect(statusOf(failed, en, now)).toEqual({
      text: "Sync failed: the calendar took too long to answer.",
      failed: true,
    });
  });

  it("says so when the calendar never synced, or the timestamp is unreadable", () => {
    expect(statusOf(calendar({ last_sync_at: null }), en, now).text).toBe("Not synced yet");
    expect(statusOf(calendar({ last_sync_at: "garbage" }), en, now).text).toBe("Not synced yet");
  });

  it("follows the locale", () => {
    const pt = createCopy("pt-BR");
    expect(statusOf(calendar(), pt, now).text).toBe("Sincronizado há 1 min");
  });
});

describe("titleOf", () => {
  it("labels a feed with its kind, as 'Work, iCal subscription' in mockup 03", () => {
    expect(titleOf(calendar(), en)).toBe("Work, iCal subscription");
  });

  it("labels a Google calendar as signed in", () => {
    expect(titleOf(calendar({ kind: "google", name: "Team", account: "a@example.com" }), en)).toBe(
      "Team, Google Calendar, signed in",
    );
  });

  it("leaves a kind it does not know with the bare name", () => {
    expect(titleOf(calendar({ kind: "other" }), en)).toBe("Work");
  });
});

describe("error mapping", () => {
  it("turns a command error code into its sentence", () => {
    expect(errorText({ code: "invalid_url" }, en)).toBe("That is not a valid calendar URL.");
    expect(errorText({ code: "unsupported_scheme" }, en)).toBe("Tolon can only add https or webcal links.");
    expect(errorText({ code: "not_found" }, en)).toBe("That calendar no longer exists.");
  });

  it("reads anything that is not a { code } as the internal error", () => {
    for (const odd of ["boom", null, undefined, 42, new Error("x"), { code: 7 }]) {
      expect(asCommandError(odd)).toEqual({ code: "internal" });
      expect(errorText(odd, en)).toBe("Something went wrong inside Tolon. Try again.");
    }
  });

  it("falls back to the internal sentence for a code from a newer core", () => {
    expect(errorText({ code: "from_the_future" }, en)).toBe("Something went wrong inside Tolon. Try again.");
  });
});

describe("renaming a calendar (L-067)", () => {
  it("sends the trimmed name, and null while it is the name already shown", () => {
    expect(renameValue("  Standups  ", "Work")).toBe("Standups");
    expect(renameValue("Work ", "Work")).toBeNull();
  });

  it("sends an empty string for a blank name, so the feed name comes back", () => {
    expect(renameValue("   ", "Standups")).toBe("");
  });

  it("caps the field at 80, as Rust does", () => {
    expect(MAX_NAME_LENGTH).toBe(80);
  });

  it("cancels on Esc, not while an input method is composing, and not on other keys", () => {
    expect(cancelsRename({ key: "Escape", isComposing: false })).toBe(true);
    expect(cancelsRename({ key: "Escape", isComposing: true })).toBe(false);
    expect(cancelsRename({ key: "Enter", isComposing: false })).toBe(false);
  });

  it("shows the custom name in the row title, as the core sends it in `name`", () => {
    expect(titleOf(calendar({ name: "Standups" }), en)).toBe("Standups, iCal subscription");
  });

  it("splits the title around the name so the page can isolate it", () => {
    expect(titleParts(calendar({ name: "Standups" }), en)).toEqual(["", "Standups", ", iCal subscription"]);
    expect(titleParts(calendar({ name: "Team", kind: "google" }), en)).toEqual(["", "Team", ", Google Calendar, signed in"]);
    expect(titleParts(calendar({ name: "Team", kind: "other" }), en)).toEqual(["", "Team", ""]);
  });

  it("has the strings in the catalogue", () => {
    const { calendars } = en.t;
    expect([calendars.rename, calendars.renameSave, calendars.cancel]).toEqual(["Rename", "Save", "Cancel"]);
    expect(calendars.renameLabel("Work")).toBe("Rename calendar Work");
  });
});

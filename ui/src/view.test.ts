import { describe, expect, it } from "vitest";
import { asCommandError } from "./api";
import type { CalendarDto } from "./api-types";
import { createCopy } from "./copy";
import { displayUrl, errorText, statusOf, titleOf } from "./view";

const en = createCopy("en-US");
const now = Date.parse("2026-10-20T10:00:00Z");

const calendar = (over: Partial<CalendarDto> = {}): CalendarDto => ({
  id: "c1",
  name: "Work",
  kind: "ics",
  enabled: true,
  url: "https://calendar.google.com/calendar/ical/me%40example.com/private-123/basic.ics",
  last_sync_at: "2026-10-20T09:59:00Z",
  last_error: null,
  ...over,
});

describe("displayUrl", () => {
  // AC1.3: only scheme and host may reach the page. Each shape hides its token somewhere different.
  const shapes: [string, string, string][] = [
    ["Google", "https://calendar.google.com/calendar/ical/me%40example.com/private-SECRET/basic.ics", "https://calendar.google.com"],
    ["Outlook", "https://outlook.office365.com/owa/calendar/SECRET@example.com/SECRET/calendar.ics", "https://outlook.office365.com"],
    ["iCloud", "https://p42-caldav.icloud.com/published/2/SECRET", "https://p42-caldav.icloud.com"],
    ["Nextcloud", "https://cloud.example.com/remote.php/dav/public-calendars/SECRET?export", "https://cloud.example.com"],
    ["a token-named file", "https://example.com/SECRET.ics", "https://example.com"],
    ["a query token", "https://example.com/feed.ics?token=SECRET", "https://example.com"],
    ["a fragment", "https://example.com/feed.ics#SECRET", "https://example.com"],
    ["user info", "https://user:SECRET@example.com/feed.ics", "https://example.com"],
    ["a port", "https://example.com:8443/SECRET/feed.ics", "https://example.com"],
  ];

  it.each(shapes)("shows scheme and host only for %s", (_name, raw, shown) => {
    expect(displayUrl(raw)).toBe(shown);
    expect(displayUrl(raw)).not.toContain("SECRET");
  });

  it("keeps the host of an IP address", () => {
    expect(displayUrl("https://127.0.0.1:4444/a/b.ics")).toBe("https://127.0.0.1");
  });

  it("shows nothing for a missing or malformed URL", () => {
    expect(displayUrl(null)).toBe("");
    expect(displayUrl("")).toBe("");
    expect(displayUrl("not a url")).toBe("");
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

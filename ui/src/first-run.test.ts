import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CalendarDto } from "./api-types";
import { createCopy, initCopy, LOCALES } from "./copy";
import { asksForPage, finishFirstRun, startFirstRunWatch, useFirstRun } from "./first-run";
import { FirstRunPage, trayTitle } from "./pages/FirstRunPage";
import { useCalendars } from "./store";

const hidden: (() => void)[] = [];
const appHideSettings = vi.fn(async () => undefined);

vi.mock("./api", () => ({
  api: { appHideSettings: () => appHideSettings(), appTrayPlace: async () => "menu_bar" },
  onSettingsHidden: vi.fn(async (handler: () => void) => {
    hidden.push(handler);
    return () => undefined;
  }),
  asCommandError: () => ({ code: "internal" }),
}));

const calendar: CalendarDto = {
  id: "c1",
  name: "Work",
  kind: "ics",
  enabled: true,
  display_url: "https://example.com",
  last_sync_at: null,
  last_error: null,
};

/** The window as it opens: the list not in yet, first run undecided. */
function openWindow(): () => void {
  hidden.length = 0;
  useFirstRun.setState({ shows: null });
  useCalendars.setState({ calendars: [], loaded: false, loadError: null });
  return startFirstRunWatch();
}

const listed = (calendars: CalendarDto[]) => useCalendars.setState({ calendars, loaded: true });

beforeEach(() => appHideSettings.mockClear());

describe("first run (F10)", () => {
  it("AC10.1 opens on first run when the first list has no calendar, and waits for the list", () => {
    const stop = openWindow();
    expect(useFirstRun.getState().shows).toBeNull();
    listed([]);
    expect(useFirstRun.getState().shows).toBe(true);
    stop();
  });

  it("AC10.2 opens on the settings pages when a calendar exists", () => {
    const stop = openWindow();
    listed([calendar]);
    expect(useFirstRun.getState().shows).toBe(false);
    // Removing the last calendar later does not bring first run back in this session.
    listed([]);
    expect(useFirstRun.getState().shows).toBe(false);
    stop();
  });

  it("AC10.2 the first calendar hides the window, then first run ends, also when the hide fails", async () => {
    const stop = openWindow();
    listed([]);
    // The list answering with the new calendar does not end it: the window must hide first, with no flash of the settings pages.
    listed([calendar]);
    expect(useFirstRun.getState().shows).toBe(true);
    await finishFirstRun();
    expect(appHideSettings).toHaveBeenCalledTimes(1);
    expect(useFirstRun.getState().shows).toBe(false);

    useFirstRun.setState({ shows: true });
    appHideSettings.mockRejectedValueOnce({ code: "not_found" });
    await finishFirstRun();
    expect(useFirstRun.getState().shows).toBe(false);
    stop();
  });

  it("skip: closing the window ends first run for the session; the tray's Settings then shows the settings pages", async () => {
    const stop = openWindow();
    listed([]);
    await Promise.resolve();
    expect(hidden).toHaveLength(1);
    hidden[0]();
    expect(useFirstRun.getState().shows).toBe(false);
    stop();
  });

  it("a route in the URL asks for a settings page, so first run does not show", () => {
    expect(asksForPage("")).toBe(false);
    expect(asksForPage("#")).toBe(false);
    expect(asksForPage("#/")).toBe(false);
    expect(asksForPage("#/calendars")).toBe(true);
    expect(asksForPage("#/alarms")).toBe(true);
  });

  it("AC10.1 names the menu bar or the system tray from where Rust says the icon lives", () => {
    const t = createCopy("en-US").t.firstRun;
    expect(trayTitle(t, "menu_bar")).toBe("Tolon is in the menu bar");
    expect(trayTitle(t, "tray")).toBe("Tolon is in the system tray");
    expect(trayTitle(t, null)).toBe(" ");
  });

  it("AC10.1 shows mockup 05: three steps, Paste an iCal link, Try a test alarm and the alarm preview", () => {
    initCopy("en-US");
    const html = renderToStaticMarkup(createElement(FirstRunPage));
    expect(html).toContain("Never miss a meeting again.");
    expect(html.match(/<ol class="sm-steps[^"]*">/g)).toHaveLength(1);
    expect(html.match(/<li[ >]/g)).toHaveLength(3);
    for (const text of ["Add a calendar", "Try it", "Paste an iCal link", "Try a test alarm", "Preview of the alarm"]) {
      expect(html).toContain(text);
    }
    // The iCal button opens the shared add form, the one the Calendars page uses.
    expect(html).toContain('aria-controls="add-calendar"');
    // The preview is a picture: its sample buttons are not buttons and screen readers skip it.
    expect(html).toMatch(/<div class="tl-preview" aria-hidden="true">/);
    expect(html.match(/<button/g)).toHaveLength(2);
    expect(html).toContain("Platform weekly sync");
    expect(html).toContain("Join the call");
    expect(html).toContain("Snooze 5 min");
    expect(html).toContain("Tolon is open source under Apache 2.0. No account, no telemetry.");
  });

  it.each(LOCALES)("%s renders first run from its own catalogue", (locale) => {
    const c = initCopy(locale);
    const html = renderToStaticMarkup(createElement(FirstRunPage));
    for (const text of [c.t.firstRun.title, c.t.firstRun.pasteIcal, c.t.firstRun.tryAlarm, c.t.firstRun.footer]) {
      expect(html).toContain(text.replace(/'/g, "&#x27;"));
    }
    expect(c.t.firstRun.trayMenuBar).toContain("Tolon");
    expect(c.t.firstRun.traySystemTray).toContain("Tolon");
    expect(c.t.firstRun.trayMenuBar).not.toBe(c.t.firstRun.traySystemTray);
  });
});

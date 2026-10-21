import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CalendarDto } from "./api-types";
import { createCopy, initCopy, LOCALES } from "./copy";
import {
  asksForPage,
  firstCalendarAdded,
  focusAtTop,
  hideAndEndFirstRun,
  listenForHide,
  startFirstRunWatch,
  useFirstRun,
} from "./first-run";
import { FirstRunPage, trayHidden, trayTitle } from "./pages/FirstRunPage";
import local from "./styles/local.css?raw";
import { useCalendars } from "./store";

const hidden: (() => void)[] = [];
const appHideSettings = vi.fn(async () => undefined);

vi.mock("./api", () => ({
  api: { appHideSettings: () => appHideSettings(), appTrayPlace: async () => "menu_bar" },
  onSettingsHidden: vi.fn(async (handler: () => void) => {
    hidden.push(handler);
    return () => undefined;
  }),
  onAlarmHide: vi.fn(async () => () => undefined),
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
  const stopHide = listenForHide();
  const stopWatch = startFirstRunWatch();
  return () => {
    stopHide();
    stopWatch();
  };
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

  it("a calendar list that failed opens the Calendars page with its error, not first run", () => {
    const stop = openWindow();
    useCalendars.setState({ calendars: [], loaded: true, loadError: { code: "db_error" } });
    expect(useFirstRun.getState().shows).toBe(false);
    stop();
  });

  it("AC10.2 a first calendar that syncs hides the window, then first run ends, also when the hide fails", async () => {
    const stop = openWindow();
    listed([]);
    // The list answering with the new calendar does not end it: the window must hide first, with no flash of the settings pages.
    listed([calendar]);
    expect(useFirstRun.getState().shows).toBe(true);
    await firstCalendarAdded(calendar);
    expect(appHideSettings).toHaveBeenCalledTimes(1);
    expect(useFirstRun.getState().shows).toBe(false);

    useFirstRun.setState({ shows: true });
    appHideSettings.mockRejectedValueOnce({ code: "not_found" });
    await firstCalendarAdded(calendar);
    expect(useFirstRun.getState().shows).toBe(false);
    stop();
  });

  it("B1 a first calendar whose sync failed keeps the window and moves to the Calendars page with its error row", async () => {
    const stop = openWindow();
    listed([]);
    await firstCalendarAdded({ ...calendar, last_error: { kind: "http", status: 404 } });
    expect(appHideSettings).not.toHaveBeenCalled();
    expect(useFirstRun.getState().shows).toBe(false);
    stop();
  });

  it("skip: Skip for now hides the window and ends first run; so does the close button (settings:hidden)", async () => {
    const stop = openWindow();
    listed([]);
    await hideAndEndFirstRun();
    expect(appHideSettings).toHaveBeenCalledTimes(1);
    expect(useFirstRun.getState().shows).toBe(false);

    useFirstRun.setState({ shows: true });
    await Promise.resolve();
    expect(hidden).toHaveLength(1);
    hidden[0]();
    expect(useFirstRun.getState().shows).toBe(false);
    stop();
  });

  it("the hide listener works before the list is in, so a close before the first render is not lost", async () => {
    const stop = openWindow();
    await Promise.resolve();
    hidden[0]();
    listed([]);
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
    expect(trayTitle(t, null)).toBeNull();
  });

  it("AC10.1 shows mockup 05: three steps, Paste an iCal link, Try a test alarm, Skip for now and the alarm preview", () => {
    initCopy("en-US");
    const html = renderToStaticMarkup(createElement(FirstRunPage));
    expect(html).toContain("Never miss a meeting again.");
    expect(html.match(/<ol class="sm-steps[^"]*" role="list">/g)).toHaveLength(1);
    expect(html.match(/<li[ >]/g)).toHaveLength(3);
    // Step headings sit in block wrappers; step 3's waits for Rust, holding its line without an empty heading.
    expect(html.match(/<h2 /g)).toHaveLength(2);
    expect(html).toMatch(/<li><div><h2 class="sm-heading">Add a calendar<\/h2><div class="sm-body">/);
    expect(html).toMatch(/<div class="sm-heading" aria-hidden="true">\u00a0<\/div>/);
    for (const text of ["Add a calendar", "Try it", "Paste an iCal link", "Try a test alarm", "Preview of the alarm"]) {
      expect(html).toContain(text);
    }
    // The iCal button opens the shared add form, the one the Calendars page uses, and has focus.
    expect(html).toMatch(/<button[^>]*class="sm-btn primary"[^>]*aria-controls="add-calendar"[^>]*>Paste an iCal link/);
    // Skip is quiet and carries its consequence.
    expect(html).toMatch(/<button[^>]*class="sm-btn quiet"[^>]*aria-describedby="skip-consequence"[^>]*>Skip for now<\/button>/);
    expect(html).toContain('id="skip-consequence">Tolon does nothing until you add a calendar.</p>');
    // The preview is a picture: its sample buttons are not buttons, screen readers skip it and it takes no pointer.
    // It takes the reminder colour; with no settings yet, the default.
    expect(html).toMatch(/<div class="tl-preview" aria-hidden="true" data-color="theme">/);
    expect(local).toMatch(/\.tl-preview \{[^}]*pointer-events: none;/);
    expect(html.match(/<button/g)).toHaveLength(3);
    expect(html).toContain("Platform weekly sync");
    // The preview's Join is the short label, so its three buttons fit on one row (D7).
    expect(html).toContain('<span class="sm-btn sm tl-primary">Join</span>');
    expect(html).not.toContain("Join the call");
    expect(html).not.toContain("autofocus");
    expect(html).toContain("Snooze 5 min");
    expect(html).toContain("Tolon is open source under Apache 2.0. No account, no telemetry.");
  });

  it.each(LOCALES)("%s renders first run from its own catalogue", (locale) => {
    const c = initCopy(locale);
    const html = renderToStaticMarkup(createElement(FirstRunPage));
    const t = c.t.firstRun;
    for (const text of [t.title, t.pasteIcal, t.tryAlarm, t.skip, t.skipConsequence, t.footer]) {
      expect(html).toContain(text.replace(/'/g, "&#x27;"));
    }
    expect(t.trayMenuBar).toContain("Tolon");
    expect(t.traySystemTray).toContain("Tolon");
    expect(t.skipConsequence).toContain("Tolon");
    expect(t.trayMenuBar).not.toBe(t.traySystemTray);
  });

  it.each(LOCALES)("%s step 3 says how to bring the window back when the tray icon is hidden", (locale) => {
    const t = createCopy(locale).t.firstRun;
    expect(trayHidden(t, "menu_bar")).toBe(t.trayHiddenMenuBar);
    expect(trayHidden(t, "tray")).toBe(t.trayHiddenSystemTray);
    // macOS names Finder and Spotlight; Windows and Linux do not have them.
    expect(t.trayHiddenMenuBar).toMatch(/Finder|访达/);
    expect(t.trayHiddenSystemTray).not.toMatch(/Finder|Spotlight|访达/);
    for (const s of [t.trayHiddenMenuBar, t.trayHiddenSystemTray]) expect(s).toContain("Tolon");
  });

  it("D6, D8 focus starts on Paste an iCal link without scrolling, and the page opens at its top", () => {
    const focus = vi.fn();
    const scrollTo = vi.fn();
    focusAtTop({ focus }, { scrollTo });
    expect(focus).toHaveBeenCalledExactlyOnceWith({ preventScroll: true });
    expect(scrollTo).toHaveBeenCalledWith(0, 0);
    // No element yet: still at the top.
    focusAtTop(null, { scrollTo });
    expect(scrollTo).toHaveBeenCalledTimes(2);
  });

  it("D7, D9 the first-run text wraps without widows and Skip for now keeps a 40 px hit area", () => {
    expect(local).toMatch(/\.tl-first :is\(h1, p, \.sm-body\) \{[^}]*text-wrap: pretty;/);
    expect(local).toMatch(/\.tl-first \.sm-btn\.quiet \{[^}]*min-block-size: 40px;[^}]*min-inline-size: 40px;/);
  });

  it("uses the review's words for he and ar", () => {
    expect(createCopy("he").t.firstRun.tryTitle).toBe("לנסות");
    expect(createCopy("ar").t.firstRun.traySystemTray).toBe("يوجد Tolon في منطقة الإعلام");
  });
});

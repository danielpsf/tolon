import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createCopy, initCopy } from "./copy";
import { CalendarsPage, GoogleSignIn, opensSignIn, watchGoogleAvailable } from "./pages/CalendarsPage";
import { FirstRunPage } from "./pages/FirstRunPage";

const available = vi.fn<() => Promise<boolean>>();

vi.mock("./api", () => ({
  api: {
    calendarsGoogleAvailable: () => available(),
    calendarsList: async () => [],
    appTrayPlace: async () => "menu_bar",
    settingsGet: async () => null,
  },
  onCalendarsChanged: vi.fn(async () => () => undefined),
  onSettingsChanged: vi.fn(async () => () => undefined),
  onSettingsHidden: vi.fn(async () => () => undefined),
  onAlarmHide: vi.fn(async () => () => undefined),
  asCommandError: () => ({ code: "internal" }),
}));

/** Lets the promise callbacks of `watchGoogleAvailable` run. */
const flush = () => new Promise<void>((done) => setTimeout(done, 0));

beforeEach(() => available.mockReset());

describe("Sign in with Google is hidden without a client ID (F13)", () => {
  it("AC13.1 a build without a client ID never says Google is available", async () => {
    available.mockResolvedValue(false);
    const set = vi.fn();
    const stop = watchGoogleAvailable(set);
    await flush();
    expect(available).toHaveBeenCalledTimes(1);
    expect(set).not.toHaveBeenCalled();
    stop();
  });

  it("AC13.1 a build with a client ID says so once", async () => {
    available.mockResolvedValue(true);
    const set = vi.fn();
    const stop = watchGoogleAvailable(set);
    await flush();
    expect(set).toHaveBeenCalledExactlyOnceWith(true);
    stop();
  });

  it("AC13.1 a failed question leaves the button hidden, and an answer after unmount is dropped", async () => {
    available.mockRejectedValue({ code: "not_ready" });
    const failed = vi.fn();
    const stop = watchGoogleAvailable(failed);
    await flush();
    expect(failed).not.toHaveBeenCalled();
    stop();

    available.mockResolvedValue(true);
    const late = vi.fn();
    watchGoogleAvailable(late)();
    await flush();
    expect(late).not.toHaveBeenCalled();
  });

  it("AC13.1 first run and the Calendars page render no Google button or Google wording before Rust says yes", () => {
    const c = initCopy("en-US");
    const first = renderToStaticMarkup(createElement(FirstRunPage));
    const calendars = renderToStaticMarkup(createElement(CalendarsPage));
    for (const html of [first, calendars]) {
      expect(html).not.toContain(c.t.calendars.signInGoogle);
      expect(html).not.toContain("google-calendars");
    }
    expect(first).toContain(c.t.firstRun.addBody);
    expect(first).not.toContain(c.t.firstRun.addBodyGoogle);
  });

  it("AC13.3 leaving the picker abandons a sign-in that is waiting or unpicked, not one being added", () => {
    expect(opensSignIn("idle", false)).toBe(false);
    expect(opensSignIn("waiting", false)).toBe(true);
    expect(opensSignIn("idle", true)).toBe(true);
    expect(opensSignIn("adding", true)).toBe(false);
  });

  it("the button itself is the one the copy names", () => {
    const html = renderToStaticMarkup(createElement(GoogleSignIn, { onAdded: () => undefined }));
    expect(html).toContain(createCopy("en-US").t.calendars.signInGoogle);
  });
});

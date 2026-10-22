import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SettingsDto } from "./api-types";
import { createCopy, initCopy, LOCALES } from "./copy";
import { AlarmPreview, liveClock, type PreviewClock } from "./pages/AlarmPreview";
import { FirstRunPage } from "./pages/FirstRunPage";
import { AppearancePage } from "./pages/SettingsPages";
import { resetSettings, useSettings } from "./store";

// L-062: the Appearance and first-run previews show Peek when Peek is chosen, and their clock is the real one
// (the test alarm's: the meeting starts 2 minutes from now), hand-in-able so these tests are deterministic.

vi.mock("./api", () => ({
  api: {
    settingsSet: vi.fn(async (patch: object) => patch),
    alarmsTest: vi.fn(async () => undefined),
    appTrayPlace: vi.fn(async () => "menu_bar"),
  },
  onAlarmHide: vi.fn(async () => () => undefined),
  onAlarmShow: vi.fn(async () => () => undefined),
  asCommandError: (e: unknown) => e,
}));

// A server render reads a zustand store's initial state; this hook reads the current one.
vi.mock("./store", async (original) => {
  const real = await original<typeof import("./store")>();
  type State = ReturnType<typeof real.useSettings.getState>;
  const current = (select?: (s: State) => unknown) => (select ? select(real.useSettings.getState()) : real.useSettings.getState());
  return { ...real, useSettings: Object.assign(current, real.useSettings) };
});

const settings = (over: Partial<SettingsDto> = {}): SettingsDto => ({
  lead_minutes: 2,
  again_at_start: false,
  sound: "bell",
  sound_repeat: true,
  quiet: { enabled: false, start: "18:00", end: "09:00", weekends: true },
  ignore: { all_day: true, declined: true, cancelled: true, no_attendees: false, keywords: [] },
  appearance: "auto",
  interrupt_color: "theme",
  style: "swiss-moss",
  locale: null,
  start_at_login: false,
  start_at_login_status: null,
  ...over,
});
const shown = (over: Partial<SettingsDto> = {}) => useSettings.setState({ settings: settings(over), error: null });

/** A clock that stands still at a local time on 22 October 2026. */
const at = (hour: number, minute: number, second = 0): PreviewClock => {
  const now = new Date(2026, 9, 22, hour, minute, second).getTime();
  return { now: () => now, tick: () => () => undefined };
};
const preview = (props: Partial<Parameters<typeof AlarmPreview>[0]> = {}) =>
  renderToStaticMarkup(createElement(AlarmPreview, { color: "theme", clock: at(10, 20, 35), ...props }));
const text = (html: string) => html.replace(/<[^>]*>/g, "|").replace(/\|+/g, "|");

beforeEach(() => initCopy("en-US"));
afterEach(() => {
  resetSettings();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("the preview's clock (L-062)", () => {
  it("shows the clock it is given, to the minute, with the meeting 2 minutes on and 45 minutes long", () => {
    const html = text(preview({ clock: at(10, 20, 35) }));
    expect(html).toContain("|10:20 AM|Starts in 2 min|");
    expect(html).toContain("10:22 AM to 11:07 AM");
  });

  it("follows the clock, in the locale's own format: no hard-coded 1:58 or 2:00 PM", () => {
    const evening = text(preview({ clock: at(17, 5) }));
    expect(evening).toContain("|5:05 PM|Starts in 2 min|");
    expect(evening).toContain("5:07 PM to 5:52 PM");
    for (const html of [evening, text(preview({ clock: at(10, 20) }))]) expect(html).not.toMatch(/1:58 PM|2:00 PM|2:45 PM/);
    initCopy("de");
    expect(text(preview({ clock: at(17, 5) }))).toContain("17:05");
  });

  it("starts the meeting in 2 minutes across an hour and a day", () => {
    expect(text(preview({ clock: at(23, 59) }))).toContain("|11:59 PM|Starts in 2 min|");
    expect(text(preview({ clock: at(11, 59) }))).toContain("12:01 PM to 12:46 PM");
  });

  it("the live clock reads the system time and ticks every second, until stopped", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 9, 22, 8, 15, 0));
    expect(liveClock.now()).toBe(new Date(2026, 9, 22, 8, 15, 0).getTime());
    const ticks: number[] = [];
    const stop = liveClock.tick((now) => ticks.push(now));
    vi.advanceTimersByTime(2000);
    expect(ticks).toEqual([0, 1000, 2000].map((ms) => new Date(2026, 9, 22, 8, 15, 0).getTime() + ms));
    stop();
    vi.advanceTimersByTime(3000);
    expect(ticks).toHaveLength(3);
  });

  it("both pages show the real current time with no clock handed in", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 9, 22, 9, 41, 12));
    for (const page of [FirstRunPage, AppearancePage]) {
      shown();
      const html = text(renderToStaticMarkup(createElement(page)));
      expect(html, page.name).toContain("|9:41 AM|Starts in 2 min|");
      expect(html, page.name).toContain("9:43 AM to 10:28 AM");
    }
  });
});

describe("Peek in the previews (L-062)", () => {
  it("draws the Swiss Moss card unchanged when Peek is not chosen", () => {
    const html = preview();
    expect(html).toContain('<div class="tl-preview" aria-hidden="true" data-color="theme">');
    expect(html).not.toContain("tl-peek");
  });

  it("draws a small Peek card when it is: the reminder's page, Join, the hill, Peek and the bubble, all hidden from screen readers", () => {
    const html = preview({ peek: true });
    expect(html).toContain('<div class="tl-preview tl-preview-peek" aria-hidden="true">');
    expect(html).toContain('<div class="tl-preview-page">');
    expect(html).toContain('<span class="sm-btn sm tl-primary" data-color="moss-deep" data-tokens="light">Join</span>');
    expect(html).toContain("Snooze 5 min");
    expect(html).toContain('<div class="tl-peek tl-peek-card" aria-hidden="true" data-color="moss-deep" data-tokens="light">');
    expect(html).toContain('<p class="tl-peek-say">It&#x27;s time.</p>');
    expect(html).toContain("Platform weekly sync");
    expect(html).toContain("Preview of the alarm");
    // A picture: no button, no focusable element.
    expect(html).not.toMatch(/<button|tabindex|<a /);
  });

  it("the card's page follows the tokens it is given; Join and the hill take the reminder colour, pinned to light", () => {
    const html = preview({ peek: true, color: "ink", tokens: "dark" });
    expect(html).toContain('<div class="tl-preview tl-preview-peek" aria-hidden="true" data-tokens="dark">');
    expect(html).toContain('class="sm-btn sm tl-primary" data-color="ink" data-tokens="light"');
    expect(html).toContain('class="tl-peek tl-peek-card" aria-hidden="true" data-color="ink" data-tokens="light"');
  });

  it("gives every scene its own gradient id, so two cards on a page do not share one", () => {
    const two = renderToStaticMarkup(createElement("div", null, createElement(AlarmPreview, { color: "theme", peek: true }), createElement(AlarmPreview, { color: "theme", peek: true })));
    const ids = [...two.matchAll(/<radialGradient id="([^"]+)"/g)].map((m) => m[1]);
    expect(new Set(ids).size).toBe(2);
  });

  it.each(["first run", "Appearance"])("the %s preview is Peek when the style is Peek, and Swiss Moss otherwise", (where) => {
    const page = where === "first run" ? FirstRunPage : AppearancePage;
    shown({ style: "swiss-moss" });
    expect(renderToStaticMarkup(createElement(page))).not.toContain("tl-preview-peek");
    shown({ style: "peek", interrupt_color: "danger" });
    const html = renderToStaticMarkup(createElement(page));
    expect(html).toMatch(/<div class="tl-preview tl-preview-peek" aria-hidden="true"[^>]*>/);
    expect(html).toContain('<span class="sm-btn sm tl-primary" data-color="danger" data-tokens="light">Join</span>');
  });

  it("the Appearance page offers the dark or light preview for any colour in Peek, as the page flips, and for Default only otherwise", () => {
    shown({ style: "peek", interrupt_color: "ink" });
    expect(renderToStaticMarkup(createElement(AppearancePage))).toContain('data-action="preview-theme"');
    shown({ style: "swiss-moss", interrupt_color: "ink" });
    expect(renderToStaticMarkup(createElement(AppearancePage))).not.toContain('data-action="preview-theme"');
    shown({ style: "swiss-moss", interrupt_color: "theme" });
    expect(renderToStaticMarkup(createElement(AppearancePage))).toContain('data-action="preview-theme"');
  });

  it.each(LOCALES)("%s: the card names the sample and the bubble from its own catalogue", (locale) => {
    initCopy(locale);
    const t = createCopy(locale).t;
    const html = preview({ peek: true });
    expect(html).toContain(t.firstRun.sampleTitle.replace(/'/g, "&#x27;"));
    expect(html).toContain(t.interrupt.peekSay.replace(/'/g, "&#x27;"));
  });
});

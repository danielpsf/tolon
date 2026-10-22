import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "./api";
import { STYLES, type AlarmShowDto, type InterruptColor, type SettingsDto } from "./api-types";
import { createCopy, initCopy, LOCALES } from "./copy";
import { InterruptPage, InterruptView } from "./pages/InterruptPage";
import { AppearancePage } from "./pages/SettingsPages";
import local from "./styles/local.css?raw";
import { resetSettings, useSettings } from "./store";
import { applyStyle } from "./theme";

// L-050, decision "The poll picks Peek": the style setting reaches every window as `data-style` on <html>,
// the Appearance page offers it next to Theme, and the Peek reminder keeps the interrupt's keys, buttons,
// copy and aria while drawing the character from tokens.

vi.mock("./api", () => ({
  api: {
    settingsSet: vi.fn(async (patch: object) => ({ ...settings(), ...patch })),
    alarmsTest: vi.fn(async () => undefined),
    alarmsCurrent: vi.fn(async () => null),
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

const alarm: AlarmShowDto = {
  test: false,
  meetings: [
    {
      key: "k1",
      title: "Platform weekly sync",
      start: new Date(Date.now() + 60_000).toISOString(),
      end: new Date(Date.now() + 46 * 60_000).toISOString(),
      join: { kind: "link", host: "meet.google.com", recognised: true },
      people: 8,
    },
  ],
};
const first = alarm.meetings[0]!;
const view = (peek: boolean, color: InterruptColor = "theme") =>
  renderToStaticMarkup(createElement(InterruptView, { alarm, first, screen: "0", color, peek }));

beforeEach(() => vi.stubGlobal("window", { location: { search: "?screen=0" } }));
afterEach(() => {
  resetSettings();
  vi.unstubAllGlobals();
});

describe("data-style (L-050)", () => {
  it("applyStyle sets data-style on <html> for each style", () => {
    const root = { dataset: {} as Record<string, string> };
    vi.stubGlobal("document", { documentElement: root });
    for (const style of STYLES) {
      applyStyle(style);
      expect(root.dataset.style).toBe(style);
    }
  });

  it("local.css scopes every Peek rule under data-style, and the :lang() font stacks come after it", () => {
    const start = local.indexOf(" * Peek (L-050");
    const end = local.indexOf(" * Right to left");
    expect(start).toBeGreaterThan(0);
    const selectors = [...local.slice(start, end).matchAll(/(?:^|\})\s*([^{}/]+?)\s*\{/g)].map((m) => m[1]?.trim() ?? "");
    expect(selectors.length).toBeGreaterThan(20);
    for (const s of selectors) for (const part of s.split(",")) expect(part.trim(), s).toMatch(/^(:root(\[dir="rtl"\])?)?\[data-style="peek"\]/);
    expect(local.indexOf(":root:lang(ar) {")).toBeGreaterThan(end);
    expect(local).toMatch(/:root\[data-style="peek"\] \{\s*--sm-font-display: Fraunces, "Inter Tight", Inter, system-ui, serif;/);
  });
});

describe("Appearance page Style (L-050)", () => {
  const radios = (html: string) => [...html.matchAll(/<input type="radio" name="style"[^>]*>/g)].map((m) => m[0]);

  it("offers Swiss Moss and Peek as one radiogroup next to Theme, the stored one checked", () => {
    initCopy("en-US");
    shown({ style: "peek" });
    const html = render();
    expect(html).toContain('<span class="sm-label" id="style-heading">Style</span>');
    expect(html).toContain('<div class="sm-segmented tl-segmented" role="radiogroup" aria-labelledby="style-heading">');
    expect(radios(html).map((r) => /value="([\w-]+)"/.exec(r)?.[1])).toEqual([...STYLES]);
    expect(radios(html).filter((r) => r.includes('checked=""'))).toHaveLength(1);
    expect(radios(html).find((r) => r.includes('checked=""'))).toContain('value="peek"');
    // Theme, then Style, then the reminder colour.
    expect(html.indexOf("theme-heading")).toBeLessThan(html.indexOf("style-heading"));
    expect(html.indexOf("style-heading")).toBeLessThan(html.indexOf("color-heading"));
  });

  it("AC9.1 a new style is saved at once and round-trips", async () => {
    shown();
    await useSettings.getState().save({ style: "peek" });
    expect(api.settingsSet).toHaveBeenCalledWith({ style: "peek" });
    expect(useSettings.getState().settings?.style).toBe("peek");
  });

  it.each(LOCALES)("%s names the section and the speech bubble", (locale) => {
    const t = createCopy(locale).t;
    expect(t.settings.appearance.style.trim()).not.toBe("");
    expect(t.settings.appearance.styles).toEqual({ "swiss-moss": "Swiss Moss", peek: "Peek" });
    expect(t.interrupt.peekSay.trim()).not.toBe("");
    expect(t.interrupt.peekSay).not.toMatch(/!/);
  });

  function render() {
    return renderToStaticMarkup(createElement(AppearancePage));
  }
});

describe("the Peek reminder (AC4.3, AC4.4, L-050)", () => {
  it("keeps the dialog, its aria, the three buttons, the hint and the consequence line", () => {
    initCopy("en-US");
    const peek = view(true);
    const swiss = view(false);
    const aria = (html: string) => /^<main [^>]*>/.exec(html)?.[0].replace(/ data-tokens="[^"]*"/, "");
    expect(aria(peek)).toBe(aria(swiss));
    expect(peek).toContain('role="alertdialog"');
    expect(peek).toContain('aria-labelledby="tl-int-title"');
    for (const action of ["join", "snooze", "dismiss"]) expect(peek).toContain(`data-action="${action}"`);
    expect(peek).toContain("Dismissing silences this reminder on this computer.");
    expect(peek).toContain('<p class="tl-int-hint">');
    expect(peek).toContain("Platform weekly sync");
  });

  it("puts Join first with an Enter chip hidden from screen readers", () => {
    initCopy("en-US");
    const html = view(true);
    expect(html).toMatch(/data-action="join" data-color="moss-deep" data-tokens="light"><span>Join the call<\/span><kbd class="tl-peek-key" aria-hidden="true">Enter<\/kbd><\/button>/);
    expect(html.indexOf('data-action="join"')).toBeLessThan(html.indexOf('data-action="snooze"'));
    expect(view(false)).not.toContain("tl-peek");
  });

  it("draws Peek as decorative inline SVG with the speech bubble, and no motion", () => {
    initCopy("en-US");
    const html = view(true);
    const scene = /<div class="tl-peek" aria-hidden="true"[^>]*>.*<\/div>/.exec(html)?.[0] ?? "";
    expect(scene).toContain('<svg class="tl-peek-art" viewBox="0 0 1280 600" focusable="false">');
    for (const part of ["eye", "pupil", "shine", "cheek", "smile", "dome"]) expect(scene).toContain(`tl-peek-${part}`);
    expect(scene).toContain('<p class="tl-peek-say">It&#x27;s time.</p>');
    // Colours come from local.css: no fill or stroke in the markup but the fireflies' gradient reference (L-062), and nothing moves.
    expect(scene).not.toMatch(/fill="(?!url\(#tl-peek-glow-\w+\)")|stroke=|style=|#[0-9A-Fa-f]{3,6}\b/);
    expect(local.slice(local.indexOf(" * Peek (L-050"), local.indexOf(" * Right to left"))).not.toMatch(/animation|transition|@keyframes/);
  });

  it.each(["theme", "info", "paper", "danger"] as const)("tints the hill and Join with %s; the page keeps the window's tokens", (color) => {
    initCopy("en-US");
    const html = view(true, color);
    const want = color === "theme" ? "moss-deep" : color;
    expect(html).toContain(`<div class="tl-peek" aria-hidden="true" data-color="${want}" data-tokens="light">`);
    expect(html).toMatch(new RegExp(`data-action="join" data-color="${want}" data-tokens="light"`));
    expect(html).toMatch(new RegExp(`^<main class="tl-int"[^>]* data-color="${color}"`));
    expect(html).not.toMatch(/^<main [^>]*data-tokens=/);
  });

  it("the waiting page in Peek is not pinned either", () => {
    initCopy("en-US");
    shown({ style: "peek", interrupt_color: "ink" });
    expect(renderToStaticMarkup(createElement(InterruptPage))).not.toMatch(/^<main [^>]*data-tokens=/);
    shown({ style: "swiss-moss", interrupt_color: "ink" });
    expect(renderToStaticMarkup(createElement(InterruptPage))).toMatch(/^<main [^>]*data-tokens="light"/);
  });

  it("speaks Arabic in the bubble and keeps the Arabic stack for the display face", () => {
    initCopy("ar");
    expect(view(true)).toContain('<p class="tl-peek-say">حان الوقت.</p>');
    expect(local).toMatch(/:root:lang\(ar\) \{[^}]*--sm-font-display: "Inter Tight", Inter, "Noto Sans Arabic"/);
    expect(local).toMatch(/:root\[dir="rtl"\]\[data-style="peek"\] \.tl-peek-art \{\s*transform: scaleX\(-1\);/);
  });
});

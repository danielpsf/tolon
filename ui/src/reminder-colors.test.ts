import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "./api";
import { INTERRUPT_COLORS, type SettingsDto } from "./api-types";
import { createCopy, initCopy, LOCALES } from "./copy";
import { FirstRunPage } from "./pages/FirstRunPage";
import { InterruptPage } from "./pages/InterruptPage";
import { AppearancePage } from "./pages/SettingsPages";
import { colorAttr } from "./reminder-colors";
import local from "./styles/local.css?raw";
import { resetSettings, useSettings } from "./store";

// L-043: the reminder colour reaches the interrupt and both previews as `data-color`, and the Appearance
// page offers it as a radiogroup of swatches that saves on change (AC9.1).

vi.mock("./api", () => ({
  api: {
    settingsSet: vi.fn(async (patch: object) => ({ ...settings(), ...patch })),
    alarmsTest: vi.fn(async () => undefined),
    alarmsCurrent: vi.fn(async () => null),
    appTrayPlace: vi.fn(async () => "menu_bar"),
  },
  onAlarmHide: vi.fn(async () => () => undefined),
  onAlarmShow: vi.fn(async () => () => undefined),
  asCommandError: (e: unknown) => e,
}));

// A server render reads a zustand store's initial state; this hook reads the current one, so a page
// renders the settings a test put in.
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
const render = (page: () => React.ReactElement) => renderToStaticMarkup(createElement(page));

afterEach(() => {
  resetSettings();
  vi.unstubAllGlobals();
});

describe("data-color (L-043)", () => {
  beforeEach(() => vi.stubGlobal("window", { location: { search: "?screen=0" } }));

  it("maps each setting to its own value and anything else to the default", () => {
    for (const color of INTERRUPT_COLORS) expect(colorAttr(color)).toBe(color);
    for (const other of [undefined, null, "", "blue", "MOSS", "moss_deep"]) expect(colorAttr(other)).toBe("theme");
  });

  it.each(INTERRUPT_COLORS)("the interrupt root carries data-color=%s from the settings, pinned to light unless Default", (color) => {
    initCopy("en-US");
    shown({ interrupt_color: color });
    const html = render(InterruptPage);
    expect(html).toMatch(new RegExp(`^<main class="tl-int"[^>]* data-color="${color}"`));
    if (color === "theme") expect(html).not.toMatch(/^<main [^>]*data-tokens=/);
    else expect(html).toMatch(/^<main [^>]*data-tokens="light"><style>\[data-tokens=light\]\{/);
  });

  it("the interrupt is the default before the settings are in", () => {
    initCopy("en-US");
    expect(render(InterruptPage)).toContain('data-color="theme"');
  });

  it("the first-run preview follows the choice", () => {
    initCopy("en-US");
    shown({ interrupt_color: "danger" });
    expect(render(FirstRunPage)).toMatch(/<div class="tl-preview" aria-hidden="true" data-color="danger" data-tokens="light">/);
  });
});

describe("Appearance page (L-043)", () => {
  const radios = (html: string) =>
    [...html.matchAll(/<input type="radio" name="interrupt_color"[^>]*>/g)].map((m) => [
      m[0],
      /value="([\w-]+)"/.exec(m[0])?.[1],
      m[0].includes('checked=""'),
    ]);

  it("offers the eight colours as one radiogroup of named swatches, the stored one checked", () => {
    initCopy("en-US");
    shown({ interrupt_color: "info" });
    const html = render(AppearancePage);
    expect(html).toContain('<div class="tl-colors" role="radiogroup" aria-labelledby="color-heading" aria-describedby="color-hint">');
    expect(html).toContain('<span class="sm-label" id="color-heading">Alarm color</span>');
    // One name group of native radios: the arrow keys move the choice, Tab reaches the checked one.
    expect(radios(html).map((m) => m[1])).toEqual([...INTERRUPT_COLORS]);
    expect(radios(html).filter((m) => m[2]).map((m) => m[1])).toEqual(["info"]);
    // Each radio is named by its label's text; the sample chip is hidden from screen readers.
    const t = createCopy("en-US").t.settings.appearance.colors;
    // Default is drawn in both appearances' moss (L-046); every other chip is one pinned colour.
    const chip = (color: string) =>
      color === "theme"
        ? '<span class="tl-split"><span data-color="theme" data-tokens="light">Aa</span><span data-color="theme" data-tokens="dark">Aa</span></span>'
        : `<span data-color="${color}" data-tokens="light">Aa</span>`;
    for (const color of INTERRUPT_COLORS) {
      expect(html).toContain(`<i aria-hidden="true">${chip(color)}</i><span>${t[color]}</span></label>`);
    }
  });

  it("B1 in a dark window Default turns dark and every other swatch keeps its light values", () => {
    initCopy("en-US");
    shown({ interrupt_color: "ink", appearance: "dark" });
    vi.stubGlobal("document", { documentElement: { dataset: { theme: "dark" } } });
    const html = render(AppearancePage);
    // The Default chip is half light, half dark whatever the window is.
    expect(html).toContain('<span class="tl-split"><span data-color="theme" data-tokens="light">Aa</span><span data-color="theme" data-tokens="dark">Aa</span></span>');
    for (const color of INTERRUPT_COLORS.filter((c) => c !== "theme")) {
      expect(html).toContain(`<span data-color="${color}" data-tokens="light">Aa</span>`);
    }
    // The preview of a pinned colour is light too, and the dark/light toggle is for Default only.
    expect(html).toMatch(/<div class="tl-preview" aria-hidden="true" data-color="ink" data-tokens="light">/);
    expect(html).not.toContain('data-action="preview-theme"');
  });

  it("previews the alarm in the chosen colour and the window's appearance, with a button for the other one", () => {
    initCopy("en-US");
    shown({ interrupt_color: "theme" });
    let html = render(AppearancePage);
    expect(html).toMatch(/<div class="tl-preview" aria-hidden="true" data-color="theme" data-tokens="light">/);
    expect(html).toMatch(/<button type="button" class="sm-btn" data-action="preview-theme">Preview in dark<\/button>/);
    expect(html).toMatch(/<button type="button" class="sm-btn" data-action="test-alarm" aria-disabled="false">Show a test alarm<\/button>/);
    expect(html).toContain("<style>[data-tokens=light]{");

    vi.stubGlobal("document", { documentElement: { dataset: { theme: "dark" } } });
    html = render(AppearancePage);
    expect(html).toMatch(/<div class="tl-preview" aria-hidden="true" data-color="theme" data-tokens="dark">/);
    expect(html).toContain(">Preview in light</button>");
  });

  it("marks the checked swatch with an outline under forced colours, where box-shadow is dropped", () => {
    expect(local).toMatch(/@media \(forced-colors: active\) \{\s*\.tl-swatch input:checked \+ i \{[^}]*outline: 3px solid CanvasText;/);
  });

  it("renders nothing to choose before the settings are in", () => {
    initCopy("en-US");
    expect(render(AppearancePage)).not.toContain("tl-colors");
  });

  it("AC9.1 a new colour is saved at once", async () => {
    shown();
    await useSettings.getState().save({ interrupt_color: "ink" });
    expect(api.settingsSet).toHaveBeenCalledWith({ interrupt_color: "ink" });
    expect(useSettings.getState().settings?.interrupt_color).toBe("ink");
  });

  it.each(LOCALES)("%s names every colour, the section, its hint and the preview buttons", (locale) => {
    const p = createCopy(locale).t.settings.appearance;
    const names = INTERRUPT_COLORS.map((c) => p.colors[c]);
    expect(new Set(names).size, "names are distinct").toBe(8);
    for (const text of [...names, p.color, p.colorHint, p.previewDark, p.previewLight, p.testAlarm]) expect(text.trim()).not.toBe("");
    expect(p.previewDark).not.toBe(p.previewLight);
  });
});

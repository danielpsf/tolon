import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { JoinDto, ListedDto, SettingsDto } from "./api-types";
import { createCopy, LOCALES } from "./copy";
import {
  firstLine,
  focusFirstAction,
  hidesPanel,
  isolate,
  nextOf,
  pausedText,
  remaining,
  rowTime,
  rowWhere,
  summaryText,
  tooltipText,
} from "./panel";
import { PanelPage } from "./pages/PanelPage";

vi.mock("./api", () => ({
  api: {},
  onOccurrencesChanged: vi.fn(async () => () => undefined),
  onPaused: vi.fn(async () => () => undefined),
  onSettingsChanged: vi.fn(async () => () => undefined),
  onTrayHover: vi.fn(async () => () => undefined),
  asCommandError: () => ({ code: "internal" }),
}));

const en = createCopy("en-US");
const MIN = 60_000;
const now = Date.parse("2026-10-20T13:48:00Z");

const meet: JoinDto = { kind: "link", host: "meet.google.com", recognised: true };
const item = (uid: string, start: string, over: Partial<ListedDto> = {}): ListedDto => ({
  key: `cal:${uid}@${start}`,
  title: uid,
  start,
  end: new Date(Date.parse(start) + 30 * MIN).toISOString(),
  all_day: false,
  people: 3,
  ignored: null,
  join: meet,
  ...over,
});

const TODAY = [
  item("Incident review", "2026-10-20T17:00:00Z", { join: { kind: "none" } }),
  item("Platform weekly sync", "2026-10-20T14:00:00Z"),
  item("Focus time", "2026-10-20T13:00:00Z", { ignored: "keyword", end: "2026-10-20T15:00:00Z" }),
  item("Holiday", "2026-10-20T00:00:00Z", { all_day: true, end: "2026-10-21T00:00:00Z" }),
  item("Breakfast", "2026-10-20T08:00:00Z"),
];

const settings = (over: Partial<SettingsDto["quiet"]> = {}): SettingsDto => ({
  lead_minutes: 2,
  again_at_start: false,
  sound: "bell",
  sound_repeat: true,
  quiet: { enabled: false, start: "18:00", end: "09:00", weekends: true, ...over },
  ignore: { all_day: true, declined: true, cancelled: true, no_attendees: false, keywords: [] },
  appearance: "auto",
  locale: null,
  start_at_login: false,
  start_at_login_status: null,
});

describe("tray panel (F7)", () => {
  it("AC7.2 lists today's remaining meetings in order, ignored and all-day ones included (S8.1)", () => {
    expect(remaining(TODAY, now).map((m) => m.title)).toEqual([
      "Holiday",
      "Focus time",
      "Platform weekly sync",
      "Incident review",
    ]);
  });

  it("AC7.2 puts the first meeting that gets an alarm first, also while it runs", () => {
    expect(nextOf(TODAY, now)?.title).toBe("Platform weekly sync");
    expect(nextOf(TODAY, Date.parse("2026-10-20T14:10:00Z"))?.title).toBe("Platform weekly sync");
    expect(nextOf(TODAY, Date.parse("2026-10-20T14:31:00Z"))?.title).toBe("Incident review");
    expect(nextOf(TODAY, Date.parse("2026-10-20T18:00:00Z"))).toBeNull();
  });

  it("AC7.1 the first line says the minutes until the next meeting", () => {
    const next = TODAY[1];
    expect(firstLine(en, next, now)).toBe("Next, in 12 min");
    expect(firstLine(en, next, Date.parse("2026-10-20T13:59:30Z"))).toBe("Next, in 1 min");
    expect(firstLine(en, next, Date.parse("2026-10-20T14:03:00Z"))).toBe("Started 3 min ago");
  });

  it("AC7.1 the tooltip names the next meeting and the minutes until it", () => {
    expect(tooltipText(en, TODAY, null, now)).toBe("\u2068Platform weekly sync\u2069 in 12 min");
    expect(tooltipText(en, TODAY, null, Date.parse("2026-10-20T14:05:00Z"))).toBe("Now: \u2068Platform weekly sync\u2069");
    expect(tooltipText(en, [], null, now)).toBe("No more meetings today.");
  });

  it("S7.1 AC7.3 shows Paused until HH:MM in the locale's clock, on the panel and in the tooltip", () => {
    const until = "2026-10-20T14:48:00Z";
    const hhmm = en.formatTime(Date.parse(until));
    expect(pausedText(en, until)).toBe(`Paused until ${hhmm}`);
    expect(tooltipText(en, TODAY, until, now)).toBe(`Paused until ${hhmm}\n\u2068Platform weekly sync\u2069 in 12 min`);
    const de = createCopy("de");
    expect(pausedText(de, until)).toBe(`Pausiert bis ${de.formatTime(Date.parse(until))}`);
  });

  it("AC7.2 rows show the start or All day, and the provider, the host, Event page or No link", () => {
    expect(rowTime(en, TODAY[3])).toBe("All day");
    expect(rowTime(en, TODAY[1])).toBe(en.formatTime(Date.parse(TODAY[1].start)));
    expect(rowWhere(en, TODAY[1])).toBe("Google Meet");
    expect(rowWhere(en, item("x", TODAY[1].start, { join: { kind: "link", host: "example.com", recognised: false } }))).toBe(
      "example.com",
    );
    expect(rowWhere(en, item("x", TODAY[1].start, { join: { kind: "event_page" } }))).toBe("Event page");
    expect(rowWhere(en, TODAY[0])).toBe("No link");
  });

  it("states the lead time and the quiet hours under the actions", () => {
    expect(summaryText(en, settings())).toBe("Alarms 2 min before. Quiet hours off.");
    expect(summaryText(en, settings({ enabled: true }))).toBe("Alarms 2 min before. Quiet hours 18:00 to 09:00.");
  });

  it.each(LOCALES)("%s renders every panel string", (locale) => {
    const c = createCopy(locale);
    const texts = [
      tooltipText(c, TODAY, "2026-10-20T14:48:00Z", now),
      tooltipText(c, TODAY, null, Date.parse("2026-10-20T14:05:00Z")),
      firstLine(c, TODAY[1], now),
      summaryText(c, settings({ enabled: true })),
      summaryText(c, settings()),
    ];
    for (const text of texts) {
      expect(text).not.toContain("undefined");
      expect(text.length).toBeGreaterThan(3);
    }
    expect(texts[0]).toContain("Platform weekly sync");
  });

  it.each(LOCALES)("AC7.1 %s wraps the title in FSI and PDI in the tooltip, with its format characters dropped", (locale) => {
    const sneaky = item("Stand\u202Eup\u200B", "2026-10-20T14:00:00Z");
    const text = tooltipText(createCopy(locale), [sneaky], null, now);
    expect(text).toContain("\u2068Standup\u2069");
    expect(text.match(/\u2068/g)).toHaveLength(1);
    expect(text.match(/\u2069/g)).toHaveLength(1);
    expect(text).not.toMatch(/[\u200B\u202E]/);
  });

  it("AC7.1 isolates a title: format characters out, then FSI and PDI around it", () => {
    expect(isolate("Team (EU)")).toBe("\u2068Team (EU)\u2069");
    expect(isolate("\u2068a\u2069\u202Eb\u2066")).toBe("\u2068ab\u2069");
  });

  it("AC7.2 Esc hides the panel; modified Esc, IME input and other keys do not", () => {
    const key = (over: Partial<KeyboardEvent>) => ({
      key: "Escape",
      ctrlKey: false,
      metaKey: false,
      altKey: false,
      isComposing: false,
      ...over,
    });
    expect(hidesPanel(key({}))).toBe(true);
    for (const over of [{ key: "Enter" }, { key: "d" }, { metaKey: true }, { ctrlKey: true }, { altKey: true }, { isComposing: true }]) {
      expect(hidesPanel(key(over)), JSON.stringify(over)).toBe(false);
    }
  });

  it("AC7.2 focus starts on the first action when the panel shows", () => {
    const focus = vi.fn();
    const select = vi.fn(() => ({ focus }));
    expect(focusFirstAction({ querySelector: select } as unknown as ParentNode)).toBe(true);
    expect(select).toHaveBeenCalledWith(".tl-panel-actions button");
    expect(focus).toHaveBeenCalledExactlyOnceWith({ preventScroll: true });
    expect(focusFirstAction({ querySelector: () => null } as unknown as ParentNode)).toBe(false);
    expect(focusFirstAction(null)).toBe(false);
  });

  it.each(LOCALES)("%s has the panel's Dismiss consequence and the snooze-ended sentence", (locale) => {
    const { panel } = createCopy(locale).t;
    for (const text of [panel.dismissConsequence, panel.snoozeEnded]) {
      expect(text.length).toBeGreaterThan(10);
      expect(text).not.toContain("!");
    }
    expect(panel.snoozeEnded).toContain("Tolon");
  });

  it("AC7.2 offers Pause for 1 hour, Settings and Quit before anything loads", () => {
    const html = renderToStaticMarkup(createElement(PanelPage));
    for (const label of ["Pause for 1 hour", "Settings", "Quit", "No more meetings today."]) {
      expect(html).toContain(label);
    }
  });
});

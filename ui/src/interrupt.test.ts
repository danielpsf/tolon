import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "./api";
import type { JoinDto, MeetingDto } from "./api-types";
import { createCopy, LOCALES } from "./copy";
import {
  countdownText,
  detailsText,
  handlesKey,
  hintText,
  joinHost,
  primaryParts,
  keyAction,
  nextAlarmView,
  nextFocusIndex,
  NO_ALARM,
  perform,
  primaryAction,
  primaryLabel,
  providerOf,
  startClock,
  titleSize,
} from "./interrupt";
import page from "./pages/InterruptPage.tsx?raw";

vi.mock("./api", () => ({
  api: {
    alarmsSnooze: vi.fn(async () => undefined),
    alarmsDismiss: vi.fn(async () => undefined),
    alarmsJoin: vi.fn(async () => undefined),
    alarmsAlreadyThere: vi.fn(async () => undefined),
  },
}));

const en = createCopy("en-US");
const start = Date.parse("2026-10-20T14:00:00Z");
const MIN = 60_000;

const link = (host: string, recognised: boolean): JoinDto => ({ kind: "link", host, recognised });
const meeting = (join: JoinDto, over: Partial<MeetingDto> = {}): MeetingDto => ({
  key: "cal:a@2026-10-20T14:00:00Z",
  title: "Platform weekly sync",
  start: "2026-10-20T14:00:00Z",
  end: "2026-10-20T14:45:00Z",
  join,
  people: 8,
  ...over,
});
const press = (key: string, over: Partial<KeyboardEvent> = {}) => ({
  key,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  repeat: false,
  isComposing: false,
  ...over,
});

describe("countdown (AC4.7, S3.4)", () => {
  it("AC4.7 reads 'Starts in N min' before the start and rounds up", () => {
    expect(countdownText(en, start, start - 2 * MIN)).toBe("Starts in 2 min");
    expect(countdownText(en, start, start - 61_000)).toBe("Starts in 2 min");
    expect(countdownText(en, start, start - 1_000)).toBe("Starts in 1 min");
  });

  it("AC4.7 flips to 'Started just now' at the start and 'Started N min ago' after it", () => {
    expect(countdownText(en, start, start)).toBe("Started just now");
    expect(countdownText(en, start, start + 59_000)).toBe("Started just now");
    expect(countdownText(en, start, start + 3 * MIN)).toBe("Started 3 min ago");
  });

  it("S3.4 a wake inside the window shows 'Started 3 min ago'", () => {
    expect(countdownText(en, start, start + 3 * MIN + 10_000)).toBe("Started 3 min ago");
  });

  it("AC4.7 the text follows the locale", () => {
    expect(countdownText(createCopy("pt-BR"), start, start - 2 * MIN)).toBe("Começa em 2 min");
    expect(countdownText(createCopy("pt-PT"), start, start + 3 * MIN)).toBe("Começou há 3 min");
  });
});

describe("the clock (AC4.7)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("AC4.7 ticks at once and then every second, so the countdown text changes at the minute", () => {
    vi.setSystemTime(start - 61_000);
    const seen: string[] = [];
    const stop = startClock((now) => seen.push(countdownText(en, start, now)));
    vi.advanceTimersByTime(3_000);
    stop();
    vi.advanceTimersByTime(5_000);
    // 61 s, 60 s, 59 s, 58 s left: the first tick plus one per second, none after stop().
    expect(seen).toEqual(["Starts in 2 min", "Starts in 1 min", "Starts in 1 min", "Starts in 1 min"]);
  });

  it("AC4.7 crosses the start with one tick per second", () => {
    vi.setSystemTime(start - 1_000);
    const seen: string[] = [];
    const stop = startClock((now) => seen.push(countdownText(en, start, now)));
    vi.advanceTimersByTime(1_000);
    stop();
    expect(seen).toEqual(["Starts in 1 min", "Started just now"]);
  });
});

describe("primary button (AC5.2)", () => {
  it("S5.2 a link joins, an event page opens the event, nothing is 'I am already there'", () => {
    expect(primaryAction(link("meet.google.com", true))).toBe("join");
    expect(primaryAction({ kind: "event_page" })).toBe("open_event");
    expect(primaryAction({ kind: "none" })).toBe("already_there");
  });

  it("S5.2 labels each case", () => {
    expect(primaryLabel(en, meeting(link("meet.google.com", true)))).toBe("Join the call");
    expect(primaryLabel(en, meeting({ kind: "event_page" }))).toBe("Open the event");
    expect(primaryLabel(en, meeting({ kind: "none" }))).toBe("I am already there");
  });

  it("S5.2 the key hint says what Enter does", () => {
    expect(hintText(en, "join")).toContain("Enter joins");
    expect(hintText(en, "open_event")).toContain("Enter opens the event");
    expect(hintText(en, "already_there")).toContain("Enter: I am already there");
  });

  it("AC4.4 the hint names Space only while Space means 'I am already there'", () => {
    expect(hintText(en, "join", true)).toBe("Esc snoozes. Enter joins. D dismisses. Space marks you as already there.");
    expect(hintText(en, "join", false)).toBe("Esc snoozes. Enter joins. D dismisses.");
    for (const locale of LOCALES) {
      const c = createCopy(locale);
      expect(hintText(c, "join", true)).toContain(c.t.interrupt.hintSpace);
      expect(hintText(c, "join", false)).not.toContain(c.t.interrupt.hintSpace);
    }
  });

  it("joins the hint sentences per locale: a space, but nothing after 。 in zh-CN and ja", () => {
    expect(hintText(createCopy("zh-CN"), "join")).toBe("Esc：稍后提醒。Enter：加入。D：关闭。按空格键标记你已在会上。");
    expect(hintText(createCopy("ja"), "open_event")).toBe(
      "Esc：再通知。Enter：予定を開く。D：閉じる。スペースキーで参加済みにします。",
    );
    expect(hintText(createCopy("de"), "join")).toBe(
      "Esc: schlummern. Enter: teilnehmen. D: verwerfen. Leertaste markiert dich als schon dabei.",
    );
    for (const locale of LOCALES) {
      for (const primary of ["join", "open_event", "already_there"] as const) {
        const text = hintText(createCopy(locale), primary);
        expect(text).not.toContain("。 ");
        expect(text).not.toMatch(/\s{2}/);
      }
    }
  });
});

describe("join host (decisions.md, join links are untrusted)", () => {
  it("shows the host only when the link is not recognised", () => {
    expect(joinHost(link("example.com", false))).toBe("example.com");
    expect(joinHost(link("meet.google.com", true))).toBeNull();
    expect(joinHost({ kind: "event_page" })).toBeNull();
    expect(joinHost({ kind: "none" })).toBeNull();
  });

  it("puts the host on the join button, punycode as xn--, never a decoded name", () => {
    expect(primaryLabel(en, meeting(link("example.com", false)))).toBe("Join on example.com");
    expect(primaryLabel(en, meeting(link("xn--80ak6aa92e.com", false)))).toBe("Join on xn--80ak6aa92e.com");
    expect(primaryLabel(en, meeting(link("meet.google.com", true)))).not.toContain("meet.google.com");
  });

  it.each(LOCALES)("%s names the host in the button label", (locale) => {
    expect(primaryLabel(createCopy(locale), meeting(link("example.com", false)))).toContain("example.com");
  });

  // D3: the page shortens a long host with an ellipsis at its start, so the end that names the site stays.
  it.each(LOCALES)("%s splits the label around the host and nothing else (D3)", (locale) => {
    const c = createCopy(locale);
    const m = meeting(link("video.conferencing.example.com", false));
    const [before, host, after] = primaryParts(c, m);
    expect(host).toBe("video.conferencing.example.com");
    expect(before + host + after).toBe(primaryLabel(c, m));
    expect(primaryParts(c, meeting(link("meet.google.com", true)))).toEqual([c.t.interrupt.joinCall, "", ""]);
  });
});

describe("details line", () => {
  it("names a known provider by its host, with the time range and the people", () => {
    expect(providerOf("meet.google.com")).toBe("Google Meet");
    expect(providerOf("us02web.zoom.us")).toBe("Zoom");
    expect(providerOf("evilzoom.us")).toBeNull();
    expect(providerOf("zoom.us.evil.example")).toBeNull();
    const text = detailsText(createCopy("en-GB"), meeting(link("meet.google.com", true)));
    expect(text).toMatch(/^\d{2}:\d{2} to \d{2}:\d{2} on Google Meet with 8 people\.$/);
  });

  it("names an unrecognised host in full, as the button may cut it, and drops 'with 0 people'", () => {
    const host = "video.conferencing.example.com";
    const text = detailsText(createCopy("en-GB"), meeting(link(host, false), { people: 0 }));
    expect(text).toMatch(/^\d{2}:\d{2} to \d{2}:\d{2} on video\.conferencing\.example\.com\.$/);
    for (const locale of LOCALES) expect(detailsText(createCopy(locale), meeting(link(host, false))), locale).toContain(host);
  });

  it("uses the singular for one person", () => {
    expect(detailsText(en, meeting({ kind: "none" }, { people: 1 }))).toContain("with 1 person.");
  });
});

describe("keys (S4.3, AC4.4)", () => {
  it("S4.3 maps Enter, Esc, D and Space", () => {
    expect(keyAction(press("Enter"))).toBe("enter");
    expect(keyAction(press("Escape"))).toBe("snooze");
    expect(keyAction(press("d"))).toBe("dismiss");
    expect(keyAction(press("D"))).toBe("dismiss");
    expect(keyAction(press(" "))).toBe("already_there");
  });

  it("AC4.4 D dismisses on any layout", () => {
    expect(keyAction(press("ي", { code: "KeyD" }))).toBe("dismiss"); // Arabic
    expect(keyAction(press("ג", { code: "KeyD" }))).toBe("dismiss"); // Hebrew
    expect(keyAction(press("्", { code: "KeyD" }))).toBe("dismiss"); // Hindi InScript
    expect(keyAction(press("в", { code: "KeyD" }))).toBe("dismiss"); // Russian
    expect(keyAction(press("d", { code: "KeyD" }))).toBe("dismiss");
    // Dvorak: KeyD types "e", which must not dismiss; its D character (on KeyH) does.
    expect(keyAction(press("e", { code: "KeyD" }))).toBeNull();
    expect(keyAction(press("d", { code: "KeyH" }))).toBe("dismiss");
    expect(keyAction(press("ي", { code: "KeyS" }))).toBeNull();
    expect(keyAction(press("ي", { code: "KeyD", ctrlKey: true }))).toBeNull();
    expect(keyAction(press("ي", { code: "KeyD", repeat: true }))).toBeNull();
  });

  it("AC4.4 ignores other keys, shortcuts, auto-repeat and IME input", () => {
    expect(keyAction(press("x"))).toBeNull();
    expect(keyAction(press("Tab"))).toBeNull();
    expect(keyAction(press("d", { metaKey: true }))).toBeNull();
    expect(keyAction(press("d", { ctrlKey: true }))).toBeNull();
    expect(keyAction(press("Enter", { altKey: true }))).toBeNull();
    expect(keyAction(press("Enter", { repeat: true }))).toBeNull();
    expect(keyAction(press("d", { isComposing: true }))).toBeNull();
  });

  it("AC4.4 the focus trap wraps in both directions", () => {
    expect(nextFocusIndex(0, 3, false)).toBe(1);
    expect(nextFocusIndex(2, 3, false)).toBe(0);
    expect(nextFocusIndex(0, 3, true)).toBe(2);
    expect(nextFocusIndex(-1, 3, false)).toBe(0);
    expect(nextFocusIndex(-1, 3, true)).toBe(2);
    expect(nextFocusIndex(0, 0, false)).toBe(-1);
  });
});

describe("Space (AC4.4)", () => {
  it("AC4.4 means 'I am already there' from a primary button or with nothing focused", () => {
    for (const focused of ["join", "open_event", "already_there", null] as const) {
      expect(handlesKey("already_there", focused)).toBe(true);
    }
  });

  it("AC4.4 on Snooze or Dismiss it is left to the button, so it never marks a meeting joined", () => {
    expect(handlesKey("already_there", "snooze")).toBe(false);
    expect(handlesKey("already_there", "dismiss")).toBe(false);
  });

  it("AC4.4 Enter, Esc and D stay the page's own wherever the focus is", () => {
    for (const wanted of ["enter", "snooze", "dismiss"] as const) {
      for (const focused of ["join", "snooze", "dismiss", null] as const) {
        expect(handlesKey(wanted, focused)).toBe(true);
      }
    }
  });
});

describe("commands (AC4.6, S3.6)", () => {
  afterEach(() => vi.clearAllMocks());

  it("AC4.6 each action calls its command with the meeting key", async () => {
    await perform("snooze", "k1");
    await perform("dismiss", "k1");
    await perform("join", "k2");
    await perform("open_event", "k2");
    await perform("already_there", "k3");
    expect(api.alarmsSnooze).toHaveBeenCalledWith("k1", 5);
    expect(api.alarmsDismiss).toHaveBeenCalledWith("k1");
    expect(api.alarmsJoin).toHaveBeenCalledTimes(2);
    expect(api.alarmsJoin).toHaveBeenCalledWith("k2");
    expect(api.alarmsAlreadyThere).toHaveBeenCalledWith("k3");
  });
});

describe("long titles", () => {
  it("steps the display-xl title down so it still fits", () => {
    expect(titleSize("Platform weekly sync")).toBe("xl");
    expect(titleSize("x".repeat(37))).toBe("long");
    expect(titleSize("x".repeat(91))).toBe("longer");
  });
});

describe("a reused window (L-048)", () => {
  const show = (title: string, key = "cal:a@2026-10-20T14:00:00Z") => ({
    test: false,
    meetings: [meeting(link("meet.google.com", true), { title, key })],
  });

  it("an alarm:show while one is on screen keeps the view, so the focus stays where the person put it", () => {
    const first = show("First", "cal:a@1");
    const view = nextAlarmView(NO_ALARM, { type: "show", alarm: first });
    const regrouped = { test: false, meetings: [...first.meetings, ...show("Second", "cal:b@1").meetings] };
    const updated = nextAlarmView(view, { type: "show", alarm: regrouped });
    // Same count: the page keys its view by it, so it does not remount or move the focus to the primary button.
    expect(updated.shown).toBe(view.shown);
    expect(updated.alarm?.meetings.map((m) => m.title)).toEqual(["First", "Second"]);
    // The stale-meeting drop is the same event with fewer meetings.
    expect(nextAlarmView(updated, { type: "show", alarm: show("Second", "cal:b@1") }).shown).toBe(view.shown);
  });

  it("a hide then a show is a fresh view, even a repeat of the same meeting", () => {
    let view = nextAlarmView(NO_ALARM, { type: "show", alarm: show("First") });
    const first = view.shown;
    view = nextAlarmView(view, { type: "hide" });
    expect(view.alarm).toBeNull();
    view = nextAlarmView(view, { type: "show", alarm: show("First") });
    expect(view.alarm?.meetings[0]?.title).toBe("First");
    // The page keys its view by `shown`: a new number remounts it, which drops focus, error, busy and key handlers.
    expect(view.shown).toBeGreaterThan(first);
  });

  it("the second alarm shows its own data and keeps none of the first", () => {
    let view = nextAlarmView(NO_ALARM, { type: "show", alarm: show("First", "cal:a@1") });
    view = nextAlarmView(view, { type: "hide" });
    view = nextAlarmView(view, { type: "show", alarm: show("Second", "cal:b@2") });
    expect(view.alarm?.meetings.map((m) => m.title)).toEqual(["Second"]);
  });

  it("an alarm:show that arrives while the page loads beats the pulled answer", () => {
    const early = nextAlarmView(NO_ALARM, { type: "show", alarm: show("Early") });
    expect(nextAlarmView(early, { type: "current", alarm: show("Stale") })).toBe(early);
    expect(nextAlarmView(NO_ALARM, { type: "current", alarm: null })).toBe(NO_ALARM);
    expect(nextAlarmView(NO_ALARM, { type: "current", alarm: show("Pulled") }).shown).toBe(1);
  });

  it("the page keys its view by the alarm count and keeps listening after a hide", () => {
    expect(page).toContain("<InterruptView key={shown}");
    expect(page).not.toContain("key={first.key}");
    // `alarm:hide` only clears the alarm: the window outlives it and listens for the next one.
    expect(page).toContain('onAlarmHide(() => dispatch({ type: "hide" }))');
  });
});

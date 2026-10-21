import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "./api";
import { createCopy } from "./copy";
import { LOCALES, RTL_LOCALES } from "./i18n/locale";
import { SNOOZE_MINUTES } from "./interrupt";
import { buildNotifyCopy, NOTIFY_MANY_MAX, syncNotifyCopy } from "./notify-copy";

vi.mock("./api", async (original) => ({
  ...(await original<typeof import("./api")>()),
  api: { notifyCopySet: vi.fn(async () => undefined), settingsGet: vi.fn() },
  onSettingsChanged: vi.fn(async () => () => undefined),
}));
vi.mock("./theme", () => ({ applyAppearance: vi.fn() }));

beforeEach(() => vi.clearAllMocks());

/** The limits `NotifyCopy::from_page` enforces in Rust. */
const MAX_CHARS = 200;
const MAX_TIME_CHARS = 32;
// Control characters and Unicode format characters (Cf), except the isolates U+2068 and U+2069.
const CONTROL = /\p{Cc}/u;
const FORMAT = /(?![⁨⁩])\p{Cf}/u;

describe("the notification bundle (L-038)", () => {
  for (const locale of LOCALES) {
    describe(locale, () => {
      const c = createCopy(locale);
      const bundle = buildNotifyCopy(c);

      it("carries its locale and the sentences of its catalogue", () => {
        expect(bundle.locale).toBe(locale);
        expect(bundle.test_title).toBe(c.t.interrupt.testTitle);
        expect(bundle.join).toBe(c.t.interrupt.joinCall);
        expect(bundle.snooze).toBe(c.t.interrupt.snooze(SNOOZE_MINUTES));
        expect(bundle.many_over).toBe(c.t.notification.manyOver);
        expect(bundle.untitled).toBe(c.t.notification.untitled);
      });

      it("has the two placeholders exactly where Rust expects them", () => {
        expect(bundle.body.match(/\{time\}/g)).toHaveLength(1);
        expect(bundle.body.replaceAll("{time}", "")).not.toMatch(/[{}]/);
        expect(bundle.one_title.match(/\{title\}/g)).toHaveLength(1);
        expect(bundle.one_title.replaceAll("{title}", "")).not.toMatch(/[{}]/);
        for (const text of [bundle.test_title, bundle.many_over, bundle.snooze, bundle.join, bundle.untitled, ...bundle.many_titles]) {
          expect(text).not.toMatch(/[{}]/);
        }
      });

      it("isolates the meeting title only for a right-to-left language", () => {
        expect(bundle.one_title).toBe(RTL_LOCALES.has(locale) ? "⁨{title}⁩" : "{title}");
      });

      it("writes a title for 2 to the maximum meetings, each with its own count", () => {
        expect(bundle.many_titles).toHaveLength(NOTIFY_MANY_MAX - 1);
        expect(new Set(bundle.many_titles).size).toBeGreaterThan(1);
        const digits = new Intl.NumberFormat(locale);
        for (const n of [3, 7, NOTIFY_MANY_MAX]) {
          // Plural forms with a number in the word (Arabic 2, Hebrew 2) are exempt; the others show the count.
          expect(bundle.many_titles[n - 2]).toContain(digits.format(n));
        }
      });

      it("lists the 1440 minutes of the day as the locale's clock writes them", () => {
        expect(bundle.times).toHaveLength(1440);
        for (const [h, m] of [
          [0, 0],
          [0, 59],
          [9, 5],
          [12, 0],
          [14, 5],
          [23, 59],
        ]) {
          // The same Date in UTC: the table is the wall clock, not an instant in the machine's zone.
          const at = Date.UTC(2026, 0, 1, h, m);
          const expected = new Intl.DateTimeFormat(locale, {
            hour: new Intl.DateTimeFormat(locale, { hour: "numeric" }).resolvedOptions().hour12 ? "numeric" : "2-digit",
            minute: "2-digit",
            timeZone: "UTC",
          }).format(at);
          expect(bundle.times[h * 60 + m]).toBe(expected);
        }
        expect(new Set(bundle.times).size).toBe(1440);
      });

      it("stays inside the bounds Rust checks and has no control or format characters", () => {
        const sentences = [
          bundle.test_title,
          bundle.one_title,
          bundle.many_over,
          bundle.body,
          bundle.snooze,
          bundle.join,
          bundle.untitled,
          ...bundle.many_titles,
        ];
        for (const text of sentences) {
          expect(text.trim()).not.toBe("");
          expect([...text].length).toBeLessThanOrEqual(MAX_CHARS);
          expect(text).not.toMatch(CONTROL);
          expect(text).not.toMatch(FORMAT);
        }
        for (const time of bundle.times) {
          expect(time.trim()).not.toBe("");
          expect([...time].length).toBeLessThanOrEqual(MAX_TIME_CHARS);
          expect(time).not.toMatch(CONTROL);
          // ICU writes the Arabic and Hebrew clock with direction marks in some versions; Rust drops them.
          expect(time.replace(/\p{Cf}/gu, "")).not.toMatch(FORMAT);
        }
      });

      it("fills into a sentence that still reads: the time lands in the body", () => {
        expect(bundle.body.replace("{time}", bundle.times[14 * 60 + 5])).toContain(bundle.times[14 * 60 + 5]);
      });
    });
  }

  it("pins the en-US bundle to the built-in English in Rust (notify_copy.rs, `english`)", () => {
    const b = buildNotifyCopy(createCopy("en-US"));
    expect(b.test_title).toBe("Test alarm");
    expect(b.one_title).toBe("{title}");
    expect(b.many_titles[0]).toBe("2 meetings are about to start");
    expect(b.many_titles[NOTIFY_MANY_MAX - 2]).toBe(`${NOTIFY_MANY_MAX} meetings are about to start`);
    expect(b.many_over).toBe("Several meetings are about to start");
    expect(b.body).toBe("Starts at {time}. Tolon could not cover the screen.");
    expect(b.snooze).toBe("Snooze 5 min");
    expect(b.join).toBe("Join the call");
    expect(b.untitled).toBe("Untitled meeting");
    expect(b.times[14 * 60 + 5].replace(/\s/g, " ")).toBe("2:05 PM");
  });

  it("writes the 12-hour and 24-hour clocks of the locales as they are used", () => {
    const at = (locale: Parameters<typeof createCopy>[0], minute: number) => buildNotifyCopy(createCopy(locale)).times[minute];
    expect(at("de", 14 * 60 + 5)).toBe("14:05");
    expect(at("pt-BR", 9 * 60 + 5)).toBe("09:05");
    expect(at("en-GB", 0)).toBe("00:00");
    expect(at("en-US", 9 * 60 + 5).replace(/\s/g, " ")).toBe("9:05 AM");
    expect(at("ja", 14 * 60 + 5)).toBe("14:05");
    // The 12-hour locales (decision "Clock, digits and keys across locales") write their own day period.
    expect(at("hi", 14 * 60 + 5)).toMatch(/pm$/i);
    expect(at("ar", 14 * 60 + 5)).toMatch(/م$/);
  });

  it("counts meetings with the locale's plural forms in Arabic and Hebrew", () => {
    expect(buildNotifyCopy(createCopy("ar")).many_titles[0]).toBe("اجتماعان على وشك البدء");
    expect(buildNotifyCopy(createCopy("he")).many_titles[0]).toBe("שתי פגישות עומדות להתחיל");
    expect(buildNotifyCopy(createCopy("ar")).many_titles[1]).toMatch(/اجتماعات/);
  });

  it("sends the bundle for the active language with notify_copy_set", () => {
    syncNotifyCopy(createCopy("pt-PT"));
    expect(api.notifyCopySet).toHaveBeenCalledTimes(1);
    expect(vi.mocked(api.notifyCopySet).mock.calls[0][0]).toEqual(buildNotifyCopy(createCopy("pt-PT")));
  });

  it("does not throw when the command is refused or there is no IPC", async () => {
    vi.mocked(api.notifyCopySet).mockRejectedValueOnce({ code: "not_found" });
    expect(() => syncNotifyCopy(createCopy("de"))).not.toThrow();
    await Promise.resolve();
  });
});

describe("the feed hands Rust the bundle", () => {
  async function feed() {
    vi.resetModules();
    const store = await import("./store");
    const apiNow = (await import("./api")).api;
    const base = {
      lead_minutes: 2,
      again_at_start: true,
      sound: "bell",
      sound_repeat: true,
      quiet: { enabled: false, start: "18:00", end: "09:00", weekends: true },
      ignore: { all_day: true, declined: true, cancelled: true, no_attendees: false, keywords: [] },
      appearance: "auto",
      interrupt_color: "theme",
      locale: null,
      start_at_login: false,
      start_at_login_status: null,
    };
    return { store, apiNow, base };
  }

  it("sends once the settings are loaded, and again when the language setting changes", async () => {
    const { store, apiNow, base } = await feed();
    vi.mocked(apiNow.settingsGet).mockResolvedValueOnce(base as never);
    await store.startSettingsFeed("de", true);
    expect(apiNow.notifyCopySet).toHaveBeenCalledTimes(1);
    expect(vi.mocked(apiNow.notifyCopySet).mock.calls[0][0].locale).toBe("de");

    vi.mocked(apiNow.settingsGet).mockResolvedValueOnce({ ...base, locale: "ja" } as never);
    await store.useSettings.getState().load();
    expect(vi.mocked(apiNow.notifyCopySet).mock.calls.at(-1)?.[0].locale).toBe("ja");
    vi.mocked(apiNow.settingsGet).mockResolvedValueOnce({ ...base, locale: "ja" } as never);
    const before = vi.mocked(apiNow.notifyCopySet).mock.calls.length;
    await store.useSettings.getState().load();
    expect(apiNow.notifyCopySet, "the same language is no change").toHaveBeenCalledTimes(before);
  });

  it("sends nothing from the interrupt and the panel windows, which may not call it", async () => {
    const { store, apiNow, base } = await feed();
    vi.mocked(apiNow.settingsGet).mockResolvedValueOnce({ ...base, locale: "es" } as never);
    await store.startSettingsFeed(null);
    expect(apiNow.notifyCopySet).not.toHaveBeenCalled();
  });
});

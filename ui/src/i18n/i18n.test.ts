import { describe, expect, it } from "vitest";
import { COMMAND_ERROR_CODES, type SyncError } from "../api-types";
import { createCopy } from "../copy";
import { enUS } from "./en-US";
import { catalogues, LOCALES, resolveLocale, type Locale } from "./locale";

const SYNC_KINDS = [
  "timeout",
  "connect",
  "not_calendar",
  "too_large",
  "network",
  "unsupported_scheme",
  "invalid_url",
] as const;

/** Dotted path of every leaf, with its type ("string" or "function"). */
function leaves(node: object, prefix = ""): [string, string][] {
  return Object.entries(node).flatMap(([key, value]) =>
    typeof value === "object" && value !== null
      ? leaves(value, `${prefix}${key}.`)
      : [[`${prefix}${key}`, typeof value] as [string, string]],
  );
}

const sourceLeaves = leaves(enUS);

/** The string leaves of a catalogue, with their dotted path. */
function strings(t: object): [string, string][] {
  return leaves(t)
    .filter(([, kind]) => kind === "string")
    .map(([path]) => [
      path,
      path.split(".").reduce<unknown>((node, key) => (node as Record<string, unknown>)[key], t) as string,
    ]);
}

/** Arabic-Indic digits to ASCII: older ICU writes ar numbers in Arabic-Indic, CLDR 46 and later in Latin digits. */
const latn = (text: string) => text.replace(/[\u0660-\u0669]/g, (d) => String(d.charCodeAt(0) - 0x660));

describe("catalogues", () => {
  it("ships en-US, en-GB, pt-BR, pt-PT and the beta 1 list (de, es, ar, tr, hi, zh-CN, ja, he)", () => {
    expect(LOCALES).toEqual(["en-US", "en-GB", "pt-BR", "pt-PT", "de", "es", "ar", "tr", "hi", "zh-CN", "ja", "he"]);
  });

  it.each(LOCALES)("%s has exactly the en-US key set, with the same string or function shape", (locale) => {
    expect(leaves(catalogues[locale])).toEqual(sourceLeaves);
  });

  it.each(LOCALES)("%s has every placeholder function and each returns text", (locale) => {
    const t = catalogues[locale];
    const results = [
      t.calendars.googleName("a@example.com"),
      t.calendars.toggleLabel("Work"),
      t.calendars.ignoreMatching("Focus, Lunch"),
      t.relative.syncedAgo("1 min"),
      t.relative.startsIn("2 min"),
      t.relative.startedAgo("3 min"),
      t.syncLog.skipped(2),
      t.interrupt.joinOn("example.com"),
      t.interrupt.snooze(5),
      t.interrupt.timeRange("14:00", "14:45"),
      t.interrupt.details("14:00 to 14:45", "Zoom", 3),
      t.errors.sync.http(500),
      t.errors.sync.httpHint(401, "x"),
    ];
    for (const text of results) expect(text.length).toBeGreaterThan(0);
    expect(t.calendars.googleName("a@example.com")).toContain("a@example.com");
    expect(t.errors.sync.http(503)).toContain("503");
  });

  it.each(LOCALES)("%s has no empty string and follows the writing rules", (locale) => {
    const t = catalogues[locale];
    const strings = leaves(t)
      .filter(([, kind]) => kind === "string")
      .map(([path]) => path.split(".").reduce<unknown>((node, key) => (node as Record<string, unknown>)[key], t) as string);
    for (const text of strings) {
      expect(text.trim()).not.toBe("");
      expect(text).not.toContain("!");
      expect(text.toLowerCase()).not.toContain("oops");
      expect(text).not.toMatch(/\bthe app\b/i);
    }
  });

  it.each(LOCALES)("%s never translates the name Tolon", (locale) => {
    const t = catalogues[locale];
    expect(t.app.windowTitle).toBe("Tolon");
    for (const text of [t.calendars.subtitle, t.calendars.removeConsequence, t.errors.command.internal]) {
      expect(text).toContain("Tolon");
    }
  });

  it.each(["zh-CN", "ja"] as const)("%s uses full-width punctuation", (locale) => {
    for (const [path, text] of strings(catalogues[locale])) {
      if (path === "calendars.urlPlaceholder") continue;
      expect(text, path).not.toMatch(/[,.:;?]/);
    }
  });

  it("uses the Arabic comma and question mark in ar", () => {
    const t = catalogues.ar;
    for (const [path, text] of strings(t)) {
      if (path === "calendars.urlPlaceholder") continue;
      expect(text, path).not.toMatch(/[,?]/);
    }
    expect(t.calendars.removeTitle("Work")).toMatch(/؟$/);
    expect(t.errors.sync.httpHint(401, "x")).toContain("، ");
  });

  it("keeps a Latin name in order inside an Arabic sentence", () => {
    expect(catalogues.ar.calendars.removeTitle("Team (EU)")).toBe("إزالة \u2068Team (EU)\u2069؟");
  });

  it("addresses the person as du in de and as tú in es", () => {
    expect(catalogues.de.calendars.subtitle).toContain("deine");
    expect(catalogues.de.errors.command.db_error).toContain("Versuch es");
    expect(catalogues.es.calendars.subtitle).toContain("tus");
    expect(catalogues.es.errors.command.db_error).toContain("Inténtalo");
  });

  it("keeps pt-BR and pt-PT as separate catalogues", () => {
    expect(catalogues["pt-PT"].nav.appearance).toBe("Aspeto");
    expect(catalogues["pt-BR"].nav.appearance).toBe("Aparência");
    expect(catalogues["pt-PT"].nav.label).toBe("Definições");
    expect(catalogues["pt-PT"].calendars.changesSave).toBe("As alterações são guardadas automaticamente.");
    expect(catalogues["pt-BR"].calendars.changesSave).toBe("As alterações são salvas automaticamente.");
    expect(catalogues["pt-BR"].nav.quietHours).toBe("Horário de silêncio");
    expect(catalogues["pt-PT"].nav.quietHours).toBe("Horário de silêncio");
  });

  it("keeps the mockup 03 sentence in en-US", () => {
    expect(catalogues["en-US"].calendars.changesSave).toBe("Changes save as you make them.");
    expect(catalogues["en-GB"].calendars.changesSave).toBe("Changes save as you make them.");
  });
});

describe("resolveLocale", () => {
  const cases: [string[], string | undefined, Locale][] = [
    [["en-US"], undefined, "en-US"],
    [["en-GB"], undefined, "en-GB"],
    [["pt-BR"], undefined, "pt-BR"],
    [["pt-PT"], undefined, "pt-PT"],
    [["pt"], undefined, "pt-BR"],
    [["pt-AO"], undefined, "pt-PT"],
    [["pt-MZ"], undefined, "pt-PT"],
    [["pt-CV"], undefined, "pt-PT"],
    [["pt-GW"], undefined, "pt-PT"],
    [["pt-ST"], undefined, "pt-PT"],
    [["pt-TL"], undefined, "pt-PT"],
    [["pt-MO"], undefined, "pt-PT"],
    [["pt-FR"], undefined, "pt-BR"],
    [["pt-Latn-PT"], undefined, "pt-PT"],
    [["pt_PT"], undefined, "pt-PT"],
    [["PT-pt"], undefined, "pt-PT"],
    [["en-IE"], undefined, "en-GB"],
    [["en-AU"], undefined, "en-GB"],
    [["en-NZ"], undefined, "en-GB"],
    [["en"], undefined, "en-US"],
    [["en-CA"], undefined, "en-US"],
    [["en-PH"], undefined, "en-US"],
    [["en-IN"], undefined, "en-GB"],
    [["en-ZA"], undefined, "en-GB"],
    [["en-SG"], undefined, "en-GB"],
    [["en-HK"], undefined, "en-GB"],
    [["en-001"], undefined, "en-GB"],
    // The beta 1 list: one catalogue per language, any region; every Chinese tag reads Simplified.
    [["de-DE"], undefined, "de"],
    [["de-AT"], undefined, "de"],
    [["de-CH"], undefined, "de"],
    [["de"], undefined, "de"],
    [["es-ES"], undefined, "es"],
    [["es-MX"], undefined, "es"],
    [["es-419"], undefined, "es"],
    [["es-ES", "fr-FR"], undefined, "es"],
    [["zh-CN"], undefined, "zh-CN"],
    [["zh-TW"], undefined, "zh-CN"],
    [["zh-HK"], undefined, "zh-CN"],
    [["zh-Hant-TW"], undefined, "zh-CN"],
    [["zh"], undefined, "zh-CN"],
    [["ar-EG"], undefined, "ar"],
    [["ar-SA"], undefined, "ar"],
    [["ar"], undefined, "ar"],
    [["hi-IN"], undefined, "hi"],
    [["hi"], undefined, "hi"],
    [["ja-JP"], undefined, "ja"],
    [["ja"], undefined, "ja"],
    [["tr-TR"], undefined, "tr"],
    [["tr-CY"], undefined, "tr"],
    [["he-IL"], undefined, "he"],
    [["he"], undefined, "he"],
    [["iw"], undefined, "he"],
    [["iw-IL"], undefined, "he"],
    [["fr-FR"], undefined, "en-US"],
    [["ru-RU", "ko-KR"], undefined, "en-US"],
    [[], undefined, "en-US"],
    [["not a tag"], undefined, "en-US"],
    // Preference order: an unshipped language is skipped, the next shipped one wins.
    [["fr-FR", "pt-PT"], undefined, "pt-PT"],
    [["fr-FR", "ja-JP", "de-DE"], undefined, "ja"],
    [["pt-PT", "en-GB"], undefined, "pt-PT"],
    // The override wins, and a bad override falls back to the system list.
    [["pt-BR"], "pt-PT", "pt-PT"],
    [["en-US"], "en-AU", "en-GB"],
    [["pt-BR"], "xx-YY", "pt-BR"],
    [["pt-BR"], "", "pt-BR"],
    [["fr-FR"], null as unknown as undefined, "en-US"],
    [["en-US"], "ar-EG", "ar"],
    [["ar-EG"], "de-AT", "de"],
  ];
  it.each(cases)("%j with override %j gives %s", (languages, override, expected) => {
    expect(resolveLocale(languages, override)).toBe(expected);
  });
});

describe("plurals", () => {
  const expected: Record<Locale, [string, string]> = {
    "en-US": ["1 component skipped", "2 components skipped"],
    "en-GB": ["1 component skipped", "2 components skipped"],
    "pt-BR": ["1 componente ignorado", "2 componentes ignorados"],
    "pt-PT": ["1 componente ignorado", "2 componentes ignorados"],
    de: ["1 Komponente übersprungen", "2 Komponenten übersprungen"],
    es: ["1 componente omitido", "2 componentes omitidos"],
    ar: ["تم تخطي مكوّن واحد", "تم تخطي مكوّنين"],
    tr: ["1 bileşen atlandı", "2 bileşen atlandı"],
    hi: ["1 घटक छोड़ा गया", "2 घटक छोड़े गए"],
    "zh-CN": ["已跳过 1 个组件", "已跳过 2 个组件"],
    ja: ["1 件のコンポーネントをスキップしました", "2 件のコンポーネントをスキップしました"],
    he: ["רכיב אחד דולג", "שני רכיבים דולגו"],
  };
  it.each(LOCALES)("%s renders skipped:1 and skipped:2", (locale) => {
    const c = createCopy(locale);
    expect(c.syncLogDetail("skipped:1")).toBe(expected[locale][0]);
    expect(c.syncLogDetail("skipped:2")).toBe(expected[locale][1]);
  });

  it("uses all six Arabic plural forms (zero, one, two, few, many, other)", () => {
    const c = createCopy("ar");
    const skipped = (n: number) => latn(c.syncLogDetail(`skipped:${n}`) ?? "");
    expect(skipped(0)).toBe("لم يتم تخطي أي مكوّن");
    expect(skipped(1)).toBe("تم تخطي مكوّن واحد");
    expect(skipped(2)).toBe("تم تخطي مكوّنين");
    expect(skipped(3)).toBe("تم تخطي 3 مكوّنات");
    expect(skipped(10)).toBe("تم تخطي 10 مكوّنات");
    expect(skipped(11)).toBe("تم تخطي 11 مكوّنًا");
    expect(skipped(99)).toBe("تم تخطي 99 مكوّنًا");
    expect(skipped(100)).toBe("تم تخطي 100 مكوّن");
    const people = (n: number) => latn(c.t.interrupt.details("R", null, n));
    expect(people(1)).toBe("R مع شخص واحد.");
    expect(people(2)).toBe("R مع شخصين.");
    expect(people(5)).toBe("R مع 5 أشخاص.");
    expect(people(12)).toBe("R مع 12 شخصًا.");
    expect(people(100)).toBe("R مع 100 شخص.");
    expect(latn(c.t.interrupt.snooze(5))).toBe("تأجيل 5 دقائق");
    expect(c.t.interrupt.snooze(1)).toBe("تأجيل دقيقة واحدة");
    expect(c.t.interrupt.snooze(2)).toBe("تأجيل دقيقتين");
  });

  it("uses the three Hebrew plural forms (one, two, other)", () => {
    const c = createCopy("he");
    expect(c.syncLogDetail("skipped:1")).toBe("רכיב אחד דולג");
    expect(c.syncLogDetail("skipped:2")).toBe("שני רכיבים דולגו");
    expect(c.syncLogDetail("skipped:3")).toBe("3 רכיבים דולגו");
    expect(c.syncLogDetail("skipped:0")).toBe("0 רכיבים דולגו");
    expect(c.syncLogDetail("skipped:20")).toBe("20 רכיבים דולגו");
    expect(c.t.interrupt.details("R", null, 1)).toBe("R, עם אדם אחד.");
    expect(c.t.interrupt.details("R", null, 2)).toBe("R, עם שני אנשים.");
    expect(c.t.interrupt.details("R", null, 8)).toBe("R, עם 8 אנשים.");
    expect(c.t.interrupt.snooze(1)).toBe("דחייה בדקה אחת");
    expect(c.t.interrupt.snooze(2)).toBe("דחייה בשתי דקות");
    expect(c.t.interrupt.snooze(5)).toBe("דחייה ב-5 דקות");
  });

  it("keeps Hebrew instructions gender neutral: action nouns and אפשר with an infinitive", () => {
    const t = catalogues.he;
    // Gendered imperatives a translator reaches for first: נסה/נסי, הדבק/הדביקי, הוסף/הוסיפי.
    for (const [path, text] of strings(t)) expect(text, path).not.toMatch(/(^|\s)(נסה|נסי|הדבק|הדביקי|הוסף|הוסיפי)(\s|$|\.)/);
    expect(t.errors.command.db_error).toContain("אפשר לנסות שוב");
    expect(t.calendars.addCalendar).toBe("הוספת יומן");
  });

  it("follows the Hindi plural rule, where 0 and 1 are both singular", () => {
    const c = createCopy("hi");
    expect(c.syncLogDetail("skipped:0")).toBe("0 घटक छोड़ा गया");
    expect(c.syncLogDetail("skipped:1")).toBe("1 घटक छोड़ा गया");
    expect(c.syncLogDetail("skipped:5")).toBe("5 घटक छोड़े गए");
    expect(c.t.interrupt.details("R", "Zoom", 1)).toBe("R, Zoom पर, 1 व्यक्ति के साथ।");
    expect(c.t.interrupt.details("R", null, 8)).toBe("R, 8 लोगों के साथ।");
  });

  it("returns null for details that are not coded copy", () => {
    expect(createCopy("en-US").syncLogDetail("ok")).toBeNull();
    expect(createCopy("en-US").syncLogDetail("skipped:x")).toBeNull();
  });
});

describe("error codes", () => {
  const syncErrors: SyncError[] = [
    ...SYNC_KINDS.map((kind) => ({ kind })),
    { kind: "http", status: 401 },
    { kind: "http", status: 403 },
    { kind: "http", status: 404 },
    { kind: "http", status: 429 },
    { kind: "http", status: 503 },
    { kind: "http", status: 418 },
  ];

  it.each(LOCALES)("%s maps every sync error to text", (locale) => {
    const c = createCopy(locale);
    for (const error of syncErrors) expect(c.syncError(error).trim()).not.toBe("");
  });

  it.each(LOCALES)("%s maps every command error code to its own text", (locale) => {
    const c = createCopy(locale);
    const seen = new Set<string>();
    for (const code of COMMAND_ERROR_CODES) {
      const text = c.commandError({ code });
      expect(text.trim()).not.toBe("");
      seen.add(text);
    }
    expect(seen.size).toBe(COMMAND_ERROR_CODES.length);
  });

  it("matches the mockup for an expired link", () => {
    expect(createCopy("en-US").syncError({ kind: "http", status: 401 })).toBe("Sync failed: 401, link expired");
  });

  it("states the bare status when there is no hint", () => {
    expect(createCopy("en-US").syncError({ kind: "http", status: 418 })).toBe("Sync failed: 418");
  });

  it("pins the S1.3 and S2.3 sentences verbatim in en-US", () => {
    const c = createCopy("en-US");
    expect(c.syncError({ kind: "http", status: 404 })).toBe("Sync failed: 404");
    expect(c.syncLogDetail("skipped:1")).toBe("1 component skipped");
  });

  const s13: Record<Locale, string> = {
    "en-US": "Sync failed: 404",
    "en-GB": "Sync failed: 404",
    "pt-BR": "Falha na sincronização: 404",
    "pt-PT": "A sincronização falhou: 404",
    de: "Synchronisierung fehlgeschlagen: 404",
    es: "Falló la sincronización: 404",
    ar: "فشلت المزامنة: 404",
    tr: "Eşitleme başarısız: 404",
    hi: "सिंक विफल: 404",
    "zh-CN": "同步失败：404",
    ja: "同期に失敗しました：404",
    he: "הסנכרון נכשל: 404",
  };
  it.each(LOCALES)("%s pins the S1.3 sentence for http:404", (locale) => {
    expect(createCopy(locale).syncError({ kind: "http", status: 404 })).toBe(s13[locale]);
  });

  it.each(LOCALES)("%s gives http:404 no hint", (locale) => {
    const c = createCopy(locale);
    expect(c.syncError({ kind: "http", status: 404 })).toBe(c.t.errors.sync.http(404));
  });

  it("never renders undefined for an unknown kind or an http error without a status", () => {
    for (const locale of LOCALES) {
      const c = createCopy(locale);
      const unknownKind = c.syncError({ kind: "from_the_future" } as unknown as SyncError);
      const noStatus = c.syncError({ kind: "http" } as unknown as SyncError);
      const badStatus = c.syncError({ kind: "http", status: "404" } as unknown as SyncError);
      for (const text of [unknownKind, noStatus, badStatus]) {
        expect(text).toBe(c.t.errors.sync.unknown);
        expect(text).not.toContain("undefined");
      }
      // Keys of the catalogue that are not error kinds must not leak through.
      expect(c.syncError({ kind: "httpHint" } as unknown as SyncError)).toBe(c.t.errors.sync.unknown);
    }
  });

  it("uses the pt-PT hint 'endereço expirado' for 401", () => {
    expect(createCopy("pt-PT").syncError({ kind: "http", status: 401 })).toBe(
      "A sincronização falhou: 401, endereço expirado",
    );
  });

  it("falls back to the internal error for a code from a newer core", () => {
    const c = createCopy("pt-BR");
    expect(c.commandError({ code: "brand_new" as never })).toBe(c.t.errors.command.internal);
  });
});

describe("formatting", () => {
  const afternoon = new Date(2026, 0, 1, 14, 5);
  const morning = new Date(2026, 0, 1, 9, 5);

  it("formats time in the locale's style", () => {
    expect(createCopy("en-US").formatTime(afternoon).replace(/\s/g, " ")).toBe("2:05 PM");
    expect(createCopy("en-US").formatTime(morning).replace(/\s/g, " ")).toBe("9:05 AM");
    expect(createCopy("en-GB").formatTime(afternoon)).toBe("14:05");
    expect(createCopy("pt-BR").formatTime(morning)).toBe("09:05");
    expect(createCopy("pt-PT").formatTime(morning)).toBe("09:05");
    expect(createCopy("pt-PT").formatTime(afternoon)).toBe("14:05");
  });

  const now = new Date(2026, 0, 1, 12, 0, 0);
  const at = (seconds: number) => new Date(now.getTime() + seconds * 1000);

  it("writes relative copy from the mockup in en-US", () => {
    const c = createCopy("en-US");
    expect(c.syncedAgo(at(-60), now)).toBe("Synced 1 min ago");
    expect(c.startsIn(at(120), now)).toBe("Starts in 2 min");
    expect(c.startedAgo(at(-180), now)).toBe("Started 3 min ago");
  });

  it("rounds down for the past and up for the future, with just-now edges", () => {
    const c = createCopy("en-US");
    expect(c.syncedAgo(at(-119), now)).toBe("Synced 1 min ago");
    expect(c.syncedAgo(at(-30), now)).toBe("Synced just now");
    expect(c.syncedAgo(at(30), now)).toBe("Synced just now");
    expect(c.startsIn(at(61), now)).toBe("Starts in 2 min");
    expect(c.startsIn(at(0), now)).toBe("Starts now");
    expect(c.startedAgo(at(-10), now)).toBe("Started just now");
  });

  it("moves to hours and days and lets Intl pick the plural", () => {
    const c = createCopy("en-US");
    expect(c.syncedAgo(at(-3 * 3600), now)).toBe("Synced 3 hr ago");
    expect(c.syncedAgo(at(-24 * 3600), now)).toBe("Synced 1 day ago");
    expect(c.syncedAgo(at(-2 * 24 * 3600), now)).toBe("Synced 2 days ago");
  });

  it("writes relative copy in pt-BR and pt-PT", () => {
    expect(createCopy("pt-BR").syncedAgo(at(-60), now)).toBe("Sincronizado há 1 min");
    expect(createCopy("pt-BR").startsIn(at(120), now)).toBe("Começa em 2 min");
    expect(createCopy("pt-PT").startsIn(at(120), now)).toBe("Começa dentro de 2 min");
    expect(createCopy("pt-PT").startedAgo(at(-180), now)).toBe("Começou há 3 min");
  });

  it("formats time in the new locales: 24-hour with two digits, 12-hour without the leading zero", () => {
    for (const locale of ["de", "es", "tr", "zh-CN", "ja", "he"] as const) {
      expect(createCopy(locale).formatTime(morning)).toBe("09:05");
      expect(createCopy(locale).formatTime(afternoon)).toBe("14:05");
    }
    // Intl's own digits: Latin in current engines, Arabic-Indic in older ICU for ar.
    expect(latn(createCopy("ar").formatTime(afternoon)).replace(/\s/g, " ")).toBe("2:05 م");
    expect(latn(createCopy("ar").formatTime(morning)).replace(/\s/g, " ")).toBe("9:05 ص");
    expect(createCopy("hi").formatTime(afternoon).replace(/\s/g, " ")).toBe("2:05 pm");
  });

  it("writes relative copy in every new locale", () => {
    const pinned: [Locale, string, string, string][] = [
      ["de", "Vor 1 Min. synchronisiert", "Beginnt in 2 Min.", "Hat vor 3 Min. begonnen"],
      ["es", "Sincronizado hace 1 min", "Empieza en 2 min", "Empezó hace 3 min"],
      ["tr", "1 dk. önce eşitlendi", "2 dk. içinde başlıyor", "3 dk. önce başladı"],
      ["zh-CN", "1分钟前同步", "2分钟后开始", "已开始3分钟"],
      ["ja", "1 分前に同期しました", "2 分後に開始", "3 分前に開始しました"],
    ];
    for (const [locale, synced, starts, started] of pinned) {
      const c = createCopy(locale);
      expect(c.syncedAgo(at(-60), now).replace(/\s/g, " ")).toBe(synced);
      expect(c.startsIn(at(120), now).replace(/\s/g, " ")).toBe(starts);
      expect(c.startedAgo(at(-180), now).replace(/\s/g, " ")).toBe(started);
    }
    // Arabic, Hindi and Hebrew unit labels vary between CLDR versions: check the sentence around Intl's value.
    for (const locale of ["ar", "hi", "he"] as const) {
      const c = createCopy(locale);
      const span = (minutes: number) =>
        new Intl.NumberFormat(locale, { style: "unit", unit: "minute", unitDisplay: "short" }).format(minutes);
      expect(c.syncedAgo(at(-60), now)).toBe(c.t.relative.syncedAgo(span(1)));
      expect(c.startsIn(at(120), now)).toBe(c.t.relative.startsIn(span(2)));
      expect(c.startedAgo(at(-180), now)).toBe(c.t.relative.startedAgo(span(3)));
      expect(c.syncedAgo(at(-30), now)).toBe(c.t.relative.syncedJustNow);
    }
    expect(createCopy("ar").syncedAgo(at(-2 * 24 * 3600), now)).toContain("يوم");
  });

  it("joins the ignore keywords in the locale's list style", () => {
    expect(createCopy("en-US").list(["Focus", "Lunch", "OOO"])).toBe("Focus, Lunch, OOO");
    expect(createCopy("pt-PT").list(["Focus", "Lunch", "OOO"])).toBe("Focus, Lunch e OOO");
  });
});

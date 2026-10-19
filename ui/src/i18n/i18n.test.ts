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

describe("catalogues", () => {
  it("ships en-US, en-GB, pt-BR and pt-PT", () => {
    expect(LOCALES).toEqual(["en-US", "en-GB", "pt-BR", "pt-PT"]);
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

  it("keeps pt-BR and pt-PT as separate catalogues", () => {
    expect(catalogues["pt-PT"].nav.appearance).toBe("Aspeto");
    expect(catalogues["pt-BR"].nav.appearance).toBe("Aparência");
    expect(catalogues["pt-PT"].nav.label).toBe("Definições");
  });
});

describe("resolveLocale", () => {
  const cases: [string[], string | undefined, Locale][] = [
    [["en-US"], undefined, "en-US"],
    [["en-GB"], undefined, "en-GB"],
    [["pt-BR"], undefined, "pt-BR"],
    [["pt-PT"], undefined, "pt-PT"],
    [["pt"], undefined, "pt-BR"],
    [["pt-AO"], undefined, "pt-BR"],
    [["pt-MZ"], undefined, "pt-BR"],
    [["pt-Latn-PT"], undefined, "pt-PT"],
    [["pt_PT"], undefined, "pt-PT"],
    [["PT-pt"], undefined, "pt-PT"],
    [["en-IE"], undefined, "en-GB"],
    [["en-AU"], undefined, "en-GB"],
    [["en-NZ"], undefined, "en-GB"],
    [["en"], undefined, "en-US"],
    [["en-CA"], undefined, "en-US"],
    [["de-DE"], undefined, "en-US"],
    [["es-ES", "fr-FR"], undefined, "en-US"],
    [[], undefined, "en-US"],
    [["not a tag"], undefined, "en-US"],
    // Preference order: an unshipped language is skipped, the next shipped one wins.
    [["de-DE", "pt-PT"], undefined, "pt-PT"],
    [["pt-PT", "en-GB"], undefined, "pt-PT"],
    // The override wins, and a bad override falls back to the system list.
    [["pt-BR"], "pt-PT", "pt-PT"],
    [["en-US"], "en-AU", "en-GB"],
    [["pt-BR"], "xx-YY", "pt-BR"],
    [["pt-BR"], "", "pt-BR"],
    [["de-DE"], null as unknown as undefined, "en-US"],
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
  };
  it.each(LOCALES)("%s renders skipped:1 and skipped:2", (locale) => {
    const c = createCopy(locale);
    expect(c.syncLogDetail("skipped:1")).toBe(expected[locale][0]);
    expect(c.syncLogDetail("skipped:2")).toBe(expected[locale][1]);
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
    expect(createCopy("en-GB").formatTime(afternoon)).toBe("14:05");
    expect(createCopy("pt-BR").formatTime(morning)).toBe("9:05");
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

  it("joins the ignore keywords in the locale's list style", () => {
    expect(createCopy("en-US").list(["Focus", "Lunch", "OOO"])).toBe("Focus, Lunch, OOO");
    expect(createCopy("pt-PT").list(["Focus", "Lunch", "OOO"])).toBe("Focus, Lunch e OOO");
  });
});

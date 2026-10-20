import { afterEach, describe, expect, it, vi } from "vitest";
import { PAGES } from "./App";
import { createCopy, LOCALES, NATIVE_NAMES } from "./copy";
import { applyAppearance } from "./theme";
import { addKeywords, MAX_KEYWORDS, parseLead, splitTyped } from "./view";

afterEach(() => vi.unstubAllGlobals());

describe("settings pages", () => {
  it("AC9.2 the nav has one page per topic", () => {
    expect(PAGES.map((p) => p.id)).toEqual([
      "calendars",
      "alarms",
      "quiet-hours",
      "rules",
      "appearance",
      "language",
      "startup",
      "about",
    ]);
  });

  it("AC9.5 accepts a lead time of 1 to 60 whole minutes only", () => {
    expect([1, 2, 60].map((n) => parseLead(String(n)))).toEqual([1, 2, 60]);
    expect(parseLead(" 15 ")).toBe(15);
    for (const raw of ["", "0", "61", "2.5", "-1", "1e1", "abc"]) expect(parseLead(raw), raw).toBeNull();
  });

  it("names every shipped language in its own words", () => {
    expect(Object.keys(NATIVE_NAMES).sort()).toEqual([...LOCALES].sort());
    expect(NATIVE_NAMES.ar).toBe("العربية");
    expect(NATIVE_NAMES.he).toBe("עברית");
  });

  it("renders a refused setting from its invalid:<field> code", () => {
    const en = createCopy("en-US");
    expect(en.commandError({ code: "invalid:lead_minutes" })).toBe("Choose a lead time from 1 to 60 minutes.");
    expect(en.commandError({ code: "invalid:quiet.start" })).toBe("Enter a time as hours and minutes.");
    expect(en.commandError({ code: "invalid:quiet_end" })).toBe("Enter a time as hours and minutes.");
    expect(en.commandError({ code: "invalid:ignore.keywords" })).toBe("Use at most 50 words of up to 64 characters each.");
    expect(en.commandError({ code: "invalid:start_at_login" })).toBe("Tolon could not save that value. Try another one.");
    expect(createCopy("de").commandError({ code: "invalid:lead_minutes" })).toBe("Wähle eine Vorlaufzeit von 1 bis 60 Minuten.");
  });
});

describe("ignore keywords (AC8.2)", () => {
  it("adds trimmed words, split on any comma, without blanks or case-insensitive repeats", () => {
    expect(addKeywords(["Focus"], " Lunch , ooo,, focus")).toEqual(["Focus", "Lunch", "ooo"]);
    expect(addKeywords([], "تركيز،غداء")).toEqual(["تركيز", "غداء"]);
    expect(addKeywords([], "午餐，专注、休息")).toEqual(["午餐", "专注", "休息"]);
  });

  it("stops at 50 words of up to 64 characters", () => {
    const full = Array.from({ length: MAX_KEYWORDS }, (_, i) => `w${i}`);
    expect(addKeywords(full, "one more")).toEqual(full);
    expect(addKeywords([], "x".repeat(80))[0]).toHaveLength(64);
  });

  it("keeps the text after the last comma in the box", () => {
    expect(splitTyped("Focus, Lun")).toEqual({ done: "Focus", rest: " Lun" });
    expect(splitTyped("Focus")).toEqual({ done: "", rest: "Focus" });
  });
});

describe("appearance (AC9.3)", () => {
  function stubSystem(dark: boolean) {
    const root = { dataset: {} as Record<string, string> };
    const listeners = new Set<() => void>();
    const query = {
      matches: dark,
      addEventListener: (_: string, l: () => void) => listeners.add(l),
      removeEventListener: (_: string, l: () => void) => listeners.delete(l),
    };
    vi.stubGlobal("document", { documentElement: root });
    vi.stubGlobal("window", { matchMedia: () => query });
    return { root, query, listeners };
  }

  it("AC9.3 light and dark fix data-theme; auto follows the system and its changes", () => {
    const system = stubSystem(true);
    applyAppearance("light");
    expect(system.root.dataset.theme).toBe("light");
    applyAppearance("auto");
    expect(system.root.dataset.theme).toBe("dark");
    system.query.matches = false;
    system.listeners.forEach((l) => l());
    expect(system.root.dataset.theme).toBe("light");
    applyAppearance("dark");
    expect(system.root.dataset.theme).toBe("dark");
    expect(system.listeners.size).toBe(0);
  });
});

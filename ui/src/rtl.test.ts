import { describe, expect, it } from "vitest";
import { LOCALES, RTL_LOCALES } from "./i18n/locale";
import swissMoss from "swiss-moss/css/swiss-moss.css?raw";
import local from "./styles/local.css?raw";

// Arabic and Hebrew are right to left (docs/decisions.md): local.css uses logical
// properties, so only the rules written for rtl itself may name a physical side.

const rules = [...local.replace(/\/\*[\s\S]*?\*\//g, "").matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((m) => ({
  selector: m[1].trim(),
  body: m[2],
}));

describe("right to left", () => {
  it("uses no physical left or right outside the rtl rules", () => {
    const physical = /(^|[\s;])(margin|padding|border)-(left|right)\s*:|(^|[\s;])(left|right)\s*:|text-align\s*:\s*(left|right)/;
    const offenders = rules.filter((r) => !r.selector.includes('[dir="rtl"]') && physical.test(r.body));
    expect(offenders.map((r) => r.selector)).toEqual([]);
  });

  it("lists exactly ar and he as right-to-left locales", () => {
    expect(LOCALES.filter((l) => RTL_LOCALES.has(l))).toEqual(["ar", "he"]);
  });

  it("leaves the switch knob to Swiss Moss, which places it with inset-inline-start", () => {
    expect(swissMoss).toMatch(/\.sm-switch input::after\{[^}]*inset-inline-start:3px/);
    expect(swissMoss).toMatch(/\.sm-switch input:checked::after\{[^}]*inset-inline-start:19px/);
    expect(rules.filter((r) => r.selector.includes(".sm-switch input")).map((r) => r.selector)).toEqual([]);
  });

  it.each(["ar", "he", "hi", "ja", "zh"])("resets the Swiss Moss display letter-spacing for :lang(%s)", (lang) => {
    const reset = rules.find((r) => r.selector.split(/\s*,\s*/).includes(`:root:lang(${lang}) *`));
    expect(reset?.body).toMatch(/letter-spacing:\s*normal/);
  });

  it.each([
    ["ar", ['"Noto Sans Arabic"', '"Segoe UI"', '"Geeza Pro"']],
    ["hi", ['"Kohinoor Devanagari"', '"Nirmala UI"', '"Noto Sans Devanagari"']],
    ["zh", ['"PingFang SC"', '"Microsoft YaHei"', '"Noto Sans CJK SC"']],
    ["ja", ['"Hiragino Sans"', '"Yu Gothic"', '"Noto Sans CJK JP"']],
    ["he", ['"Arial Hebrew"', '"Segoe UI"', '"Noto Sans Hebrew"']],
  ])("falls back to system fonts for :lang(%s), Inter first", (lang, fonts) => {
    const body = rules.find((r) => r.selector === `:root:lang(${lang})`)?.body ?? "";
    for (const token of ["--sm-font-sans", "--sm-font-display"]) {
      const stack = new RegExp(`${token}:([^;]+);`).exec(body)?.[1] ?? "";
      expect(stack.indexOf("Inter")).toBeGreaterThanOrEqual(0);
      expect(fonts.map((f) => stack.indexOf(f))).toEqual(fonts.map((f) => stack.indexOf(f)).sort((a, b) => a - b));
      for (const font of fonts) expect(stack.indexOf(font)).toBeGreaterThan(stack.indexOf("Inter"));
    }
  });
});

import { describe, expect, it } from "vitest";
import local from "./styles/local.css?raw";

// Arabic is right to left (docs/decisions.md, "Locale list for beta 1"): local.css uses logical
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

  it("mirrors the Swiss Moss switch knob", () => {
    const knob = rules.find((r) => r.selector === ':root[dir="rtl"] .sm-switch input:checked::after');
    expect(knob?.body).toContain("translateX(-16px)");
  });

  it.each([
    ["ar", ['"Noto Sans Arabic"', '"Segoe UI"', '"Geeza Pro"']],
    ["hi", ['"Kohinoor Devanagari"', '"Nirmala UI"', '"Noto Sans Devanagari"']],
    ["zh", ['"PingFang SC"', '"Microsoft YaHei"', '"Noto Sans CJK SC"']],
    ["ja", ['"Hiragino Sans"', '"Yu Gothic"', '"Noto Sans CJK JP"']],
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

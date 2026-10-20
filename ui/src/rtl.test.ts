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
});

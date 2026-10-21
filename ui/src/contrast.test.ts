import { describe, expect, it } from "vitest";
import local from "./styles/local.css?raw";
import tokens from "swiss-moss/tokens/tokens.css?raw";

// WCAG 2 contrast for the pairs the interrupt page draws, read from the Swiss Moss tokens and from
// local.css itself, so a changed token or rule that breaks AA fails here (AC4.3, both themes).

const lightBlock = tokens.slice(tokens.indexOf(":root {"), tokens.indexOf("@media"));
const darkBlock = tokens.slice(tokens.indexOf(':root[data-theme="dark"] {'));

function palette(block: string): Record<string, string> {
  return Object.fromEntries([...block.matchAll(/(--sm-[\w-]+):\s*(#[0-9A-Fa-f]{6})/g)].map((m) => [m[1], m[2]]));
}

const THEMES = { light: palette(lightBlock), dark: palette(darkBlock) } as const;

function luminance(hex: string): number {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** The value of `property` in the rule whose selector is exactly `selector`. */
function declaration(selector: string, property: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\:]/g, "\\$&");
  const rule = new RegExp(`(?:^|\\})\\s*${escaped}\\s*\\{([^}]*)\\}`, "m").exec(local);
  const value = new RegExp(`(?:^|[;\\s])${property}:\\s*([^;]+);`).exec(rule?.[1] ?? "")?.[1];
  if (!value) throw new Error(`no ${property} in ${selector}`);
  return value.trim();
}

function resolve(theme: keyof typeof THEMES, value: string): string {
  const name = /^var\((--sm-[\w-]+)\)$/.exec(value)?.[1];
  const hex = name ? THEMES[theme][name] : undefined;
  if (!hex) throw new Error(`${value} is not a Swiss Moss token`);
  return hex;
}

// [what it is, background rule, colour rule]
const PAIRS: [string, [string, string], [string, string]][] = [
  ["text on the moss fill", [".tl-int", "background"], [".tl-int", "color"]],
  ["quiet button text", [".tl-int", "background"], [".tl-int .sm-btn", "color"]],
  ["quiet button hovered", [".tl-int .sm-btn:hover", "background"], [".tl-int .sm-btn:hover", "color"]],
  ["primary button", [".tl-int .sm-btn.tl-primary", "background"], [".tl-int .sm-btn.tl-primary", "color"]],
  ["primary button hovered", [".tl-int .sm-btn.tl-primary:hover", "background"], [".tl-int .sm-btn.tl-primary:hover", "color"]],
  // The first-run preview of the alarm (mockup 05) draws the same pairs.
  ["preview text", [".tl-preview", "background"], [".tl-preview", "color"]],
  ["preview quiet button", [".tl-preview", "background"], [".tl-preview .sm-btn", "color"]],
  ["preview primary button", [".tl-preview .sm-btn.tl-primary", "background"], [".tl-preview .sm-btn.tl-primary", "color"]],
];

/** `fg` at `share` (0 to 1) over `bg`, as `color-mix(in srgb, fg share, transparent)` draws on that fill. */
function mix(fg: string, bg: string, share: number): string {
  const channel = (hex: string, i: number) => parseInt(hex.slice(i, i + 2), 16);
  return `#${[1, 3, 5]
    .map((i) => Math.round(channel(fg, i) * share + channel(bg, i) * (1 - share)).toString(16).padStart(2, "0"))
    .join("")}`;
}

/** The token in a value such as `calc(3 * var(--tl-u)) solid var(--sm-on-moss)`. */
const tokenIn = (value: string) => `var(${/var\((--sm-[\w-]+)\)/.exec(value)?.[1]})`;

describe("interrupt contrast (AC4.3)", () => {
  // D5: the quiet buttons' line, the primary button's edge and the focus ring are boundaries (3:1).
  it.each(["light", "dark"] as const)("%s: every button boundary reaches 3:1", (theme) => {
    const bg = resolve(theme, declaration(".tl-int", "background"));
    const soft = /--tl-ring:\s*color-mix\(in srgb, (var\(--sm-[\w-]+\)) (\d+)%, transparent\)/.exec(local);
    if (!soft) throw new Error("no soft --tl-ring");
    const boundaries: [string, string][] = [
      ["quiet button line", mix(resolve(theme, soft[1]), bg, Number(soft[2]) / 100)],
      ["quiet button line without color-mix", resolve(theme, declaration(".tl-int", "--tl-ring"))],
      ["primary button edge", resolve(theme, declaration(".tl-int .sm-btn.tl-primary", "background"))],
      ["focus ring", resolve(theme, tokenIn(declaration(".tl-int :focus-visible", "outline")))],
    ];
    for (const [name, colour] of boundaries) expect(contrast(colour, bg), `${theme}: ${name}`).toBeGreaterThanOrEqual(3);
  });

  // D5: the hint, details and consequence lines take the full text colour; nothing dims them.
  it.each([".tl-int-hint", ".tl-int-details", ".tl-int-consequence", ".tl-int-top"])("%s is not dimmed", (selector) => {
    for (const property of ["color", "opacity"]) expect(() => declaration(selector, property)).toThrow();
  });


  it.each(["light", "dark"] as const)("%s: every text pair reaches 4.5:1", (theme) => {
    for (const [name, [bgRule, bgProp], [fgRule, fgProp]] of PAIRS) {
      const ratio = contrast(resolve(theme, declaration(bgRule, bgProp)), resolve(theme, declaration(fgRule, fgProp)));
      expect(ratio, `${theme}: ${name}`).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("the hovered quiet button is moss-deep with on-moss text: 7.7:1 in light", () => {
    const bg = resolve("light", declaration(".tl-int .sm-btn:hover", "background"));
    const fg = resolve("light", declaration(".tl-int .sm-btn:hover", "color"));
    expect(bg).toBe(THEMES.light["--sm-moss-deep"]);
    expect(contrast(bg, fg)).toBeCloseTo(7.74, 1);
  });

  it("computes WCAG ratios", () => {
    expect(contrast("#FFFFFF", "#000000")).toBeCloseTo(21, 5);
    // The failing pair the review found: white on a 45% white mix over moss.
    expect(contrast("#FFFFFF", "#9BB6A0")).toBeLessThan(2.5);
  });
});

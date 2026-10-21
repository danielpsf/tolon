import { describe, expect, it } from "vitest";
import { INTERRUPT_COLORS, type InterruptColor } from "./api-types";
import { LINE_MIX, PREVIEW_TOKENS, REMINDER_PAIRS } from "./reminder-colors";
import local from "./styles/local.css?raw";
import tokens from "swiss-moss/tokens/tokens.css?raw";

// WCAG 2 contrast for every pair the interrupt page draws, for every reminder colour (L-043) in light
// and dark, read from the Swiss Moss tokens Tolon ships (ui/node_modules/swiss-moss) and from
// local.css itself, so a changed token or rule that breaks AA fails here (AC4.3, D5).

const lightBlock = tokens.slice(tokens.indexOf(":root {"), tokens.indexOf("@media"));
const darkBlock = tokens.slice(tokens.indexOf(':root[data-theme="dark"] {'));

function palette(block: string): Record<string, string> {
  return Object.fromEntries([...block.matchAll(/(--sm-[\w-]+):\s*(#[0-9A-Fa-f]{6})/g)].map((m) => [m[1], m[2]]));
}

const THEMES = { light: palette(lightBlock), dark: palette(darkBlock) } as const;
type Theme = keyof typeof THEMES;
const APPEARANCES = ["light", "dark"] as const;

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

/** `fg` at `share` (0 to 1) over `bg`, as `color-mix(in srgb, fg share, transparent)` draws on that fill. */
function mix(fg: string, bg: string, share: number): string {
  const channel = (hex: string, i: number) => parseInt(hex.slice(i, i + 2), 16);
  return `#${[1, 3, 5]
    .map((i) => Math.round(channel(fg, i) * share + channel(bg, i) * (1 - share)).toString(16).padStart(2, "0"))
    .join("")}`;
}

const escape = (selector: string) => selector.replace(/[.*+?^${}()|[\]\\:"=]/g, "\\$&");

/** The body of the top-level rule whose selector is exactly `selector`. */
function rule(selector: string): string {
  const body = new RegExp(`(?:^|\\})\\s*${escape(selector)}\\s*\\{([^}]*)\\}`, "m").exec(local)?.[1];
  if (body === undefined) throw new Error(`no rule ${selector}`);
  return body;
}

/** The value of `property` in the rule whose selector is exactly `selector`. */
function declaration(selector: string, property: string): string {
  const value = new RegExp(`(?:^|[;\\s])${property}:\\s*([^;]+);`).exec(rule(selector))?.[1];
  if (!value) throw new Error(`no ${property} in ${selector}`);
  return value.trim();
}

const hexOf = (theme: Theme, token: string): string => {
  const hex = THEMES[theme][token];
  if (!hex) throw new Error(`${token} is not a Swiss Moss token`);
  return hex;
};

const TL = { "--tl-bg": "bg", "--tl-fg": "fg", "--tl-primary-fg": "primaryFg", "--tl-hover": "hover" } as const;

/** A `var(--sm-*)` or `var(--tl-*)` value as hex, in `theme` for the reminder colour `color`. */
function resolve(theme: Theme, color: InterruptColor, value: string): string {
  const name = /^var\((--(?:sm|tl)-[\w-]+)\)$/.exec(value)?.[1];
  if (!name) throw new Error(`${value} is not a token`);
  const field = TL[name as keyof typeof TL];
  return hexOf(theme, field ? REMINDER_PAIRS[color][field] : name);
}

// [what it is, background rule, colour rule]
const PAIRS: [string, [string, string], [string, string]][] = [
  ["text on the fill", [".tl-int", "background"], [".tl-int", "color"]],
  ["quiet button text", [".tl-int", "background"], [".tl-int .sm-btn", "color"]],
  ["quiet button hovered", [".tl-int .sm-btn:hover", "background"], [".tl-int .sm-btn:hover", "color"]],
  ["primary button", [".tl-int .sm-btn.tl-primary", "background"], [".tl-int .sm-btn.tl-primary", "color"]],
  ["primary button hovered", [".tl-int .sm-btn.tl-primary:hover", "background"], [".tl-int .sm-btn.tl-primary:hover", "color"]],
  // The first-run and Appearance previews of the alarm (mockup 05) draw the same pairs.
  ["preview text", [".tl-preview", "background"], [".tl-preview", "color"]],
  ["preview quiet button", [".tl-preview", "background"], [".tl-preview .sm-btn", "color"]],
  ["preview primary button", [".tl-preview .sm-btn.tl-primary", "background"], [".tl-preview .sm-btn.tl-primary", "color"]],
  ["swatch sample", [".tl-swatch i span", "background"], [".tl-swatch i span", "color"]],
];

const EVERY = INTERRUPT_COLORS.flatMap((color) => APPEARANCES.map((theme) => [color, theme] as const));

/** The ratios of one pair, for the table in the report. */
function ratios(color: InterruptColor, theme: Theme) {
  const p = REMINDER_PAIRS[color];
  const [bg, fg] = [hexOf(theme, p.bg), hexOf(theme, p.fg)];
  return {
    text: contrast(bg, fg),
    primary: contrast(fg, hexOf(theme, p.primaryFg)),
    hover: contrast(hexOf(theme, p.hover), fg),
    line: contrast(mix(fg, bg, LINE_MIX / 100), bg),
  };
}

describe("interrupt contrast for every reminder colour (AC4.3, L-043)", () => {
  it("offers the eight choices of the decision, each a Swiss Moss token pair", () => {
    expect(Object.keys(REMINDER_PAIRS)).toEqual([...INTERRUPT_COLORS]);
    for (const pair of Object.values(REMINDER_PAIRS)) {
      for (const token of Object.values(pair)) expect(THEMES.light[token] && THEMES.dark[token], token).toBeTruthy();
    }
    // "theme" is today's look: moss with on-moss text and the moss-deep primary label.
    expect(REMINDER_PAIRS.theme).toEqual({ bg: "--sm-moss", fg: "--sm-on-moss", primaryFg: "--sm-moss-deep", hover: "--sm-moss-deep" });
  });

  it("local.css maps every data-color to the same tokens as the table", () => {
    const declared = (selector: string) =>
      Object.fromEntries([...rule(selector).matchAll(/(--tl-[\w-]+):\s*var\((--sm-[\w-]+)\)/g)].map((m) => [m[1], m[2]]));
    const fallback = declared(":where([data-color])");
    for (const color of INTERRUPT_COLORS) {
      const own = color === "theme" || color === "moss" ? {} : declared(`[data-color="${color}"]`);
      const css = { ...fallback, ...own };
      const p = REMINDER_PAIRS[color];
      expect(css, color).toEqual({ "--tl-bg": p.bg, "--tl-fg": p.fg, "--tl-primary-fg": p.primaryFg, "--tl-hover": p.hover });
    }
    // "theme" and "moss" are the default: no rule of their own.
    expect(local).not.toMatch(/\[data-color="(theme|moss)"\]/);
  });

  it.each(EVERY)("%s in %s: every text pair reaches 4.5:1", (color, theme) => {
    for (const [name, [bgRule, bgProp], [fgRule, fgProp]] of PAIRS) {
      const bg = resolve(theme, color, declaration(bgRule, bgProp));
      const fg = resolve(theme, color, declaration(fgRule, fgProp));
      expect(contrast(bg, fg), `${color} ${theme}: ${name}`).toBeGreaterThanOrEqual(4.5);
    }
  });

  // D5: the quiet buttons' line, the primary button's edge and the focus ring are boundaries (3:1).
  it.each(EVERY)("%s in %s: every button boundary reaches 3:1", (color, theme) => {
    const bg = resolve(theme, color, declaration(".tl-int", "background"));
    const soft = /--tl-ring:\s*color-mix\(in srgb, (var\(--tl-[\w-]+\)) (\d+)%, transparent\)/.exec(local);
    if (!soft) throw new Error("no soft --tl-ring");
    expect(Number(soft[2])).toBe(LINE_MIX);
    const focus = /var\(--tl-[\w-]+\)$/.exec(declaration(".tl-int :focus-visible", "outline"))?.[0] ?? "";
    const boundaries: [string, string][] = [
      ["quiet button line", mix(resolve(theme, color, soft[1]), bg, LINE_MIX / 100)],
      ["quiet button line without color-mix", resolve(theme, color, declaration(".tl-int", "--tl-ring"))],
      ["primary button edge", resolve(theme, color, declaration(".tl-int .sm-btn.tl-primary", "background"))],
      ["focus ring", resolve(theme, color, focus)],
    ];
    for (const [name, colour] of boundaries) {
      expect(contrast(colour, bg), `${color} ${theme}: ${name}`).toBeGreaterThanOrEqual(3);
    }
  });

  it.each(EVERY)("%s in %s: a hovered quiet button can be told from the fill", (color, theme) => {
    const p = REMINDER_PAIRS[color];
    expect(contrast(hexOf(theme, p.hover), hexOf(theme, p.bg))).toBeGreaterThan(1.15);
  });

  // D5: the hint, details and consequence lines take the full text colour; nothing dims them.
  it.each([".tl-int-hint", ".tl-int-details", ".tl-int-consequence", ".tl-int-top"])("%s is not dimmed", (selector) => {
    for (const property of ["color", "opacity"]) expect(() => declaration(selector, property)).toThrow();
  });

  it("the hovered quiet button of the default is moss-deep with on-moss text: 7.7:1 in light", () => {
    expect(ratios("theme", "light").hover).toBeCloseTo(7.74, 1);
  });

  // The table in the L-043 report: a token change shows up here as a diff.
  it("has these ratios per pair and appearance", () => {
    const rows = EVERY.map(([color, theme]) => {
      const r = ratios(color, theme);
      return `${color} ${theme}: text ${r.text.toFixed(2)}, primary ${r.primary.toFixed(2)}, hover ${r.hover.toFixed(2)}, line ${r.line.toFixed(2)}`;
    });
    expect(rows).toEqual([
      "theme light: text 5.00, primary 7.74, hover 7.74, line 3.35",
      "theme dark: text 8.61, primary 11.11, hover 11.11, line 4.50",
      "moss light: text 5.00, primary 7.74, hover 7.74, line 3.35",
      "moss dark: text 8.61, primary 11.11, hover 11.11, line 4.50",
      "moss-deep light: text 7.74, primary 7.74, hover 5.00, line 4.77",
      "moss-deep dark: text 11.11, primary 11.11, hover 8.61, line 5.19",
      "ink light: text 16.64, primary 16.64, hover 4.98, line 8.59",
      "ink dark: text 14.26, primary 14.26, hover 7.15, line 5.92",
      "paper light: text 16.64, primary 16.64, hover 13.40, line 6.33",
      "paper dark: text 14.26, primary 14.26, hover 9.67, line 7.50",
      "info light: text 7.16, primary 7.16, hover 17.66, line 4.46",
      "info dark: text 8.40, primary 8.40, hover 14.26, line 4.46",
      "signal light: text 5.33, primary 5.33, hover 17.66, line 3.49",
      "signal dark: text 8.12, primary 8.12, hover 14.26, line 4.40",
      "danger light: text 6.82, primary 6.82, hover 17.66, line 4.18",
      "danger dark: text 6.84, primary 6.84, hover 14.26, line 3.97",
    ]);
  });

  it("computes WCAG ratios", () => {
    expect(contrast("#FFFFFF", "#000000")).toBeCloseTo(21, 5);
    // The failing pair the review found: white on a 45% white mix over moss.
    expect(contrast("#FFFFFF", "#9BB6A0")).toBeLessThan(2.5);
  });
});

describe("preview tokens (AC4.3, L-043)", () => {
  it("scopes Swiss Moss's own light and dark values to data-tokens, so a preview shows either appearance", () => {
    for (const [theme, values] of Object.entries(THEMES)) {
      const block = new RegExp(`\\[data-tokens=${theme}\\]\\{([^}]*)\\}`).exec(PREVIEW_TOKENS)?.[1] ?? "";
      expect(palette(block), theme).toEqual(values);
    }
  });
});

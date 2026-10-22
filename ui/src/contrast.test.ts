import { describe, expect, it } from "vitest";
import { INTERRUPT_COLORS, type InterruptColor } from "./api-types";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { peekColor, PeekScene } from "./pages/PeekScene";
import { LINE_MIX, PREVIEW_TOKENS, REMINDER_PAIRS, tokensFor } from "./reminder-colors";
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

it("finds both Swiss Moss token blocks in tokens.css", () => {
  expect(tokens.indexOf(":root {")).toBeGreaterThanOrEqual(0);
  expect(tokens.indexOf(':root[data-theme="dark"] {')).toBeGreaterThanOrEqual(0);
});

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

// Where each colour is drawn: Default in both appearances, every other choice pinned to light (L-043).
const EVERY = INTERRUPT_COLORS.flatMap((color) =>
  (color === "theme" ? APPEARANCES : (["light"] as const)).map((theme) => [color, theme] as const),
);

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
      "moss-deep light: text 7.74, primary 7.74, hover 5.00, line 4.77",
      "ink light: text 16.64, primary 16.64, hover 4.98, line 8.59",
      "paper light: text 16.64, primary 16.64, hover 13.40, line 6.33",
      "info light: text 7.16, primary 7.16, hover 17.66, line 4.46",
      "signal light: text 5.33, primary 5.33, hover 17.66, line 3.49",
      "danger light: text 6.82, primary 6.82, hover 17.66, line 4.18",
    ]);
  });

  it("computes WCAG ratios", () => {
    expect(contrast("#FFFFFF", "#000000")).toBeCloseTo(21, 5);
    // The failing pair the review found: white on a 45% white mix over moss.
    expect(contrast("#FFFFFF", "#9BB6A0")).toBeLessThan(2.5);
  });
});

// L-046, WCAG 1.4.11: the preview card and every swatch chip carry a 1px border that reaches 3:1 against the
// PAGE (--sm-paper of the window's appearance), not against the pinned block they draw in.
describe("preview and swatch borders against the page (L-046)", () => {
  /** The `--sm-*` token behind a border: through `--tl-edge` (declared on the unpinned figure) or directly. */
  const edge = (selector: string) => {
    const border = declaration(selector, "border");
    expect(border, selector).toMatch(/^1px solid var\(--(?:tl-edge|sm-control-line)\)$/);
    const name = /var\((--[\w-]+)\)/.exec(border)?.[1] ?? "";
    return name === "--tl-edge" ? (/var\((--sm-[\w-]+)\)/.exec(declaration(".tl-first-preview", "--tl-edge"))?.[1] ?? "") : name;
  };
  const measured = (selector: string, appearance: Theme) => contrast(hexOf(appearance, edge(selector)), hexOf(appearance, "--sm-paper"));

  it.each(APPEARANCES)("the preview card border reaches 3:1 against the page in %s", (appearance) => {
    expect(measured(".tl-preview", appearance)).toBeGreaterThanOrEqual(3);
  });

  it.each(APPEARANCES)("the swatch chip border reaches 3:1 against the page in %s", (appearance) => {
    expect(measured(".tl-swatch i", appearance)).toBeGreaterThanOrEqual(3);
  });

  it("resolves the card's edge where the window's tokens apply, never inside a data-tokens block", () => {
    // `.tl-preview` carries data-tokens, so a Swiss Moss token written on it would be the pinned one.
    expect(declaration(".tl-preview", "border")).toBe("1px solid var(--tl-edge)");
    expect(rule(".tl-first-preview")).toMatch(/--tl-edge:\s*var\(--sm-control-line\)/);
    // The chip is the unpinned parent of the pinned sample span.
    expect(declaration(".tl-swatch i", "border")).toBe("1px solid var(--sm-control-line)");
  });

  it("the numbers: light and dark", () => {
    expect(APPEARANCES.map((a) => `${a}: card ${measured(".tl-preview", a).toFixed(2)}, chip ${measured(".tl-swatch i", a).toFixed(2)}`)).toEqual([
      "light: card 3.33, chip 3.33",
      "dark: card 3.46, chip 3.46",
    ]);
  });
});

describe("the Default swatch (L-046)", () => {
  it("is half light-appearance moss and half dark-appearance moss, so it differs from Moss", () => {
    const light = hexOf("light", REMINDER_PAIRS.theme.bg);
    const dark = hexOf("dark", REMINDER_PAIRS.theme.bg);
    expect(light).toBe(hexOf("light", REMINDER_PAIRS.moss.bg));
    expect(dark).not.toBe(light);
    // The two halves differ by more than hue.
    expect(contrast(light, dark)).toBeGreaterThan(1.5);
  });
});

describe("preview tokens (AC4.3, L-043)", () => {
  it("pins every choice but Default to the light block, so a dark window draws them with the light values", () => {
    for (const color of INTERRUPT_COLORS) {
      expect(tokensFor(color, "dark"), color).toBe(color === "theme" ? "dark" : "light");
      expect(tokensFor(color), color).toBe(color === "theme" ? undefined : "light");
    }
  });

  it("carries colours, shadows and color-scheme only, so the :lang() font stacks hold inside a pinned element", () => {
    for (const other of ["--sm-font", "--sm-space", "--sm-radius", "--sm-measure", "--sm-fast", "--sm-base", "--sm-ease"]) {
      expect(PREVIEW_TOKENS, other).not.toContain(other);
    }
    for (const theme of APPEARANCES) {
      const block = new RegExp(`\\[data-tokens=${theme}\\]\\{([^}]*)\\}`).exec(PREVIEW_TOKENS)?.[1] ?? "";
      expect(block).toContain(`color-scheme: ${theme}`);
      expect(block).toMatch(/--sm-shadow-rest:[^;]*rgba\(/);
    }
  });

  it("scopes Swiss Moss's own light and dark values to data-tokens, so a preview shows either appearance", () => {
    for (const [theme, values] of Object.entries(THEMES)) {
      const block = new RegExp(`\\[data-tokens=${theme}\\]\\{([^}]*)\\}`).exec(PREVIEW_TOKENS)?.[1] ?? "";
      expect(palette(block), theme).toEqual(values);
    }
  });
});

// ---- Peek (L-050) ----------------------------------------------------------------------------------------
// The page follows the window's appearance; the hill, Join and the speech bubble are pinned to Swiss Moss's
// light block with the reminder colour (Default draws Peek's deep moss). The --pk-* properties carry the
// page's own tokens into the pinned Join (its edge and focus ring).

const PEEK = '[data-style="peek"]';
const HINT = `.tl-int-hint,\n${PEEK} .tl-int-consequence`;

/** A Peek value as hex: `--sm-*` in `theme`, `--tl-*` from the pinned pair of `color`, `--pk-*` from the page. */
function peekResolve(theme: Theme, color: InterruptColor, value: string): string {
  const name = /^var\((--(?:sm|tl|pk)-[\w-]+)\)$/.exec(value)?.[1];
  if (!name) throw new Error(`${value} is not a token`);
  if (name.startsWith("--pk-")) return peekResolve(theme, color, declaration(`${PEEK} .tl-int`, name));
  const field = TL[name as keyof typeof TL];
  return field ? hexOf("light", REMINDER_PAIRS[peekColor(color)][field]) : hexOf(theme, name);
}

type At = [selector: string, property: string];
const peekHex = (theme: Theme, color: InterruptColor, [selector, property]: At) =>
  peekResolve(theme, color, declaration(`${PEEK} ${selector}`, property));
/** The same for a rule inside the pinned hill (`data-tokens="light"`): its `--sm-*` tokens are the light ones in a dark window too (L-062). */
const pinnedHex = (color: InterruptColor, at: At) => peekHex("light", color, at);
const PAGE: At = [".tl-int", "background"];

// [what it is, background, text]; Join and the chip are pinned through --tl-*, the speech bubble through its
// ancestor's data-tokens (PINNED below), the rest follow the page.
const PEEK_PAIRS: [string, At, At][] = [
  ["title and page text", PAGE, [".tl-int", "color"]],
  ["details", PAGE, [".tl-int-details", "color"]],
  ["key hint and consequence", PAGE, [HINT, "color"]],
  ["countdown in the time pill", [".tl-int-top", "background"], [".tl-int", "color"]],
  ["clock on the moss chip", [".tl-int-top > :first-child", "background"], [".tl-int-top > :first-child", "color"]],
  ["Snooze and Dismiss", [".tl-int .sm-btn", "background"], [".tl-int .sm-btn", "color"]],
  ["Snooze and Dismiss hovered", [".tl-int .sm-btn:hover", "background"], [".tl-int .sm-btn:hover", "color"]],
  ["Join", [".tl-int .sm-btn.tl-primary", "background"], [".tl-int .sm-btn.tl-primary", "color"]],
  ["Join hovered", [".tl-int .sm-btn.tl-primary:hover", "background"], [".tl-int .sm-btn.tl-primary:hover", "color"]],
  ["the Enter chip", [".tl-peek-key", "background"], [".tl-peek-key", "color"]],
  ["the speech bubble", [".tl-peek-say", "background"], [".tl-peek-say", "color"]],
];

/** Pairs drawn inside the hill's `data-tokens="light"` block: they resolve in the light tokens whatever the appearance. */
const PINNED = new Set(["the speech bubble"]);
const peekPair = (theme: Theme, color: InterruptColor, [name, bg, fg]: [string, At, At]) =>
  contrast(...(PINNED.has(name) ? ([pinnedHex(color, bg), pinnedHex(color, fg)] as const) : ([peekHex(theme, color, bg), peekHex(theme, color, fg)] as const)));

// Every button's edge and the focus ring sit on the page, so they are measured against it (3:1).
const PEEK_EDGES: [string, At][] = [
  ["Snooze and Dismiss edge", [".tl-int .sm-btn", "border-color"]],
  ["Join edge", [".tl-int .sm-btn.tl-primary", "border-color"]],
  ["focus ring", [".tl-int :focus-visible", "outline-color"]],
];

const PEEK_EVERY = INTERRUPT_COLORS.flatMap((color) => APPEARANCES.map((theme) => [color, theme] as const));

describe("Peek contrast in light and dark (AC4.3, L-050)", () => {
  it("Default draws Peek's deep moss; every other colour is its own", () => {
    for (const color of INTERRUPT_COLORS) expect(peekColor(color)).toBe(color === "theme" ? "moss-deep" : color);
  });

  it.each(PEEK_EVERY)("%s in %s: every text pair reaches 4.5:1", (color, theme) => {
    for (const pair of PEEK_PAIRS) expect(peekPair(theme, color, pair), `${color} ${theme}: ${pair[0]}`).toBeGreaterThanOrEqual(4.5);
  });

  it.each(PEEK_EVERY)("%s in %s: button edges and the focus ring reach 3:1 on the page", (color, theme) => {
    for (const [name, edge] of PEEK_EDGES) {
      expect(contrast(peekHex(theme, color, edge), peekHex(theme, color, PAGE)), `${color} ${theme}: ${name}`).toBeGreaterThanOrEqual(3);
    }
  });

  it("the speech bubble sits inside the hill's light-pinned block, so it is measured in light tokens (L-062)", () => {
    const html = renderToStaticMarkup(createElement(PeekScene, { color: "theme", say: "It's time." }));
    expect(html).toMatch(/^<div class="tl-peek"[^>]* data-tokens="light"[\s\S]*<p class="tl-peek-say">/);
    expect(PINNED.has("the speech bubble")).toBe(true);
  });

  it("uses Swiss Moss tokens only, no hex, in the Peek block", () => {
    const block = local.slice(local.indexOf(" * Peek (L-050"), local.indexOf(" * Right to left"));
    expect(block.length).toBeGreaterThan(1000);
    expect(block).not.toMatch(/#[0-9A-Fa-f]{3,8}\b/);
    expect(block).not.toMatch(/rgba?\(|hsla?\(/);
    for (const [, name] of block.matchAll(/var\((--[\w-]+)\)/g)) expect(name, name).toMatch(/^--(sm|tl|pk)-/);
  });

  // The table for the L-050 report (Default): a token change shows up here as a diff.
  it("has these ratios for Default in light and dark", () => {
    const rows = APPEARANCES.map((theme) => {
      const r = (bg: At, fg: At) => contrast(peekHex(theme, "theme", bg), peekHex(theme, "theme", fg)).toFixed(2);
      const text = PEEK_PAIRS.map((pair) => `${pair[0]} ${peekPair(theme, "theme", pair).toFixed(2)}`);
      const edges = PEEK_EDGES.map(([name, edge]) => `${name} ${r(PAGE, edge)}`);
      return `${theme}: ${[...text, ...edges].join(", ")}`;
    });
    expect(rows).toEqual([
      "light: title and page text 16.64, details 4.71, key hint and consequence 4.98, countdown in the time pill 15.08, clock on the moss chip 7.74, Snooze and Dismiss 15.08, Snooze and Dismiss hovered 13.40, Join 7.74, Join hovered 5.00, the Enter chip 7.74, the speech bubble 15.08, Snooze and Dismiss edge 3.33, Join edge 3.33, focus ring 16.64",
      "dark: title and page text 14.26, details 8.61, key hint and consequence 7.15, countdown in the time pill 13.03, clock on the moss chip 11.11, Snooze and Dismiss 13.03, Snooze and Dismiss hovered 9.67, Join 7.74, Join hovered 5.00, the Enter chip 7.74, the speech bubble 15.08, Snooze and Dismiss edge 3.46, Join edge 3.46, focus ring 14.26",
    ]);
  });
});

// ---- Tolon's mark (L-052, WCAG 1.4.11) ---------------------------------------------------------------------
// The mark's body is moss-deep, which is not 3:1 against every page it sits on (Default's moss, a dark page),
// so its 1.5px outline (`--tl-mark-edge`, resolved in the window's own tokens) is the boundary measured here.
// Swiss Moss reminder: the text colour against the reminder colour. Peek: --sm-muted against the time pill and
// the page. Settings sidebar, tray panel and first run: the stylesheet's default edge against their backgrounds.

describe("the mark's outline reaches 3:1 against what it sits on (L-052)", () => {
  it.each(EVERY)("Swiss Moss reminder, %s in %s: the outline is the text colour on the fill", (color, theme) => {
    expect(declaration(".tl-int-count", "--tl-mark-edge")).toBe("var(--tl-fg)");
    const edge = resolve(theme, color, declaration(".tl-int-count", "--tl-mark-edge"));
    const bg = resolve(theme, color, declaration(".tl-int", "background"));
    expect(contrast(edge, bg), `${color} ${theme}`).toBeGreaterThanOrEqual(3);
  });

  it.each(PEEK_EVERY)("Peek reminder, %s in %s: the outline reaches 3:1 on the time pill and on the page", (color, theme) => {
    const edge = peekHex(theme, color, [".tl-int-count", "--tl-mark-edge"]);
    expect(contrast(edge, peekHex(theme, color, [".tl-int-top", "background"])), `${color} ${theme}: pill`).toBeGreaterThanOrEqual(3);
    expect(contrast(edge, peekHex(theme, color, PAGE)), `${color} ${theme}: page`).toBeGreaterThanOrEqual(3);
  });

  it.each(APPEARANCES)("settings sidebar, tray panel and first run in %s: the default outline reaches 3:1", (theme) => {
    const edge = hexOf(theme, "--sm-muted");
    expect(declaration(".tl-mark-edge", "stroke")).toBe("var(--tl-mark-edge, var(--sm-muted))");
    // The sidebar is paper-deep (.tl-side), the panel is --sm-card (.tl-panel, L-062) and first run sits on the paper.
    const surface = (selector: string) => /^var\((--sm-[\w-]+)\)$/.exec(declaration(selector, "background"))?.[1] ?? "";
    expect([surface(".tl-side"), surface(".tl-panel")]).toEqual(["--sm-paper-deep", "--sm-card"]);
    for (const page of ["--sm-paper-deep", "--sm-card", "--sm-paper"]) {
      expect(contrast(edge, hexOf(theme, page)), `${theme}: ${page}`).toBeGreaterThanOrEqual(3);
    }
  });

  it("the numbers: Default light and dark, Peek pill, sidebar, panel", () => {
    const rows = APPEARANCES.map((theme) => {
      const swiss = contrast(hexOf(theme, REMINDER_PAIRS.theme.fg), hexOf(theme, REMINDER_PAIRS.theme.bg));
      const pill = contrast(peekHex(theme, "theme", [".tl-int-count", "--tl-mark-edge"]), peekHex(theme, "theme", [".tl-int-top", "background"]));
      const side = contrast(hexOf(theme, "--sm-muted"), hexOf(theme, "--sm-paper-deep"));
      const panel = contrast(hexOf(theme, "--sm-muted"), hexOf(theme, "--sm-card"));
      return `${theme}: reminder ${swiss.toFixed(2)}, Peek pill ${pill.toFixed(2)}, sidebar ${side.toFixed(2)}, panel ${panel.toFixed(2)}`;
    });
    expect(rows).toEqual([
      "light: reminder 5.00, Peek pill 4.51, sidebar 4.51, panel 5.28",
      "dark: reminder 8.61, Peek pill 6.53, sidebar 6.53, panel 6.26",
    ]);
  });
});

// The reminder colours (L-043, docs/decisions.md "Reminder colour (L-043)"): one Swiss Moss token pair
// per `interrupt_color`. Only "theme" (Default) follows the light or dark appearance; every other choice
// is pinned to Swiss Moss's light values (`tokensFor`), so the colour a person picks is the one they get.
// local.css maps the same pairs to `--tl-*` properties under `[data-color]`; contrast.test.ts checks that
// the two agree and that every pair reaches AA where it is drawn.
import tokens from "swiss-moss/tokens/tokens.css?raw";
import { INTERRUPT_COLORS, type InterruptColor } from "./api-types";

type Token = `--sm-${string}`;

export interface ReminderPair {
  /** The fill of the page. */
  bg: Token;
  /** Text, quiet button text, the focus ring and the quiet button line. */
  fg: Token;
  /** The primary button inverts the pair: `fg` behind, this in front. */
  primaryFg: Token;
  /** The fill of a hovered quiet button. */
  hover: Token;
}

/** The quiet button line: `fg` at this share over the fill (`color-mix`), so it stays a soft line that reaches 3:1. */
export const LINE_MIX = 70;

const moss = { bg: "--sm-moss", fg: "--sm-on-moss", primaryFg: "--sm-moss-deep", hover: "--sm-moss-deep" } as const;

export const REMINDER_PAIRS: Record<InterruptColor, ReminderPair> = {
  theme: moss,
  moss,
  "moss-deep": { bg: "--sm-moss-deep", fg: "--sm-on-moss", primaryFg: "--sm-moss-deep", hover: "--sm-moss" },
  ink: { bg: "--sm-ink", fg: "--sm-on-toast", primaryFg: "--sm-ink", hover: "--sm-muted" },
  paper: { bg: "--sm-paper", fg: "--sm-ink", primaryFg: "--sm-paper", hover: "--sm-line" },
  info: { bg: "--sm-info", fg: "--sm-on-moss", primaryFg: "--sm-info", hover: "--sm-ink" },
  signal: { bg: "--sm-signal", fg: "--sm-on-moss", primaryFg: "--sm-signal", hover: "--sm-ink" },
  danger: { bg: "--sm-danger", fg: "--sm-on-moss", primaryFg: "--sm-danger", hover: "--sm-ink" },
};

/** The `data-color` for a setting; anything unknown (an older or newer backend) is the default. */
export function colorAttr(color: string | null | undefined): InterruptColor {
  return INTERRUPT_COLORS.find((c) => c === color) ?? "theme";
}

/**
 * The `data-tokens` a reminder colour draws with: Default follows `appearance` (undefined: the window's
 * own), every other choice is pinned to Swiss Moss's light block. The element also needs PREVIEW_TOKENS.
 */
export function tokensFor(color: InterruptColor, appearance?: "light" | "dark"): "light" | "dark" | undefined {
  return color === "theme" ? appearance : "light";
}

/**
 * The colour and shadow declarations of one Swiss Moss block, and its `color-scheme`. Fonts, spacing,
 * radii, the measure and motion are left out, so the page's own values (the :lang() font stacks in
 * local.css among them) still apply inside a pinned element.
 */
function colours(start: number): string {
  const block = tokens.slice(tokens.indexOf("{", start) + 1, tokens.indexOf("}", start));
  return block
    .split(";")
    .map((d) => d.trim())
    .filter((d) => /^--sm-[\w-]+:.*(#[0-9A-Fa-f]{6}|rgba\()/.test(d) || d.startsWith("color-scheme:"))
    .join(";");
}

/**
 * Swiss Moss's colours of one appearance on any element with `data-tokens="light"` or `"dark"`, so a
 * preview can show the other appearance and a pinned colour keeps its light values (L-043). The values
 * are Swiss Moss's own, read from the vendored tokens.css at build time; none is written here.
 */
export const PREVIEW_TOKENS = `[data-tokens=light]{${colours(tokens.indexOf(":root {"))}}[data-tokens=dark]{${colours(
  tokens.indexOf(':root[data-theme="dark"] {'),
)}}`;

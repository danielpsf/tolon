import { describe, expect, it } from "vitest";
import local from "./styles/local.css?raw";

// L-045, decision "Reminder controls keep a fixed size": on the interrupt the title, the page padding and
// the gaps between blocks scale with the screen (--tl-u), but buttons and small text use the capped unit
// --tl-c, which never grows past one pixel. These tests read local.css, so a control sized with --tl-u again
// (89 pt buttons with 26 pt labels on a 1728x1117 MacBook) fails here.

const escape = (selector: string) => selector.replace(/[.*+?^${}()|[\]\\:"=]/g, "\\$&");

/** The bodies of every rule whose selector is exactly `selector`, so a later rule cannot dodge a test. */
function rules(selector: string): string[] {
  const bodies = [...local.matchAll(new RegExp(`(?:^|\\})\\s*${escape(selector)}\\s*\\{([^}]*)\\}`, "gm"))].map((m) => m[1] ?? "");
  if (bodies.length === 0) throw new Error(`no rule ${selector}`);
  return bodies;
}

/** Declarations of `property` in every rule for `selector` (a size set twice is seen). */
function values(selector: string, property: string): string[] {
  return rules(selector).flatMap((body) =>
    [...body.matchAll(new RegExp(`(?:^|[;\\s])${property}:\\s*([^;]+);`, "g"))].map((m) => m[1]?.trim() ?? ""),
  );
}

const value = (selector: string, property: string): string => {
  const all = values(selector, property);
  if (all.length !== 1) throw new Error(`${selector} has ${all.length} ${property}`);
  return all[0] ?? "";
};

/** Every selector of the controls and small text, with the length properties it sets. */
const CAPPED: Record<string, string[]> = {
  ".tl-int .sm-btn": ["min-height", "padding", "font-size"],
  ".tl-int .sm-btn.tl-primary": ["padding-inline", "font-size"],
  ".tl-int-top": ["gap"],
  ".tl-int-count": ["gap"],
  ".tl-int-count .tl-mark": ["width", "height"],
  ".tl-int-details": ["font-size"],
  ".tl-int-hint": ["font-size"],
  ".tl-int-consequence": ["flex", "max-width", "font-size"],
  ".tl-int-error": ["font-size"],
  ".tl-int .tl-host": ["max-width"],
};

/** The title, the page padding and the gaps between blocks. */
const SCALED: Record<string, string[]> = {
  ".tl-int": ["padding"],
  ".tl-int .tl-int-title": ["font-size"],
  '.tl-int .tl-int-title[data-len="long"]': ["font-size"],
  '.tl-int .tl-int-title[data-len="longer"]': ["font-size"],
  ".tl-int-head": ["gap"],
  ".tl-int-actions": ["gap"],
  ".tl-int-hint": ["margin"],
  ".tl-int-list": ["gap"],
  ".tl-int .tl-int-item .sm-display-l": ["font-size"],
};

describe("L-045 the reminder's controls keep a fixed size", () => {
  it("--tl-c is --tl-u's design pixel capped at one pixel, and shrinks only on a small screen", () => {
    expect(value(".tl-int", "--tl-u")).toBe("max(1px, calc(100vmin / 800))");
    expect(value(".tl-int", "--tl-c")).toBe("clamp(0.8px, calc(100vmin / 800), 1px)");
  });

  it.each(Object.entries(CAPPED))("%s sizes %j with --tl-c, never --tl-u", (selector, properties) => {
    for (const property of properties) {
      const all = values(selector, property);
      expect(all.length, `${selector} ${property}`).toBeGreaterThan(0);
      for (const v of all) {
        expect(v, `${selector} ${property}`).toContain("var(--tl-c)");
        expect(v, `${selector} ${property}`).not.toContain("--tl-u");
      }
    }
  });

  it.each(Object.entries(SCALED))("%s keeps scaling %j with --tl-u", (selector, properties) => {
    for (const property of properties) expect(value(selector, property)).toContain("var(--tl-u)");
  });

  it("B1 the gap between the buttons is at least the focus ring's reach, in the same unit", () => {
    const unit = (v: string) => {
      const m = /^calc\((\d+) \* var\((--tl-[uc])\)\)$/.exec(v);
      if (!m) throw new Error(`not calc(N * var(--tl-u|c)): ${v}`);
      return { n: Number(m[1]), unit: m[2] };
    };
    const ring = unit(value(".tl-int :focus-visible", "outline").replace(/ solid .*$/, ""));
    const offset = unit(value(".tl-int :focus-visible", "outline-offset"));
    const gap = unit(value(".tl-int-actions", "gap"));
    expect(offset.unit).toBe(ring.unit);
    expect(gap.unit).toBe(ring.unit);
    expect(gap.n).toBeGreaterThanOrEqual(ring.n + offset.n);
  });

  it("the interrupt's base text is the capped 15 pt, so nothing inherits a growing size", () => {
    expect(value(".tl-int", "font-size")).toBe("calc(15 * var(--tl-c))");
  });

  it("buttons are about 52 pt tall with 16 pt labels, the primary one 17 pt and semibold", () => {
    expect(value(".tl-int .sm-btn", "min-height")).toBe("calc(52 * var(--tl-c))");
    expect(value(".tl-int .sm-btn", "font-size")).toBe("calc(16 * var(--tl-c))");
    expect(value(".tl-int .sm-btn.tl-primary", "font-size")).toBe("calc(17 * var(--tl-c))");
    expect(value(".tl-int .sm-btn.tl-primary", "font-weight")).toBe("600");
  });

  it("the mark keeps the size the bell had next to the countdown (38 against 15), square (L-052)", () => {
    expect(value(".tl-int", "font-size")).toContain("15 *");
    expect(value(".tl-int-count .tl-mark", "width")).toBe("calc(38 * var(--tl-c))");
    expect(value(".tl-int-count .tl-mark", "height")).toBe("calc(38 * var(--tl-c))");
  });

  it("D3 an unknown host is cut to about the width of Snooze (a little under 200 pt, in the capped unit)", () => {
    expect(value(".tl-int .tl-host", "max-width")).toBe("calc(200 * var(--tl-c))");
    expect(value(".tl-int .tl-host", "text-overflow")).toBe("ellipsis");
    expect(value(".tl-int .tl-host", "direction")).toBe("rtl");
  });

  it("D4 the consequence line takes a small basis and may shrink, so it sits beside the buttons in balanced lines", () => {
    expect(value(".tl-int-consequence", "flex")).toBe("1 1 calc(200 * var(--tl-c))");
    expect(value(".tl-int-consequence", "min-width")).toBe("0");
    expect(value(".tl-int-consequence", "text-wrap")).toBe("balance");
  });
});

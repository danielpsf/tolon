import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { PeekMark } from "./PeekMark";
import local from "./styles/local.css?raw";

// L-052, decision "Peek is Tolon's mark everywhere": Peek's mark replaces the bell wherever the bell stood for
// Tolon. Sources are read raw so the bell cannot come back in a file no test renders.

const sources = import.meta.glob(["./*.tsx", "./pages/*.tsx"], { query: "?raw", import: "default", eager: true }) as Record<string, string>;
const mark = renderToStaticMarkup(createElement(PeekMark));
const markCss = local.slice(local.indexOf(".tl-mark {"), local.indexOf(".tl-version {"));
const rule = (selector: string) =>
  new RegExp(`(?:^|\\})\\s*${selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*\\{([^}]*)\\}`, "m").exec(local)?.[1] ?? "";

describe("PeekMark (L-052)", () => {
  it("is decorative: aria-hidden, not focusable, no title or role", () => {
    expect(mark).toMatch(/^<svg class="tl-mark" viewBox="0 0 128 128" aria-hidden="true" focusable="false">/);
    expect(mark).not.toMatch(/role=|<title|tabindex/i);
  });

  it("draws the body, the hill, two eyes with pupils and shines, two cheeks and the edge", () => {
    for (const [name, count] of [["body", 1], ["hill", 1], ["eye", 4], ["pupil", 2], ["cheek", 2], ["edge", 1]] as const) {
      expect([...mark.matchAll(new RegExp(`class="tl-mark-${name}"`, "g"))], name).toHaveLength(count);
    }
  });

  it("has no animation, no colour of its own and no raster", () => {
    const source = sources["./PeekMark.tsx"] ?? "";
    expect(source.length).toBeGreaterThan(500);
    expect(source).not.toMatch(/<animate|<set\b|animation|transition|<image|data:image|#[0-9A-Fa-f]{3,8}\b|rgba?\(|hsla?\(| fill=| stroke=/);
    expect(markCss).not.toMatch(/animation|transition/);
  });

  it("draws with Swiss Moss tokens only", () => {
    expect(markCss.length).toBeGreaterThan(300);
    expect(markCss).not.toMatch(/#[0-9A-Fa-f]{3,8}\b|rgba?\(/);
    expect(rule(".tl-mark-body")).toContain("var(--sm-moss-deep)");
    expect(rule(".tl-mark-hill")).toContain("var(--sm-moss)");
    expect(rule(".tl-mark-eye")).toContain("var(--sm-paper)");
    expect(rule(".tl-mark-pupil")).toContain("var(--sm-ink)");
    expect(rule(".tl-mark-cheek")).toMatch(/--sm-signal[\s\S]*--sm-danger/);
    for (const [, name] of markCss.matchAll(/var\((--[\w-]+)/g)) expect(name, name).toMatch(/^--(sm|tl)-/);
  });

  it("keeps its colours in a dark window: the picture sits in a light-pinned group, the edge outside it", () => {
    expect(mark).toContain('<g data-tokens="light">');
    expect(mark.indexOf("</g>")).toBeLessThan(mark.indexOf('class="tl-mark-edge"'));
    expect(mark).toContain("[data-tokens=light]");
  });

  it("uses a clip id that is unique per instance", () => {
    const two = renderToStaticMarkup(createElement("div", null, createElement(PeekMark), createElement(PeekMark)));
    const ids = [...two.matchAll(/<clipPath id="([^"]+)"/g)].map((m) => m[1]);
    expect(new Set(ids).size).toBe(2);
  });
});

describe("where the mark stands (L-052)", () => {
  it.each([
    ["./pages/InterruptPage.tsx", "the reminder's top corner, in both styles"],
    ["./App.tsx", "the settings sidebar header"],
    ["./pages/PanelPage.tsx", "the tray panel header"],
    ["./pages/FirstRunPage.tsx", "first run"],
  ])("%s draws the mark (%s)", (file) => {
    expect(sources[file]).toContain("<PeekMark />");
  });

  it("nothing imports Bell any more, and Bell.tsx is gone", () => {
    expect(Object.keys(sources)).not.toContain("./Bell.tsx");
    for (const [file, text] of Object.entries(sources)) expect(text, file).not.toMatch(/\bBell\b/);
  });

  it("on the reminder it sits in the countdown group, where the bell was, at the countdown's size", () => {
    const page = sources["./pages/InterruptPage.tsx"] ?? "";
    expect(page).toMatch(/<span className="tl-int-count">\s*<span>\{countdownText\(c, start, now\)\}<\/span>\s*<PeekMark \/>/);
    expect(rule(".tl-int-count .tl-mark")).toMatch(/width: calc\(38 \* var\(--tl-c\)\)/);
  });

  it("is mirrored by the layout in right to left: no physical side in its rules", () => {
    expect(markCss).not.toMatch(/\b(left|right)\b/);
  });
});

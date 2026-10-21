import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { UpdateDto } from "./api-types";
import { createCopy, LOCALES } from "./copy";
import { UpdateSection } from "./pages/AboutPage";
import { updateView } from "./update";

vi.mock("./api", () => ({
  api: {},
  onUpdateChanged: vi.fn(async () => () => undefined),
  asCommandError: () => ({ code: "internal" }),
}));

const offer: UpdateDto = { available: "0.2.0", installing: false };
const none: UpdateDto = { available: null, installing: false };

function section(update: UpdateDto | null, checked = false, error: string | null = null): string {
  const noop = () => undefined;
  return renderToStaticMarkup(
    createElement(UpdateSection, {
      update,
      checked,
      busy: false,
      error,
      onCheck: noop,
      onInstall: noop,
      onLater: noop,
      onSkip: noop,
    }),
  );
}

type Tree = { type: unknown; key: string | null; props: { children?: unknown } };
const isTree = (node: unknown): node is Tree => typeof node === "object" && node !== null && "props" in node;
const textOf = (node: unknown): string =>
  isTree(node) ? [node.props.children].flat().map(textOf).join("") : typeof node === "string" ? node : "";

/**
 * Where React keeps a button: the child index, element type and key at every level from the section down.
 * The same path in two renders means React updates that button in place, so its DOM node and focus stay.
 */
function pathTo(node: unknown, label: string, root = true): string[] | null {
  if (!isTree(node)) return null;
  if (node.type === "button" && textOf(node) === label) return ["button"];
  const children = [node.props.children].flat();
  for (const [i, child] of children.entries()) {
    const below = pathTo(child, label, false);
    if (below) {
      const step = [`${i}:${String(isTree(child) ? child.type : "")}:${isTree(child) ? child.key : ""}`, ...below];
      // The section's own type and key too: a new key there would remount everything under it.
      return root ? [`${String(node.type)}:${node.key}`, ...step] : step;
    }
  }
  return null;
}

function tree(update: UpdateDto | null, checked = false, busy = false, error: string | null = null): Tree {
  const noop = () => undefined;
  return UpdateSection({ update, checked, busy, error, onCheck: noop, onInstall: noop, onLater: noop, onSkip: noop }) as Tree;
}

describe("update prompt (F11)", () => {
  it("AC11.1 a status change keeps every button that stays on screen in place, so it keeps its node and focus", () => {
    // Check for updates: before and after a check that found nothing, while busy, and with an error.
    const check = [tree(null), tree(none), tree(none, true), tree(none, true, true), tree(none, true, false, "x")].map((t) =>
      pathTo(t, "Check for updates"),
    );
    expect(check[0]).not.toBeNull();
    for (const path of check) expect(path).toEqual(check[0]);
    // The offer's buttons: an update:changed with the same offer, busy, or with an error.
    for (const label of ["Install and restart", "Later", "Skip this version"]) {
      const paths = [tree(offer), tree(offer, true), tree(offer, false, true), tree(offer, false, false, "x")].map((t) =>
        pathTo(t, label),
      );
      expect(paths[0], label).not.toBeNull();
      for (const path of paths) expect(path, label).toEqual(paths[0]);
    }
  });

  it("AC11.1 offers the version Rust offers, shows the install under way, and says when a check found nothing", () => {
    expect(updateView(null, false)).toBe("idle");
    expect(updateView(none, false)).toBe("idle");
    expect(updateView(none, true)).toBe("current");
    expect(updateView(offer, false)).toBe("offer");
    expect(updateView(offer, true)).toBe("offer");
    expect(updateView({ available: "0.2.0", installing: true }, false)).toBe("installing");
  });

  it("AC11.1 asks before installing: Install and restart, Later and Skip this version, with the restart consequence", () => {
    const html = section(offer);
    for (const text of ["Tolon 0.2.0 is available.", "Install and restart", "Later", "Skip this version"]) {
      expect(html).toContain(text);
    }
    expect(html).toContain('aria-describedby="update-restart"');
    expect(html).toContain("Tolon closes for a moment to update.");
    expect(html).not.toContain("Check for updates");
    // The heading takes focus after Later, Skip or Install (AboutPage), so it must be focusable.
    expect(html).toContain('id="updates-heading" tabindex="-1"');
  });

  it("AC11.1 shows no buttons while installing, and Check for updates otherwise", () => {
    const installing = section({ available: "0.2.0", installing: true });
    expect(installing).toContain("Installing the update.");
    expect(installing).not.toContain("<button");
    expect(section(none)).toContain("Check for updates");
    expect(section(none, true)).toContain("There is no new version to install.");
    expect(section(none, true, "Tolon could not reach GitHub to check for updates. Try again later.")).toContain(
      'role="alert"',
    );
  });

  it("AC11.1 every locale names the version and Tolon, and the panel line names the Settings and About pages", () => {
    for (const locale of LOCALES) {
      const { t } = createCopy(locale);
      expect(t.settings.about.available("9.8.7"), locale).toContain("9.8.7");
      expect(t.settings.about.available("9.8.7"), locale).toContain("Tolon");
      const line = t.panel.update("9.8.7", t.nav.label, t.nav.about);
      for (const part of ["9.8.7", t.nav.label, t.nav.about]) expect(line, locale).toContain(part);
      expect(t.settings.about.version("9.8.7"), locale).toContain("9.8.7");
      expect(t.errors.command.update_check, locale).toContain("GitHub");
    }
  });
});

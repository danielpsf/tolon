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

describe("update prompt (F11)", () => {
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

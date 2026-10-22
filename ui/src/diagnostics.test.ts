// L-072: "Help us fix a problem" on the About page.
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { DiagnosticsPlanDto } from "./api-types";
import { createCopy, initCopy, LOCALES } from "./copy";
import { HelpSection, supportParts } from "./pages/AboutPage";

vi.mock("./api", () => ({
  api: {},
  onUpdateChanged: vi.fn(async () => () => undefined),
  asCommandError: () => ({ code: "internal" }),
}));

const MAILTO = "mailto:tolon@danielpsf.net?subject=Tolon%20diagnostics%200.1.0-beta.1&body=What%20happened%3A";

const plan = (file_manager: DiagnosticsPlanDto["file_manager"]): DiagnosticsPlanDto => ({
  file_manager,
  support_email: "tolon@danielpsf.net",
  support_url: MAILTO,
  issues_url: "https://github.com/danielpsf/tolon/issues",
  doc_url: "https://github.com/danielpsf/tolon/blob/v0.1.0-beta.1/docs/help/diagnostics.md",
});

function render(p: DiagnosticsPlanDto | null, saved: string | null, error: string | null = null): string {
  const noop = () => undefined;
  return renderToStaticMarkup(
    createElement(HelpSection, { plan: p, saved, busy: false, error, onSave: noop, onReveal: noop, onOpen: noop }),
  );
}

describe("l072 help section", () => {
  it("every locale names the support link exactly once and keeps the path", () => {
    for (const locale of LOCALES) {
      const h = createCopy(locale).t.settings.about.help;
      expect(h.sendTo.split("{support}").length, locale).toBe(2);
      expect(h.savedTo("/tmp/a.zip"), locale).toContain("/tmp/a.zip");
      for (const name of [h.heading, h.intro, h.save, h.issues, h.reveal.finder, h.reveal.explorer, h.reveal.files]) {
        expect(name, locale).not.toMatch(/!/);
      }
    }
  });

  it("splits the sentence around the link", () => {
    expect(supportParts("Send the file to {support} with a note.")).toEqual(["Send the file to ", " with a note."]);
    expect(supportParts("No link.")).toEqual(["No link.", ""]);
  });

  it("before a save: the button and where to send the file, as links that never leave the window", () => {
    initCopy("en-US");
    const html = render(plan("finder"), null);
    expect(html).toContain("Help us fix a problem");
    expect(html).toContain(">Save diagnostics</button>");
    expect(html).toContain(
      `Send it to <a href="${MAILTO.replace("&", "&amp;")}">tolon@danielpsf.net</a> with the file attached and a note on what happened.`,
    );
    expect(html).toContain('<a href="https://github.com/danielpsf/tolon/issues">Or open a public issue on GitHub</a>');
    expect(html).toContain("What the file holds, and what it never holds");
    expect(html).not.toContain("Saved to");
    expect(html).not.toContain("Show in");
  });

  it("after a save: where it went, and Show in the file manager of the OS", () => {
    initCopy("en-US");
    const path = "/Users/x/Downloads/Tolon-diagnostics.zip";
    expect(render(plan("finder"), path)).toContain(`Saved to ${path}`);
    expect(render(plan("finder"), path)).toContain(">Show in Finder</button>");
    expect(render(plan("explorer"), path)).toContain(">Show in Explorer</button>");
    expect(render(plan("files"), path)).toContain(">Show in Files</button>");
  });

  it("an error shows as an alert; without a plan the button waits", () => {
    initCopy("en-US");
    const html = render(null, null, "Tolon could not save the file there. Choose another folder.");
    expect(html).toContain('role="alert"');
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>Save diagnostics<\/button>/);
  });
});

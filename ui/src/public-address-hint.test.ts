import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { initCopy, LOCALES, createCopy } from "./copy";
import { PublicAddressHint } from "./pages/CalendarsPage";
import { isGooglePublicAddress, publicAddressHintParts } from "./view";

vi.mock("./api", () => ({ api: {}, asCommandError: () => ({ code: "internal" }) }));

const PUBLIC = "https://calendar.google.com/calendar/ical/abc%40example.com/public/basic.ics";
const PRIVATE = "https://calendar.google.com/calendar/ical/abc%40example.com/private-123abc/basic.ics";
const html = (url: string) => renderToStaticMarkup(createElement(PublicAddressHint, { url }));

describe("Google public address hint (S1.6)", () => {
  it("S1.6 shows the hint, with a link to the iCal help, for a public address, also as webcal", () => {
    initCopy("en-US");
    for (const url of [PUBLIC, PUBLIC.replace("https:", "webcal:"), `  ${PUBLIC} `]) {
      const out = html(url);
      expect(out).toContain("This is the public address, which only works for public calendars.");
      expect(out).toContain("Paste the Secret address in iCal format instead.");
      expect(out).toContain('<a href="#/help/ical-links/google-calendar">Help</a>.');
    }
  });

  it("S1.6 shows no hint for a private address, another host, an empty field or a half-typed address", () => {
    initCopy("en-US");
    for (const url of [PRIVATE, "https://example.com/public/basic.ics", "", "https://calendar.google.com/calendar/ical/abc/public/"]) {
      expect(html(url)).toBe("");
      expect(isGooglePublicAddress(url)).toBe(false);
    }
  });

  it.each(LOCALES)("S1.6 %s splits the hint around exactly one {help} slot", (locale) => {
    const c = createCopy(locale);
    expect(c.t.calendars.publicAddressHint.split("{help}")).toHaveLength(2);
    expect(publicAddressHintParts(c).join("")).not.toContain("{");
  });
});

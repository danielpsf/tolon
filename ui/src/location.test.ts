import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AlarmShowDto, MeetingDto } from "./api-types";
import { initCopy } from "./copy";
import { locationText } from "./interrupt";
import { InterruptView } from "./pages/InterruptPage";

vi.mock("./api", () => ({ api: {}, asCommandError: (e: unknown) => e }));

const meeting = (over: Partial<MeetingDto>): MeetingDto => ({
  key: "k1",
  title: "Platform weekly sync",
  start: "2026-10-20T14:00:00Z",
  end: "2026-10-20T14:45:00Z",
  join: { kind: "none" },
  people: 8,
  ...over,
});

const html = (m: MeetingDto) => {
  const alarm: AlarmShowDto = { test: false, meetings: [m] };
  return renderToStaticMarkup(createElement(InterruptView, { alarm, first: m, screen: "0", color: "theme", peek: false }));
};

beforeEach(() => initCopy("en-US"));

describe("location (S5.3)", () => {
  it("S5.3 shows the location on the interrupt details when there is no join link", () => {
    expect(html(meeting({ location: "Room 4.12" }))).toContain("<bdi>Room 4.12</bdi>");
    expect(html(meeting({ join: { kind: "event_page" }, location: "Room 4.12" }))).toContain("Room 4.12");
  });

  it("S5.3 shows the room next to a call link too, and nothing when it is blank or absent", () => {
    const link = { kind: "link", host: "meet.google.com", recognised: true } as const;
    expect(html(meeting({ join: link, location: "Room 4.12" }))).toContain("<bdi>Room 4.12</bdi>");
    for (const location of [null, undefined, "", "  "]) expect(html(meeting({ location }))).not.toContain("<bdi></bdi>");
    expect(locationText(meeting({ location: "  Room 4.12 " }))).toBe("Room 4.12");
    expect(locationText(meeting({ location: " " }))).toBeNull();
  });

  it("S5.3 reduces Google generated room names to the room and leaves other text alone", () => {
    const cases: [string, string][] = [
      ["Grover HQ-1-1P Mitte (8) [Google Meet, Zoom]", "1P Mitte"],
      ["Grover Office 3rd floor-3-Interstellar (8), Grover HQ-1-1P Mitte(8) [Zoom]", "Interstellar, 1P Mitte"],
      ["Grover HQ, Grover HQ-1-All Hands Area (100) [Zoom]", "All Hands Area"],
      ["San Francisco HQ-10-A-Beehive Room (4) [VC]", "Beehive Room"],
      ["Del Valle-1-Del Valle Theater (430)", "Del Valle Theater"],
      ["Torstrasse 1 (3), Berlin", "Torstrasse 1 (3), Berlin"],
      ["Conference Room 2B; Microsoft Teams Meeting", "Conference Room 2B"],
      ["Microsoft Teams", ""],
      ["Zoom-Room (4)", "Zoom-Room (4)"],
      ["Berlin-Mitte Office (2) Street 5", "Berlin-Mitte Office (2) Street 5"],
      ["Room 3 (3)", "Room 3 (3)"],
    ];
    for (const [raw, shown] of cases) expect(locationText({ location: raw }), raw).toBe(shown || null);
  });
});

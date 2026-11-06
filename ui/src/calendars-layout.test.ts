import { describe, expect, it } from "vitest";
import local from "./styles/local.css?raw";

// F3 (beta 1 feedback): a long sync error sentence took its max-content width in the `auto` status column, the
// name column shrank to nothing and `overflow-wrap: anywhere` drew the name one character per line.
describe("Calendar row layout (S1.1)", () => {
  it("S1.1 caps the status column so a long error wraps by word and the name keeps its room", () => {
    const status = local.match(/\.tl-status\s*\{([^}]*)\}/)?.[1] ?? "";
    expect(status).toMatch(/max-width:\s*\d+px/);
    expect(local).toMatch(/\.tl-row\s*\{[^}]*grid-template-columns:\s*auto minmax\(0, 1fr\) auto auto auto/);
  });
});

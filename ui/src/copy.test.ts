import { describe, expect, it } from "vitest";
import { copy } from "./copy";

describe("copy", () => {
  it("names the product Tolon in the window title", () => {
    expect(copy.windowTitle).toBe("Tolon");
  });
});

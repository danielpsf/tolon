import { afterEach, describe, expect, it, vi } from "vitest";
import { copy, initCopy } from "./copy";

afterEach(() => vi.unstubAllGlobals());

describe("copy", () => {
  it("names the product Tolon in the window title", () => {
    expect(initCopy("en-US").t.app.windowTitle).toBe("Tolon");
    expect(initCopy("pt-PT").t.app.windowTitle).toBe("Tolon");
  });

  it("follows the system language and sets <html lang>", () => {
    const documentElement = { lang: "" };
    vi.stubGlobal("document", { documentElement });
    vi.stubGlobal("navigator", { languages: ["fr-FR", "pt-PT"] });
    expect(initCopy().locale).toBe("pt-PT");
    expect(documentElement.lang).toBe("pt-PT");
    expect(copy().locale).toBe("pt-PT");
  });

  it("lets the override parameter win", () => {
    vi.stubGlobal("document", { documentElement: { lang: "" } });
    vi.stubGlobal("navigator", { languages: ["pt-PT"] });
    expect(initCopy("en-GB").locale).toBe("en-GB");
  });
});

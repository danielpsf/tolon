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

  it("sets <html dir> to rtl for Arabic and back to ltr for any other locale", () => {
    const documentElement = { lang: "", dir: "" };
    vi.stubGlobal("document", { documentElement });
    vi.stubGlobal("navigator", { languages: ["ar-EG"] });
    expect(initCopy().locale).toBe("ar");
    expect(documentElement).toEqual({ lang: "ar", dir: "rtl" });
    expect(initCopy("ja-JP").locale).toBe("ja");
    expect(documentElement).toEqual({ lang: "ja", dir: "ltr" });
  });

  it("lets the override parameter win", () => {
    vi.stubGlobal("document", { documentElement: { lang: "" } });
    vi.stubGlobal("navigator", { languages: ["pt-PT"] });
    expect(initCopy("en-GB").locale).toBe("en-GB");
  });
});

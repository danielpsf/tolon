import { describe, expect, it } from "vitest";
import fraunces from "./fraunces.css?raw";

// L-062: Peek's serif ships as woff2 only. WKWebView, WebView2 and WebKitGTK all load woff2, and the package's
// own CSS also pulls a .woff per face (about 135 KB in dist). The imports are read raw so a new one cannot
// bring the fallbacks back in a file no test renders.

const sources = import.meta.glob(["./*.tsx"], { query: "?raw", import: "default", eager: true }) as Record<string, string>;

describe("Fraunces (L-062)", () => {
  it("declares Latin and Latin Extended at 400, 600 and 700, each from one woff2 file", () => {
    const faces = [...fraunces.matchAll(/@font-face \{([^}]*)\}/g)].map((m) => m[1] ?? "");
    expect(faces).toHaveLength(6);
    const files = faces.map((f) => /fraunces-([\w-]+?)-(\d+)-normal\.woff2\) format\("woff2"\);/.exec(f)?.slice(1, 3).join(" "));
    expect(files).toEqual(["latin 400", "latin 600", "latin 700", "latin-ext 400", "latin-ext 600", "latin-ext 700"]);
    for (const face of faces) expect(face).toContain('font-family: "Fraunces";');
    expect(fraunces.replace(/\/\*[\s\S]*?\*\//g, "")).not.toMatch(/\.woff[^2]|format\("woff"\)/);
  });

  it("every window imports it once and never the package's CSS", () => {
    for (const file of ["./main.tsx", "./interrupt-main.tsx", "./panel-main.tsx"]) {
      expect(sources[file], file).toContain('import "./fraunces.css";');
      expect(sources[file], file).not.toContain("@fontsource/fraunces");
    }
  });
});

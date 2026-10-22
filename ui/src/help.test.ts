import { describe, expect, it } from "vitest";
import { docsHash, sections } from "virtual:help";
import { convertHelp, IMAGE_SCHEME } from "../help/convert";
import { readDocs } from "../help/docs.mjs";
import pkg from "../package.json";

// L-071: Settings, Help is the release's own docs, converted when the UI is built.
const exists = () => true;
const doc = (text: string, path = "install.md") => convertHelp([{ path, text }], exists).sections[0]?.html ?? "";

describe("help conversion", () => {
  it("rewrites a link between docs to the Help route, and an anchor to the section it is in", () => {
    const { sections: s } = convertHelp(
      [
        { path: "install.md", text: "# Install\n\n[iCal](help/ical-links.md) and [again](help/ical-links.md#outlook) and [below](#check-it)\n\n## Check it\n" },
        { path: "help/ical-links.md", text: "# Get a link\n\n## Outlook\n\n[back](../install.md#check-it)" },
      ],
      exists,
    );
    expect(s.map((x) => x.id)).toEqual(["install", "ical-links"]);
    expect(s[0]?.html).toContain(`<a href="#/help/ical-links">iCal</a>`);
    expect(s[0]?.html).toContain(`<a href="#/help/ical-links/outlook">again</a>`);
    expect(s[0]?.html).toContain(`<a href="#/help/install/check-it">below</a>`);
    expect(s[1]?.html).toContain(`<a href="#/help/install/check-it">back</a>`);
    expect(s[0]?.html).toContain(`<h3 id="help-check-it" tabindex="-1">Check it</h3>`);
  });

  it("makes a doc's own # an h2, since the page owns the h1, and takes the section title from it", () => {
    const { sections: s } = convertHelp([{ path: "install.md", text: "# Install `Tolon`\n" }], exists);
    expect(s[0]?.title).toBe("Install Tolon");
    expect(s[0]?.html).toContain("<h2 ");
    expect(s[0]?.html).not.toContain("<h1");
  });

  it("rewrites an image to a bundled one, relative to its own doc, and lists it once", () => {
    const seen: string[] = [];
    const { sections: s, images } = convertHelp(
      [
        { path: "install.md", text: "![A](images/a.png) ![A again](images/a.png)" },
        { path: "help/ical-links.md", text: "![B](images/b.png)" },
      ],
      (p) => (seen.push(p), true),
    );
    expect(images).toEqual(["images/a.png", "help/images/b.png"]);
    expect(s[0]?.html).toContain(`<img src="${IMAGE_SCHEME}0" alt="A">`);
    expect(s[0]?.html).toContain(`<img src="${IMAGE_SCHEME}0" alt="A again">`);
    expect(s[1]?.html).toContain(`<img src="${IMAGE_SCHEME}1" alt="B">`);
  });

  it("fails the build for an image that is missing or not a file, a doc that is not bundled, and a missing anchor", () => {
    expect(() => convertHelp([{ path: "install.md", text: "![x](images/x.png)" }], () => false)).toThrow(/does not exist/);
    expect(() => doc("![x](https://example.com/x.png)")).toThrow(/not a file/);
    expect(() => doc("[x](other.md)")).toThrow(/not a bundled doc/);
    expect(() => doc("[x](#nowhere)")).toThrow(/no heading/);
    expect(() => doc("![x](../../outside.png)")).toThrow(/leaves docs/);
  });

  it("escapes raw HTML: script tags never reach the page, and <version> stays as text", () => {
    const html = doc('<script>alert(1)</script>\n\nTolon <version> is <b onclick="x()">here</b> <img src=x onerror=y>\n\n<div>\nraw\n</div>');
    expect(html).not.toMatch(/<script|<b\b|<div>|onerror=y>/);
    expect(html).toContain("&lt;script&gt;alert(1)&lt;/script&gt;");
    expect(html).toContain("Tolon &lt;version&gt; is");
  });

  it("marks http and https links to open in the system browser and keeps the words of any other scheme", () => {
    const html = doc("[a](https://example.com/a?b=1&c=2) [b](http://example.com) [c](javascript:alert(1)) [d](mailto:x@y.z) [e](data:text/html,x) [f](//evil.test)");
    expect(html).toContain(`<a href="https://example.com/a?b=1&amp;c=2" data-external="">a</a>`);
    expect(html).toContain(`<a href="http://example.com" data-external="">b</a>`);
    expect(html).not.toMatch(/javascript:|mailto:|data:text|evil/);
    expect(html).toContain("c d e f");
  });

  it("wraps tables and makes code blocks reachable by keyboard", () => {
    const html = doc("| a | b |\n|---|---|\n| 1 | 2 |\n\n```\nls\n```\n");
    expect(html).toContain(`<div class="sm-table-wrap" tabindex="0"><table class="sm-table">`);
    expect(html).toContain(`<pre tabindex="0"><code>ls`);
  });

  it("puts install, the iCal guide and diagnostics first, then any other doc by name", () => {
    const text = "# T\n";
    const { sections: s } = convertHelp(
      ["help/zeta.md", "help/diagnostics.md", "help/ical-links.md", "install.md", "help/alpha.md"].map((path) => ({ path, text })),
      exists,
    );
    expect(s.map((x) => x.id)).toEqual(["install", "ical-links", "diagnostics", "alpha", "zeta"]);
  });
});

describe("the bundled help", () => {
  const docs = readDocs();

  it("has a section for install.md and for every docs/help/*.md", () => {
    expect(sections.map((s) => s.id).sort()).toEqual(
      docs.sources.map((s) => (s.path.split("/").pop() ?? "").replace(/\.md$/, "")).sort(),
    );
    expect(sections[0]?.id).toBe("install");
    expect(sections.find((s) => s.id === "ical-links")?.title).toBe("Get an iCal link for your calendar");
  });

  it("is not stale: the build's hash and text are those of the docs on disk now", () => {
    expect(docsHash).toBe(docs.hash);
    const fresh = convertHelp(docs.sources, docs.has).sections;
    const bare = (html: string) => html.replace(/src="[^"]*"/g, 'src=""');
    expect(sections.map((s) => [s.id, s.title, bare(s.html)])).toEqual(fresh.map((s) => [s.id, s.title, bare(s.html)]));
  });

  it("points every image at a bundled file, none at a scheme that needs the network or data:", () => {
    const urls = sections.flatMap((s) => [...s.html.matchAll(/<img src="([^"]*)"/g)].map((m) => m[1] ?? ""));
    expect(urls.length).toBeGreaterThan(0);
    for (const url of urls) expect(url, url).toMatch(/\.png$/);
    expect(urls.some((u) => u.includes(IMAGE_SCHEME))).toBe(false);
  });
});

describe("the not-public message's help link", () => {
  it("lands on a heading that exists in the bundled iCal guide (L-066)", () => {
    const guide = sections.find((s) => s.id === "ical-links");
    expect(guide?.html).toMatch(/<h\d id="help-google-calendar" tabindex="-1">Google Calendar<\/h\d>/);
  });
});

describe("no markdown parser ships", () => {
  it("is a devDependency only, and nothing under src/ imports it", () => {
    const runtime = pkg.dependencies as Record<string, string>;
    expect(runtime.marked).toBeUndefined();
    expect((pkg.devDependencies as Record<string, string>).marked).toBeDefined();
    const files = import.meta.glob("./**/*.{ts,tsx}", { query: "?raw", import: "default", eager: true }) as Record<string, string>;
    expect(Object.keys(files).length).toBeGreaterThan(20);
    for (const [name, source] of Object.entries(files)) expect(source, name).not.toMatch(/from\s+["']marked/);
  });
});

// Build time only (vite.config.ts): the release's own docs become the static HTML of Settings, Help. `marked` is a
// devDependency and is imported nowhere under src/, so no markdown parser ships (help.test.ts checks both).
// Raw HTML in a doc is escaped, never passed on; links and images are rewritten or the build fails.
import { Marked, type Renderer, type Tokens } from "marked";

/** A markdown file, its path relative to docs/ with forward slashes: `install.md`, `help/ical-links.md`. */
export type Source = { path: string; text: string };
export type Section = { id: string; title: string; html: string };

/** Images in the HTML are `tolon-img:<n>`, the n-th path of `images` (relative to docs/); the bundler swaps in its URL. */
export const IMAGE_SCHEME = "tolon-img:";

/** Section order in the Help list; other docs follow, sorted by id. */
const ORDER = ["install", "ical-links", "diagnostics"];

/** What markdown can produce here. Anything else in the output fails the build. */
const TAGS = new Set("p h2 h3 h4 h5 h6 ul ol li pre code em strong del a img table thead tbody tr th td blockquote hr br div".split(" "));

const esc = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&#39;");

/** GitHub's heading anchors, so a link that works on GitHub works here. */
export const slugOf = (text: string) =>
  text.toLowerCase().replace(/[^\p{L}\p{N}\s-]/gu, "").trim().replace(/\s/g, "-");

/** `help/../install.md` from `help/x.md` to `install.md`; leaving docs/ is an error. */
function resolve(from: string, href: string): string {
  const out = from.split("/").slice(0, -1);
  for (const part of href.split("/")) {
    if (part === "..") {
      if (!out.pop()) throw new Error(`${from}: "${href}" leaves docs/`);
    } else if (part !== "." && part !== "") out.push(part);
  }
  return out.join("/");
}

/** `install.md` to `install`, `help/ical-links.md` to `ical-links`. */
const idOf = (path: string) => (path.split("/").pop() ?? path).replace(/\.md$/, "");

export function convertHelp(
  sources: Source[],
  imageExists: (path: string) => boolean,
): { sections: Section[]; images: string[] } {
  const ids = new Map<string, string>();
  for (const { path } of sources) {
    const id = idOf(path);
    if (ids.has(path) || [...ids.values()].includes(id)) throw new Error(`${path}: the section id "${id}" is taken`);
    ids.set(path, id);
  }
  const images: string[] = [];
  const anchors = new Map<string, Set<string>>(); // section id to its heading anchors
  const jumps: { from: string; id: string; slug: string }[] = [];

  const sections = sources.map(({ path, text }): Section => {
    const id = ids.get(path) ?? path;
    const slugs = new Set<string>();
    anchors.set(id, slugs);
    let title = "";
    const link = (href: string, body: string, attrs = "") => `<a href="${esc(href)}"${attrs}>${body}</a>`;

    const marked = new Marked({
      renderer: {
        // Escaped, not dropped: `<version>` is plain text in install.md.
        html: (t: Tokens.HTML | Tokens.Tag) => (t.block ? `<p>${esc(t.text.trim())}</p>\n` : esc(t.text)),
        heading(this: Renderer, t: Tokens.Heading) {
          const plain = t.text.replace(/[`*_]/g, "");
          if (t.depth === 1 && !title) title = plain;
          let slug = slugOf(plain);
          for (let n = 2; slugs.has(slug); n++) slug = `${slugOf(plain)}-${n}`;
          slugs.add(slug);
          // The page owns the h1: a doc's `#` is an h2, and so on down.
          const level = Math.min(t.depth + 1, 6);
          return `<h${level} id="help-${esc(slug)}" tabindex="-1">${this.parser.parseInline(t.tokens)}</h${level}>\n`;
        },
        link(this: Renderer, t: Tokens.Link) {
          const body = this.parser.parseInline(t.tokens);
          const href = t.href;
          const scheme = /^([a-z][a-z0-9+.-]*):/i.exec(href)?.[1]?.toLowerCase();
          if (scheme === "http" || scheme === "https") return link(href, body, ` data-external=""`);
          // Any other scheme (javascript:, data:, file:, mailto:) keeps its words and loses the link.
          if (scheme || href.startsWith("//")) return body;
          const [file = "", frag = ""] = href.split("#");
          let target = id;
          if (file) {
            const to = resolve(path, file);
            const found = ids.get(to);
            if (!found) throw new Error(`${path}: "${href}" is not a bundled doc`);
            target = found;
          }
          if (frag) jumps.push({ from: path, id: target, slug: frag });
          return link(`#/help/${target}${frag ? `/${frag}` : ""}`, body);
        },
        image(t: Tokens.Image) {
          const href = t.href;
          if (/^([a-z][a-z0-9+.-]*:|\/\/)/i.test(href)) throw new Error(`${path}: image "${href}" is not a file in docs/`);
          const file = resolve(path, href);
          if (!imageExists(file)) throw new Error(`${path}: image "${href}" does not exist`);
          let n = images.indexOf(file);
          if (n < 0) n = images.push(file) - 1;
          return `<img src="${IMAGE_SCHEME}${n}" alt="${esc(t.text)}">`;
        },
      },
    });

    const html = (marked.parse(text, { async: false }) as string)
      // Scrollable regions can be reached with the keyboard.
      .replace(/<pre>/g, `<pre tabindex="0">`)
      .replace(/<table>/g, `<div class="sm-table-wrap" tabindex="0"><table class="sm-table">`)
      .replace(/<\/table>/g, "</table></div>");
    for (const [, tag] of html.matchAll(/<\/?([a-zA-Z][a-zA-Z0-9]*)/g)) {
      if (!TAGS.has((tag ?? "").toLowerCase())) throw new Error(`${path}: <${tag}> is not allowed in help HTML`);
    }
    return { id, title: title || id, html };
  });

  for (const { from, id, slug } of jumps) {
    if (!anchors.get(id)?.has(slug)) throw new Error(`${from}: no heading "#${slug}" in ${id}`);
  }
  const rank = (id: string) => (ORDER.includes(id) ? ORDER.indexOf(id) : ORDER.length);
  sections.sort((a, b) => rank(a.id) - rank(b.id) || a.id.localeCompare(b.id));
  return { sections, images };
}

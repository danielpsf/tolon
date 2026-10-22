// Node side of the Help build (vite.config.ts, the vitest staleness test and the e2e suite): reads the docs that
// become the Help page. Plain .mjs because ui/ has no Node typings; docs.d.mts types it.
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const DOCS = fileURLToPath(new URL("../../docs", import.meta.url));

/** `docs/install.md` and every `docs/help/*.md`, with a hash of exactly those texts. */
export function readDocs(docsDir = DOCS) {
  const helpDir = join(docsDir, "help");
  const names = existsSync(helpDir) ? readdirSync(helpDir).filter((n) => n.endsWith(".md")) : [];
  const paths = ["install.md", ...names.map((n) => `help/${n}`)].sort();
  const sources = paths.map((path) => ({ path, text: readFileSync(join(docsDir, path), "utf8") }));
  const hash = createHash("sha256");
  for (const { path, text } of sources) hash.update(`${path}\0${text}\0`);
  return {
    sources,
    hash: hash.digest("hex"),
    has: (path) => existsSync(join(docsDir, path)),
  };
}

/**
 * `import { sections, docsHash } from "virtual:help"`: every doc converted when the bundle is built, so the
 * release's help is the help of its own commit and cannot drift. `convert` is help/convert.ts's convertHelp.
 */
export function helpPlugin(convert) {
  const id = "virtual:help";
  return {
    name: "tolon-help",
    resolveId: (source) => (source === id ? `\0${id}` : undefined),
    load(loaded) {
      if (loaded !== `\0${id}`) return undefined;
      const docs = readDocs();
      for (const { path } of docs.sources) this.addWatchFile(join(DOCS, path));
      const { sections, images } = convert(docs.sources, docs.has);
      return [
        ...images.map((path, n) => `import img${n} from ${JSON.stringify(join(DOCS, path))};`),
        `const imgs = [${images.map((_, n) => `img${n}`).join(", ")}];`,
        `const swap = (html) => html.replace(/tolon-img:(\\d+)/g, (_, n) => imgs[n]);`,
        `export const sections = ${JSON.stringify(sections)}.map((s) => ({ ...s, html: swap(s.html) }));`,
        `export const docsHash = ${JSON.stringify(docs.hash)};`,
      ].join("\n");
    },
  };
}

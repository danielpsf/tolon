// Generated at build time by help/docs.mjs from docs/install.md and docs/help/*.md.
declare module "virtual:help" {
  /** In display order. `html` is sanitised, with images as bundled URLs. */
  export const sections: { id: string; title: string; html: string }[];
  /** Hash of the markdown the sections came from (readDocs in help/docs.mjs). */
  export const docsHash: string;
}

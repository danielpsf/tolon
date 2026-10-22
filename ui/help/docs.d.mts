import type { Plugin } from "vite";

type Source = { path: string; text: string };

export function readDocs(docsDir?: string): { sources: Source[]; hash: string; has: (path: string) => boolean };

export function helpPlugin(
  convert: (sources: Source[], imageExists: (path: string) => boolean) => { sections: unknown[]; images: string[] },
): Plugin;

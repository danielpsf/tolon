import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";
import { convertHelp } from "./help/convert.ts";
import { helpPlugin } from "./help/docs.mjs";

// Port must match build.devUrl in crates/tolon-app/tauri.conf.json.
export default defineConfig({
  base: "./",
  // helpPlugin: Settings, Help is the docs of this commit, converted at build time (help/convert.ts).
  plugins: [react(), helpPlugin(convertHelp)],
  clearScreen: false,
  // `..` lets the dev server serve the doc images in ../docs.
  server: { port: 1420, strictPort: true, fs: { allow: [".."] } },
  // Vitest empties CSS files by default; the contrast test reads local.css and the Swiss Moss tokens as text.
  test: { css: { include: [/\.css\?raw$/] } },
  // Three pages: the app, the interrupt window the app opens once per screen, and the tray panel.
  build: {
    rollupOptions: {
      input: { main: "index.html", interrupt: "interrupt.html", panel: "panel.html" },
    },
  },
});

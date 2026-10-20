import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// Port must match build.devUrl in crates/tolon-app/tauri.conf.json.
export default defineConfig({
  base: "./",
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  // Vitest empties CSS files by default; the contrast test reads local.css and the Swiss Moss tokens as text.
  test: { css: { include: [/\.css\?raw$/] } },
  // Two pages: the app, and the interrupt window the app opens once per screen.
  build: {
    rollupOptions: {
      input: { main: "index.html", interrupt: "interrupt.html" },
    },
  },
});

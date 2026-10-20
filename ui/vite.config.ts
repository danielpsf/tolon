import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Port must match build.devUrl in crates/tolon-app/tauri.conf.json.
export default defineConfig({
  base: "./",
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  // Two pages: the app, and the interrupt window the app opens once per screen.
  build: {
    rollupOptions: {
      input: { main: "index.html", interrupt: "interrupt.html" },
    },
  },
});

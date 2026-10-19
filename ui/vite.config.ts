import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Port must match build.devUrl in crates/tolon-app/tauri.conf.json.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
});

// The Tauri CLI only searches downward from the cwd for tauri.conf.json, and the
// Tauri crate lives in crates/tolon-app, so point it there. Works on every OS.
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const appDir = fileURLToPath(new URL("../../crates/tolon-app", import.meta.url));
const cli = fileURLToPath(new URL("../node_modules/@tauri-apps/cli/tauri.js", import.meta.url));

const result = spawnSync(process.execPath, [cli, ...process.argv.slice(2)], {
  stdio: "inherit",
  env: { ...process.env, TAURI_APP_PATH: appDir },
});
process.exit(result.status ?? 1);

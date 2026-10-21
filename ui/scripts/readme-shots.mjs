// Renders the README screenshots (docs/images/app-*.png) from the built UI: headless Chrome, a fake Tauri
// invoke, sample data only. Nothing opens on screen and no real calendar is read.
//   npm --prefix ui run readme-shots          (builds ui/ first; CHROME=<path> if Chrome is not found)
// No dependencies: it talks to Chrome over the DevTools protocol with Node's own WebSocket and fetch.
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import http from "node:http";
import { tmpdir } from "node:os";
import { extname, join, sep } from "node:path";
import process from "node:process";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const ui = fileURLToPath(new URL("../", import.meta.url));
const root = join(ui, "..");
const out = join(root, "docs", "images");
const MAX_BYTES = 300 * 1024;

// The clock the pages see: Thursday 22 October 2026, 09:58:00 in Berlin, so "Platform weekly sync" starts in 2 min.
const NOW = Date.parse("2026-10-22T07:58:00Z");
const at = (hhmm) => Date.parse(`2026-10-22T${hhmm}:00+02:00`);
const MEET = { kind: "link", host: "meet.google.com", recognised: true };

// Names never reach the UI (MeetingDto has a head count only): Priya, Tomas, Leah and 5 others make 8.
const SYNC = { title: "Platform weekly sync", start: at("10:00"), end: at("10:45"), join: MEET, people: 8 };
const TODAY = [
  { ...SYNC, key: "sync", all_day: false, ignored: null },
  { key: "design", title: "Design review", start: at("11:30"), end: at("12:15"), all_day: false, people: 5, ignored: null, join: { kind: "link", host: "zoom.us", recognised: true } },
  { key: "focus", title: "Focus time", start: at("13:00"), end: at("14:30"), all_day: false, people: 0, ignored: "keyword", join: { kind: "none" } },
  { key: "one", title: "1:1 with Priya", start: at("15:00"), end: at("15:30"), all_day: false, people: 2, ignored: null, join: { kind: "link", host: "teams.microsoft.com", recognised: true } },
].map((m) => ({ ...m, start: new Date(m.start).toISOString(), end: new Date(m.end).toISOString() }));

const mins = (n) => new Date(NOW - n * 60000).toISOString();
const CALENDARS = [
  { id: "c1", name: "Work", kind: "ics", enabled: true, display_url: "https://calendar.google.com", last_sync_at: mins(1), last_error: null },
  { id: "c2", name: "Personal", kind: "ics", enabled: true, display_url: "https://outlook.live.com", last_sync_at: mins(3), last_error: null },
  { id: "c3", name: "Team", kind: "ics", enabled: true, display_url: "https://calendar.example.com", last_sync_at: mins(4), last_error: null },
];

const SETTINGS = {
  lead_minutes: 2, again_at_start: false, sound: "bell", sound_repeat: true,
  quiet: { enabled: false, start: "18:00", end: "09:00", weekends: true },
  ignore: { all_day: true, declined: true, cancelled: true, no_attendees: false, keywords: ["Focus", "Lunch", "OOO"] },
  appearance: "light", interrupt_color: "theme", style: "swiss-moss", locale: "en-US", start_at_login: false, start_at_login_status: "disabled",
};

const interrupt = { url: "interrupt.html", window: "interrupt", width: 1280, height: 800, alarm: SYNC };
const SHOTS = [
  { name: "app-interrupt-light", ...interrupt, settings: { appearance: "light" } },
  { name: "app-interrupt-dark", ...interrupt, settings: { appearance: "dark" } },
  { name: "app-interrupt-peek-light", ...interrupt, settings: { appearance: "light", style: "peek" } },
  { name: "app-tray-panel", url: "panel.html", window: "panel", width: 420, height: 560, settings: { appearance: "light" } },
  { name: "app-settings-calendars", url: "index.html#/calendars", window: "main", width: 1040, height: 760, settings: { appearance: "light" }, calendars: CALENDARS },
  { name: "app-first-run", url: "index.html", window: "main", width: 1040, height: 760, settings: { appearance: "light" }, calendars: [] },
];

// Runs in the page before any script: a Tauri that answers from `state`, and a clock fixed at NOW.
function fake(state) {
  const RealDate = Date;
  const t0 = performance.now();
  const now = () => state.now + performance.now() - t0;
  globalThis.Date = class extends RealDate {
    constructor(...a) {
      super(...(a.length ? a : [now()]));
    }
    static now() {
      return now();
    }
  };
  let next = 1;
  globalThis.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => undefined };
  globalThis.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: state.window }, currentWebview: { label: state.window } },
    transformCallback: () => next++,
    unregisterCallback: () => undefined,
    convertFileSrc: (p) => p,
    invoke: async (cmd) => {
      switch (cmd) {
        case "settings_get": return state.settings;
        case "calendars_list": return state.calendars;
        case "app_tray_place": return "menu_bar";
        case "app_status": return { paused_until: null };
        case "alarms_current": return { test: false, meetings: [{ ...state.alarm, key: "k0", start: new RealDate(state.alarm.start).toISOString(), end: new RealDate(state.alarm.end).toISOString() }] };
        case "update_status": return { available: null, installing: false };
        case "occurrences_today": return state.today;
        case "plugin:event|listen": return next++;
        default: return null;
      }
    },
  };
}

function chromePath() {
  const found = [
    process.env.CHROME,
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
    "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
  ].find((p) => p && existsSync(p));
  if (!found) throw new Error("Chrome not found: set CHROME to its path");
  return found;
}

/** One DevTools connection to a page: `send(method, params)` resolves with the result. */
async function connect(url) {
  const ws = new WebSocket(url);
  await new Promise((ok, fail) => {
    ws.onopen = ok;
    ws.onerror = () => fail(new Error("DevTools connection failed"));
  });
  let id = 0;
  const waiting = new Map();
  ws.onmessage = ({ data }) => {
    const m = JSON.parse(data);
    const w = waiting.get(m.id);
    if (!w) return;
    waiting.delete(m.id);
    if (m.error) w.fail(new Error(`${w.method}: ${m.error.message}`));
    else w.ok(m.result);
  };
  return {
    send: (method, params = {}) =>
      new Promise((ok, fail) => {
        waiting.set(++id, { ok, fail, method });
        ws.send(JSON.stringify({ id, method, params }));
      }),
    close: () => ws.close(),
  };
}

// Lossless first, then 256 colours only when a PNG is still over the limit (Pillow through uv; pngquant if present).
const SHRINK = `
import sys
from PIL import Image
for p in sys.argv[2:]:
    im = Image.open(p).convert("RGB")
    im.save(p, optimize=True)
    import os
    if os.path.getsize(p) > int(sys.argv[1]):
        im.quantize(256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE).save(p, optimize=True)
`;

function shrink(files) {
  const quant = spawnSync("pngquant", ["--version"], { stdio: "ignore" });
  const run = quant.error
    ? spawnSync("uv", ["run", "--quiet", "--with", "pillow", "python", "-c", SHRINK, String(MAX_BYTES), ...files], { stdio: "inherit" })
    : spawnSync("pngquant", ["--force", "--ext", ".png", "--skip-if-larger", "--quality", "80-100", ...files], { stdio: "inherit" });
  if (run.status !== 0) throw new Error("shrinking the PNGs failed (needs pngquant, or uv with Pillow)");
}

if (!process.argv.includes("--no-build")) {
  const build = spawnSync("npm", ["--prefix", ui, "run", "build"], { stdio: "inherit", shell: process.platform === "win32" });
  if (build.status !== 0) process.exit(build.status ?? 1);
}
const dist = join(ui, "dist");
const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".woff2": "font/woff2", ".woff": "font/woff", ".svg": "image/svg+xml", ".png": "image/png" };
const server = http.createServer((req, res) => {
  const path = join(dist, decodeURIComponent(new URL(req.url, "http://x").pathname));
  if (!path.startsWith(dist + sep) || !existsSync(path) || !statSync(path).isFile()) return res.writeHead(404).end();
  res.writeHead(200, { "content-type": types[extname(path)] ?? "application/octet-stream" }).end(readFileSync(path));
});
await new Promise((ok) => server.listen(0, "127.0.0.1", ok));
const base = `http://127.0.0.1:${server.address().port}`;

const profile = mkdtempSync(join(tmpdir(), "tolon-shots-"));
const chrome = spawn(chromePath(), ["--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "--hide-scrollbars", "--force-color-profile=srgb", "--no-first-run", "--lang=en-US", "about:blank"], { stdio: "ignore" });
mkdirSync(out, { recursive: true });
const files = [];
try {
  const portFile = join(profile, "DevToolsActivePort");
  for (let i = 0; i < 100 && !existsSync(portFile); i++) await sleep(100);
  const port = readFileSync(portFile, "utf8").split("\n")[0];
  for (const shot of SHOTS) {
    const target = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })).json();
    const page = await connect(target.webSocketDebuggerUrl);
    const state = { now: NOW, window: shot.window, settings: { ...SETTINGS, ...shot.settings }, calendars: shot.calendars ?? [], alarm: shot.alarm, today: TODAY };
    await page.send("Page.enable");
    await page.send("Emulation.setDeviceMetricsOverride", { width: shot.width, height: shot.height, deviceScaleFactor: 1, mobile: false });
    await page.send("Emulation.setTimezoneOverride", { timezoneId: "Europe/Berlin" });
    await page.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: shot.settings.appearance }] });
    await page.send("Page.addScriptToEvaluateOnNewDocument", { source: `(${fake})(${JSON.stringify(state)})` });
    await page.send("Page.navigate", { url: `${base}/${shot.url}` });
    await sleep(1200);
    await page.send("Runtime.evaluate", { expression: "document.fonts.ready", awaitPromise: true });
    const { data } = await page.send("Page.captureScreenshot", { format: "png" });
    const file = join(out, `${shot.name}.png`);
    writeFileSync(file, Buffer.from(data, "base64"));
    files.push(file);
    page.close();
    await fetch(`http://127.0.0.1:${port}/json/close/${target.id}`);
  }
} finally {
  chrome.kill();
  server.close();
  await sleep(500);
  rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}
shrink(files);
for (const f of files) {
  const size = statSync(f).size;
  console.log(`${f.slice(root.length + 1)}  ${(size / 1024).toFixed(0)} KB`);
  if (size > MAX_BYTES) process.exitCode = 1;
}

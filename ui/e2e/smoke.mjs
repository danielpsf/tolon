// WebDriver smoke test: tauri-driver starts the debug app and selenium-webdriver drives it.
// Run with `npm --prefix ui run e2e` after `npm --prefix ui run tauri build -- --debug --no-bundle`.
import { spawn } from "node:child_process";
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { join } from "node:path";
import process from "node:process";
import { after, before, test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { Builder } from "selenium-webdriver";

const root = fileURLToPath(new URL("../../", import.meta.url));
const application = join(root, "target", "debug", process.platform === "win32" ? "tolon-app.exe" : "tolon-app");
const driverUrl = "http://127.0.0.1:4444";

// On Windows tauri-driver needs msedgedriver; GitHub runners ship it under EDGEWEBDRIVER, not on PATH.
function driverArgs() {
  const dir = process.env.EDGEWEBDRIVER;
  if (process.platform !== "win32" || !dir) return [];
  return ["--native-driver", join(dir, "msedgedriver.exe")];
}

async function waitForDriver(child) {
  for (let i = 0; i < 100; i++) {
    if (child.exitCode !== null) throw new Error(`tauri-driver exited with code ${child.exitCode}`);
    try {
      if ((await fetch(`${driverUrl}/status`)).ok) return;
    } catch {
      // not listening yet
    }
    await sleep(100);
  }
  throw new Error("tauri-driver did not start listening on port 4444");
}

let tauriDriver;
let driver;

before(async () => {
  assert.ok(existsSync(application), `debug app not built: ${application}`);
  tauriDriver = spawn("tauri-driver", driverArgs(), { stdio: "inherit" });
  tauriDriver.on("error", (err) => {
    throw err;
  });
  await waitForDriver(tauriDriver);
  driver = await new Builder()
    .usingServer(driverUrl)
    .forBrowser("wry")
    .withCapabilities({ "tauri:options": { application } })
    .build();
});

after(async () => {
  await driver?.quit();
  tauriDriver?.kill();
});

test("the app boots and shows the paper background", async () => {
  assert.equal((await driver.getAllWindowHandles()).length, 1);
  assert.equal(await driver.getTitle(), "Tolon");

  const { actual, expected, family, inter } = await driver.executeAsyncScript(async (done) => {
    const probe = document.createElement("div");
    probe.style.background = "var(--sm-paper)";
    document.body.append(probe);
    const expected = getComputedStyle(probe).backgroundColor;
    probe.remove();
    const loaded = await document.fonts.load("16px Inter");
    done({
      actual: getComputedStyle(document.body).backgroundColor,
      expected,
      family: getComputedStyle(document.body).fontFamily,
      inter: loaded.length,
    });
  });

  assert.notEqual(expected, "rgba(0, 0, 0, 0)", "--sm-paper is not defined");
  assert.equal(actual, expected);
  assert.match(family, /^Inter\b/);
  assert.ok(inter > 0, "the bundled Inter font face did not load");
});

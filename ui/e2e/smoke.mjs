// WebDriver smoke test: tauri-driver starts the debug app and selenium-webdriver drives it.
// Run with `npm --prefix ui run e2e` after `npm --prefix ui run tauri build -- --debug --no-bundle`.
import { spawn } from "node:child_process";
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import net from "node:net";
import { join } from "node:path";
import process from "node:process";
import { after, before, test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { Builder, By, Key, until } from "selenium-webdriver";

const root = fileURLToPath(new URL("../../", import.meta.url));
const application = join(root, "target", "debug", process.platform === "win32" ? "tolon-app.exe" : "tolon-app");
const driverUrl = "http://127.0.0.1:4444";

// tauri-driver finds msedgedriver.exe on PATH (CI installs the one matching WebView2); TOLON_NATIVE_DRIVER overrides it.
function driverArgs() {
  const native = process.env.TOLON_NATIVE_DRIVER;
  console.log(`native driver: ${native ?? "tauri-driver default (PATH lookup)"}`);
  return native ? ["--native-driver", native] : [];
}

async function waitForDriver(child) {
  for (let i = 0; i < 100; i++) {
    if (spawnError) throw spawnError;
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
let spawnError;
let driver;

before(async () => {
  assert.ok(existsSync(application), `debug app not built: ${application}`);
  tauriDriver = spawn("tauri-driver", driverArgs(), { stdio: "inherit" });
  tauriDriver.on("error", (err) => {
    spawnError = err;
  });
  await waitForDriver(tauriDriver);
  driver = await new Builder()
    .usingServer(driverUrl)
    .withCapabilities({ browserName: "wry", "tauri:options": { application } })
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

// ---- Calendars page (L-008) -------------------------------------------------------------------
// Production only fetches https, and a test cannot hand the app a certificate it trusts, so no test here
// can make a fetch succeed: S1.1 (a feed that syncs) needs a test fetch path in tolon-app. What can run
// is the failure path, which touches the same commands: a closed local port refuses the connection at once
// and needs no network. The 404 wording of S1.3 is covered by the Rust test s1_3 and the Vitest mapping.

const WAIT = 15_000;
const english = "Tolon could not connect to the server.";

/** A loopback port nobody listens on, so a fetch to it is refused without leaving the machine. */
async function closedPort() {
  const server = net.createServer();
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address();
  await new Promise((resolve) => server.close(resolve));
  return port;
}

const find = (css) => driver.wait(until.elementLocated(By.css(css)), WAIT);

/** The row whose detail line shows this loopback port, or false while it is not listed. */
async function rowFor(port) {
  for (const row of await driver.findElements(By.css(".tl-row"))) {
    if ((await row.getText()).includes(`:${port}/`)) return row;
  }
  return false;
}
async function waitForRow(port) {
  try {
    return await driver.wait(() => rowFor(port), WAIT);
  } catch (e) {
    const page = await driver.findElement(By.css("body")).getText();
    throw new Error(`no row for port ${port}; the page shows: ${JSON.stringify(page)}`, { cause: e });
  }
}

async function openAddForm() {
  await (await find('button[aria-controls="add-calendar"]')).click();
  return find("#calendar-url");
}

let port;

test("s1_3 unreachable link is added with its status", async () => {
  port = await closedPort();
  const input = await openAddForm();
  await input.sendKeys(`https://127.0.0.1:${port}/calendar/private-123/basic.ics`);
  await (await find('#add-calendar button[type="submit"]')).click();

  const row = await waitForRow(port);
  // The calendar stays listed so the person can fix the link (AC1.2), with the sentence for its error kind.
  assert.equal(await row.findElement(By.css(".tl-status.failed")).getText(), `Sync failed: ${english}`);
  assert.equal(await row.findElement(By.css(".tl-name")).getText(), "127.0.0.1, iCal subscription");
  // The row shows host and last segment only; the secret in the middle of the path never reaches the page.
  assert.equal(await row.findElement(By.css(".sm-caption")).getText(), `https://127.0.0.1:${port}/.../basic.ics`);
  assert.ok(!(await driver.findElement(By.css("body")).getText()).includes("private-123"));
  assert.equal((await driver.findElements(By.css("#add-calendar"))).length, 0, "the form closes after a successful add");
});

test("a calendar switch turns the calendar off and on", async () => {
  const row = await waitForRow(port);
  const toggle = await row.findElement(By.css('input[role="switch"]'));
  assert.equal(await toggle.getAttribute("aria-label"), "Enable calendar 127.0.0.1");
  assert.equal(await toggle.isSelected(), true);
  await toggle.click();
  await driver.wait(async () => !(await (await waitForRow(port)).findElement(By.css('input[role="switch"]')).isSelected()), WAIT);
  await (await waitForRow(port)).findElement(By.css('input[role="switch"]')).click();
  await driver.wait(async () => (await waitForRow(port)).findElement(By.css('input[role="switch"]')).isSelected(), WAIT);
});

test("a link Tolon cannot add shows its sentence and adds no row", async () => {
  const before = (await driver.findElements(By.css(".tl-row"))).length;
  const input = await openAddForm();
  await input.sendKeys("ftp://example.com/team.ics", Key.ENTER); // Enter submits: the form is usable from the keyboard
  const alert = await find("#add-calendar .sm-error");
  assert.equal(await alert.getText(), "Tolon can only add https or webcal links.");
  assert.equal((await driver.findElements(By.css(".tl-row"))).length, before);
  await (await find('#add-calendar button[type="button"]')).click(); // Cancel
  await driver.wait(async () => (await driver.findElements(By.css("#add-calendar"))).length === 0, WAIT);
});

test("removing a calendar asks first and then removes it", async () => {
  const row = await waitForRow(port);
  await row.findElement(By.css(".tl-remove button")).click();
  const dialog = await find("dialog[open]");
  assert.match(await dialog.getText(), /Tolon stops reading this calendar\. The calendar itself is not changed\./);

  const cancel = await dialog.findElement(By.css(".sm-dialog-actions button"));
  await cancel.click();
  await driver.wait(async () => (await driver.findElements(By.css("dialog[open]"))).length === 0, WAIT);
  assert.ok(await rowFor(port), "Cancel keeps the calendar");

  await (await waitForRow(port)).findElement(By.css(".tl-remove button")).click();
  await (await find("dialog[open] .sm-btn.danger")).click();
  await driver.wait(async () => !(await rowFor(port)), WAIT, "the calendar is still listed");
});

// WebDriver smoke test: tauri-driver starts the debug app and selenium-webdriver drives it.
// Run with `npm --prefix ui run e2e` after `npm --prefix ui run tauri build -- --debug --no-bundle`.
import { spawn } from "node:child_process";
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import net from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import process from "node:process";
import { after, before, test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { Builder, By, error, Key, until } from "selenium-webdriver";

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
// Linux: the app keeps tolon.db under XDG_DATA_HOME, so a temp dir gives every run an empty calendar list.
// Windows keeps it under %APPDATA% and is left alone: the tests remove what they add.
const dataHome = mkdtempSync(join(tmpdir(), "tolon-e2e-"));

before(async () => {
  assert.ok(existsSync(application), `debug app not built: ${application}`);
  tauriDriver = spawn("tauri-driver", driverArgs(), {
    stdio: "inherit",
    env: { ...process.env, XDG_DATA_HOME: dataHome },
  });
  tauriDriver.on("error", (err) => {
    spawnError = err;
  });
  await waitForDriver(tauriDriver);
  driver = await new Builder()
    .usingServer(driverUrl)
    .withCapabilities({ browserName: "wry", "tauri:options": { application } })
    .build();
  // The language follows the system by default; the `lang` query parameter pins it so the copy below is English.
  const url = new URL(await driver.getCurrentUrl());
  url.search = "?lang=en-US";
  await driver.get(url.href);
});

after(async () => {
  await driver?.quit();
  tauriDriver?.kill();
  rmSync(dataHome, { recursive: true, force: true });
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

/** The row of the calendar these tests add (the host is the name), or false while it is not listed. */
async function rowForTestCalendar() {
  for (const row of await driver.findElements(By.css(".tl-row"))) {
    try {
      if ((await row.findElement(By.css(".tl-name")).getText()).startsWith("127.0.0.1")) return row;
    } catch (e) {
      // The row was removed between the lookup and the read: it is not listed any more.
      if (!(e instanceof error.StaleElementReferenceError)) throw e;
    }
  }
  return false;
}
async function waitForRow() {
  try {
    return await driver.wait(rowForTestCalendar, WAIT);
  } catch (e) {
    const page = await driver.findElement(By.css("body")).getText();
    throw new Error(`no row for the test calendar; the page shows: ${JSON.stringify(page)}`, { cause: e });
  }
}

/** What the page looks like now, for a failure message: Windows cannot be debugged from here. */
function snapshot() {
  return driver.executeScript(() => ({
    openDialogs: [...document.querySelectorAll("dialog[open]")].map((d) => d.innerText),
    active: `${document.activeElement?.tagName}:${document.activeElement?.textContent}`,
    rows: [...document.querySelectorAll(".tl-row .tl-name")].map((n) => n.textContent),
    body: document.body.innerText,
  }));
}

/** `driver.wait` that says which step timed out and what the page showed. */
async function waitFor(step, condition) {
  try {
    return await driver.wait(condition, WAIT);
  } catch (e) {
    throw new Error(`timed out waiting for: ${step}; page: ${JSON.stringify(await snapshot())}`, { cause: e });
  }
}

async function openAddForm() {
  await (await find('button[aria-controls="add-calendar"]')).click();
  return find("#calendar-url");
}

test("s1_3 unreachable link is added with its status", async () => {
  const port = await closedPort();
  const input = await openAddForm();
  await input.sendKeys(`https://127.0.0.1:${port}/calendar/private-123/basic.ics`);
  await (await find('#add-calendar button[type="submit"]')).click();

  const row = await waitForRow();
  // The calendar stays listed so the person can fix the link (AC1.2), with the sentence for its error kind.
  assert.equal(await row.findElement(By.css(".tl-status.failed")).getText(), `Sync failed: ${english}`);
  assert.equal(await row.findElement(By.css(".tl-name")).getText(), "127.0.0.1, iCal subscription");
  // The row shows scheme and host only (AC1.3); the path, where a private feed keeps its token, never reaches the page.
  assert.equal(await row.findElement(By.css(".sm-caption")).getText(), "https://127.0.0.1");
  assert.ok(!(await driver.findElement(By.css("body")).getText()).includes("private-123"));
  assert.equal((await driver.findElements(By.css("#add-calendar"))).length, 0, "the form closes after a successful add");
});

test("a calendar switch turns the calendar off and on", async () => {
  const row = await waitForRow();
  const toggle = await row.findElement(By.css('input[role="switch"]'));
  assert.equal(await toggle.getAttribute("aria-label"), "Enable calendar 127.0.0.1");
  assert.equal(await toggle.isSelected(), true);
  await toggle.click();
  await driver.wait(async () => !(await (await waitForRow()).findElement(By.css('input[role="switch"]')).isSelected()), WAIT);
  await (await waitForRow()).findElement(By.css('input[role="switch"]')).click();
  await driver.wait(async () => (await waitForRow()).findElement(By.css('input[role="switch"]')).isSelected(), WAIT);
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
  const row = await waitForRow();
  await row.findElement(By.css(".tl-remove button")).click();
  const dialog = await find("dialog[open]");
  assert.match(await dialog.getText(), /Tolon stops reading this calendar\. The calendar itself is not changed\./);

  const cancel = await dialog.findElement(By.css(".sm-dialog-actions button"));
  await cancel.click();
  await waitFor("the dialog to close after Cancel", async () => (await driver.findElements(By.css("dialog[open]"))).length === 0);
  assert.ok(await rowForTestCalendar(), "Cancel keeps the calendar");

  await (await waitForRow()).findElement(By.css(".tl-remove button")).click();
  await (await find("dialog[open] .sm-btn.danger")).click();
  await waitFor("the calendar to leave the list", async () => !(await rowForTestCalendar()));
  await waitFor("the dialog to close after Remove calendar", async () => (await driver.findElements(By.css("dialog[open]"))).length === 0);
  // The dialog's own Remove button is gone by now, so focus lands on Add calendar, not on the page body.
  await waitFor("focus on Add calendar", async () => (await driver.executeScript("return document.activeElement?.textContent ?? \"\"")) === "Add calendar");
});

test("the app keeps its data in the temp dir", { skip: process.platform !== "linux" }, () => {
  assert.ok(existsSync(join(dataHome, "dev.tolon.desktop", "tolon.db")), "tolon.db is not under XDG_DATA_HOME");
});

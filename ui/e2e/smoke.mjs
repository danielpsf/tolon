// WebDriver smoke test: tauri-driver starts the debug app and selenium-webdriver drives it.
// Run with `npm --prefix ui run e2e` after `npm --prefix ui run tauri build -- --debug --no-bundle`.
import { spawn } from "node:child_process";
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import http from "node:http";
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
// Production only fetches https. A feed that syncs (S1.1) needs the e2e fetch seam: ci.yml builds
// the debug app with it, which lets the app fetch http from a loopback host and from nothing
// else. The failure path (S1.3) needs no feature: a closed local port refuses the connection at once.
// The 404 wording of S1.3 is covered by the Rust test s1_3 and the Vitest mapping.

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

// ---- S1.1 with a fixture feed (L-037) ---------------------------------------------------------

const pad = (n) => String(n).padStart(2, "0");
const now = new Date();
const day = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}`;
// The scheduler is live: an event close to the real time would open an interrupt in the middle of the
// calendar tests. 03:00 or 15:00 today, whichever is farther from now (floating time, the machine's zone).
const fixtureHour = Math.abs(now.getHours() - 3) > Math.abs(now.getHours() - 15) ? "03" : "15";
/**
 * A feed named by X-WR-CALNAME with one event today (floating time reads in the machine's zone). The event is not
 * asserted yet: the tray panel scenario (S1.1, "today's events appear in the tray panel") will read it.
 */
const FIXTURE_FEED = [
  "BEGIN:VCALENDAR",
  "VERSION:2.0",
  "PRODID:-//Tolon e2e//EN",
  "X-WR-CALNAME:Team fixture",
  "BEGIN:VEVENT",
  "UID:e2e-standup@example.com",
  `DTSTAMP:${day}T000000Z`,
  `DTSTART:${day}T${fixtureHour}0000`,
  `DTEND:${day}T${fixtureHour}3000`,
  "SUMMARY:Team standup",
  "END:VEVENT",
  "END:VCALENDAR",
  "",
].join("\r\n");

/** Serves the fixture feed on 127.0.0.1 and a random port; `requests` lists the paths asked for. */
async function fixtureServer(feed = FIXTURE_FEED) {
  const requests = [];
  const server = http.createServer((req, res) => {
    requests.push(req.url);
    res.writeHead(200, { "Content-Type": "text/calendar; charset=utf-8" });
    res.end(feed);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return { server, requests, port: server.address().port };
}

const calendarRows = (name) =>
  driver.findElements(By.xpath(`//*[contains(@class, "tl-name") and starts-with(., "${name}")]/ancestor::*[contains(@class, "tl-row")][1]`));
const fixtureRows = () => calendarRows("Team fixture");

/** Leaves nothing behind (Windows keeps its data outside the temp dir), also when an assertion failed. */
async function removeCalendar(name) {
  try {
    const [row] = await calendarRows(name);
    if (!row) return;
    await row.findElement(By.css(".tl-remove button")).click();
    await (await find("dialog[open] .sm-btn.danger")).click();
    await waitFor(`the ${name} calendar to leave the list`, async () => (await calendarRows(name)).length === 0);
  } catch (e) {
    // The test's own failure is the one to report, not a failed cleanup after it.
    console.warn(`could not remove the ${name} calendar: ${e.message}`);
  }
}
const removeFixtureCalendar = () => removeCalendar("Team fixture");

test("s1_1 a valid feed is added, named from X-WR-CALNAME and synced", async () => {
  const { server, requests, port } = await fixtureServer();
  try {
    const input = await openAddForm();
    await input.sendKeys(`http://127.0.0.1:${port}/calendar/private-123/basic.ics`);
    await (await find('#add-calendar button[type="submit"]')).click();

    const row = await waitFor("a row named from the feed", async () => (await fixtureRows())[0]);
    // The name carries the kind after a comma, as in the s1_3 row ("127.0.0.1, iCal subscription").
    assert.equal(await row.findElement(By.css(".tl-name")).getText(), "Team fixture, iCal subscription");
    assert.equal(await row.findElement(By.css(".tl-status")).getText(), "Synced just now");
    assert.equal((await row.findElements(By.css(".tl-status.failed"))).length, 0);
    // The row shows scheme and host only (AC1.3); the token in the path never reaches the page.
    assert.equal(await row.findElement(By.css(".sm-caption")).getText(), "http://127.0.0.1");
    assert.ok(!(await driver.findElement(By.css("body")).getText()).includes("private-123"));
    assert.deepEqual(requests, ["/calendar/private-123/basic.ics"], "one fetch, made by the add");
  } finally {
    await removeFixtureCalendar();
    server.closeAllConnections(); // reqwest keeps the connection alive; close() alone would wait for it
    await new Promise((resolve) => server.close(resolve));
  }
});

// ---- Interrupt window (L-010) -----------------------------------------------------------------
// `alarms_test` opens the sample interrupt on every screen (one here). The windows are separate
// WebDriver handles: the test switches to one, checks focus, presses a key and expects every
// interrupt window to close (AC4.6). The copy is left alone because it follows the system language.

const interruptHandles = async (main) => (await driver.getAllWindowHandles()).filter((h) => h !== main);

async function openTestInterrupt(main) {
  const failure = await driver.executeAsyncScript((done) => {
    window.__TAURI_INTERNALS__.invoke("alarms_test").then(
      () => done(null),
      (e) => done(JSON.stringify(e)),
    );
  });
  assert.equal(failure, null, "alarms_test failed");
  const handles = await waitFor("an interrupt window", async () => {
    const found = await interruptHandles(main);
    return found.length > 0 && found;
  });
  await driver.switchTo().window(handles[0]);
  return handles;
}

// [name, keys sent to the focused element one after the other]. Which action each one ran is proven by the
// alarm state in the s3_1 tests below and by the Rust tests; the test alarm has no alarm behind it.
const KEY_CASES = [
  ["Esc", [Key.ESCAPE]],
  ["D", ["d"]],
  ["Space", [Key.SPACE]],
  ["Enter", [Key.ENTER]],
  // Space on Snooze or Dismiss presses that button; it must not mark the meeting joined (AC4.4).
  ["Tab, Space on Snooze", [Key.TAB, Key.SPACE]],
  ["Tab, Tab, Space on Dismiss", [Key.TAB, Key.TAB, Key.SPACE]],
];

for (const [name, keys] of KEY_CASES) {
  test(`s4_3 ${name} on the interrupt closes every interrupt window (AC4.4, AC4.6, S4.2)`, async () => {
    const main = await driver.getWindowHandle();
    try {
      const handles = await openTestInterrupt(main);
      // One window per screen: xvfb has one. Each is told its screen in the URL (docs/architecture.md, Scheduler).
      assert.equal(handles.length, 1);
      assert.match(await driver.getCurrentUrl(), /interrupt\.html\?screen=\d+/);

      // The primary button has focus on open (S4.3). The sample link is not a known provider, so its host is on the button (L-013).
      const primary = await waitFor("focus on the primary button", async () => {
        const active = await driver.switchTo().activeElement();
        return (await active.getAttribute("data-action")) === "join" && active;
      });
      assert.match(await primary.getText(), /example\.com/);

      // A screen reader announces the dialog by its title and details.
      const dialog = await driver.executeScript(() => {
        const main = document.querySelector("main");
        const named = (attribute) => document.getElementById(main.getAttribute(attribute));
        return {
          role: main.getAttribute("role"),
          label: named("aria-labelledby")?.tagName,
          description: named("aria-describedby")?.textContent.length > 0,
        };
      });
      assert.deepEqual(dialog, { role: "alertdialog", label: "H1", description: true });

      try {
        for (const key of keys) await (await driver.switchTo().activeElement()).sendKeys(key);
      } catch (e) {
        // The last key can close the window before the driver has its answer; the wait below is the check.
        if (!(e instanceof error.NoSuchWindowError)) throw e;
      }
      await waitFor("every interrupt window to close", async () => (await interruptHandles(main)).length === 0);
    } finally {
      // The current window may be gone: go back to the main one for the next test.
      await driver.switchTo().window(main);
    }
    assert.equal((await driver.getAllWindowHandles()).length, 1);
  });
}

// ---- The scheduler end to end (L-032) ---------------------------------------------------------
// A feed with a meeting that starts in 90 s, inside the default 2-minute lead time: the sync wakes the
// scheduler, which plans the alarm, fires it and opens the interrupt. The state is read from the store
// through `alarms_e2e_state`, a command that only the e2e build (the fetch seam above) contains.

const stamp = (date) => date.toISOString().replace(/[-:]/g, "").replace(/\.\d{3}/, "");

function alarmFeed(uid, title, start) {
  return [
    "BEGIN:VCALENDAR",
    "VERSION:2.0",
    "PRODID:-//Tolon e2e//EN",
    "X-WR-CALNAME:Alarm fixture",
    "BEGIN:VEVENT",
    `UID:${uid}@example.com`,
    `DTSTAMP:${stamp(new Date())}`,
    `DTSTART:${stamp(start)}`,
    `DTEND:${stamp(new Date(start.getTime() + 30 * 60_000))}`,
    `SUMMARY:${title}`,
    "END:VEVENT",
    "END:VCALENDAR",
    "",
  ].join("\r\n");
}

/** The alarm rows, asked from the main window. */
const alarmRows = () =>
  driver.executeAsyncScript((done) => {
    window.__TAURI_INTERNALS__.invoke("alarms_e2e_state").then(done, (e) => done(JSON.stringify(e)));
  });

/** Adds the feed, waits for its interrupt, runs `body` on it, then closes up. `body` gets the main handle. */
async function withImminentMeeting(uid, title, body) {
  const { server, port } = await fixtureServer(alarmFeed(uid, title, new Date(Date.now() + 90_000)));
  const main = await driver.getWindowHandle();
  try {
    const input = await openAddForm();
    await input.sendKeys(`http://127.0.0.1:${port}/calendar/alarm-${uid}/basic.ics`);
    await (await find('#add-calendar button[type="submit"]')).click();
    const handles = await waitFor("the interrupt of the meeting", async () => {
      const found = await interruptHandles(main);
      return found.length > 0 && found;
    });
    await driver.switchTo().window(handles[0]);
    assert.equal(await (await find(".tl-int-title")).getText(), title);
    await body(main);
  } finally {
    await driver.switchTo().window(main).catch(() => undefined);
    await removeCalendar("Alarm fixture");
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

async function pressAndWaitForClose(main, key) {
  try {
    await (await driver.switchTo().activeElement()).sendKeys(key);
  } catch (e) {
    // The key can close the window before the driver has its answer; the wait below is the check.
    if (!(e instanceof error.NoSuchWindowError)) throw e;
  }
  await waitFor("every interrupt window to close", async () => (await interruptHandles(main)).length === 0);
  await driver.switchTo().window(main);
}

const leadOf = async (uid) => (await alarmRows()).find((r) => r.key.includes(`:${uid}@`) && r.kind === "lead");

test("s3_1 a meeting inside its lead time opens the interrupt at once, and s3_2 Esc snoozes its alarm for 5 minutes", async () => {
  await withImminentMeeting("e2e-snooze", "Snooze meeting", async (main) => {
    // The window is up, but the store is the proof that the alarm itself is firing.
    await driver.switchTo().window(main);
    assert.equal((await leadOf("e2e-snooze")).state, "firing");
    await driver.switchTo().window((await interruptHandles(main))[0]);
    await pressAndWaitForClose(main, Key.ESCAPE);
    const lead = await leadOf("e2e-snooze");
    assert.equal(lead.state, "snoozed");
    const ahead = Date.parse(lead.until) - Date.now();
    assert.ok(ahead > 4 * 60_000 && ahead <= 5 * 60_000 + 5_000, `snoozed until ${lead.until}, ${ahead} ms ahead`);
  });
});

test("s3_3 D dismisses the alarm for good", async () => {
  await withImminentMeeting("e2e-dismiss", "Dismiss meeting", async (main) => {
    await pressAndWaitForClose(main, "d");
    assert.equal((await leadOf("e2e-dismiss")).state, "dismissed");
  });
});

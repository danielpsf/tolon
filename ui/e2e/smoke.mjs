// WebDriver smoke test: tauri-driver starts the debug app and selenium-webdriver drives it.
// Run with `npm --prefix ui run e2e` after `npm --prefix ui run tauri build -- --debug --no-bundle`.
import { spawn, spawnSync } from "node:child_process";
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import http from "node:http";
import net from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import process from "node:process";
import { after, before, test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { Builder, By, error, Key, until } from "selenium-webdriver";
import { readDocs } from "../help/docs.mjs";

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
  await startApp();
});

/** A launch that has not answered for this long after it started may be a single-instance handover. */
const HANDOVER_MS = 2_000;

/**
 * Starts the app in a new WebDriver session; the data dir is tauri-driver's, so it survives a restart.
 * `route` opens a settings page; with none, a launch without calendars opens on first run (F10), as Tolon does.
 * Tolon is a single instance (L-044): a launch right after `driver.quit()`, while the old process is still
 * closing, can hand over to it and exit 0 at once. The driver starts the process, so the suite sees that only
 * as a session that fails or stops answering; such a launch is retried once.
 */
async function startApp(route = "#/calendars") {
  try {
    await launch(route);
  } catch (err) {
    console.log(`::warning::launch failed, retrying once: ${err.message}`);
    await driver?.quit().catch(() => {});
    await sleep(HANDOVER_MS);
    await launch(route);
  }
}

async function launch(route) {
  const started = Date.now();
  driver = await new Builder()
    .usingServer(driverUrl)
    .withCapabilities({ browserName: "wry", "tauri:options": { application } })
    .build();
  // WebView2 can answer with about:blank before the app's page loads; a URL built from that loads no app,
  // and its commands fail with "Origin header is not a valid URL".
  const current = await driver.wait(async () => {
    const href = await driver.getCurrentUrl();
    return !href.startsWith("about:") && href;
  }, WAIT);
  // The language follows the system by default; the `lang` query parameter pins it so the copy below is English.
  const url = new URL(current);
  url.search = "?lang=en-US";
  url.hash = route;
  await driver.get(url.href);
  // A process that handed over has exited by now; this call then fails and the launch is retried.
  await sleep(Math.max(0, HANDOVER_MS - (Date.now() - started)));
  await driver.getTitle();
}

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
  let rows;
  // WebKitGTK's lookup itself can fail as stale when a row leaves the list during it: look again.
  while (!rows) {
    rows = await driver.findElements(By.css(".tl-row")).catch((e) => {
      if (!(e instanceof error.StaleElementReferenceError)) throw e;
    });
  }
  for (const row of rows) {
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
    throw new Error(`timed out waiting for: ${step} (${e.message}); page: ${JSON.stringify(await snapshot())}`, { cause: e });
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
  await row.findElement(By.css(".tl-remove button:last-child")).click();
  const dialog = await find("dialog[open]");
  assert.match(await dialog.getText(), /Tolon stops reading this calendar\. The calendar itself is not changed\./);

  const cancel = await dialog.findElement(By.css(".sm-dialog-actions button"));
  await cancel.click();
  await waitFor("the dialog to close after Cancel", async () => (await driver.findElements(By.css("dialog[open]"))).length === 0);
  assert.ok(await rowForTestCalendar(), "Cancel keeps the calendar");

  await (await waitForRow()).findElement(By.css(".tl-remove button:last-child")).click();
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
    await row.findElement(By.css(".tl-remove button:last-child")).click();
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

test("l067 a calendar is renamed in its row, and clearing the name brings the feed name back", async () => {
  const { server, port } = await fixtureServer();
  try {
    const input = await openAddForm();
    await input.sendKeys(`http://127.0.0.1:${port}/calendar/private-123/basic.ics`);
    await (await find('#add-calendar button[type="submit"]')).click();
    const row = await waitFor("a row named from the feed", async () => (await fixtureRows())[0]);

    // The Rename button opens the edit on the name shown; Enter saves.
    await row.findElement(By.css(".tl-remove button:first-child")).click();
    const field = await row.findElement(By.css("input.sm-input"));
    assert.equal(await field.getAttribute("value"), "Team fixture");
    assert.equal(await driver.executeScript("return document.activeElement === arguments[0]", field), true, "the edit takes focus");
    // select() works on every OS; Ctrl+A is a line start on macOS.
    await driver.executeScript("arguments[0].select()", field);
    await field.sendKeys("  Standups  ");
    assert.equal(await field.getAttribute("value"), "  Standups  ");
    await field.sendKeys(Key.ENTER);
    const renamed = await waitFor("the new name", async () => (await calendarRows("Standups"))[0]);
    assert.equal(await renamed.findElement(By.css(".tl-name")).getText(), "Standups, iCal subscription");
    assert.equal((await driver.findElements(By.css("input.sm-input"))).length, 0, "the edit closes");
    assert.equal(await driver.executeScript("return document.activeElement?.textContent ?? ''"), "Rename", "focus returns to the row");

    // Esc cancels and keeps the name.
    await renamed.findElement(By.css(".tl-remove button:first-child")).click();
    await (await renamed.findElement(By.css("input.sm-input"))).sendKeys("Other", Key.ESCAPE);
    await waitFor("the edit to close", async () => (await renamed.findElements(By.css("input.sm-input"))).length === 0);
    assert.equal(await renamed.findElement(By.css(".tl-name")).getText(), "Standups, iCal subscription");

    // An empty name clears it: the feed's own name shows again.
    await renamed.findElement(By.css(".tl-remove button:first-child")).click();
    const again = await renamed.findElement(By.css("input.sm-input"));
    await driver.executeScript("arguments[0].select()", again);
    await again.sendKeys(Key.BACK_SPACE);
    assert.equal(await again.getAttribute("value"), "");
    await again.sendKeys(Key.ENTER);
    const restored = await waitFor("the feed name", async () => (await fixtureRows())[0]);
    assert.equal(await restored.findElement(By.css(".tl-name")).getText(), "Team fixture, iCal subscription");
  } finally {
    await removeFixtureCalendar();
    await removeCalendar("Standups");
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

// ---- Interrupt window (L-010) -----------------------------------------------------------------
// `alarms_test` opens the sample interrupt on every screen (one here). The windows are separate
// WebDriver handles: the test switches to one, checks focus, presses a key and expects every
// interrupt window to close (AC4.6). The copy is left alone because it follows the system language.

// Tolon keeps its interrupt windows after an alarm and only hides them (L-048), so a WebDriver handle
// says nothing about whether an interrupt is up. `app_e2e_interrupt_windows` (end-to-end build only)
// says which windows exist and which are shown; the handles of the shown ones are the interrupts.

/** The interrupt windows that exist, `{ label, visible, focused }` each. Leaves the main window current. */
async function interruptStates(main) {
  await driver.switchTo().window(main);
  const { answer, code } = await invoke("app_e2e_interrupt_windows");
  assert.equal(code, undefined, `app_e2e_interrupt_windows failed: ${code}`);
  return answer;
}

/** The handles of the interrupt windows that are shown. Leaves the main window current. */
async function interruptHandles(main) {
  const others = (await driver.getAllWindowHandles()).filter((h) => h !== main);
  if (others.length === 0) return [];
  const shown = new Set((await interruptStates(main)).filter((s) => s.visible).map((s) => s.label));
  const found = [];
  for (const handle of others) {
    try {
      await driver.switchTo().window(handle);
      const screen = new URL(await driver.getCurrentUrl()).searchParams.get("screen");
      if (shown.has(`interrupt-${screen}`)) found.push(handle);
    } catch (e) {
      // A window that went away between the list and the switch is not shown.
      if (!(e instanceof error.NoSuchWindowError)) throw e;
    }
  }
  await driver.switchTo().window(main);
  return found;
}

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
    assert.equal((await interruptHandles(main)).length, 0);
  });
}

// ---- The scheduler end to end (L-032) ---------------------------------------------------------
// A feed with a meeting that starts in 90 s, inside the default 2-minute lead time: the sync wakes the
// scheduler, which plans the alarm, fires it and opens the interrupt. The state is read from the store
// through `alarms_e2e_state`, a command that only the e2e build (the fetch seam above) contains.

const stamp = (date) => date.toISOString().replace(/[-:]/g, "").replace(/\.\d{3}/, "");

function alarmFeed(uid, title, start, twin = null) {
  const event = (id, summary) => [
    "BEGIN:VEVENT",
    `UID:${id}@example.com`,
    `DTSTAMP:${stamp(new Date())}`,
    `DTSTART:${stamp(start)}`,
    `DTEND:${stamp(new Date(start.getTime() + 30 * 60_000))}`,
    `SUMMARY:${summary}`,
    "END:VEVENT",
  ];
  return [
    "BEGIN:VCALENDAR",
    "VERSION:2.0",
    "PRODID:-//Tolon e2e//EN",
    "X-WR-CALNAME:Alarm fixture",
    ...event(uid, title),
    ...(twin ? event(twin.uid, twin.title) : []),
    "END:VCALENDAR",
    "",
  ].join("\r\n");
}

/** The alarm rows, asked from the main window. */
const alarmRows = () =>
  driver.executeAsyncScript((done) => {
    window.__TAURI_INTERNALS__.invoke("alarms_e2e_state").then(done, (e) => done(JSON.stringify(e)));
  });

/**
 * Adds the feed, waits for its interrupt, runs `body` on it, then closes up. `body` gets the main handle.
 * A `twin` ({ uid, title }) starts in the same minute, so one reminder shows both meetings.
 */
async function withImminentMeeting(uid, title, body, twin = null) {
  const { server, port } = await fixtureServer(alarmFeed(uid, title, new Date(Date.now() + 90_000), twin));
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
    if (twin) await find(".tl-int-list");
    else assert.equal(await (await find(".tl-int-title")).getText(), title);
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

// L-059: two meetings in the same minute share one reminder. Settling one keeps the window (and, since L-048,
// the page with its focus), and the meeting that is left must take its keys at once.
for (const [name, key, state] of [
  ["D", "d", "dismissed"],
  ["Esc", Key.ESCAPE, "snoozed"],
]) {
  test(`l059 after one of two same-minute meetings is settled, ${name} still works on the other`, async () => {
    const [a, b] = [`e2e-twin-a-${name}`, `e2e-twin-b-${name}`];
    await withImminentMeeting(
      a,
      "Twin A",
      async (main) => {
        await driver.switchTo().window((await interruptHandles(main))[0]);
        const titles = await driver.executeScript(() => [...document.querySelectorAll(".tl-int-item h2")].map((h) => h.textContent));
        assert.deepEqual([...titles].sort(), ["Twin A", "Twin B"]);
        // No join link in the feed: each primary button is "I am already there", a partial join without a browser.
        await (await find('.tl-int-item [data-action="already_there"]')).click();
        await waitFor("the other meeting to be the only one left", async () => {
          if ((await driver.findElements(By.css(".tl-int-list"))).length > 0) return false;
          return (await (await find(".tl-int-title")).getText()) === titles[1];
        });
        assert.equal((await interruptHandles(main)).length, 1);
        await driver.switchTo().window((await interruptHandles(main))[0]);
        await pressAndWaitForClose(main, key);
        const rows = await alarmRows();
        const lead = (uid) => rows.find((r) => r.key.includes(`:${uid}@`) && r.kind === "lead");
        assert.deepEqual([lead(a).state, lead(b).state].sort(), ["joined", state].sort());
      },
      { uid: b, title: "Twin B" },
    );
  });
}

// The windows are kept between alarms (L-048): one per screen for the whole run, hidden on close and
// shown again for the next alarm, which shows its own meeting and starts with the focus on the primary button.
test("l048 two alarms in a row leave one window per screen, hidden between them, and the second shows its own title", async () => {
  const shownStates = async (main) => (await interruptStates(main)).filter((s) => s.visible);
  let label;
  await withImminentMeeting("e2e-reuse-a", "First reuse meeting", async (main) => {
    // Earlier tests opened and closed many interrupts: still one window per screen (xvfb has one).
    const states = await interruptStates(main);
    assert.equal(states.length, 1, JSON.stringify(states));
    assert.deepEqual(states.map((s) => s.visible), [true]);
    label = states[0].label;
    await driver.switchTo().window((await interruptHandles(main))[0]);
    // Mark this page and move the focus off the primary button: the second alarm must start without either.
    await driver.executeScript(() => {
      window.__tolonReuseMarker = "first alarm";
    });
    await (await driver.switchTo().activeElement()).sendKeys(Key.TAB);
    assert.equal(await driver.executeScript(() => document.activeElement?.dataset.action), "snooze");
    await pressAndWaitForClose(main, "d");
    // Closed means hidden, not gone.
    const hidden = await interruptStates(main);
    assert.deepEqual(hidden.map((s) => [s.label, s.visible]), [[label, false]]);
    assert.equal((await interruptHandles(main)).length, 0);
  });
  await withImminentMeeting("e2e-reuse-b", "Second reuse meeting", async (main) => {
    // `withImminentMeeting` has read the title of the second meeting from the window it found.
    assert.deepEqual((await shownStates(main)).map((s) => s.label), [label]);
    assert.equal((await interruptStates(main)).length, 1, "no second window was made");
    await driver.switchTo().window((await interruptHandles(main))[0]);
    // The same page, not a new one: the marker of the first alarm is still there.
    assert.equal(await driver.executeScript(() => window.__tolonReuseMarker), "first alarm");
    // The feed has no join link, so the primary button is "I am already there"; the first alarm left the focus on Snooze.
    await waitFor("focus on the primary button of the second alarm", async () => {
      const active = await driver.switchTo().activeElement();
      return (await active.getAttribute("data-action")) === "already_there";
    });
    assert.equal(await (await find(".tl-int-title")).getText(), "Second reuse meeting");
    // The keys work again on the reused page.
    await pressAndWaitForClose(main, "d");
    assert.equal((await leadOf("e2e-reuse-b")).state, "dismissed");
    assert.deepEqual((await interruptStates(main)).map((s) => [s.label, s.visible]), [[label, false]]);
  });
});

// ---- Settings round trip (L-016 backend, L-017 pages) --------------------------------------------
// The pages save through `settings_set` on every change (AC9.1); the backend validates and refuses a
// bad value with `invalid:<field>`, which the page shows as a sentence.

/** Shows a settings page by its route, as the left nav does. */
async function openPage(id) {
  await (await find(`nav a[href="#/${id}"]`)).click();
  await waitFor(`the ${id} page`, async () => (await driver.getCurrentUrl()).endsWith(`#/${id}`));
}

/** Sets a field's value as typing does for React: the native value setter, then an input event. */
const typeInto = (input, value) =>
  driver.executeScript(
    (field, text) => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(field, text);
      field.dispatchEvent(new Event("input", { bubbles: true }));
    },
    input,
    value,
  );

/** The settings as stored, asked from the main window. */
const storedSettings = () =>
  driver.executeAsyncScript((done) => {
    window.__TAURI_INTERNALS__.invoke("settings_get").then(done, (e) => done(JSON.stringify(e)));
  });

test("ac9_1 the Alarms page saves the lead time at once and it survives a restart", async () => {
  await openPage("alarms");
  const lead = await find("#lead-minutes");
  assert.equal(await lead.getAttribute("value"), "2", "the default lead time");
  // WebKitGTK's number field ignores select-all over WebDriver, so the value is typed the way React
  // sees typing: the native setter, then an input event.
  await typeInto(lead, "7");
  await waitFor("lead time 7 in the store", async () => (await storedSettings()).lead_minutes === 7);

  // Quit the app and start it again on the same data dir.
  await driver.quit();
  await startApp();
  await openPage("alarms");
  await waitFor("lead time 7 after the restart", async () => (await (await find("#lead-minutes")).getAttribute("value")) === "7");
  assert.equal((await storedSettings()).lead_minutes, 7);
});

test("ac8_1 a quiet-hours time the backend refuses shows the error and keeps the stored time", async () => {
  await openPage("quiet-hours");
  const enable = await find('main input[role="switch"]');
  if (!(await enable.isSelected())) await enable.click();
  await waitFor("quiet hours on", async () => (await storedSettings()).quiet.enabled === true);
  const start = await find("#quiet-start");
  await waitFor("the start field enabled", () => start.isEnabled());
  // A time with seconds is a valid <input type="time"> value but not HH:MM: only the backend refuses it.
  await typeInto(start, "09:00:30");
  const alert = await find("main .sm-alert[role=alert]");
  await waitFor("the error sentence", async () => (await alert.getText()) === "Enter a time as hours and minutes.");
  assert.equal((await storedSettings()).quiet.start, "18:00");
  await waitFor("the stored time back in the field", async () => (await start.getAttribute("value")) === "18:00");
});

// ---- Tray and panel (L-015) ---------------------------------------------------------------------
// Tray clicks cannot be driven here (xvfb has no tray host, and AppIndicator reports no clicks), and
// WebDriver cannot close a window the way the window manager does: its close destroys the webview
// without the CloseRequested event that S7.2 hides on. So S7.2 is proven by the Rust tests and the
// manual checklist; this checks that Tolon runs with the tray built and that the window checks hold.

/** A command's answer, or its error code, asked from the main window. */
const invoke = (command, args = {}) =>
  driver.executeAsyncScript(
    (name, payload, done) => {
      window.__TAURI_INTERNALS__.invoke(name, payload).then(
        (answer) => done({ answer }),
        (e) => done({ code: e?.code ?? String(e) }),
      );
    },
    command,
    args,
  );

test("window rights: the settings window reads the pause and the current alarm, but only the tray panel may pause, hide, quit or act ahead", async () => {
  assert.deepEqual(await invoke("app_status"), { answer: { paused_until: null } });
  for (const [command, args] of [
    ["app_pause", { minutes: 60 }],
    ["app_resume", {}],
    ["app_quit", {}],
    ["app_show_settings", {}],
    ["app_hide_panel", {}],
    ["occurrences_snooze", { key: "x", minutes: 5 }],
    ["occurrences_dismiss", { key: "x" }],
  ]) {
    assert.deepEqual(await invoke(command, args), { code: "not_found" }, command);
  }
  // The interrupt's commands answer the settings window too (no interrupt is up: null).
  assert.deepEqual(await invoke("alarms_current"), { answer: null });
  // Still running, still not paused, and the tooltip command answers the settings window.
  assert.deepEqual(await invoke("app_status"), { answer: { paused_until: null } });
  assert.deepEqual(await invoke("tray_tooltip", { text: "Tolon" }), { answer: null });
  // The notification's wording (L-038) answers the settings window, which refuses a bundle that is not valid.
  assert.deepEqual(await invoke("notify_copy_set", { copy: {} }), { code: "invalid:notify_copy" });
});

// ---- Save diagnostics (L-072) ---------------------------------------------------------------------
// The system save dialog opens from Rust and cannot be driven here, so the suite saves through
// `diagnostics_e2e_save` (end-to-end build only), which skips the dialog and writes to a temp path.

/** The names in a zip's central directory (no zip64, as Tolon writes it). */
function zipNames(bytes) {
  const end = bytes.length - 22;
  assert.equal(bytes.readUInt32LE(end), 0x06054b50, "end of central directory");
  const count = bytes.readUInt16LE(end + 10);
  let at = bytes.readUInt32LE(end + 16);
  const names = [];
  for (let i = 0; i < count; i++) {
    assert.equal(bytes.readUInt32LE(at), 0x02014b50, "central directory entry");
    const nameLength = bytes.readUInt16LE(at + 28);
    names.push(bytes.toString("utf8", at + 46, at + 46 + nameLength));
    at += 46 + nameLength + bytes.readUInt16LE(at + 30) + bytes.readUInt16LE(at + 32);
  }
  return names;
}

test("l072 Save diagnostics writes a zip with the listed files to the chosen path", async () => {
  const plan = await invoke("diagnostics_plan");
  assert.equal(plan.answer?.support_email, "tolon@danielpsf.net", JSON.stringify(plan));
  assert.match(plan.answer.support_url, /^mailto:tolon@danielpsf\.net\?subject=Tolon%20diagnostics%20/);
  assert.equal(plan.answer.issues_url, "https://github.com/danielpsf/tolon/issues");
  // The page cannot choose a path: the command that opens the dialog takes none.
  const dir = mkdtempSync(join(tmpdir(), "tolon-diagnostics-"));
  try {
    // No `.zip`: Tolon adds it, as for a dialog that leaves the extension off.
    const saved = await invoke("diagnostics_e2e_save", {
      path: join(dir, "diagnostics"),
      client: { locale: "en-US", languages: ["en-US"], user_agent: "e2e" },
    });
    assert.equal(saved.answer, join(dir, "diagnostics.zip"), JSON.stringify(saved));
    const names = zipNames(readFileSync(saved.answer));
    for (const name of ["README.txt", "environment.json", "settings.json", "calendars.json", "sync-log.json", "alarms.json"]) {
      assert.ok(names.includes(name), `${name} in ${names}`);
    }
    assert.ok(names.some((n) => /^logs\/tolon\.\d{4}-\d{2}-\d{2}\.log$/.test(n)), `a log file in ${names}`);
    // A relative path is refused; nothing is written.
    assert.deepEqual(await invoke("diagnostics_e2e_save", { path: "relative.zip", client: { locale: "", languages: [], user_agent: "" } }), {
      code: "diagnostics_save",
    });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

// ---- Reminder colour (L-043) ---------------------------------------------------------------------
// The interrupt reads `interrupt_color` from `settings_get` and draws its fill from the Swiss Moss token
// of that pair. The expected colour is the token itself, resolved by the same webview.

test("the reminder colour reaches the interrupt: data-color info on a --sm-info fill (L-043)", async () => {
  const main = await driver.getWindowHandle();
  const saved = await invoke("settings_set", { patch: { interrupt_color: "info" } });
  assert.equal(saved.answer?.interrupt_color, "info", JSON.stringify(saved));
  try {
    await openTestInterrupt(main);
    const seen = await waitFor("data-color on the interrupt root", () =>
      driver.executeScript(() => {
        const root = document.querySelector("main.tl-int");
        if (root?.dataset.color !== "info") return null;
        // Pinned to Swiss Moss's light block (decision "Reminder colour (L-043)"), so the probe uses it too.
        const probe = document.createElement("i");
        probe.dataset.tokens = "light";
        probe.style.background = "var(--sm-info)";
        document.body.append(probe);
        const want = getComputedStyle(probe).backgroundColor;
        probe.remove();
        return { color: root.dataset.color, tokens: root.dataset.tokens, background: getComputedStyle(root).backgroundColor, want };
      }),
    );
    assert.equal(seen.color, "info");
    assert.equal(seen.tokens, "light");
    assert.notEqual(seen.want, "rgba(0, 0, 0, 0)", "the token resolves");
    assert.equal(seen.background, seen.want);
    try {
      await (await driver.switchTo().activeElement()).sendKeys(Key.ESCAPE);
    } catch (e) {
      if (!(e instanceof error.NoSuchWindowError)) throw e;
    }
    await waitFor("every interrupt window to close", async () => (await interruptHandles(main)).length === 0);
  } finally {
    await driver.switchTo().window(main);
    await invoke("settings_set", { patch: { interrupt_color: "theme" } });
  }
});

test("a pinned colour in a dark window keeps the light moss and the Arabic font stack (L-043)", async () => {
  const main = await driver.getWindowHandle();
  const saved = await invoke("settings_set", { patch: { appearance: "dark", interrupt_color: "moss", locale: "ar" } });
  assert.equal(saved.answer?.interrupt_color, "moss", JSON.stringify(saved));
  try {
    await openTestInterrupt(main);
    const seen = await waitFor("the pinned interrupt in a dark, Arabic window", () =>
      driver.executeScript(() => {
        const root = document.querySelector("main.tl-int");
        const html = document.documentElement;
        if (root?.dataset.color !== "moss" || html.dataset.theme !== "dark" || html.lang !== "ar") return null;
        // --sm-moss as Swiss Moss's light block draws it, and as the dark window around the page does.
        const fill = (tokens) => {
          const probe = document.createElement("i");
          if (tokens) probe.dataset.tokens = tokens;
          probe.style.background = "var(--sm-moss)";
          (tokens ? root : document.body).append(probe);
          const value = getComputedStyle(probe).backgroundColor;
          probe.remove();
          return value;
        };
        return {
          tokens: root.dataset.tokens,
          background: getComputedStyle(root).backgroundColor,
          lightMoss: fill("light"),
          darkMoss: fill(null),
          font: getComputedStyle(root).getPropertyValue("--sm-font-sans"),
          titleFont: getComputedStyle(document.querySelector(".tl-int-title") ?? root).fontFamily,
        };
      }),
    );
    assert.equal(seen.tokens, "light");
    assert.equal(seen.background, seen.lightMoss, "the light moss, not the dark one");
    assert.notEqual(seen.background, seen.darkMoss, "the window itself is dark, the pin is not");
    assert.match(seen.font, /Noto Sans Arabic/, "the pin leaves the :lang(ar) font stack alone");
    assert.match(seen.titleFont, /Noto Sans Arabic/);
    try {
      await (await driver.switchTo().activeElement()).sendKeys(Key.ESCAPE);
    } catch (e) {
      if (!(e instanceof error.NoSuchWindowError)) throw e;
    }
    await waitFor("every interrupt window to close", async () => (await interruptHandles(main)).length === 0);
  } finally {
    await driver.switchTo().window(main);
    await invoke("settings_set", { patch: { appearance: "auto", interrupt_color: "theme", locale: null } });
  }
});

// ---- Peek style (L-050) --------------------------------------------------------------------------------
// `style` reaches every window as data-style on <html>, and the Peek reminder draws the character.

test("switching the style to Peek sets data-style and shows Peek on the test alarm (L-050)", async () => {
  const main = await driver.getWindowHandle();
  const saved = await invoke("settings_set", { patch: { style: "peek" } });
  assert.equal(saved.answer?.style, "peek", JSON.stringify(saved));
  try {
    // The settings window applies it from settings:changed.
    await waitFor("data-style on the settings window", () => driver.executeScript(() => document.documentElement.dataset.style === "peek"));
    await openTestInterrupt(main);
    const seen = await waitFor("Peek on the interrupt", () =>
      driver.executeScript(() => {
        const scene = document.querySelector("main.tl-int .tl-peek");
        if (document.documentElement.dataset.style !== "peek" || !scene) return null;
        const art = scene.querySelector("svg.tl-peek-art");
        return {
          hidden: scene.getAttribute("aria-hidden"),
          eyes: art?.querySelectorAll(".tl-peek-eye").length ?? 0,
          shown: (art?.getBoundingClientRect().height ?? 0) > 0,
          say: scene.querySelector(".tl-peek-say")?.textContent ?? "",
          titleFont: getComputedStyle(document.querySelector(".tl-int-title") ?? document.body).fontFamily,
          lang: document.documentElement.lang,
        };
      }),
    );
    assert.equal(seen.hidden, "true");
    assert.equal(seen.eyes, 2);
    assert.ok(seen.shown, "the character is drawn");
    assert.ok(seen.say.trim(), "the speech bubble has its line");
    // Latin locales draw the title in Peek's serif; other scripts keep their :lang() stack.
    if (!/^(ar|he|hi|ja|zh)/.test(seen.lang)) assert.match(seen.titleFont, /^Fraunces/);
    try {
      await (await driver.switchTo().activeElement()).sendKeys(Key.ESCAPE);
    } catch (e) {
      if (!(e instanceof error.NoSuchWindowError)) throw e;
    }
    await waitFor("every interrupt window to close", async () => (await interruptHandles(main)).length === 0);
  } finally {
    await driver.switchTo().window(main);
    await invoke("settings_set", { patch: { style: "swiss-moss" } });
  }
});

test("Preview in dark or light recolours the Appearance preview under the app's CSP (L-043)", async () => {
  await openPage("appearance");
  /** The preview's fill and token block, and the fill Swiss Moss's moss has in that block. */
  const preview = () =>
    driver.executeScript(() => {
      const el = document.querySelector(".tl-preview");
      if (!el) return null;
      const probe = document.createElement("i");
      probe.dataset.tokens = el.dataset.tokens ?? "";
      probe.style.background = "var(--sm-moss)";
      document.body.append(probe);
      const moss = getComputedStyle(probe).backgroundColor;
      probe.remove();
      return { tokens: el.dataset.tokens ?? null, background: getComputedStyle(el).backgroundColor, moss };
    });
  const before = await waitFor("the Default preview", preview);
  await (await find('[data-action="preview-theme"]')).click();
  const after = await waitFor("the preview in the other appearance", async () => {
    const now = await preview();
    return now?.tokens && now.tokens !== before.tokens && now;
  });
  // The injected token block applies (a CSP that refused it would leave the fill as before).
  assert.notEqual(after.background, before.background);
  assert.equal(after.background, after.moss);
});

// ---- Updates (L-019) ----------------------------------------------------------------------------
// A debug build never checks for updates. The e2e build may point the updater at a `latest.json` on
// 127.0.0.1 (`update_e2e_endpoint`, compiled only with the fetch seam). The package is signed with a
// throwaway minisign key made for this run (no password), a real signature for the right version that
// the pubkey in tauri.conf.json did not make, so Install downloads it and the signature check refuses
// it: Tolon keeps running. Versions grow with the clock, because Windows keeps the skipped version in
// its data dir between runs.

const TARGETS = ["linux", "windows", "darwin"].flatMap((os) => ["x86_64", "aarch64"].map((arch) => `${os}-${arch}`));

const tauriCli = fileURLToPath(new URL("../node_modules/@tauri-apps/cli/tauri.js", import.meta.url));

/** The Tauri CLI's `signer` subcommand; fails the test with its output when it fails. */
function signer(...args) {
  const done = spawnSync(process.execPath, [tauriCli, "signer", ...args], { encoding: "utf8", env: { ...process.env, CI: "true" } });
  assert.equal(done.status, 0, `tauri signer ${args[0]}: ${done.stdout}${done.stderr}`);
}

/** A package signed for `version` by a fresh key that is not Tolon's: its `.sig` text, as latest.json carries it. */
function signedByAnotherKey(version) {
  const dir = mkdtempSync(join(tmpdir(), "tolon-e2e-key-"));
  try {
    signer("generate", "--ci", "-p", "", "-w", join(dir, "key"));
    writeFileSync(join(dir, "tolon.bin"), "not an update");
    signer("sign", "-f", join(dir, "key"), "-p", "", "--app-version", version, join(dir, "tolon.bin"));
    return readFileSync(join(dir, "tolon.bin.sig"), "utf8").trim();
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

/** Serves `latest.json` offering `state.version` with `state.signature`, and the package it points at. */
async function updateServer(state) {
  const requests = [];
  const server = http.createServer((req, res) => {
    requests.push(req.url);
    const port = server.address().port;
    if (req.url === "/latest.json") {
      const platform = { signature: state.signature, url: `http://127.0.0.1:${port}/tolon.bin` };
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ version: state.version, notes: "", pub_date: new Date().toISOString(), platforms: Object.fromEntries(TARGETS.map((t) => [t, platform])) }));
    } else {
      res.writeHead(200, { "Content-Type": "application/octet-stream" });
      res.end(Buffer.from("not an update"));
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return { server, requests, port: server.address().port };
}

const byText = (text) => driver.findElements(By.xpath(`//*[normalize-space()=${JSON.stringify(text)}]`));
/**
 * Clicks the button once it is there and enabled, then waits for its command to answer: the page disables
 * the update buttons while a command runs. Without that wait a status line already on screen could pass the
 * next check while the command was still running, and its late answer replaced the next button to click.
 */
async function clickButton(text) {
  const found = await driver.wait(until.elementLocated(By.xpath(`//button[normalize-space()=${JSON.stringify(text)}]`)), WAIT);
  await driver.wait(until.elementIsEnabled(found), WAIT);
  await found.click();
  await waitFor(`the ${text} command to answer`, () =>
    driver.executeScript(() => document.querySelectorAll("button:disabled").length === 0),
  );
}
const shows = (step, text) => waitFor(step, async () => (await byText(text)).length > 0);

test("ac11_1 a debug build does not check; a found update asks first, Later and Skip this version hold, a newer one asks again, and a bad signature installs nothing", async () => {
  const run = Math.floor(Date.now() / 1000);
  const state = { version: `9.${run}.0` };
  state.signature = signedByAnotherKey(state.version);
  const { server, requests, port } = await updateServer(state);
  try {
    assert.deepEqual(await invoke("update_status"), { answer: { available: null, installing: false } });
    // No endpoint set: a debug build never asks the network.
    assert.deepEqual(await invoke("update_check_now"), { answer: { available: null, installing: false } });
    assert.deepEqual(requests, []);
    // The test endpoint is loopback http only.
    assert.deepEqual(await invoke("update_e2e_endpoint", { url: "https://example.com/latest.json" }), { code: "invalid_url" });
    assert.deepEqual(await invoke("update_e2e_endpoint", { url: `http://127.0.0.1:${port}/latest.json` }), { answer: null });

    await driver.executeScript(() => {
      window.location.hash = "#/about";
    });
    await clickButton("Check for updates");
    await shows("the offer", `Tolon ${state.version} is available.`);
    assert.ok(requests.includes("/latest.json"));

    // Later hides the prompt until the next check, and focus moves to the Updates heading.
    await clickButton("Later");
    const offer = `Tolon ${state.version} is available.`;
    await waitFor("Later to hide the offer", async () => !(await driver.executeScript(() => document.body.innerText)).includes(offer));
    assert.deepEqual(await invoke("update_status"), { answer: { available: null, installing: false } });
    await waitFor("focus on the Updates heading", async () => (await driver.executeScript(() => document.activeElement?.id)) === "updates-heading");
    await clickButton("Check for updates");
    await shows("the offer again after a check", `Tolon ${state.version} is available.`);

    // Install downloads the package, the signature check refuses it, and Tolon keeps running.
    await clickButton("Install and restart");
    await shows("the refused install", "The update did not install. Tolon keeps running this version.");
    assert.ok(requests.includes("/tolon.bin"), JSON.stringify(requests));
    assert.equal((await invoke("update_status")).answer.installing, false);

    // Skip this version is remembered; a newer version asks again.
    await clickButton("Skip this version");
    await clickButton("Check for updates");
    await shows("no offer for the skipped version", "There is no new version to install.");
    assert.deepEqual(await invoke("update_status"), { answer: { available: null, installing: false } });
    state.version = `9.${run}.1`;
    await clickButton("Check for updates");
    await shows("the newer version", `Tolon ${state.version} is available.`);
    await clickButton("Skip this version");
  } finally {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

// ---- Help (L-071) ------------------------------------------------------------------------------
// The release's own docs, converted when the UI was built. Under the app's CSP, so a bundled image that loads
// proves img-src. A web address goes to the system browser through `app_open_url`; this suite only checks the
// command refuses what is not one, so no browser opens on the runner.

test("l071 Help shows the install guide of this commit, a link inside it opens the iCal guide, and only web addresses leave Tolon", async () => {
  await driver.executeScript(() => {
    window.location.hash = "#/help";
  });
  const article = await find(".tl-help");
  await waitFor("the install heading", async () => (await article.findElement(By.css("h2")).getText()) === "Install Tolon");
  // Staleness: the hash inside the build is the hash of the docs this suite runs from.
  assert.equal(await article.getAttribute("data-docs-hash"), readDocs(join(root, "docs")).hash);
  const imagesLoaded = await driver.executeAsyncScript((done) => {
    const images = [...document.querySelectorAll(".tl-help img")];
    Promise.all(images.map((i) => i.decode().then(() => i.naturalWidth > 0, () => false))).then((ok) => done(images.length > 0 && ok.every(Boolean)));
  });
  assert.ok(imagesLoaded, "the install guide's images did not load");

  await (await find('.tl-help a[href="#/help/ical-links"]')).click();
  await waitFor("the iCal guide", async () => (await (await find(".tl-help h2")).getText()) === "Get an iCal link for your calendar");
  assert.equal(await driver.executeScript(() => window.location.hash), "#/help/ical-links");
  assert.equal(await (await find('.sm-tabs a[aria-current="page"]')).getText(), "Get an iCal link");
  const external = await driver.findElements(By.css(".tl-help a[data-external]"));
  assert.ok(external.length > 0, "the iCal guide has web links that open in the system browser");

  for (const url of ["javascript:alert(1)", "file:///etc/passwd", "ftp://example.com/x", "https://example.com/a b"]) {
    assert.deepEqual(await invoke("app_open_url", { url }), { code: "invalid_url" }, url);
  }
  await driver.executeScript(() => {
    window.location.hash = "#/calendars";
  });
});

// ---- First run (L-018) --------------------------------------------------------------------------
// Every test above removes what it adds, so Tolon has no calendar again, as on a first launch. A launch on
// no page opens the settings window on first run (AC10.1); a first calendar that syncs hides it, and the
// next launch opens no window (AC10.2). WebDriver cannot tell a hidden window, and WebView2 keeps
// `document.visibilityState` at visible when Windows hides one, so the window is asked through
// `app_e2e_settings_visible` (end-to-end build only). A hidden window is driven by script only. Runs
// last: the window ends up hidden.

const visibility = async () => {
  const shown = await invoke("app_e2e_settings_visible");
  assert.equal(typeof shown.answer, "boolean", JSON.stringify(shown));
  return shown.answer ? "visible" : "hidden";
};

/** Quits and starts Tolon on no page, as it starts itself; with no calendar that is first run. */
async function launchOnFirstRun() {
  await driver.quit();
  await startApp("");
  assert.equal(await (await find("#first-run-title")).getText(), "Never miss a meeting again.");
  assert.equal(await visibility(), "visible");
}

test("ac10_1 with no calendar Tolon opens on first run and its test alarm fires; only the settings window may hide itself or ask where the tray is", async () => {
  await launchOnFirstRun();
  assert.equal((await driver.findElements(By.css('.tl-first .sm-steps[role="list"] > li'))).length, 3);
  // No template tray icon on Linux or Windows, so the tray step says system tray.
  await waitFor(
    "the tray step",
    async () => (await driver.findElement(By.css(".tl-first .sm-steps > li:last-child h2")).getText()) === "Tolon is in the system tray",
  );
  assert.deepEqual(await invoke("app_tray_place"), { answer: "tray" });
  // "Paste an iCal link" has focus.
  assert.equal(await driver.executeScript(() => document.activeElement?.textContent), "Paste an iCal link");

  // "Try a test alarm" opens a real interrupt from this window; Esc closes it.
  const main = await driver.getWindowHandle();
  await (await find('button[data-action="test-alarm"]')).click();
  const handles = await waitFor("the test interrupt", async () => {
    const found = await interruptHandles(main);
    return found.length > 0 && found;
  });
  await driver.switchTo().window(handles[0]);
  // The first-run commands answer the settings window only, like notify_copy_set.
  assert.deepEqual(await invoke("app_hide_settings"), { code: "not_found" });
  assert.deepEqual(await invoke("app_tray_place"), { code: "not_found" });
  await pressAndWaitForClose(main, Key.ESCAPE);
  assert.equal(await visibility(), "visible");
});

test("ac10_2 a first link that fails to sync keeps the window and shows its error on the Calendars page", async () => {
  const port = await closedPort();
  const input = await openAddForm();
  await input.sendKeys(`https://127.0.0.1:${port}/calendar/first-run/basic.ics`);
  await (await find('#add-calendar button[type="submit"]')).click();
  const row = await waitForRow();
  assert.equal(await row.findElement(By.css(".tl-status.failed")).getText(), `Sync failed: ${english}`);
  assert.equal((await driver.findElements(By.css("#first-run-title"))).length, 0);
  assert.equal(await visibility(), "visible");
  await removeCalendar("127.0.0.1");
});

test("skip: Skip for now hides first run, and the next launch without a calendar shows it again", async () => {
  await launchOnFirstRun();
  await (await find('button[data-action="skip"]')).click();
  await waitFor("the window to hide", async () => (await visibility()) === "hidden");
  await waitFor("first run to end", async () => (await driver.findElements(By.css("#first-run-title"))).length === 0);
  await launchOnFirstRun();
});

test("ac10_2 the first calendar that syncs closes first run for good: the next launch opens no window", async () => {
  const { server, port } = await fixtureServer();
  try {
    const input = await openAddForm();
    await input.sendKeys(`http://127.0.0.1:${port}/calendar/first-run/basic.ics`);
    await (await find('#add-calendar button[type="submit"]')).click();
    await waitFor("the window to hide", async () => (await visibility()) === "hidden");
    await waitFor("first run to end", async () => (await driver.findElements(By.css("#first-run-title"))).length === 0);

    await driver.quit();
    await startApp("");
    await waitFor("the settings pages, loaded hidden", async () => (await driver.findElements(By.css(".tl-shell"))).length === 1);
    assert.equal(await visibility(), "hidden");
  } finally {
    // The window is hidden: the calendar goes by command, not by a click (Windows keeps its data dir).
    const listed = await invoke("calendars_list");
    for (const calendar of listed.answer ?? []) {
      if (calendar.name === "Team fixture") await invoke("calendars_remove", { id: calendar.id });
    }
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

// Real browser checks for session undo. All saved records and meshes are synthetic.
// Run against a built preview served at /Jig/: node tests/undo_browser.mjs
import assert from "node:assert/strict";
import { mkdir, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium } from "playwright";

const base = process.env.JIG_PREVIEW_URL ?? "http://127.0.0.1:4173/Jig/";
const origin = new URL(base).origin;
const storageKey = "jig.personal.v1";
const results = resolve("playwright-results");
const screenshotOptions = { timeout: 60000, animations: "disabled", caret: "hide" };
await mkdir(results, { recursive: true });
const fixture = {
  version: 1,
  preferences: { decimals: 2, focus: 3, weather: null },
  history: [{ expression: "2 + 2", result: 4 }],
  tasks: [{ id: 1, text: "Sample: photograph sculpture", complete: false, deadline: 101, color: "rose" }],
  deadlines: [{ id: 101, title: "Sample: photograph sculpture", at: "2030-01-15T18:00:00Z" }],
  entries: [{ id: 301, title: "Sample: hidden studio note", date: "2030-01-15" }],
  featured: 101,
};
let phase = "launch Chromium";
let current;
const contexts = [];
const issues = [];
// Track only layout shortcuts sent by this test driver, not private app state.
const expanded = new WeakMap();
const timer = setTimeout(() => {
  console.error(`Undo browser checks exceeded 240 seconds during: ${phase}`);
  process.exit(1);
}, 240000);
function progress(message) { phase = message; console.log(`Undo browser phase: ${message}`); }
async function paint(page) {
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await page.waitForTimeout(100);
}
async function key(page, value) {
  await page.keyboard.press(value);
  if (value === "Escape") expanded.set(page, false);
  else if (value === "Control+m" || value === "F2") expanded.set(page, !expanded.get(page));
  await paint(page);
}
async function tab(page, count) { for (let index = 0; index < count; index++) await key(page, "Tab"); }
async function capture(page, name) {
  const png = await page.screenshot({ ...screenshotOptions, path: resolve(results, `${name}.png`) });
  const allowed = new Set(["undo-desktop", "undo-viewer-cleared", "undo-failure"]);
  const requested = new Set((process.env.JIG_RENDER_LOGS ?? "").split(","));
  if (allowed.has(name) && requested.has(name) && png.length <= 1024 * 1024) {
    console.log(`JIG_RENDER_BEGIN_${name}`);
    const encoded = png.toString("base64");
    for (let offset = 0; offset < encoded.length; offset += 2048) console.log(encoded.slice(offset, offset + 2048));
    console.log(`JIG_RENDER_END_${name}`);
  }
}
const browser = await chromium.launch({ headless: true, args: ["--use-gl=angle", "--use-angle=swiftshader-webgl", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"] });
async function open(label, state = fixture) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 1000 }, acceptDownloads: true, timezoneId: "Etc/UTC" });
  contexts.push(context);
  await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin });
  await context.addInitScript(({ raw, storageKey, origin }) => {
    if (location.origin === origin && sessionStorage.getItem("jig-undo-seeded") !== "yes") {
      localStorage.setItem(storageKey, raw);
      sessionStorage.setItem("jig-undo-seeded", "yes");
    }
  }, { raw: JSON.stringify(state), storageKey, origin });
  await context.route("**/*", async route => {
    const url = new URL(route.request().url());
    if (["blob:", "data:", "about:"].includes(url.protocol) || url.origin === origin) await route.continue();
    else { issues.push(`${label}: unexpected remote request ${url.origin}${url.pathname}`); await route.abort(); }
  });
  const page = await context.newPage();
  page.on("pageerror", error => issues.push(`${label}: ${error.message}`));
  page.setDefaultTimeout(15000);
  await page.clock.setFixedTime(new Date("2026-10-30T12:00:00Z"));
  await boot(page);
  return { context, page };
}
async function boot(page, reload = false) {
  expanded.set(page, false);
  if (reload) await page.reload({ waitUntil: "load" });
  else await page.goto(base, { waitUntil: "load" });
  await page.bringToFront();
  await page.waitForFunction(() => window.jigReady === true || typeof window.jigBootError === "string", undefined, { timeout: 60000 });
  assert.equal(await page.evaluate(() => window.jigBootError), undefined);
  await page.waitForTimeout(500);
}
async function state(page) { return page.evaluate(key => JSON.parse(localStorage.getItem(key)), storageKey); }
async function raw(page) { return page.evaluate(key => localStorage.getItem(key), storageKey); }
async function expectState(page, expected, message) {
  const deadline = Date.now() + 5000;
  let actual;
  while (Date.now() < deadline) {
    actual = await state(page);
    try { assert.deepEqual(actual, expected); return; } catch { await page.waitForTimeout(50); }
  }
  assert.deepEqual(actual, expected, message);
}
async function resetFocus(page) {
  await page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
  await key(page, "Escape");
}
async function calculate(page, expression, expectedLength) {
  await resetFocus(page);
  await key(page, "Control+Alt+4");
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 3000 });
  await page.keyboard.press("Control+a");
  await page.keyboard.type(expression, { delay: 20 });
  await key(page, "Enter");
  await page.waitForFunction(({ key, expression, length }) => {
    const state = JSON.parse(localStorage.getItem(key));
    return state.history.length === length && state.history.at(-1).expression === expression;
  }, { key: storageKey, expression, length: expectedLength });
}
async function undoKey(page) { await key(page, "Escape"); await key(page, "Control+z"); }
async function undoButton(page) {
  await resetFocus(page);
  // A nonempty journal enables Undo immediately after the header's Data control.
  await tab(page, 2);
  await key(page, "Space");
}
async function tasks(page) {
  await resetFocus(page);
  await key(page, "Control+Alt+4");
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 3000 });
  await key(page, "Escape");
  await key(page, "ArrowLeft");
  await key(page, "Control+m");
}
async function stl(page) {
  if (!expanded.get(page)) {
    await resetFocus(page);
    await key(page, "Control+Alt+5");
    await key(page, "Control+m");
  }
  await key(page, "Control+Alt+5");
  await page.waitForTimeout(250);
}
async function readReport(page) {
  await page.evaluate(() => navigator.clipboard.writeText("Undo report pending"));
  await stl(page);
  // The shortcut focuses Open STL; Sample, Clear, then Copy follow it.
  await tab(page, 3);
  await key(page, "Space");
  const deadline = Date.now() + 3000;
  while (Date.now() < deadline) {
    const report = await page.evaluate(() => navigator.clipboard.readText());
    if (report.startsWith("Jig STL report\n")) return report;
    await page.waitForTimeout(50);
  }
  assert.fail("The visible Copy report control did not produce an STL report");
}
async function exportData(page, hasUndo) {
  await resetFocus(page);
  await key(page, "Control+Alt+d");
  // Data, optional Undo, Start empty, Open local, Sample mode, Import, Export.
  await tab(page, hasUndo ? 7 : 6);
  const downloadEvent = page.waitForEvent("download", { timeout: 5000 });
  await key(page, "Space");
  const download = await downloadEvent;
  assert.equal(download.suggestedFilename(), "jig.json");
  const exported = JSON.parse(await readFile(await download.path(), "utf8"));
  await key(page, "Control+Alt+d");
  return exported;
}
function cube() {
  const points = [[0, 0, 0], [10, 0, 0], [10, 10, 0], [0, 10, 0], [0, 0, 10], [10, 0, 10], [10, 10, 10], [0, 10, 10]];
  const faces = [[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [3, 7, 6], [3, 6, 2], [0, 4, 7], [0, 7, 3], [1, 2, 6], [1, 6, 5]];
  const bytes = Buffer.alloc(84 + faces.length * 50);
  bytes.write("Sample: undo cube");
  bytes.writeUInt32LE(faces.length, 80);
  faces.forEach((face, index) => face.forEach((point, vertex) => points[point].forEach((value, axis) => {
    bytes.writeFloatLE(value, 84 + index * 50 + 12 + vertex * 12 + axis * 4);
  })));
  return bytes;
}

try {
  progress("undo calculator results while preserving precision and text-editor undo");
  current = await open("undo-calculator");
  await calculate(current.page, "1 / 3", 2);
  await calculate(current.page, "ans * 3", 3);
  await undoKey(current.page);
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).history.length === 2, storageKey);
  assert.equal((await state(current.page)).history.at(-1).result, 1 / 3);
  await calculate(current.page, "ans * 3", 3);
  assert.equal((await state(current.page)).history.at(-1).result, 1);
  await undoButton(current.page);
  assert.equal((await state(current.page)).history.at(-1).result, 1 / 3);
  const beforeTyping = await state(current.page);
  await key(current.page, "Control+Alt+4");
  await current.page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 3000 });
  await current.page.keyboard.type("12 + 3", { delay: 30 });
  await key(current.page, "Control+z");
  assert.deepEqual(await state(current.page), beforeTyping, "Ctrl-Z inside an editor must not pop global history");
  await undoKey(current.page);
  await expectState(current.page, fixture, "Global undo must resume after leaving the text editor");
  await current.context.close();

  progress("undo task deletion, completion, date edits and color changes with stable links");
  current = await open("undo-dated-task");
  await tasks(current.page);
  await tab(current.page, 8); // One dated row: Data, Source, star, checkbox, color, date, edit, del.
  await key(current.page, "Space");
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).tasks.length === 0, storageKey);
  assert.equal((await state(current.page)).deadlines.length, 0);
  await undoButton(current.page);
  await expectState(current.page, fixture, "Undo must restore task, linked date, color and featured selection exactly");
  await tasks(current.page);
  await tab(current.page, 4); // Empty journal again, first checkbox.
  await key(current.page, "Space");
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).tasks[0].complete, storageKey);
  assert.equal((await state(current.page)).featured, null);
  await undoKey(current.page);
  await expectState(current.page, fixture, "Completion undo must restore the featured dated task");
  await tasks(current.page);
  await tab(current.page, 6);
  await key(current.page, "Space");
  await current.page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 3000 });
  await current.page.keyboard.press("Control+a");
  await current.page.keyboard.type("2030-01-16T18:00:00Z", { delay: 20 });
  await key(current.page, "Enter");
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).deadlines[0].at === "2030-01-16T18:00:00Z", storageKey);
  await undoKey(current.page);
  await expectState(current.page, fixture, "Date undo must move the existing linked deadline back without copying it");
  await tasks(current.page);
  await tab(current.page, 5);
  await key(current.page, "Space");
  // Date, edit, del, None, Rose, Amber.
  await tab(current.page, 6);
  await key(current.page, "Space");
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).tasks[0].color === "amber", storageKey);
  await undoKey(current.page);
  await expectState(current.page, fixture, "Color undo must restore the original task data");
  await capture(current.page, "undo-desktop");
  await current.context.close();

  progress("undo bulk completed-task removal including dates and hidden notes");
  const bulk = structuredClone(fixture);
  bulk.tasks = [
    { id: 1, text: "Sample: completed casting", complete: true, deadline: 101, color: "rose" },
    { id: 2, text: "Sample: completed labels", complete: true, deadline: 102, color: "sage" },
    { id: 3, text: "Sample: unfinished exhibition", complete: false, deadline: 103, color: "lilac" },
  ];
  bulk.deadlines = bulk.tasks.map(task => ({ id: task.deadline, title: task.text, at: "2030-01-15T18:00:00Z" }));
  bulk.featured = 103;
  current = await open("undo-bulk", bulk);
  await tasks(current.page);
  // Disabled stars and edge reorder buttons are skipped in this fixed three-row fixture.
  await tab(current.page, 25);
  await key(current.page, "Space");
  await tasks(current.page); // Reset focus; the inline confirmation stays open.
  await tab(current.page, 25); // Confirm replaces the old bulk button at this position.
  await key(current.page, "Space");
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).tasks.length === 1, storageKey);
  assert.equal((await state(current.page)).tasks[0].id, 3);
  await undoButton(current.page);
  await expectState(current.page, bulk, "Bulk undo must restore order, completion, colors, linked IDs and notes");
  await current.context.close();

  progress("reverse STL and calculator actions in one chronological history");
  current = await open("undo-cross-applet");
  await stl(current.page);
  const emptyModel = await current.page.screenshot({ ...screenshotOptions, clip: { x: 50, y: 180, width: 700, height: 600 } });
  await current.page.locator("#jig-stl-import").setInputFiles({ name: "Sample-undo-cube.stl", mimeType: "model/stl", buffer: cube() });
  await current.page.waitForTimeout(1300);
  assert.match(await readReport(current.page), /Total volume: 1000\.000000 mm³/);
  await stl(current.page);
  await tab(current.page, 16); // Open STL -> physical scale, independent of header controls.
  await current.page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 3000 });
  await current.page.keyboard.press("Control+a");
  await current.page.keyboard.type("200", { delay: 30 });
  await key(current.page, "Enter");
  assert.match(await readReport(current.page), /Total volume: 8000\.000000 mm³/);
  await calculate(current.page, "7 + 8", 2);
  await undoKey(current.page);
  await expectState(current.page, fixture, "The newest calculator action must undo before the earlier model scale");
  assert.match(await readReport(current.page), /Total volume: 8000\.000000 mm³/);
  await key(current.page, "Control+z");
  assert.match(await readReport(current.page), /Total volume: 1000\.000000 mm³/);
  assert.deepEqual(await state(current.page), fixture, "Viewer undo must not rewrite workbench state");
  await key(current.page, "Control+z"); // The initial local-file open is the remaining entry.
  await paint(current.page);
  assert.deepEqual(await current.page.screenshot({ ...screenshotOptions, clip: { x: 50, y: 180, width: 700, height: 600 } }), emptyModel, "Undoing the file open must restore the empty viewer");
  await capture(current.page, "undo-viewer-cleared");
  await current.context.close();

  progress("discard undo history on reload and when switching datasets");
  current = await open("undo-dataset-boundary");
  await calculate(current.page, "7 + 8", 2);
  const savedPersonal = await state(current.page);
  await boot(current.page, true);
  await undoKey(current.page);
  assert.deepEqual(await state(current.page), savedPersonal, "Reload must retain data while clearing session undo");
  await calculate(current.page, "9 + 1", 3);
  const latestPersonal = await state(current.page);
  await resetFocus(current.page);
  await key(current.page, "Control+Alt+d");
  await tab(current.page, 5); // Data, Undo, Start empty, Open local, Sample mode.
  await key(current.page, "Space");
  await key(current.page, "Control+Alt+d"); // Collapse before using the export helper.
  const sampleBeforeUndo = await exportData(current.page, false);
  assert.notDeepEqual(sampleBeforeUndo.tasks, latestPersonal.tasks);
  await undoKey(current.page);
  assert.deepEqual(await exportData(current.page, false), sampleBeforeUndo, "Sample mode must not undo back into a personal dataset");
  assert.deepEqual(await state(current.page), latestPersonal, "Sample edits and exports must preserve the saved personal dataset");
  await key(current.page, "Control+Alt+o");
  await undoKey(current.page);
  assert.deepEqual(await state(current.page), latestPersonal, "Reopening local data must also start a fresh undo session");
  await current.context.close();

  progress("preserve newer-tab and corrupt saved files when undo cannot save");
  for (const corrupt of [false, true]) {
    current = await open(corrupt ? "undo-corrupt-preserved" : "undo-stale-tab");
    await calculate(current.page, "7 + 8", 2);
    const remote = structuredClone(fixture);
    remote.history.push({ expression: "9 + 1", result: 10 });
    const remoteRaw = corrupt ? "{broken sample state" : JSON.stringify(remote);
    await current.page.evaluate(({ key, raw }) => localStorage.setItem(key, raw), { key: storageKey, raw: remoteRaw });
    if (corrupt) { await key(current.page, "Control+Alt+o"); await paint(current.page); }
    await undoKey(current.page);
    assert.equal(await raw(current.page), remoteRaw, "Undo must not overwrite newer or undecodable saved data");
    assert.deepEqual(await exportData(current.page, false), fixture, "A blocked save must leave the undone work available for local export");
    assert.equal(await raw(current.page), remoteRaw);
    await current.context.close();
  }
  assert.deepEqual(issues, [], "Undo tests must not emit page errors or contact external services");
  console.log("Undo browser passed: global button and Ctrl-Z, precise calculator history, native text undo isolation, dated-task deletion/completion/date/color restoration, bulk linked-record restoration, chronological STL/data undo, reload/dataset boundaries, and stale/corrupt-file preservation.");
} catch (error) {
  console.error(`Undo browser failed during: ${phase}`);
  if (current && !current.page.isClosed()) await capture(current.page, "undo-failure").catch(() => {});
  throw error;
} finally {
  progress("close undo contexts and Chromium");
  let cleanupTimer;
  let cleanupFailed = false;
  try {
    await Promise.race([
      Promise.all([
        ...contexts.map(context => context.close().catch(() => {})),
        browser.close(),
      ]),
      new Promise((_, reject) => {
        cleanupTimer = setTimeout(() => reject(new Error("Undo browser shutdown exceeded 10 seconds")), 10000);
      }),
    ]);
  } catch (error) {
    cleanupFailed = true;
    console.error("Undo browser cleanup failed:", error);
  } finally {
    clearTimeout(cleanupTimer);
    clearTimeout(timer);
  }
  // A stuck Chromium socket can keep Node alive after a rejected close promise.
  if (cleanupFailed) process.exit(1);
}

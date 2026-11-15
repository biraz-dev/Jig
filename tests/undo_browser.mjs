// Real browser checks for session Undo/Redo. Every saved record is synthetic.
// Run against a built preview served at /Jig/: node tests/undo_browser.mjs
import assert from "node:assert/strict";
import { mkdir, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium } from "playwright";
import { installFrameFence, waitForAppPaint } from "./browser_frames.mjs";

const base = process.env.JIG_PREVIEW_URL ?? "http://127.0.0.1:4173/Jig/";
const origin = new URL(base).origin;
const storageKey = "jig.personal.v1";
const results = resolve("playwright-results");
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
let phase = "launch Chromium", current;
const contexts = [], issues = [];
// Counts describe actions sent through the public UI, only to navigate enabled
// Undo/Redo header buttons. They never read the application's private journal.
const journals = new WeakMap();
const timer = setTimeout(() => {
  console.error(`Undo/Redo browser checks exceeded 300 seconds during: ${phase}`);
  process.exit(1);
}, 300000);
function progress(message) { phase = message; console.log(`Undo/Redo browser phase: ${message}`); }
async function paint(page) { await waitForAppPaint(page); }
async function key(page, value) { await page.keyboard.press(value); await paint(page); }
async function tab(page, count = 1) { for (let index = 0; index < count; index++) await key(page, "Tab"); }
function journal(page) { return journals.get(page); }
function mutation(page) { const item = journal(page); item.undo++; item.redo = 0; }
function undoRecorded(page) { const item = journal(page); if (item.undo) { item.undo--; item.redo++; } }
function redoRecorded(page) { const item = journal(page); if (item.redo) { item.redo--; item.undo++; } }
function header(page) { const item = journal(page); return 3 + Number(item.undo > 0) + Number(item.redo > 0); }
async function capture(page, name) {
  const png = await page.screenshot({ path: resolve(results, `${name}.png`), timeout: 60000, animations: "disabled", caret: "hide" });
  if (new Set((process.env.JIG_RENDER_LOGS ?? "").split(",")).has(name) && png.length <= 1024 * 1024) {
    console.log(`JIG_RENDER_BEGIN_${name}`);
    const encoded = png.toString("base64");
    for (let offset = 0; offset < encoded.length; offset += 2048) console.log(encoded.slice(offset, offset + 2048));
    console.log(`JIG_RENDER_END_${name}`);
  }
}
const browser = await chromium.launch({ headless: true, args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"] });
async function open(label, initial = fixture) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 1000 }, acceptDownloads: true, timezoneId: "Etc/UTC" });
  contexts.push(context);
  await installFrameFence(context);
  await context.addInitScript(({ raw, key, origin }) => {
    if (location.origin === origin && sessionStorage.getItem("jig-undo-seeded") !== "yes") {
      localStorage.setItem(key, raw);
      localStorage.setItem("jig.workspace.v1", "tasks");
      sessionStorage.setItem("jig-undo-seeded", "yes");
    }
  }, { raw: JSON.stringify(initial), key: storageKey, origin });
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
  if (reload) await page.reload({ waitUntil: "load" }); else await page.goto(base, { waitUntil: "load" });
  await page.bringToFront();
  await page.waitForFunction(() => window.jigReady === true || typeof window.jigBootError === "string", undefined, { timeout: 60000 });
  assert.equal(await page.evaluate(() => window.jigBootError), undefined);
  await page.waitForTimeout(500);
  journals.set(page, { undo: 0, redo: 0 });
  await tasks(page);
}
async function state(page) { return page.evaluate(key => JSON.parse(localStorage.getItem(key)), storageKey); }
async function raw(page) { return page.evaluate(key => localStorage.getItem(key), storageKey); }
async function expectState(page, expected, message) {
  const until = Date.now() + 10000; let actual;
  do { actual = await state(page); try { assert.deepEqual(actual, expected); return; } catch { await page.waitForTimeout(50); } } while (Date.now() < until);
  assert.deepEqual(actual, expected, message);
}
async function resetFocus(page) {
  const { width, height } = page.viewportSize();
  await page.locator("#jig_canvas").click({ position: { x: width - 6, y: height - 6 } });
  await key(page, "Escape");
}
async function tasks(page) { await resetFocus(page); await key(page, "Control+Alt+2"); }
async function calculate(page, expression, expectedLength) {
  await resetFocus(page); await key(page, "Control+Alt+4");
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 25 });
  await page.keyboard.press("Control+a"); await page.keyboard.type(expression, { delay: 15 }); await key(page, "Enter");
  await page.waitForFunction(({ key, expression, length }) => {
    const data = JSON.parse(localStorage.getItem(key)); return data.history.length === length && data.history.at(-1).expression === expression;
  }, { key: storageKey, expression, length: expectedLength });
  mutation(page);
}
async function undoKey(page) { await resetFocus(page); await key(page, "Control+z"); undoRecorded(page); }
async function redoKey(page) { await resetFocus(page); await key(page, "Control+Shift+z"); redoRecorded(page); }
async function undoButton(page) {
  await resetFocus(page); assert.ok(journal(page).undo > 0);
  await tab(page, 2 + Number(journal(page).redo > 0)); // Data, optional Redo, Undo.
  await key(page, "Space"); undoRecorded(page);
}
async function redoButton(page) {
  await resetFocus(page); assert.ok(journal(page).redo > 0);
  await tab(page, 2); // Data, Redo.
  await key(page, "Space"); redoRecorded(page);
}
async function taskControl(page, offset) {
  await tasks(page);
  await tab(page, header(page) + 4 + offset); // One Tasks tab/+, Gantt/Save, then enabled row controls.
}
async function exportData(page) {
  await tasks(page); await key(page, "Control+Alt+d");
  await tab(page, 7 + Number(journal(page).undo > 0) + Number(journal(page).redo > 0));
  const event = page.waitForEvent("download"); await key(page, "Space"); const download = await event;
  assert.equal(download.suggestedFilename(), "jig.json");
  const data = JSON.parse(await readFile(await download.path(), "utf8"));
  await key(page, "Control+Alt+d"); return data;
}

try {
  progress("redo precise calculator history through keyboard and header controls");
  current = await open("undo-redo-calculator");
  await calculate(current.page, "1 / 3", 2); const fractional = await state(current.page);
  await calculate(current.page, "ans * 3", 3); const whole = await state(current.page);
  await undoKey(current.page); await expectState(current.page, fractional);
  await capture(current.page, "redo-desktop");
  await redoKey(current.page); await expectState(current.page, whole, "Ctrl-Shift-Z must redo, never be consumed as another Undo");
  await undoButton(current.page); await expectState(current.page, fractional);
  await redoButton(current.page); await expectState(current.page, whole);
  await undoKey(current.page); await undoKey(current.page); await expectState(current.page, fixture);
  await redoKey(current.page); await expectState(current.page, fractional);
  await redoKey(current.page); await expectState(current.page, whole);

  progress("new edits discard Redo while text editors keep their own Undo/Redo");
  await undoKey(current.page); await calculate(current.page, "72 in mm", 3);
  const branched = await state(current.page);
  assert.equal(branched.history.at(-1).result, 1828.8); assert.equal(branched.history.at(-1).unit, "mm");
  await redoKey(current.page); await expectState(current.page, branched, "A new calculation must invalidate the old Redo branch");
  await key(current.page, "Control+Alt+4");
  await current.page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { polling: 25 });
  await current.page.keyboard.type("12 + 3", { delay: 30 });
  await key(current.page, "Control+z");
  await expectState(current.page, branched, "Native text Undo must leave application history untouched");
  await key(current.page, "Control+Shift+z");
  await expectState(current.page, branched, "Native text Redo must leave application history untouched");
  await undoKey(current.page); await expectState(current.page, fractional);
  await redoKey(current.page); await expectState(current.page, branched);
  await capture(current.page, "undo-desktop"); await current.context.close();

  progress("restore task deletion, completion, color and featured data with Undo/Redo");
  current = await open("undo-redo-task");
  await taskControl(current.page, 6); await key(current.page, "Space");
  const deleted = { ...structuredClone(fixture), tasks: [], deadlines: [], featured: null };
  await expectState(current.page, deleted); mutation(current.page);
  await undoButton(current.page); await expectState(current.page, fixture);
  await redoButton(current.page); await expectState(current.page, deleted);
  await undoKey(current.page); await expectState(current.page, fixture);
  await taskControl(current.page, 2); await key(current.page, "Space");
  const completed = structuredClone(fixture); completed.tasks[0].complete = true; completed.featured = null;
  await expectState(current.page, completed); mutation(current.page);
  await undoKey(current.page); await expectState(current.page, fixture);
  await redoKey(current.page); await expectState(current.page, completed);
  await undoKey(current.page); await expectState(current.page, fixture);
  await taskControl(current.page, 3); await key(current.page, "Space");
  await tab(current.page, 6); await key(current.page, "Space"); // Date, edit, del, None, Rose, Amber.
  const amber = structuredClone(fixture); amber.tasks[0].color = "amber";
  await expectState(current.page, amber); mutation(current.page);
  await undoKey(current.page); await expectState(current.page, fixture);
  await redoKey(current.page); await expectState(current.page, amber);
  await current.context.close();

  progress("undo and redo bulk completed-task removal atomically");
  const bulk = structuredClone(fixture);
  bulk.tasks = [
    { id: 1, text: "Sample: completed casting", complete: true, deadline: 101, color: "rose" },
    { id: 2, text: "Sample: completed labels", complete: true, deadline: 102, color: "sage" },
    { id: 3, text: "Sample: unfinished exhibition", complete: false, deadline: 103, color: "lilac" },
  ];
  bulk.deadlines = bulk.tasks.map(task => ({ id: task.deadline, title: task.text, at: "2030-01-15T18:00:00Z" })); bulk.featured = 103;
  current = await open("undo-redo-bulk", bulk);
  await tasks(current.page); await tab(current.page, header(current.page) + 27); await key(current.page, "Space");
  await tasks(current.page); await tab(current.page, header(current.page) + 27); await key(current.page, "Space");
  const cleared = { ...structuredClone(bulk), tasks: [bulk.tasks[2]], deadlines: [bulk.deadlines[2]] };
  await expectState(current.page, cleared); mutation(current.page);
  await undoKey(current.page); await expectState(current.page, bulk);
  await redoKey(current.page); await expectState(current.page, cleared);
  await current.context.close();

  progress("clear both journals on reload and dataset boundaries");
  current = await open("undo-redo-boundaries");
  await calculate(current.page, "7 + 8", 2); const savedPersonal = await state(current.page);
  await undoKey(current.page); await boot(current.page, true);
  await redoKey(current.page); await undoKey(current.page); await expectState(current.page, fixture, "Reload must retain saved data and discard session Undo and Redo");
  await calculate(current.page, "7 + 8", 2); await tasks(current.page); await key(current.page, "Control+Alt+d");
  await tab(current.page, 5 + Number(journal(current.page).undo > 0) + Number(journal(current.page).redo > 0));
  await key(current.page, "Space"); journals.set(current.page, { undo: 0, redo: 0 });
  await key(current.page, "Control+Alt+d");
  const sample = await exportData(current.page);
  assert.notDeepEqual(sample.tasks, savedPersonal.tasks);
  await undoKey(current.page); await redoKey(current.page);
  assert.deepEqual(await exportData(current.page), sample, "Sample mode must not traverse into a personal journal");
  assert.deepEqual(await state(current.page), savedPersonal, "Sample operations must leave the personal dataset intact");
  await key(current.page, "Control+Alt+o"); journals.set(current.page, { undo: 0, redo: 0 });
  await undoKey(current.page); await redoKey(current.page); await expectState(current.page, savedPersonal);
  await calculate(current.page, "9 + 1", 3); await undoKey(current.page);
  const imported = { ...structuredClone(fixture), notes: [{ id: 401, text: "Sample: imported note", pinned: true }] };
  await current.page.locator("#jig-import").setInputFiles({ name: "sample-redo-boundary.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(imported)) });
  await current.page.waitForTimeout(1200); await expectState(current.page, savedPersonal, "Import must wait for confirmation");
  await resetFocus(current.page);
  await tab(current.page, 3 + Number(journal(current.page).undo > 0) + Number(journal(current.page).redo > 0));
  await key(current.page, "Space"); await expectState(current.page, imported); journals.set(current.page, { undo: 0, redo: 0 });
  await undoKey(current.page); await redoKey(current.page);
  await expectState(current.page, imported, "Confirmed JSON import must clear both journals instead of crossing dataset boundaries");
  await current.context.close();

  progress("preserve newer-tab and corrupt files through Undo and Redo save failures");
  for (const corrupt of [false, true]) {
    current = await open(corrupt ? "redo-corrupt-preserved" : "redo-stale-tab");
    await calculate(current.page, "7 + 8", 2); const calculated = await state(current.page);
    const remote = structuredClone(fixture); remote.history.push({ expression: "9 + 1", result: 10 });
    const remoteRaw = corrupt ? "{broken sample state" : JSON.stringify(remote);
    await current.page.evaluate(({ key, raw }) => localStorage.setItem(key, raw), { key: storageKey, raw: remoteRaw });
    if (corrupt) await key(current.page, "Control+Alt+o");
    await undoKey(current.page); assert.equal(await raw(current.page), remoteRaw);
    assert.deepEqual(await exportData(current.page), fixture, "A blocked save must leave undone work available for export");
    await redoKey(current.page); assert.equal(await raw(current.page), remoteRaw);
    assert.deepEqual(await exportData(current.page), calculated, "Redo must preserve the raw conflict and make restored work available for export");
    await current.context.close();
  }
  assert.deepEqual(issues, [], "Undo/Redo must stay local and emit no browser errors");
  console.log("Undo/Redo browser passed: keyboard and buttons, precise calculator data, multi-step history, new-edit branching, text-local isolation, linked task and bulk restoration, reload/dataset boundaries, and stale/corrupt-file preservation.");
} catch (error) {
  console.error(`Undo/Redo browser failed during: ${phase}`, error);
  if (current && !current.page.isClosed()) await capture(current.page, "undo-failure").catch(() => {});
  throw error;
} finally {
  clearTimeout(timer);
  let cleanupTimer;
  try {
    await Promise.race([
      Promise.all([...contexts.map(context => context.close().catch(() => {})), browser.close()]),
      new Promise((_, reject) => { cleanupTimer = setTimeout(() => reject(new Error("Undo/Redo browser shutdown exceeded 10 seconds")), 10000); }),
    ]);
  } catch (error) { console.error(error); process.exit(1); }
  finally { clearTimeout(cleanupTimer); }
}

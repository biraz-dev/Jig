// Real Chromium checks for the five task-list tabs. Every record is synthetic.
// Run against a built preview served at /Jig/: node tests/task_lists_browser.mjs
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
const legacy = {
  version: 1,
  preferences: { decimals: 2, focus: 3, weather: null },
  history: [{ expression: "2 + 2", result: 4 }],
  tasks: [{ id: 1, text: "Sample: cast studies", complete: false, deadline: 101, color: "rose", start: "2026-10-26T12:34:56Z" }],
  deadlines: [{ id: 101, title: "Sample: cast studies", at: "2026-10-30T18:00:00Z" }],
  entries: [{ id: 301, title: "Sample: hidden studio note", date: "2026-10-30" }],
  featured: 101,
};
const fixture = {
  ...structuredClone(legacy),
  task_lists: [{ id: 1, name: "Studio" }, { id: 2, name: "Home" }, { id: 3, name: "Show" }],
  tasks: [
    ...structuredClone(legacy.tasks),
    { id: 2, text: "Sample: photograph work", complete: false, list_id: 2, deadline: 102, color: "amber", start: "2026-10-26T12:00:00Z" },
    { id: 3, text: "Sample: prepare exhibition", complete: false, list_id: 3, deadline: 103, color: "sage", start: "2026-10-26T12:00:00Z" },
  ],
  deadlines: [
    ...structuredClone(legacy.deadlines),
    { id: 102, title: "Sample: photograph work", at: "2026-10-30T18:00:00Z" },
    { id: 103, title: "Sample: prepare exhibition", at: "2026-10-30T18:00:00Z" },
  ],
  featured: 103,
};
const issues = [];
const contexts = [];
let phase = "launch Chromium";
let current;
const timeout = setTimeout(() => {
  console.error(`Task-list browser checks exceeded 420 seconds during: ${phase}`);
  process.exit(1);
}, 420000);
function progress(message) { phase = message; console.log(`Task-list browser phase: ${message}`); }
async function paint(page) { await waitForAppPaint(page); }
async function key(page, value) { await page.keyboard.press(value); await paint(page); }
async function tab(page, count = 1) { for (let index = 0; index < count; index++) await key(page, "Tab"); }
async function state(page) { return page.evaluate(key => JSON.parse(localStorage.getItem(key)), storageKey); }
async function raw(page) { return page.evaluate(key => localStorage.getItem(key), storageKey); }
async function expectState(page, expected, message) {
  const until = Date.now() + 10000;
  let actual;
  do {
    actual = await state(page);
    try { assert.deepEqual(actual, expected); return; } catch { await page.waitForTimeout(50); }
  } while (Date.now() < until);
  assert.deepEqual(actual, expected, message);
}
async function capture(page, name) {
  const png = await page.screenshot({ path: resolve(results, `${name}.png`), timeout: 60000, animations: "disabled", caret: "hide" });
  const requested = new Set((process.env.JIG_RENDER_LOGS ?? "").split(","));
  if (requested.has(name) && png.length <= 1024 * 1024) {
    console.log(`JIG_RENDER_BEGIN_${name}`);
    const encoded = png.toString("base64");
    for (let offset = 0; offset < encoded.length; offset += 2048) console.log(encoded.slice(offset, offset + 2048));
    console.log(`JIG_RENDER_END_${name}`);
  }
  return png;
}
const browser = await chromium.launch({ headless: true, args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"] });
async function open(label, data = fixture, phone = false) {
  const context = await browser.newContext({ viewport: phone ? { width: 390, height: 844 } : { width: 1440, height: 1000 }, hasTouch: phone, acceptDownloads: true, timezoneId: "Etc/UTC" });
  contexts.push(context);
  await installFrameFence(context);
  await context.addInitScript(({ initial, key, origin }) => {
    const NativeDate = Date;
    const fixed = NativeDate.parse("2026-10-30T12:00:00Z");
    const now = () => fixed;
    globalThis.Date = new Proxy(NativeDate, {
      apply: () => new NativeDate(fixed).toString(),
      construct: (target, args, newTarget) => Reflect.construct(target, args.length ? args : [fixed], newTarget),
      get: (target, property, receiver) => property === "now" ? now : Reflect.get(target, property, receiver),
    });
    if (location.origin === origin && sessionStorage.getItem("jig-lists-seeded") !== "yes") {
      localStorage.setItem(key, initial);
      sessionStorage.setItem("jig-lists-seeded", "yes");
    }
  }, { initial: JSON.stringify(data), key: storageKey, origin });
  await context.route("**/*", async route => {
    const url = new URL(route.request().url());
    if (["blob:", "data:", "about:"].includes(url.protocol) || url.origin === origin) await route.continue();
    else { issues.push(`${label}: unexpected request ${url.origin}${url.pathname}`); await route.abort(); }
  });
  const page = await context.newPage();
  page.on("pageerror", error => issues.push(`${label}: ${error.message}`));
  page.setDefaultTimeout(15000);
  const scene = { context, page, undo: false };
  await boot(scene);
  return scene;
}
async function boot(scene, reload = false) {
  if (reload) await scene.page.reload({ waitUntil: "load" });
  else await scene.page.goto(base, { waitUntil: "load" });
  await scene.page.bringToFront();
  await scene.page.waitForFunction(() => window.jigReady === true || typeof window.jigBootError === "string", undefined, { timeout: 60000 });
  assert.equal(await scene.page.evaluate(() => window.jigBootError), undefined);
  await scene.page.waitForTimeout(500);
  scene.undo = false;
  await resetTasks(scene);
}
async function resetTasks(scene, escape = true) {
  await scene.page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
  if (escape) await key(scene.page, "Escape");
  await key(scene.page, "Control+Alt+2");
}
async function lists(scene) { return (await state(scene.page)).task_lists ?? [{ id: 1, name: "Tasks" }]; }
function headerTargets(scene) { return scene.undo ? 3 : 2; } // Data, optional Undo, Source.
async function focusList(scene, index) {
  await resetTasks(scene);
  await tab(scene.page, headerTargets(scene) + index + 1);
}
async function selectList(scene, index, combine = false) {
  await focusList(scene, index);
  await key(scene.page, combine ? "Shift+Space" : "Space");
}
async function focusListControl(scene, control) {
  await resetTasks(scene);
  const count = (await lists(scene)).length;
  const add = count < 5 ? 1 : 0;
  const offset = control === "+" ? 1 : control === "−" ? add + 1 : add + 2;
  await tab(scene.page, headerTargets(scene) + count + offset);
}
async function replaceEditor(page, text) {
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 25 });
  await page.keyboard.press("Control+a");
  await page.keyboard.type(text, { delay: 15 });
  await paint(page);
}
async function renameList(scene, index, name) {
  await focusList(scene, index);
  await key(scene.page, "F2");
  await replaceEditor(scene.page, name);
  await key(scene.page, "Enter");
  await scene.page.waitForFunction(({ key, index, name }) => JSON.parse(localStorage.getItem(key)).task_lists?.[index]?.name === name, { key: storageKey, index, name });
  scene.undo = true;
}
async function focusedControlPoint(page) {
  // The ordinary keyboard-focused button has Jig's active graphite fill.
  // Read its visible pixels; no Rust/widget state is exposed to this driver.
  const png = await page.screenshot({ timeout: 60000, animations: "disabled", caret: "hide" });
  const point = await page.evaluate(async encoded => {
    const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
    const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height), ctx = canvas.getContext("2d"); ctx.drawImage(bitmap, 0, 0);
    const pixels = ctx.getImageData(0, 0, bitmap.width, bitmap.height).data, seen = new Uint8Array(bitmap.width * bitmap.height), regions = [];
    const matches = index => [77, 49, 39].every((value, axis) => Math.abs(pixels[index * 4 + axis] - value) <= 3);
    for (let y = 42; y < Math.min(180, bitmap.height); y++) for (let x = 6; x < bitmap.width - 6; x++) {
      const start = y * bitmap.width + x;
      if (seen[start] || !matches(start)) continue;
      const stack = [start], r = { left: x, right: x, top: y, bottom: y, count: 0 }; seen[start] = 1;
      while (stack.length) {
        const index = stack.pop(), px = index % bitmap.width, py = Math.floor(index / bitmap.width);
        r.left = Math.min(r.left, px); r.right = Math.max(r.right, px); r.top = Math.min(r.top, py); r.bottom = Math.max(r.bottom, py); r.count++;
        for (const [nx, ny] of [[px - 1, py], [px + 1, py], [px, py - 1], [px, py + 1]]) {
          if (nx < 0 || nx >= bitmap.width || ny < 42 || ny >= 180) continue;
          const next = ny * bitmap.width + nx;
          if (!seen[next] && matches(next)) { seen[next] = 1; stack.push(next); }
        }
      }
      if (r.count > 100 && r.bottom - r.top >= 10 && r.bottom - r.top <= 40 && r.right - r.left < 180) regions.push(r);
    }
    bitmap.close();
    const r = regions.sort((a, b) => a.top - b.top || b.count - a.count)[0];
    return r ? { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 } : null;
  }, png.toString("base64"));
  assert.ok(point, "A focused list control must be visibly identifiable for touch/cap checks");
  return point;
}
async function bars(page) {
  const png = await page.screenshot({ timeout: 60000, animations: "disabled", caret: "hide" });
  return page.evaluate(async encoded => {
    const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
    const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height), ctx = canvas.getContext("2d"); ctx.drawImage(bitmap, 0, 0);
    const pixels = ctx.getImageData(0, 0, bitmap.width, bitmap.height).data;
    const result = {};
    for (const [name, color] of Object.entries({ rose: [234, 133, 173], amber: [218, 185, 126], sage: [148, 188, 160] })) {
      const groups = [];
      for (let y = 170; y < bitmap.height - 45; y++) {
        let left = bitmap.width, right = 0, count = 0;
        for (let x = Math.floor(bitmap.width * (bitmap.width < 500 ? 0.34 : 0.45)); x < bitmap.width - 25; x++) {
          const offset = (y * bitmap.width + x) * 4;
          if (color.every((value, axis) => Math.abs(pixels[offset + axis] - value) <= 3)) { left = Math.min(left, x); right = x; count++; }
        }
        if (count < 16) continue;
        let g = groups.at(-1);
        if (!g || y > g.bottom + 1) { g = { top: y, bottom: y, left, right }; groups.push(g); }
        g.bottom = y; g.left = Math.min(g.left, left); g.right = Math.max(g.right, right);
      }
      result[name] = groups.find(g => g.bottom - g.top >= 5) ?? null;
    }
    for (const [name, color] of Object.entries({ amber: [44, 39, 35], sage: [50, 44, 39] })) {
      const band = result[name]; let count = 0;
      if (band) for (let y = band.top - 10; y <= band.bottom + 10; y++) for (let x = band.left - 150; x < band.left - 12; x++) {
        const offset = (y * bitmap.width + x) * 4;
        if (color.every((value, axis) => Math.abs(pixels[offset + axis] - value) <= 1)) count++;
      }
      result[`${name}Shade`] = count;
    }
    bitmap.close(); return result;
  }, png.toString("base64"));
}
async function expectBars(page, names) {
  const until = Date.now() + 10000; let actual;
  do {
    actual = await bars(page);
    if (["rose", "amber", "sage"].every(name => Boolean(actual[name]) === names.includes(name))) return actual;
  } while (Date.now() < until);
  assert.deepEqual(["rose", "amber", "sage"].filter(name => actual[name]), names, "The Gantt must show exactly the selected lists' task bars");
}
async function focusRowControl(scene, offset) {
  await resetTasks(scene);
  const count = (await lists(scene)).length;
  const strip = count + (count > 1 ? (count < 5 ? 3 : 2) : 1);
  await tab(scene.page, headerTargets(scene) + strip + 2 + offset); // Gantt chart, Save .ods.
}
async function undo(scene, expected) {
  await key(scene.page, "Escape"); await key(scene.page, "Control+z");
  await expectState(scene.page, expected, "Undo must restore list membership and linked records together");
  // Callers retain an earlier mutation in their journal, except after reload.
}
async function exportData(scene) {
  await resetTasks(scene);
  await key(scene.page, "Control+Alt+d");
  await tab(scene.page, scene.undo ? 7 : 6);
  const event = scene.page.waitForEvent("download");
  await key(scene.page, "Space");
  const download = await event;
  assert.equal(download.suggestedFilename(), "jig.json");
  const data = JSON.parse(await readFile(await download.path(), "utf8"));
  await key(scene.page, "Control+Alt+d");
  return data;
}

try {
  progress("load an older file, create five named lists and enforce the cap");
  current = await open("task-lists-legacy", legacy);
  assert.equal(await raw(current.page), JSON.stringify(legacy), "Opening older work must not rewrite it or duplicate its tasks");
  for (const name of ["Home", "Show", "Costs", "Notes"]) {
    await focusListControl(current, "+");
    const addPoint = await focusedControlPoint(current.page);
    await key(current.page, "Space");
    await replaceEditor(current.page, name);
    await key(current.page, "Enter");
    current.undo = true;
    await current.page.waitForFunction(({ key, name }) => JSON.parse(localStorage.getItem(key)).task_lists?.some(list => list.name === name), { key: storageKey, name });
    if (name === "Notes") {
      const before = await raw(current.page);
      await current.page.mouse.click(addPoint.x, addPoint.y);
      await paint(current.page);
      assert.equal(await raw(current.page), before, "The disabled + must not create a sixth list");
    }
  }
  assert.equal((await lists(current)).length, 5);
  await renameList(current, 0, "Studio");
  await focusList(current, 1);
  const renamePoint = await focusedControlPoint(current.page);
  await current.page.mouse.dblclick(renamePoint.x, renamePoint.y, { delay: 80 });
  await paint(current.page);
  await replaceEditor(current.page, "Personal");
  await key(current.page, "Enter");
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).task_lists?.[1]?.name === "Personal", storageKey);
  const five = await state(current.page);
  assert.deepEqual(five.tasks, legacy.tasks, "Creating and renaming tabs must not duplicate or move task records");
  assert.deepEqual(five.deadlines, legacy.deadlines);
  assert.equal(five.featured, legacy.featured);
  await capture(current.page, "task-lists-desktop");
  await boot(current, true);
  await expectState(current.page, five, "Five list names and existing work must survive reload");
  await current.context.close();

  progress("filter one list, then combine lists with Shift and retain their distinct row shades");
  current = await open("task-lists-combined", fixture);
  const original = await raw(current.page);
  await expectBars(current.page, ["rose"]);
  await selectList(current, 1);
  await expectBars(current.page, ["amber"]);
  await selectList(current, 2, true);
  const combined = await expectBars(current.page, ["amber", "sage"]);
  assert.ok(combined.amberShade > 100 && combined.sageShade > 100, "Combined lists must retain distinguishable graphite row shades alongside task colors");
  assert.equal(await raw(current.page), original, "Selecting or combining list tabs must not edit tasks or change the featured countdown");
  await capture(current.page, "task-lists-combined");

  progress("add new work to the last selected list in a combined view");
  await resetTasks(current);
  // Header2, strip6, commands2, two dated rows with six controls each, input.
  await tab(current.page, 23);
  await replaceEditor(current.page, "Sample: active list check");
  await key(current.page, "Enter");
  current.undo = true;
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).tasks.length === 4, storageKey);
  const added = await state(current.page);
  assert.equal(added.tasks.at(-1).list_id, 3, "New work in a combined view must belong to the most recently selected list");
  assert.deepEqual(added.tasks.slice(0, -1), fixture.tasks);
  assert.deepEqual(added.deadlines, fixture.deadlines);
  assert.equal(added.featured, fixture.featured);
  await undo(current, fixture); current.undo = false;

  progress("edit a task's membership and title atomically, then undo it");
  await selectList(current, 0);
  await focusRowControl(current, 5); // First row's edit control.
  await key(current.page, "Space");
  // Only one row is selected: del, Studio, Home. Membership choices precede input.
  await tab(current.page, 3);
  await key(current.page, "Space");
  await tab(current.page, 2); // Show, then task text input.
  await replaceEditor(current.page, "Sample: revised casting studies");
  await key(current.page, "Enter");
  current.undo = true;
  const moved = structuredClone(fixture);
  moved.tasks[0].text = "Sample: revised casting studies"; moved.tasks[0].list_id = 2;
  moved.deadlines[0].title = moved.tasks[0].text;
  await expectState(current.page, moved, "Membership editing must reuse task/deadline IDs and preserve dates, colors and featured work");
  await undo(current, fixture);
  current.undo = false;
  await expectBars(current.page, ["rose"]);

  progress("edit a linked due date and completion inside the selected list");
  const first = (await bars(current.page)).rose;
  await current.page.mouse.click((first.left + first.right) / 2, (first.top + first.bottom) / 2);
  await paint(current.page);
  await replaceEditor(current.page, "2026-10-31");
  await key(current.page, "Enter");
  current.undo = true;
  const changedDate = structuredClone(fixture); changedDate.deadlines[0].at = "2026-10-31T18:00:00Z";
  await expectState(current.page, changedDate, "List filtering must preserve the exact opposite schedule end and other lists' dates");
  await undo(current, fixture); current.undo = false;
  await focusRowControl(current, 2);
  await key(current.page, "Space");
  current.undo = true;
  const completed = structuredClone(fixture); completed.tasks[0].complete = true;
  await expectState(current.page, completed, "Completing a selected task must retain its list, color, dates and another list's featured task");
  await expectBars(current.page, []);
  await undo(current, fixture); current.undo = false;

  progress("remove a populated list by moving its existing tasks, then undo the operation");
  await selectList(current, 1);
  await focusListControl(current, "−");
  await key(current.page, "Space");
  await tab(current.page, 2); // Multi, then inline Remove.
  await key(current.page, "Space");
  current.undo = true;
  const removed = structuredClone(fixture);
  removed.task_lists.splice(1, 1); delete removed.tasks[1].list_id;
  await expectState(current.page, removed, "Removing a list must move its existing task rather than delete or duplicate it");
  await undo(current, fixture); current.undo = false;
  await renameList(current, 2, "Exhibit");
  const renamed = structuredClone(fixture); renamed.task_lists[2].name = "Exhibit";
  await expectState(current.page, renamed);
  await boot(current, true);
  await expectState(current.page, renamed, "Names and membership must survive reloading together");
  const exported = await exportData(current);
  assert.deepEqual(exported, renamed, "JSON export must include list metadata on the original linked task records");
  const imported = structuredClone(exported); imported.task_lists[1].name = "Personal"; imported.tasks[0].list_id = 2;
  await current.page.locator("#jig-import").setInputFiles({ name: "sample-task-lists.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(imported)) });
  await current.page.waitForTimeout(1200);
  await expectState(current.page, renamed, "Choosing a JSON file must wait for confirmation");
  await resetTasks(current);
  await tab(current.page, 2); // Data, Confirm import before the ordinary workspace.
  await key(current.page, "Space");
  await expectState(current.page, imported, "JSON import must retain names, list membership, dates, colors and completion");
  await boot(current, true);
  await expectState(current.page, imported, "Imported lists must survive another restart");
  await current.context.close();

  progress("combine lists using the touch-friendly Multi toggle");
  current = await open("task-lists-mobile", fixture, true);
  await focusList(current, 1);
  const home = await focusedControlPoint(current.page);
  await current.page.touchscreen.tap(home.x, home.y); await paint(current.page);
  await expectBars(current.page, ["amber"]);
  await focusList(current, 2);
  const show = await focusedControlPoint(current.page);
  await focusListControl(current, "Multi");
  const multi = await focusedControlPoint(current.page);
  await current.page.touchscreen.tap(multi.x, multi.y); await paint(current.page);
  await current.page.touchscreen.tap(show.x, show.y); await paint(current.page);
  await expectBars(current.page, ["amber", "sage"]);
  assert.deepEqual(await state(current.page), fixture, "Touch combinations must leave tasks and the featured countdown unchanged");
  assert.equal(await current.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true, "List tabs and controls must fit the phone viewport");
  await capture(current.page, "task-lists-mobile");
  await current.context.close();

  progress("delete completed work only from the selected list and undo it");
  const bulk = structuredClone(fixture); bulk.tasks[0].complete = true; bulk.tasks[1].complete = true;
  current = await open("task-lists-bulk", bulk);
  await tab(current.page, 18); // Header2, strip6, commands2, completed row5, input/+2, bulk1.
  await key(current.page, "Space");
  await resetTasks(current, false);
  await tab(current.page, 18); // Confirm replaces the bulk button.
  await key(current.page, "Space");
  current.undo = true;
  const cleared = structuredClone(bulk); cleared.tasks.shift(); cleared.deadlines.shift();
  await expectState(current.page, cleared, "Bulk deletion must retain completed work in unselected lists and all featured work");
  await undo(current, bulk);
  await current.context.close();

  assert.deepEqual(issues, [], "Task-list changes must stay local and produce no app errors");
  console.log("Task-list browser checks passed: legacy state, five-list cap, create/rename/remove with task preservation, single/Shift/touch combinations and distinct Gantt shades, linked membership/name/date/completion edits and scoped completed removal, atomic Undo, reload and real JSON export/import without featured changes.");
} catch (error) {
  console.error(`Task-list browser checks failed during: ${phase}`, error);
  if (current && !current.page.isClosed()) await capture(current.page, "task-lists-failure").catch(() => {});
  throw error;
} finally {
  clearTimeout(timeout);
  for (const context of contexts) await context.close().catch(() => {});
  await browser.close();
}

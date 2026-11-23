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
  // Exercise the existing Sheet/tool flow using a normal saved workspace preference.
  await context.addInitScript(path => {
    if (location.pathname === path) localStorage.setItem("jig.workspace.v1", "sheet");
  }, new URL(base).pathname);
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
  const scene = { context, page };
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
  await resetTasks(scene);
}
async function resetTasks(scene, escape = true) {
  const viewport = scene.page.viewportSize();
  await scene.page.locator("#jig_canvas").click({ position: { x: viewport.width - 6, y: viewport.height - 6 } });
  if (escape) await key(scene.page, "Escape");
  await key(scene.page, "Control+Alt+2");
}
async function lists(scene) { return (await state(scene.page)).task_lists ?? [{ id: 1, name: "Tasks" }]; }
function headerTargets(scene) {
  // About, Data, Settings, Help, Calculator and the footer's Source remain
  // keyboard targets; workspace navigation buttons are pointer-only.
  // The phone's stacked Tasks/Gantt/Notes board overflows vertically, adding
  // the ordinary dashboard scroll-area target before the list-tab controls.
  return 6 + (scene.page.viewportSize().width < 500 ? 1 : 0);
}
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
}
async function focusedControlPoint(page, limit = 180, minHeight = 10, fill = [77, 49, 39], options = {}) {
  const until = Date.now() + 2000;
  let point;
  do {
    point = await readFocusedControlPoint(page, limit, minHeight, fill, options);
    if (point) return point;
    await paint(page);
  } while (Date.now() < until);
  assert.ok(point, "A focused list control must be visibly identifiable for touch/cap checks");
}
async function readFocusedControlPoint(page, limit, minHeight, fill = [77, 49, 39], options = {}) {
  // The ordinary keyboard-focused button has Jig's active graphite fill.
  // Read its visible pixels; no Rust/widget state is exposed to this driver.
  const png = await page.screenshot({ timeout: 60000, animations: "disabled", caret: "hide" });
  const point = await page.evaluate(async ({ encoded, limit, minHeight, fill, options }) => {
    const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
    const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height), ctx = canvas.getContext("2d"); ctx.drawImage(bitmap, 0, 0);
    const pixels = ctx.getImageData(0, 0, bitmap.width, bitmap.height).data, seen = new Uint8Array(bitmap.width * bitmap.height), regions = [];
    const matches = index => fill.every((value, axis) => Math.abs(pixels[index * 4 + axis] - value) <= 3);
    const bottom = Math.min(limit, bitmap.height);
    const top = 42; // Skip the first header row; only the requested list control is keyboard-focused.
    for (let y = top; y < bottom; y++) for (let x = 6; x < bitmap.width - 6; x++) {
      const start = y * bitmap.width + x;
      if (seen[start] || !matches(start)) continue;
      const stack = [start], r = { left: x, right: x, top: y, bottom: y, count: 0 }; seen[start] = 1;
      while (stack.length) {
        const index = stack.pop(), px = index % bitmap.width, py = Math.floor(index / bitmap.width);
        r.left = Math.min(r.left, px); r.right = Math.max(r.right, px); r.top = Math.min(r.top, py); r.bottom = Math.max(r.bottom, py); r.count++;
        for (const [nx, ny] of [[px - 1, py], [px + 1, py], [px, py - 1], [px, py + 1]]) {
          if (nx < 0 || nx >= bitmap.width || ny < top || ny >= bottom) continue;
          const next = ny * bitmap.width + nx;
          if (!seen[next] && matches(next)) { seen[next] = 1; stack.push(next); }
        }
      }
      const owner = options.exclude;
      const overlapsOwner = owner && r.left <= owner.right && r.right >= owner.left && r.top <= owner.bottom && r.bottom >= owner.top;
      if (!overlapsOwner && r.count > 100 && r.bottom - r.top >= minHeight && r.bottom - r.top <= 40
        && r.right - r.left < (options.maxWidth ?? 180)) regions.push(r);
    }
    const r = regions.sort((a, b) => a.top - b.top || b.count - a.count)[0];
    if (r) {
      // Combined rows may wrap controls. Read their full shaded frame rather
      // than treating the first-line star as the vertical center of the row.
      const shades = [[44, 39, 35], [50, 44, 39], [40, 36, 33], [47, 42, 38]];
      const y = Math.round((r.top + r.bottom) / 2);
      for (let x = 6; x < r.left; x++) {
        const shade = shades.find(color => color.every((value, axis) => pixels[(y * bitmap.width + x) * 4 + axis] === value));
        if (!shade) continue;
        const row = py => shade.every((value, axis) => pixels[(py * bitmap.width + x) * 4 + axis] === value);
        r.rowTop = y; r.rowBottom = y;
        while (r.rowTop > 0 && row(r.rowTop - 1)) r.rowTop--;
        while (r.rowBottom + 1 < bitmap.height && row(r.rowBottom + 1)) r.rowBottom++;
        break;
      }
    }
    bitmap.close();
    return r ? { ...r, x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 } : null;
  }, { encoded: png.toString("base64"), limit, minHeight, fill, options });
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
async function expectTaskOutline(page, row, selected) {
  assert.ok(Number.isFinite(row.rowTop) && Number.isFinite(row.rowBottom), "The task row must expose its visible frame");
  const until = Date.now() + 3000;
  let actual;
  do {
    const png = await page.screenshot({ timeout: 60000, animations: "disabled", caret: "hide" });
    actual = await page.evaluate(async ({ encoded, row }) => {
      const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
      const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
      const canvas = new OffscreenCanvas(bitmap.width, bitmap.height), ctx = canvas.getContext("2d"); ctx.drawImage(bitmap, 0, 0);
      const pixels = ctx.getImageData(0, 0, bitmap.width, bitmap.height).data;
      const edge = y => {
        let longest = 0, run = 0;
        for (let x = 6; x < Math.min(bitmap.width - 6, Math.floor(bitmap.width * 0.43)); x++) {
          const offset = (y * bitmap.width + x) * 4;
          if ([244, 107, 74].every((value, axis) => Math.abs(pixels[offset + axis] - value) <= 5)) run++;
          else run = 0;
          longest = Math.max(longest, run);
        }
        return longest >= 120;
      };
      const near = y => Array.from({ length: 9 }, (_, offset) => Math.round(y) + offset - 4)
        .some(y => y >= 0 && y < bitmap.height && edge(y));
      const result = near(row.rowTop) && near(row.rowBottom);
      bitmap.close(); return result;
    }, { encoded: png.toString("base64"), row });
    if (actual === selected) return;
    await paint(page);
  } while (Date.now() < until);
  assert.equal(actual, selected, "Only the selected task must have the warm accent box around its entire row");
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
async function taskInputPoint(page) {
  const png = await page.screenshot({ timeout: 60000, animations: "disabled", caret: "hide" });
  return page.evaluate(async encoded => {
    const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
    const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height), ctx = canvas.getContext("2d"); ctx.drawImage(bitmap, 0, 0);
    const pixels = ctx.getImageData(0, 0, bitmap.width, bitmap.height).data;
    const right = bitmap.width < 500 ? bitmap.width - 20 : Math.floor(bitmap.width * 0.43);
    const border = (x, y) => x >= 0 && x < bitmap.width && y >= 0 && y < bitmap.height
      && [113, 102, 93].every((value, axis) => Math.abs(pixels[(y * bitmap.width + x) * 4 + axis] - value) <= 3);
    // The Add new task field is the wide, shallow bordered rectangle in the
    // left Tasks panel. Check all four sides, excluding separators/date buttons.
    for (let y = 75; y < bitmap.height - 40; y++) for (let x = 10; x < right; x++) {
      if (!border(x, y)) continue;
      const left = x;
      while (x < right && border(x, y)) x++;
      if (x - left < 200) continue;
      for (let height = 14; height <= 30 && y + height < bitmap.height - 30; height++) {
        const middle = y + Math.floor(height / 2);
        if (![left - 2, left - 1, left].some(edge => border(edge, middle))
          || ![x - 1, x, x + 1].some(edge => border(edge, middle))) continue;
        let bottom = 0;
        for (let edge = left; edge < x; edge++) bottom += Number(border(edge, y + height));
        if (bottom / (x - left) > 0.85) {
          bitmap.close();
          return { x: (left + x - 1) / 2, y: middle };
        }
      }
    }
    bitmap.close(); return null;
  }, png.toString("base64"));
}
async function focusSort(scene, selectedLists = [1]) {
  await resetTasks(scene);
  let input;
  for (let attempt = 0; attempt < 4; attempt++) {
    await scene.page.mouse.move(12, 12); await paint(scene.page);
    input = await taskInputPoint(scene.page);
    if (input) break;
    // Reveal an overflowing checklist through its ordinary public scroll area.
    await scene.page.mouse.move(100, 220);
    await scene.page.mouse.wheel(0, 800);
    await scene.page.waitForTimeout(200); await paint(scene.page);
  }
  assert.ok(input, "Add new task must be visibly reachable in the Tasks panel");
  await scene.page.mouse.click(input.x, input.y); await paint(scene.page);
  await scene.page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 25 });
  const data = await state(scene.page);
  const bulkDelete = Number(data.tasks.some(task => task.complete && selectedLists.includes(task.list_id ?? 1)));
  await tab(scene.page, 2 + bulkDelete); // +, optional existing delete button, Sort.
  await scene.page.waitForTimeout(200); await paint(scene.page);
  return focusedControlPoint(scene.page, scene.page.viewportSize().height - 25);
}
async function focusMoveDestination(scene, target, selectedLists = [1], method = "keyboard") {
  await focusSort(scene, selectedLists);
  await tab(scene.page); // Move all completed follows Sort.
  const viewport = scene.page.viewportSize();
  const owner = await focusedControlPoint(scene.page, viewport.height - 25, 10, [77, 49, 39], { maxWidth: viewport.width - 12 });
  if (method === "touch") await scene.page.touchscreen.tap(owner.x, owner.y);
  else if (method === "mouse") await scene.page.mouse.click(owner.x, owner.y);
  else await key(scene.page, "Space");
  await paint(scene.page);
  const data = await state(scene.page);
  const eligible = (await lists(scene)).filter(list => data.tasks.some(task => task.complete
    && selectedLists.includes(task.list_id ?? 1) && (task.list_id ?? 1) !== list.id));
  const index = eligible.findIndex(list => list.id === target);
  assert.ok(index >= 0, "The chosen list must have completed tasks to receive");
  await tab(scene.page, index + 2); // Popup Area, then destinations; disabled zero counts are skipped.
  const item = await focusedControlPoint(scene.page, viewport.height - 2, 10, [77, 49, 39], { exclude: owner, maxWidth: viewport.width - 12 });
  assert.ok(item.left > 0 && item.right < viewport.width - 2 && item.top >= 42 && item.bottom < viewport.height - 2,
    "The complete destination target must remain visible, including on a phone");
  return { owner, item };
}
async function undo(scene, expected) {
  await key(scene.page, "Escape"); await key(scene.page, "Control+z");
  await expectState(scene.page, expected, "Undo must restore list membership and linked records together");
}
async function exportData(scene) {
  await resetTasks(scene);
  await key(scene.page, "Control+Alt+d");
  await tab(scene.page, 9); // Five header controls, then Start/Open/Import/Export.
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
    await key(current.page, "Space");
    await replaceEditor(current.page, name);
    await key(current.page, "Enter");
    await current.page.waitForFunction(({ key, name }) => JSON.parse(localStorage.getItem(key)).task_lists?.some(list => list.name === name), { key: storageKey, name });
    if (name === "Notes") {
      // The + moves with the tabs. At the cap it is skipped by keyboard focus,
      // so locate it between the actual last tab and enabled minus button.
      await focusList(current, 4);
      // A selected tab keeps its selection fill even while keyboard-focused.
      const lastTab = await focusedControlPoint(current.page, 180, 24, [67, 42, 34]);
      await focusListControl(current, "−");
      const remove = await focusedControlPoint(current.page);
      assert.ok(remove.left - lastTab.right >= 28 && remove.left - lastTab.right <= 60,
        "The disabled + must sit directly between the final tab and the remove button");
      const addPoint = { x: (lastTab.right + remove.left) / 2, y: remove.y };
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

  progress("select task rows and reorder visible work with the keyboard without changing hidden lists");
  await focusRowControl(current, 4);
  const firstSelectable = await focusedControlPoint(current.page, 520);
  await focusRowControl(current, 10); // Two dated single-task lists: second row's color.
  const secondSelectable = await focusedControlPoint(current.page, 520);
  await current.page.mouse.click(secondSelectable.right + 35, secondSelectable.y); await paint(current.page);
  assert.notEqual(await current.page.evaluate(() => document.activeElement?.tagName), "INPUT", "A single task click must select its row without opening the editor");
  await expectTaskOutline(current.page, secondSelectable, true);
  await expectTaskOutline(current.page, firstSelectable, false);
  assert.equal(await raw(current.page), original, "Row selection must not change tasks, dates, lists or the featured countdown");
  await key(current.page, "ArrowUp");
  await expectTaskOutline(current.page, firstSelectable, true);
  await expectTaskOutline(current.page, secondSelectable, false);
  await key(current.page, "ArrowUp"); // Clamp at the first visible row; Studio remains hidden.
  await expectTaskOutline(current.page, firstSelectable, true);
  await key(current.page, "ArrowDown");
  await expectTaskOutline(current.page, secondSelectable, true);
  await key(current.page, "ArrowDown");
  await expectTaskOutline(current.page, secondSelectable, true);
  assert.equal(await raw(current.page), original, "Up/Down selection and boundary clamping must leave portable data untouched");
  await key(current.page, "Control+ArrowUp");
  const keyboardOrder = structuredClone(fixture);
  [keyboardOrder.tasks[1], keyboardOrder.tasks[2]] = [keyboardOrder.tasks[2], keyboardOrder.tasks[1]];
  await expectState(current.page, keyboardOrder, "Ctrl+Up must swap the selected visible task with its previous visible neighbor and preserve hidden slots, membership and all linked metadata");
  const keyboardBars = await expectBars(current.page, ["amber", "sage"]);
  assert.ok(keyboardBars.sage.top < keyboardBars.amber.top, "The Gantt must follow the task's keyboard move");
  await key(current.page, "Control+z"); await expectState(current.page, fixture, "One Undo must restore the complete task order after Ctrl+Up");
  await key(current.page, "Control+z"); await expectState(current.page, fixture, "Selection and clamped movement must create no additional Undo entries");
  await key(current.page, "Control+Shift+z"); await expectState(current.page, keyboardOrder, "Redo must restore the keyboard reorder without duplicating task records");
  await undo(current, fixture);
  await selectList(current, 1); // Home alone: hidden Studio must never become a move target.
  await focusRowControl(current, 4);
  const onlyVisible = await focusedControlPoint(current.page, 520);
  await current.page.mouse.click(onlyVisible.right + 35, onlyVisible.y); await paint(current.page);
  await key(current.page, "Control+ArrowUp");
  await expectState(current.page, fixture, "Ctrl+Up on the only visible task must not move it across work in a hidden list");
  await selectList(current, 2, true); // Restore Home + Show for the pointer-drag regression below.
  await expectBars(current.page, ["amber", "sage"]);

  progress("drag a task grip between combined lists without changing membership or saving the preview");
  await focusRowControl(current, 1);
  const firstRow = await focusedControlPoint(current.page, 520);
  await focusRowControl(current, 7); // Each visible dated task has six controls in its own list.
  const lastRow = await focusedControlPoint(current.page, 520);
  const taskGrip = { x: firstRow.left - 18, y: firstRow.y };
  assert.ok(Number.isFinite(lastRow.rowBottom), "The combined task row must expose its full shaded frame");
  const afterLast = { x: lastRow.x + 110, y: lastRow.rowBottom - 2 };
  const beginTaskDrag = async () => {
    await current.page.mouse.move(taskGrip.x, taskGrip.y); await current.page.mouse.down(); await paint(current.page);
    await current.page.mouse.move(afterLast.x, afterLast.y, { steps: 8 }); await paint(current.page);
  };
  await beginTaskDrag();
  await expectState(current.page, fixture, "A task drag preview must not save or change any linked records");
  await key(current.page, "Escape"); await current.page.mouse.up(); await paint(current.page);
  await key(current.page, "Control+z");
  await expectState(current.page, fixture, "Cancelling a task drag must not create an Undo entry");
  await beginTaskDrag();
  await expectState(current.page, fixture, "The second task drag must still wait for release");
  await current.page.mouse.up(); await paint(current.page);
  const dragOrder = structuredClone(fixture);
  [dragOrder.tasks[1], dragOrder.tasks[2]] = [dragOrder.tasks[2], dragOrder.tasks[1]];
  await expectState(current.page, dragOrder, "Task drop must preserve hidden tasks, list IDs, exact dates, colors and the featured task");
  const reorderedBars = await expectBars(current.page, ["amber", "sage"]);
  assert.ok(reorderedBars.sage.top < reorderedBars.amber.top, "The Gantt must follow the released checklist order");
  await undo(current, fixture);
  await key(current.page, "Control+z"); await expectState(current.page, fixture, "One release must create only one Undo entry");

  progress("add new work to the last selected list in a combined view");
  await resetTasks(current);
  // Header, strip6, commands2, two dated rows with six controls each, input.
  await tab(current.page, headerTargets(current) + 21);
  await replaceEditor(current.page, "Sample: active list check");
  await key(current.page, "Enter");
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).tasks.length === 4, storageKey);
  const added = await state(current.page);
  assert.equal(added.tasks.at(-1).list_id, 3, "New work in a combined view must belong to the most recently selected list");
  assert.deepEqual(added.tasks.slice(0, -1), fixture.tasks);
  assert.deepEqual(added.deadlines, fixture.deadlines);
  assert.equal(added.featured, fixture.featured);
  await undo(current, fixture);

  progress("double-click a task and save its name, details, list and timed dates atomically");
  await selectList(current, 0);
  await focusRowControl(current, 4); // Dated row: star, complete, ×, color.
  const color = await focusedControlPoint(current.page, 520);
  await current.page.mouse.dblclick(color.right + 35, color.y, { delay: 80 });
  await paint(current.page);
  await current.page.waitForFunction(() => document.activeElement?.tagName === "INPUT");
  await expectState(current.page, fixture, "Double-clicking a task must open its editor without changing any saved record");
  await replaceEditor(current.page, "Sample: revised casting studies");
  await tab(current.page); // Name -> Due, in the same editor.
  await replaceEditor(current.page, "2026-11-02 16:45");
  await tab(current.page, 2); // Due picker -> Start.
  await replaceEditor(current.page, "2026-10-27 08:30");
  await tab(current.page, 3); // Start picker, Remove date, Details.
  await replaceEditor(current.page, "Sample: revised casting studies");
  await key(current.page, "End"); await key(current.page, "Shift+Enter");
  await current.page.keyboard.type("Keep both measurements and firing notes."); await paint(current.page);
  await expectState(current.page, fixture, "Shift+Enter in Details must insert a newline without saving any staged field");
  await tab(current.page, 2); // Studio, Home.
  await key(current.page, "Space");
  const nameField = await taskInputPoint(current.page);
  assert.ok(nameField, "The unified Task editor must retain a visible Name field beside its date controls");
  await current.page.mouse.click(nameField.x, nameField.y); await paint(current.page);
  await key(current.page, "Enter");
  const moved = structuredClone(fixture);
  moved.tasks[0].text = "Sample: revised casting studies"; moved.tasks[0].list_id = 2;
  moved.tasks[0].details = moved.tasks[0].text + "\nKeep both measurements and firing notes.";
  moved.tasks[0].start = "2026-10-27T08:30:00Z";
  moved.deadlines[0].title = moved.tasks[0].text;
  moved.deadlines[0].at = "2026-11-02T16:45:00Z";
  await expectState(current.page, moved, "One Save must reuse task/deadline IDs and commit the full name/details/list/timed schedule together");
  await undo(current, fixture);
  await key(current.page, "Control+z"); await expectState(current.page, fixture, "One full name/details/list/date Save must create exactly one Undo entry");
  await key(current.page, "Control+Shift+z"); await expectState(current.page, moved, "Redo must restore every field of the unified edit together");
  await boot(current, true); await expectState(current.page, moved, "The complete unified edit must survive refresh");
  assert.deepEqual(await exportData(current), moved, "Portable JSON must preserve every field from the unified timed Task edit");
  await selectList(current, 0); // Reopen Studio, whose task moved to Home.
  // Reload resets in-memory Undo, so test an independent second edit on Home.
  await selectList(current, 1);
  await focusRowControl(current, 6); await key(current.page, "Space");
  await replaceEditor(current.page, "Sample: one atomic revision");
  await tab(current.page); await replaceEditor(current.page, "2026-11-03 17:15");
  await key(current.page, "Enter");
  const secondRevision = structuredClone(moved);
  secondRevision.tasks[0].text = "Sample: one atomic revision";
  secondRevision.deadlines[0].title = secondRevision.tasks[0].text;
  secondRevision.deadlines[0].at = "2026-11-03T17:15:00Z";
  await expectState(current.page, secondRevision, "Enter in Due must save the name and timed date through the same transaction");
  await undo(current, moved);
  await key(current.page, "Control+z"); await expectState(current.page, moved, "One combined edit must create exactly one Undo entry");

  progress("cancel all staged Task fields with Escape or an outside click while preserving Redo");
  await focusRowControl(current, 6); await key(current.page, "Space");
  await replaceEditor(current.page, "Sample: discard this name");
  await tab(current.page); await replaceEditor(current.page, "2026-12-01 14:00");
  await key(current.page, "Escape");
  await expectState(current.page, moved, "Escape must discard the entire Task edit, including its name and timed date");
  await key(current.page, "Control+Shift+z");
  await expectState(current.page, secondRevision, "Opening and cancelling a Task edit must preserve the preceding Redo branch");
  await undo(current, moved);
  await focusRowControl(current, 6); await key(current.page, "Space");
  await replaceEditor(current.page, "Sample: outside cancellation");
  await tab(current.page); await replaceEditor(current.page, "2026-12-02 14:00");
  await current.page.keyboard.type("Z");
  await current.page.locator("#jig_canvas").click({ position: { x: 1434, y: 994 } }); // No paint after queued old-field text.
  await paint(current.page);
  await expectState(current.page, moved, "Clicking outside must discard every staged field, including a final queued character");
  await current.page.waitForFunction(() => {
    const draft = JSON.parse(sessionStorage.getItem("jig.drafts.v1"));
    return !draft?.task && !draft?.schedule;
  });
  await key(current.page, "Control+Shift+z");
  await expectState(current.page, secondRevision, "Outside cancellation must preserve Redo and create no saved work");
  await undo(current, moved);
  await current.context.close();
  current = await open("task-lists-after-unified-edit", fixture);
  await selectList(current, 0);

  progress("edit a linked due date and completion inside the selected list");
  const first = (await bars(current.page)).rose;
  await current.page.mouse.click((first.left + first.right) / 2, (first.top + first.bottom) / 2);
  await paint(current.page);
  await replaceEditor(current.page, "2026-10-31");
  await key(current.page, "Enter");
  const changedDate = structuredClone(fixture); changedDate.deadlines[0].at = "2026-10-31T18:00:00Z";
  await expectState(current.page, changedDate, "List filtering must preserve the exact opposite schedule end and other lists' dates");
  await undo(current, fixture);
  await focusRowControl(current, 2);
  await key(current.page, "Space");
  const completed = structuredClone(fixture); completed.tasks[0].complete = true;
  await expectState(current.page, completed, "Completing a selected task must retain its list, color, dates and another list's featured task");
  await expectBars(current.page, []);
  await undo(current, fixture);

  progress("delete through the compact boxed × beside the completion checkbox, then restore with Undo");
  await focusRowControl(current, 2);
  const completionBox = await focusedControlPoint(current.page, 520);
  await tab(current.page);
  const deleteBox = await focusedControlPoint(current.page, 520);
  assert.ok(deleteBox.left > completionBox.right && deleteBox.left - completionBox.right < 18,
    "The boxed × delete control must sit immediately after the completion checkbox");
  await current.page.mouse.click(deleteBox.x, deleteBox.y); await paint(current.page);
  const deletedTask = structuredClone(fixture);
  deletedTask.tasks.splice(0, 1); deletedTask.deadlines.splice(0, 1);
  await expectState(current.page, deletedTask, "The boxed × must remove only its task and linked deadline, preserving other lists and featured work");
  await undo(current, fixture);

  progress("remove a populated list by moving its existing tasks, then undo the operation");
  await selectList(current, 1);
  await focusListControl(current, "−");
  await key(current.page, "Space");
  await tab(current.page, 2); // Multi, then inline Remove.
  await key(current.page, "Space");
  const removed = structuredClone(fixture);
  removed.task_lists.splice(1, 1); delete removed.tasks[1].list_id;
  await expectState(current.page, removed, "Removing a list must move its existing task rather than delete or duplicate it");
  await undo(current, fixture);

  progress("confirm list removal with the same minus button by mouse and keyboard");
  await selectList(current, 1);
  await focusListControl(current, "−");
  const removePoint = await focusedControlPoint(current.page);
  const clickMinus = async () => {
    await current.page.mouse.click(removePoint.x, removePoint.y); await paint(current.page);
  };
  await clickMinus();
  await expectState(current.page, fixture, "The first minus click must only request confirmation");
  await clickMinus();
  await expectState(current.page, removed, "A second minus click must confirm removal and preserve every linked task record");
  await undo(current, fixture);
  await key(current.page, "Control+z");
  await expectState(current.page, fixture, "Repeated-minus confirmation must create only one Undo entry");
  await selectList(current, 1);
  await focusListControl(current, "−");
  await key(current.page, "Space");
  await expectState(current.page, fixture, "The first keyboard activation must not remove a list");
  await key(current.page, "Space");
  await expectState(current.page, removed, "A second keyboard activation must confirm the same pending removal");
  await undo(current, fixture);

  progress("disarm pending list removal after Cancel, Escape or switching tabs");
  await selectList(current, 1);
  await focusListControl(current, "−");
  await key(current.page, "Space");
  await tab(current.page, 3); // Multi, Remove, Cancel.
  await key(current.page, "Space");
  await clickMinus();
  await expectState(current.page, fixture, "Minus after Cancel must start a new confirmation instead of removing the list");
  await key(current.page, "Escape");
  await clickMinus();
  await expectState(current.page, fixture, "Minus after Escape must start a new confirmation instead of removing the list");
  await key(current.page, "Escape");
  await focusList(current, 2);
  const showTabPoint = await focusedControlPoint(current.page);
  await selectList(current, 1);
  await focusListControl(current, "−");
  await key(current.page, "Space");
  // Use the actual tab pointer target: selectList() also presses Escape and
  // would hide whether tab selection itself cleared the pending removal.
  await current.page.mouse.click(showTabPoint.x, showTabPoint.y); await paint(current.page);
  await clickMinus();
  await expectState(current.page, fixture, "Switching tabs must discard the previous list's confirmation");
  await clickMinus();
  const removedShow = structuredClone(fixture);
  removedShow.task_lists.splice(2, 1); delete removedShow.tasks[2].list_id;
  await expectState(current.page, removedShow, "A fresh repeated-minus confirmation must remove only the newly active list");
  await undo(current, fixture);
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
  await tab(current.page, 6); // About, Data, Settings, Help, Calculator, Confirm import.
  await key(current.page, "Space");
  await expectState(current.page, imported, "JSON import must retain names, list membership, dates, colors and completion");
  await boot(current, true);
  await expectState(current.page, imported, "Imported lists must survive another restart");
  await current.context.close();

  progress("combine lists using the touch-friendly Multi toggle");
  current = await open("task-lists-mobile", fixture, true);
  await focusList(current, 1);
  const home = await focusedControlPoint(current.page, 180, 24);
  await focusList(current, 2);
  const show = await focusedControlPoint(current.page, 180, 24);
  console.log(`Task-list phone touch targets: ${JSON.stringify({ home, show })}`);
  assert.ok(Math.abs(home.x - show.x) >= 40 && home.bottom - home.top >= 24 && show.bottom - show.top >= 24,
    "Touch targets must be two distinct complete task-list tabs below the workspace header");
  // Observe actual public pointer events so this really exercises egui's
  // sub-300ms double-click window, rather than two slow independent choices.
  await current.page.evaluate(() => {
    window.jigListTapTimes = [];
    window.jigListTapProbe = event => window.jigListTapTimes.push(event.timeStamp);
    document.addEventListener("pointerup", window.jigListTapProbe, true);
  });
  await current.page.touchscreen.tap(home.x, home.y); await paint(current.page);
  await current.page.touchscreen.tap(show.x, show.y); await paint(current.page);
  const tapTimes = await current.page.evaluate(() => {
    document.removeEventListener("pointerup", window.jigListTapProbe, true);
    return window.jigListTapTimes;
  });
  assert.ok(tapTimes.length === 2 && tapTimes[1] - tapTimes[0] < 300,
    `The cross-tab regression must exercise two real taps within 300ms; found ${JSON.stringify(tapTimes)}`);
  assert.equal(await current.page.evaluate(() => document.activeElement?.tagName === "INPUT"), false,
    "Rapid ordinary taps on different tabs must select the second list without opening rename");
  await expectBars(current.page, ["sage"]);
  await current.page.touchscreen.tap(home.x, home.y); await paint(current.page);
  await expectBars(current.page, ["amber"]);
  await focusListControl(current, "Multi");
  const multi = await focusedControlPoint(current.page);
  await current.page.touchscreen.tap(multi.x, multi.y); await paint(current.page);
  await current.page.touchscreen.tap(show.x, show.y); await paint(current.page);
  await expectBars(current.page, ["amber", "sage"]);
  assert.equal(await current.page.evaluate(() => document.activeElement?.tagName === "INPUT"), false,
    "Rapid Multi-mode tab taps must combine lists without opening a rename editor");
  assert.deepEqual(await state(current.page), fixture, "Touch combinations must leave tasks and the featured countdown unchanged");
  assert.equal(await current.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true, "List tabs and controls must fit the phone viewport");
  await capture(current.page, "task-lists-mobile");

  progress("reorder tasks through the phone grip and cancel a real touch drag safely");
  await focusRowControl(current, 1);
  const touchFirst = await focusedControlPoint(current.page, 520);
  await focusRowControl(current, 7);
  const touchLast = await focusedControlPoint(current.page, 520);
  const touchFrom = { x: touchLast.left - 18, y: touchLast.y };
  const touchTo = { x: touchFirst.x + 110, y: touchFirst.top + 2 };
  const dragSession = await current.context.newCDPSession(current.page);
  const touchDrag = (type, point) => dragSession.send("Input.dispatchTouchEvent", {
    type, touchPoints: type === "touchEnd" || type === "touchCancel" ? [] : [{ ...point, radiusX: 1, radiusY: 1, force: 1, id: 1 }],
  });
  const beginTouchDrag = async () => {
    await touchDrag("touchStart", touchFrom); await paint(current.page);
    for (let step = 1; step <= 8; step++) {
      await touchDrag("touchMove", { x: touchFrom.x + (touchTo.x - touchFrom.x) * step / 8, y: touchFrom.y + (touchTo.y - touchFrom.y) * step / 8 });
      await current.page.waitForTimeout(25);
    }
    await paint(current.page);
  };
  await beginTouchDrag(); await expectState(current.page, fixture, "Touch reorder must not save before release");
  await touchDrag("touchCancel"); await paint(current.page);
  await key(current.page, "Control+z");
  await expectState(current.page, fixture, "A cancelled touch must leave the task order and Undo unchanged");
  await beginTouchDrag(); await expectState(current.page, fixture);
  await touchDrag("touchEnd"); await paint(current.page);
  await expectState(current.page, dragOrder, "The touch grip must reorder the original records without changing their lists or dates");
  await undo(current, fixture);

  progress("confirm list removal with two real minus taps and retain it after refresh");
  await focusListControl(current, "Multi");
  await key(current.page, "Space"); // Leave the combined phone view before choosing one active list.
  await selectList(current, 1);
  await focusListControl(current, "−");
  const touchMinus = await focusedControlPoint(current.page);
  await current.page.touchscreen.tap(touchMinus.x, touchMinus.y); await paint(current.page);
  await expectState(current.page, fixture, "The first minus tap must only show confirmation");
  await current.page.touchscreen.tap(touchMinus.x, touchMinus.y); await paint(current.page);
  await expectState(current.page, removed, "The second minus tap must move the same original tasks without altering deadlines or featured work");
  await boot(current, true);
  await expectState(current.page, removed, "Repeated-minus removal and exact task membership must survive refresh");
  await current.context.close();

  progress("sort visible tasks by due date, keep undated/completed order, and preserve hidden work");
  const sorting = {
    ...structuredClone(fixture),
    tasks: [
      { id: 1, text: "Sample: later dated work", complete: false, deadline: 101, color: "rose", start: "2026-10-26T12:34:56Z", details: "Sample: later dated work\nKeep the full note details." },
      { id: 2, text: "Sample: hidden Home work", complete: false, list_id: 2, deadline: 102, color: "amber", start: "2026-10-26T12:00:00Z" },
      { id: 3, text: "Sample: undated first", complete: false },
      { id: 4, text: "Sample: completed first", complete: true, deadline: 104, start: "2026-10-26T12:00:00Z" },
      { id: 5, text: "Sample: early dated work", complete: false, deadline: 105, color: "sage", start: "2026-10-26T12:00:00Z" },
      { id: 6, text: "Sample: undated second", complete: false },
      { id: 7, text: "Sample: equal due date", complete: false, deadline: 107, color: "blue", start: "2026-10-26T12:00:00Z" },
      { id: 8, text: "Sample: completed second", complete: true },
      { id: 9, text: "Sample: hidden Show work", complete: false, list_id: 3, deadline: 109, start: "2026-10-26T12:00:00Z" },
    ],
    deadlines: [
      { id: 101, title: "Sample: later dated work", at: "2026-10-31T18:00:00Z" },
      { id: 102, title: "Sample: hidden Home work", at: "2026-10-28T18:00:00Z" },
      { id: 104, title: "Sample: completed first", at: "2026-10-27T18:00:00Z" },
      { id: 105, title: "Sample: early dated work", at: "2026-10-29T18:00:00Z" },
      { id: 107, title: "Sample: equal due date", at: "2026-10-29T18:00:00Z" },
      { id: 109, title: "Sample: hidden Show work", at: "2026-10-30T18:00:00Z" },
    ],
    notes: [{ id: 401, text: "Sample: unrelated pinned note", pinned: true }],
    featured: 101,
  };
  const ordered = (data, ids) => ({ ...structuredClone(data), tasks: ids.map(id => structuredClone(data.tasks.find(task => task.id === id))) });
  const sorted = ordered(sorting, [5, 2, 7, 1, 3, 6, 4, 8, 9]);
  current = await open("task-lists-sort", sorting);
  const sortPoint = await focusSort(current);
  await current.page.mouse.click(sortPoint.x, sortPoint.y); await paint(current.page);
  await expectState(current.page, sorted, "Sort must preserve exact linked metadata, equal-date order, undated/completed order and hidden task slots");
  const sortedBars = await expectBars(current.page, ["rose", "sage"]);
  assert.ok(sortedBars.sage.top < sortedBars.rose.top, "The Gantt must immediately follow the sorted checklist order");
  const unchangedSort = await focusSort(current);
  await current.page.mouse.click(unchangedSort.x, unchangedSort.y); await paint(current.page);
  await expectState(current.page, sorted, "Sorting an already sorted list must be a no-op");
  await undo(current, sorting);
  await key(current.page, "Control+z");
  await expectState(current.page, sorting, "Repeated Sort must not add a second Undo entry");
  await focusSort(current); await key(current.page, "Space");
  await expectState(current.page, sorted, "Sort must also work from the keyboard");
  await boot(current, true);
  await expectState(current.page, sorted, "Sorted order must survive refresh");
  const sortedExport = await exportData(current);
  assert.deepEqual(sortedExport, sorted, "JSON export must retain the sorted order and complete original records");
  await current.page.locator("#jig-import").setInputFiles({ name: "sample-sorted-tasks.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(sorting)) });
  await current.page.waitForTimeout(1200);
  await expectState(current.page, sorted, "Choosing another task order must still wait for import confirmation");
  await resetTasks(current); await tab(current.page, 6); await key(current.page, "Space");
  await expectState(current.page, sorting, "Import must restore the original manual order without automatically sorting it");
  await current.page.locator("#jig-import").setInputFiles({ name: "sample-sorted-tasks.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(sortedExport)) });
  await current.page.waitForTimeout(1200);
  await resetTasks(current); await tab(current.page, 6); await key(current.page, "Space");
  await expectState(current.page, sorted, "JSON import must restore the saved sorted order exactly");
  await boot(current, true); await expectState(current.page, sorted);
  await current.context.close();

  progress("sort combined lists together while preserving an unselected list");
  current = await open("task-lists-sort-combined", sorting);
  await selectList(current, 1, true);
  const combinedSort = ordered(sorting, [2, 5, 7, 1, 3, 6, 4, 8, 9]);
  await focusSort(current, [1, 2]); await key(current.page, "Space");
  await expectState(current.page, combinedSort, "Combined Sort must order the selected lists together and leave the Show task untouched");
  const combinedSortBars = await expectBars(current.page, ["rose", "amber", "sage"]);
  assert.ok(combinedSortBars.amber.top < combinedSortBars.sage.top && combinedSortBars.sage.top < combinedSortBars.rose.top,
    "Combined Gantt rows must follow due-date order across the selected lists");
  await undo(current, sorting);
  await current.context.close();

  progress("sort older standalone deadlines only on request and undo their task links together");
  const standalone = {
    ...structuredClone(legacy),
    tasks: [{ id: 1, text: "Sample: older undated task", complete: false }],
    deadlines: [
      { id: 101, title: "Sample: older later deadline", at: "2026-10-31T18:00:00Z" },
      { id: 102, title: "Sample: older earlier deadline", at: "2026-10-29T18:00:00Z" },
    ],
    entries: [],
    featured: 101,
  };
  current = await open("task-lists-sort-legacy", standalone);
  assert.equal(await raw(current.page), JSON.stringify(standalone), "Opening old deadline-only rows must not materialize or sort them");
  await focusSort(current); await key(current.page, "Space");
  const linkedLegacy = {
    ...structuredClone(standalone),
    tasks: [
      { id: 104, text: "Sample: older earlier deadline", complete: false, deadline: 102 },
      { id: 103, text: "Sample: older later deadline", complete: false, deadline: 101 },
      ...structuredClone(standalone.tasks),
    ],
  };
  await expectState(current.page, linkedLegacy, "Explicit Sort must reuse old deadlines and the featured ID while placing their tasks before undated work");
  await undo(current, standalone);
  await key(current.page, "Control+z");
  await expectState(current.page, standalone, "One Undo must restore original standalone records without leftover links");
  await current.context.close();

  progress("sort from the touch-friendly button on a phone");
  const phoneSort = {
    ...structuredClone(legacy),
    tasks: [
      structuredClone(sorting.tasks[0]),
      { id: 3, text: "Sample: phone undated", complete: false },
      structuredClone(sorting.tasks[4]),
      { id: 8, text: "Sample: phone completed", complete: true },
    ],
    deadlines: [structuredClone(sorting.deadlines[0]), structuredClone(sorting.deadlines[3])],
  };
  const phoneSorted = ordered(phoneSort, [5, 1, 3, 8]);
  current = await open("task-lists-sort-mobile", phoneSort, true);
  const phoneSortPoint = await focusSort(current);
  await current.page.touchscreen.tap(phoneSortPoint.x, phoneSortPoint.y); await paint(current.page);
  await expectState(current.page, phoneSorted, "A real touch on Sort must preserve every task and its exact schedule");
  await undo(current, phoneSort);
  await current.context.close();

  progress("move completed tasks to a chosen list without changing their records or the visible list");
  const completedLists = {
    ...structuredClone(fixture),
    tasks: [
      { ...structuredClone(fixture.tasks[0]), complete: true, details: "Sample: cast studies\nKeep the full firing notes." },
      { ...structuredClone(fixture.tasks[1]), complete: true },
      structuredClone(fixture.tasks[2]),
      { id: 4, text: "Sample: unfinished studio work", complete: false, deadline: 104, color: "rose", start: "2026-10-26T12:00:00Z", details: "Sample: this open task must stay here." },
      { id: 5, text: "Sample: completed undated work", complete: true, color: "blue", details: "Sample: keep this useful reference." },
      { id: 6, text: "Sample: completed hidden work", complete: true, list_id: 3, deadline: 106, color: "sage", start: "2026-10-26T12:00:00Z" },
    ],
    deadlines: [
      ...structuredClone(fixture.deadlines),
      { id: 104, title: "Sample: unfinished studio work", at: "2026-10-30T18:00:00Z" },
      { id: 106, title: "Sample: completed hidden work", at: "2026-10-30T18:00:00Z" },
    ],
    notes: [{ id: 401, text: "Sample: unrelated pinned note", pinned: true }],
  };
  const moveMembership = (data, ids, list) => {
    const moved = structuredClone(data);
    for (const task of moved.tasks) if (ids.includes(task.id)) {
      if (list === 1) delete task.list_id; // The portable format omits the default list.
      else task.list_id = list;
    }
    return moved;
  };
  const movedCompleted = moveMembership(completedLists, [1, 5], 2);
  current = await open("task-lists-move-completed", completedLists);
  const beforeMenu = await raw(current.page);
  await focusMoveDestination(current, 2);
  await key(current.page, "Escape");
  assert.equal(await raw(current.page), beforeMenu, "Opening and cancelling the destination menu must not write or move anything");
  const destination = await focusMoveDestination(current, 2, [1], "mouse");
  await capture(current.page, "task-lists-move-completed");
  await current.page.mouse.click(destination.item.x, destination.item.y); await paint(current.page);
  await expectState(current.page, movedCompleted, "Only completed Studio tasks must change list ID; exact dates, details, colors, record order, hidden tasks and the featured countdown must stay intact");
  await expectBars(current.page, ["rose"]); // Studio's unfinished task remains visible.
  await undo(current, completedLists);
  await key(current.page, "Control+z");
  await expectState(current.page, completedLists, "Menu opening/cancellation and a bulk move must create exactly one Undo entry");
  await key(current.page, "Control+Shift+z");
  await expectState(current.page, movedCompleted, "Redo must restore the entire completed-task move together");
  await boot(current, true);
  await expectState(current.page, movedCompleted, "Moved list membership and all linked records must survive refresh");
  const movedExport = await exportData(current);
  assert.deepEqual(movedExport, movedCompleted, "JSON export must contain the original records with only the intended membership changes");
  for (const data of [completedLists, movedExport]) {
    const beforeImport = await state(current.page);
    await current.page.locator("#jig-import").setInputFiles({ name: "sample-moved-completed.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(data)) });
    await current.page.waitForTimeout(1200);
    await expectState(current.page, beforeImport, "Choosing a completed-task backup must still wait for confirmation");
    await resetTasks(current); await tab(current.page, 6); await key(current.page, "Space");
    await expectState(current.page, data, "Import must restore exact completed-task membership and metadata");
  }
  await boot(current, true); await expectState(current.page, movedCompleted);
  await current.context.close();

  progress("move completed work from combined lists, including a destination already selected");
  current = await open("task-lists-move-combined", completedLists);
  await selectList(current, 1, true);
  await focusMoveDestination(current, 3, [1, 2]);
  await key(current.page, "Space");
  const movedCombined = moveMembership(completedLists, [1, 2, 5], 3);
  await expectState(current.page, movedCombined, "A combined move must include both selected sources, preserve an existing destination task, and leave all open tasks unchanged");
  await expectBars(current.page, ["rose"]); // Keep the selected sources and featured task independent.
  await undo(current, completedLists);
  await focusMoveDestination(current, 2, [1, 2]);
  await key(current.page, "Space");
  await expectState(current.page, movedCompleted, "Choosing an already selected destination must skip its completed tasks without duplicating them");
  await undo(current, completedLists);
  await selectList(current, 1);
  await focusMoveDestination(current, 1, [2]);
  await key(current.page, "Space");
  const movedToDefault = moveMembership(completedLists, [2], 1);
  await expectState(current.page, movedToDefault, "Moving completed work back to the default list must use its canonical omitted list ID and retain exact metadata");
  await undo(current, completedLists);
  await key(current.page, "Control+Shift+z"); await expectState(current.page, movedToDefault);
  await boot(current, true); await expectState(current.page, movedToDefault);
  await current.context.close();

  progress("move completed work using the phone's touch destination picker");
  const phoneCompleted = structuredClone(completedLists);
  phoneCompleted.task_lists[2].name = "Finished sculptures and studio preparation reference archive";
  current = await open("task-lists-move-mobile", phoneCompleted, true);
  const touchDestination = await focusMoveDestination(current, 3, [1], "touch");
  await capture(current.page, "task-lists-move-completed-mobile");
  await current.page.touchscreen.tap(touchDestination.item.x, touchDestination.item.y); await paint(current.page);
  const movedPhone = moveMembership(phoneCompleted, [1, 5], 3);
  await expectState(current.page, movedPhone, "A real touch must move the same completed records to the chosen list without changing open or hidden work");
  await undo(current, phoneCompleted);
  await key(current.page, "Control+Shift+z"); await expectState(current.page, movedPhone);
  await boot(current, true); await expectState(current.page, movedPhone);
  await current.context.close();

  progress("delete completed work only from the selected list and undo it");
  const bulk = structuredClone(fixture); bulk.tasks[0].complete = true; bulk.tasks[1].complete = true;
  current = await open("task-lists-bulk", bulk);
  await focusRowControl(current, 8); // Completed row5, input/+2, bulk1.
  await key(current.page, "Space");
  await resetTasks(current, false);
  await tab(current.page, headerTargets(current) + 16); // Keep confirmation open; it replaces the bulk button.
  await key(current.page, "Space");
  const cleared = structuredClone(bulk); cleared.tasks.shift(); cleared.deadlines.shift();
  await expectState(current.page, cleared, "Bulk deletion must retain completed work in unselected lists and all featured work");
  await undo(current, bulk);
  await current.context.close();

  assert.deepEqual(issues, [], "Task-list changes must stay local and produce no app errors");
  console.log("Task-list browser checks passed: legacy state, five-list cap, create/rename/remove with task preservation, repeated-minus confirmation by mouse/keyboard/touch with cancellation and tab-switch disarming, single/Shift/touch combinations and distinct Gantt shades, single-click row boxes, bounded Up/Down selection and atomic visible-slot Ctrl+Up ordering, mouse/touch grip reorder with release-only commits/cancellation/atomic Undo, double-click unified name/details/list/timed-date editing with atomic Save/Undo, Escape/outside cancellation retaining Redo, compact boxed × deletion, linked date/completion edits and scoped completed removal, stable scoped due-date sorting, and completed-task moves to chosen lists through mouse/keyboard/touch with menu cancellation, single/Multi scope, selected destinations, preserved records and featured work, atomic Undo/Redo, refresh and JSON round trips.");
} catch (error) {
  console.error(`Task-list browser checks failed during: ${phase}`, error);
  if (current && !current.page.isClosed()) await capture(current.page, "task-lists-failure").catch(() => {});
  throw error;
} finally {
  clearTimeout(timeout);
  for (const context of contexts) await context.close().catch(() => {});
  await browser.close();
}

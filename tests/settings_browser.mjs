// Application settings, real user controls and real browser storage/files.
// All records are synthetic; the directory chooser returns a sandboxed OPFS
// handle, while IndexedDB, streams and all application logic remain native.
import assert from "node:assert/strict";
import { mkdir, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium } from "playwright";
import { installFrameFence, waitForAppPaint } from "./browser_frames.mjs";
import "./settings_bridge.mjs";

const base = process.env.JIG_PREVIEW_URL ?? "http://127.0.0.1:4173/Jig/";
const origin = new URL(base).origin;
const taskKey = "jig.personal.v1", settingsKey = "jig.settings.v1";
const results = resolve("playwright-results");
await mkdir(results, { recursive: true });
const fixture = {
  version: 1, preferences: { decimals: 2, focus: 3, weather: null },
  history: [{ expression: "2 + 2", result: 4 }],
  tasks: [{ id: 1, text: "Sample: settings task", complete: false, deadline: 101, color: "rose" }],
  deadlines: [{ id: 101, title: "Sample: settings task", at: "2030-01-15T18:00:00Z" }],
  entries: [], notes: [{ id: 201, text: "Sample: useful note", pinned: true }], featured: 101,
};
const defaults = { version: 1, theme: "graphite", undo_levels: 20, auto_backup: false };
const taskRaw = JSON.stringify(fixture);
const issues = [], contexts = [];
let current, phase = "launch Chromium";
const timer = setTimeout(() => { console.error(`Settings browser exceeded 420 seconds during: ${phase}`); process.exit(1); }, 420000);
function progress(message) { phase = message; console.log(`Settings browser phase: ${message}`); }
async function paint(page) { await waitForAppPaint(page); }
async function key(page, value) { await page.keyboard.press(value); await paint(page); }
async function tab(page, count = 1, reverse = false) { for (let index = 0; index < count; index++) await key(page, reverse ? "Shift+Tab" : "Tab"); }
async function capture(page, name) {
  const png = await page.screenshot({ path: resolve(results, `${name}.png`), timeout: 60000, animations: "disabled", caret: "hide" });
  if (new Set((process.env.JIG_RENDER_LOGS ?? "").split(",")).has(name) && png.length <= 1024 * 1024) {
    console.log(`JIG_RENDER_BEGIN_${name}`); const encoded = png.toString("base64");
    for (let offset = 0; offset < encoded.length; offset += 2048) console.log(encoded.slice(offset, offset + 2048));
    console.log(`JIG_RENDER_END_${name}`);
  }
}
const browser = await chromium.launch({ headless: true, args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"] });
async function open(label, { unsupported = false, directory = false, clock = false, phone = false } = {}) {
  const context = await browser.newContext({ viewport: phone ? { width: 390, height: 844 } : { width: 1280, height: 1400 }, hasTouch: phone, acceptDownloads: true, timezoneId: "Etc/UTC", colorScheme: "dark" });
  contexts.push(context); await installFrameFence(context);
  await context.addInitScript(({ taskKey, settingsKey, taskRaw, defaults }) => {
    if (sessionStorage.getItem("jig-settings-test-seeded") !== "yes") {
      localStorage.setItem(taskKey, taskRaw); localStorage.setItem(settingsKey, JSON.stringify(defaults));
      localStorage.setItem("jig.workspace.v1", "tasks"); sessionStorage.setItem("jig-settings-test-seeded", "yes");
    }
  }, { taskKey, settingsKey, taskRaw, defaults });
  if (unsupported) await context.addInitScript(() => Object.defineProperty(window, "showDirectoryPicker", { configurable: true, value: undefined }));
  if (directory) await context.addInitScript(() => {
    const increment = key => sessionStorage.setItem(key, String(Number(sessionStorage.getItem(key) ?? 0) + 1));
    const prototype = window.FileSystemHandle?.prototype;
    if (prototype) for (const method of ["queryPermission", "requestPermission"]) {
      const original = prototype[method];
      Object.defineProperty(prototype, method, { configurable: true, value: function (options) {
        increment(`jig-test-${method}`);
        // OPFS grants its origin private files implicitly. Older Chromium
        // versions omit permission methods for those already-authorized handles.
        return original ? original.call(this, options) : Promise.resolve("granted");
      } });
    }
    Object.defineProperty(window, "showDirectoryPicker", { configurable: true, value: async options => {
      increment("jig-test-picker");
      if (!navigator.userActivation.isActive) throw new Error("The folder chooser requires an actual user activation");
      if (options.mode !== "readwrite" || options.id !== "jig-backups") throw new Error("Unexpected folder picker options");
      const root = await navigator.storage.getDirectory();
      return root.getDirectoryHandle("jig-settings-browser", { create: true });
    } });
  });
  await context.route("**/*", async route => {
    const url = new URL(route.request().url());
    if (["data:", "blob:", "about:"].includes(url.protocol) || url.origin === origin) await route.continue();
    else { issues.push(`${label}: unexpected request ${url.origin}${url.pathname}`); await route.abort(); }
  });
  const page = await context.newPage(); page.on("pageerror", error => issues.push(`${label}: ${error.message}`)); page.setDefaultTimeout(10000);
  if (clock) await page.clock.install({ time: new Date("2026-10-30T12:00:00Z") });
  const scene = { context, page, settingsOpen: false }; await boot(scene); return scene;
}
async function boot(scene, reload = false) {
  if (reload) await scene.page.reload({ waitUntil: "load" }); else await scene.page.goto(base, { waitUntil: "load" });
  await scene.page.bringToFront();
  await scene.page.waitForFunction(() => window.jigReady === true || typeof window.jigBootError === "string", undefined, { timeout: 60000 });
  assert.equal(await scene.page.evaluate(() => window.jigBootError), undefined);
  await scene.page.waitForTimeout(500); scene.settingsOpen = false;
  await scene.page.waitForFunction(() => !JSON.parse(window.jigBackupStatus()).busy, undefined, { polling: 25 });
}
async function state(page) { return page.evaluate(key => JSON.parse(localStorage.getItem(key)), taskKey); }
async function raw(page, key = taskKey) { return page.evaluate(key => localStorage.getItem(key), key); }
async function settings(page) { return JSON.parse(await raw(page, settingsKey)); }
async function waitSettings(page, expected) {
  await page.waitForFunction(({ key, expected }) => {
    const value = JSON.parse(localStorage.getItem(key)); return Object.entries(expected).every(([field, expected]) => value[field] === expected);
  }, { key: settingsKey, expected }, { polling: 25 });
}
async function resetFocus(page) {
  const { width, height } = page.viewportSize();
  await page.locator("#jig_canvas").click({ position: { x: width - 6, y: height - 6 } }); await key(page, "Escape");
}
async function waitDoneFocus(page) {
  // Settings opens after the current app update. Fence its actual focused
  // control rather than letting the first Tab race that deferred update.
  const until = Date.now() + 10000, { width, height } = page.viewportSize(); let attempt = 0;
  while (Date.now() < until) {
    await page.mouse.move(width - 6 - attempt++ % 2, height - 6);
    const png = await page.screenshot({ clip: { x: 81, y: 31, width: 40, height: 100 }, timeout: Math.max(1, until - Date.now()), animations: "disabled", caret: "hide" });
    const focused = await page.evaluate(async encoded => {
      const bitmap = await createImageBitmap(new Blob([Uint8Array.from(atob(encoded), c => c.charCodeAt(0))], { type: "image/png" }));
      const canvas = new OffscreenCanvas(bitmap.width, bitmap.height), context = canvas.getContext("2d"); context.drawImage(bitmap, 0, 0);
      const pixels = context.getImageData(0, 0, bitmap.width, bitmap.height).data; let active = 0;
      for (let index = 0; index < pixels.length; index += 4) if ([[77, 49, 39], [226, 192, 173]].some(color => color.every((value, axis) => Math.abs(pixels[index + axis] - value) <= 3))) active++;
      bitmap.close(); return active > 80;
    }, png.toString("base64"));
    if (focused) return;
    await page.waitForTimeout(Math.min(50, Math.max(0, until - Date.now())));
  }
  assert.fail("Settings must visibly focus Done before keyboard navigation starts");
}
async function closeSettings(scene) { if (scene.settingsOpen) { await key(scene.page, "Control+Comma"); scene.settingsOpen = false; } }
async function openSettings(scene, header = false) {
  await closeSettings(scene); await resetFocus(scene.page);
  if (header) { await tab(scene.page, 2); await key(scene.page, "Space"); }
  else await key(scene.page, "Control+Comma");
  scene.settingsOpen = true; await waitDoneFocus(scene.page);
}
async function tapSettings(scene) {
  await closeSettings(scene); await resetFocus(scene.page); await tab(scene.page, 2);
  const png = await scene.page.screenshot({ clip: { x: 0, y: 0, width: scene.page.viewportSize().width, height: 31 }, timeout: 10000, animations: "disabled", caret: "hide" });
  const point = await scene.page.evaluate(async encoded => {
    const bitmap = await createImageBitmap(new Blob([Uint8Array.from(atob(encoded), c => c.charCodeAt(0))], { type: "image/png" }));
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height), context = canvas.getContext("2d"); context.drawImage(bitmap, 0, 0);
    const pixels = context.getImageData(0, 0, bitmap.width, bitmap.height).data;
    for (const color of [[77, 49, 39], [226, 192, 173]]) {
      let left = bitmap.width, right = -1, top = bitmap.height, bottom = -1, count = 0;
      for (let y = 0; y < bitmap.height; y++) for (let x = 0; x < bitmap.width; x++) {
        const index = (y * bitmap.width + x) * 4;
        if (!color.every((value, axis) => Math.abs(pixels[index + axis] - value) <= 3)) continue;
        left = Math.min(left, x); right = Math.max(right, x); top = Math.min(top, y); bottom = Math.max(bottom, y); count++;
      }
      if (count > 200 && right - left >= 45 && right - left <= 100 && bottom - top >= 16) {
        bitmap.close(); return { x: (left + right) / 2, y: (top + bottom) / 2 };
      }
    }
    bitmap.close(); return null;
  }, png.toString("base64"));
  assert.ok(point, "The focused Settings button must be visible beside Notes in the phone header");
  await scene.page.touchscreen.tap(point.x, point.y); await paint(scene.page); scene.settingsOpen = true;
  await waitDoneFocus(scene.page);
}
async function checkInfoPage(scene, index, name) {
  await closeSettings(scene); await resetFocus(scene.page);
  const beforeTask = await raw(scene.page), beforeSettings = await raw(scene.page, settingsKey);
  await tab(scene.page, index); await key(scene.page, "Space");
  // Both information pages focus Done in the main panel. The phone header
  // wraps, so inspect below its second line rather than its old desktop y.
  const top = scene.page.viewportSize().width < 500 ? 60 : 32, until = Date.now() + 10000;
  let done = false;
  do {
    const png = await scene.page.screenshot({ clip: { x: 0, y: top, width: scene.page.viewportSize().width, height: 100 }, timeout: 10000, animations: "disabled", caret: "hide" });
    done = await scene.page.evaluate(async encoded => {
      const bitmap = await createImageBitmap(new Blob([Uint8Array.from(atob(encoded), c => c.charCodeAt(0))], { type: "image/png" }));
      const canvas = new OffscreenCanvas(bitmap.width, bitmap.height), context = canvas.getContext("2d"); context.drawImage(bitmap, 0, 0);
      const pixels = context.getImageData(0, 0, bitmap.width, bitmap.height).data;
      let focused = false;
      for (const color of [[77, 49, 39], [226, 192, 173]]) {
        let left = bitmap.width, right = -1, first = bitmap.height, last = -1, count = 0;
        for (let y = 0; y < bitmap.height; y++) for (let x = 0; x < bitmap.width; x++) {
          const at = (y * bitmap.width + x) * 4;
          if (!color.every((value, axis) => Math.abs(pixels[at + axis] - value) <= 3)) continue;
          left = Math.min(left, x); right = Math.max(right, x); first = Math.min(first, y); last = Math.max(last, y); count++;
        }
        if (count > 100 && right - left >= 30 && right - left <= 100 && last - first >= 16 && last - first <= 40) focused = true;
      }
      bitmap.close(); return focused;
    }, png.toString("base64"));
    if (!done) await scene.page.waitForTimeout(50);
  } while (!done && Date.now() < until);
  assert.ok(done, `${name} must open its main panel and focus Done`);
  assert.equal(await raw(scene.page), beforeTask, `${name} must preserve the document`);
  assert.equal(await raw(scene.page, settingsKey), beforeSettings, `${name} must preserve settings`);
  assert.equal(await scene.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true, `${name} must fit the viewport`);
  await capture(scene.page, `settings-${name.toLowerCase()}${scene.page.viewportSize().width < 500 ? "-mobile" : ""}`);
  await key(scene.page, "Escape");
  assert.equal(await raw(scene.page), beforeTask); assert.equal(await raw(scene.page, settingsKey), beforeSettings);
}
async function focusSetting(scene, index) { await openSettings(scene); await tab(scene.page, index); }
async function backupStatus(page) { return page.evaluate(() => JSON.parse(window.jigBackupStatus())); }
async function prefix(page) { return 7 + Number((await backupStatus(page)).supported); }
async function chooseTheme(scene, theme) {
  await focusSetting(scene, 1); await key(scene.page, "Space");
  // The ComboBox's popup scroll target precedes its three options.
  await tab(scene.page, { graphite: 2, light: 3, system: 4 }[theme]); await key(scene.page, "Space");
  await waitSettings(scene.page, { theme });
}
async function brightness(page) {
  const png = await page.screenshot({ animations: "disabled", caret: "hide" });
  return page.evaluate(async encoded => {
    const bitmap = await createImageBitmap(new Blob([Uint8Array.from(atob(encoded), c => c.charCodeAt(0))], { type: "image/png" }));
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height), context = canvas.getContext("2d"); context.drawImage(bitmap, 0, 0);
    let sum = 0, count = 0;
    for (const [x, y] of [[100, 500], [bitmap.width - 100, 500], [bitmap.width - 40, bitmap.height - 120]]) {
      const pixels = context.getImageData(x, y, 20, 20).data;
      for (let index = 0; index < pixels.length; index += 4) { sum += 0.2126 * pixels[index] + 0.7152 * pixels[index + 1] + 0.0722 * pixels[index + 2]; count++; }
    }
    bitmap.close(); return sum / count;
  }, png.toString("base64"));
}
async function waitBrightness(page, light) {
  const until = Date.now() + 5000; let actual;
  do { actual = await brightness(page); if (light ? actual > 150 : actual < 100) return actual; await page.waitForTimeout(50); } while (Date.now() < until);
  assert.ok(light ? actual > 150 : actual < 100, `Expected ${light ? "light" : "dark"} theme, measured brightness ${actual}`);
}
async function calculate(scene, expression, count, chord = "Control+Alt+4") {
  await closeSettings(scene); await resetFocus(scene.page); await key(scene.page, chord);
  await scene.page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { polling: 25 });
  await scene.page.keyboard.press("Control+a"); await scene.page.keyboard.type(expression, { delay: 15 }); await key(scene.page, "Enter");
  await scene.page.waitForFunction(({ key, count, expression }) => {
    const value = JSON.parse(localStorage.getItem(key)); return value.history.length === count && value.history.at(-1).expression === expression;
  }, { key: taskKey, count, expression }, { polling: 25 });
}
async function undo(scene) { await closeSettings(scene); await resetFocus(scene.page); await key(scene.page, "Control+z"); }
async function redo(scene) { await closeSettings(scene); await resetFocus(scene.page); await key(scene.page, "Control+Shift+z"); }
async function setLevels(scene, levels) {
  await focusSetting(scene, 5);
  await scene.page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { polling: 25 });
  await scene.page.keyboard.press("Control+a"); await scene.page.keyboard.type(String(levels)); await key(scene.page, "Enter");
  await waitSettings(scene.page, { undo_levels: levels });
}
async function downloadAt(scene, index) {
  await focusSetting(scene, index); const event = scene.page.waitForEvent("download"); await key(scene.page, "Space");
  const download = await event; return { name: download.suggestedFilename(), data: JSON.parse(await readFile(await download.path(), "utf8")) };
}
async function importSettings(scene, value, name = "sample-settings.json") {
  await focusSetting(scene, 3);
  const routing = await scene.page.evaluate(() => ({ tag: document.activeElement?.tagName, id: document.activeElement?.id, focused: document.hasFocus(), activation: { active: navigator.userActivation.isActive, previouslyActive: navigator.userActivation.hasBeenActive } }));
  console.log(`Settings file chooser keyboard routing: ${JSON.stringify(routing)}`);
  assert.equal(routing.id, "jig_canvas", "Settings import activation must reach the app's keyboard handler");
  const event = scene.page.waitForEvent("filechooser"); await key(scene.page, "Space");
  const chooser = await event; assert.equal(await chooser.element().getAttribute("id"), "jig-settings-import");
  await chooser.setFiles({ name, mimeType: "application/json", buffer: Buffer.from(typeof value === "string" ? value : JSON.stringify(value)) });
  await scene.page.waitForFunction(() => document.activeElement?.id === "jig_canvas", undefined, { polling: 25 });
  await scene.page.waitForTimeout(1200);
  const { width, height } = scene.page.viewportSize(); await scene.page.mouse.move(width - 6, height - 6); await paint(scene.page);
}
async function backupFiles(page) {
  return page.evaluate(async () => {
    const root = await navigator.storage.getDirectory(), folder = await root.getDirectoryHandle("jig-settings-browser", { create: true }), files = [];
    for await (const [name, handle] of folder.entries()) if (handle.kind === "file" && name.startsWith("jig-backup-")) files.push({ name, data: JSON.parse(await (await handle.getFile()).text()) });
    return files.sort((a, b) => a.name.localeCompare(b.name));
  });
}
async function waitFiles(page, count) {
  try {
    // waitForFunction tests a returned Promise's truthiness before it resolves.
    // Await the actual filesystem probe here, then explicitly retry false.
    const until = Date.now() + 15000;
    while (Date.now() < until) {
      let timer;
      try {
        const snapshot = await Promise.race([page.evaluate(async count => {
          const before = JSON.parse(window.jigBackupStatus()); if (before.busy) return false;
          const folder = await (await navigator.storage.getDirectory()).getDirectoryHandle("jig-settings-browser", { create: true }), files = [];
          for await (const [name, handle] of folder.entries()) if (handle.kind === "file" && name.startsWith("jig-backup-") && name.endsWith(".json")) {
            // A write can begin after the initial status read. Retry incomplete
            // files, then recheck the writer after every asynchronous file read.
            try { files.push({ name, data: JSON.parse(await (await handle.getFile()).text()) }); }
            catch { return false; }
          }
          const after = JSON.parse(window.jigBackupStatus());
          return !after.busy && before.last_backup === after.last_backup && files.length === count
            ? files.sort((a, b) => a.name.localeCompare(b.name)) : false;
        }, count), new Promise((_, reject) => { timer = setTimeout(() => reject(new Error("Backup filesystem probe exceeded its deadline")), Math.max(1, until - Date.now())); })]);
        if (Array.isArray(snapshot) && snapshot.length === count) return snapshot;
      } finally { clearTimeout(timer); }
      await new Promise(resolve => setTimeout(resolve, Math.min(25, Math.max(0, until - Date.now()))));
    }
    throw new Error("Committed backup count did not reach the expected value before the deadline");
  } catch (error) {
    const files = await page.evaluate(async () => {
      const folder = await (await navigator.storage.getDirectory()).getDirectoryHandle("jig-settings-browser", { create: true }), files = [];
      for await (const [name, handle] of folder.entries()) if (handle.kind === "file" && name.startsWith("jig-backup-")) {
        try { files.push({ name, text: await (await handle.getFile()).text() }); }
        catch (error) { files.push({ name, error: String(error) }); }
      }
      return files;
    });
    throw new Error(`Expected ${count} committed backups: ${JSON.stringify({ status: await backupStatus(page), files, current: await state(page), settings: await settings(page) })}`, { cause: error });
  }
}

try {
  progress("settings header and persistent Graphite, Light and System themes");
  current = await open("settings-theme"); const dark = await waitBrightness(current.page, false);
  await capture(current.page, "settings-header-desktop");
  await checkInfoPage(current, 4, "About"); await checkInfoPage(current, 5, "Help");
  await chooseTheme(current, "light"); await closeSettings(current); const light = await waitBrightness(current.page, true);
  assert.ok(light > dark + 100); assert.equal(await raw(current.page), taskRaw);
  await capture(current.page, "settings-light"); await boot(current, true); await waitBrightness(current.page, true);
  assert.equal((await settings(current.page)).theme, "light"); assert.equal(await raw(current.page), taskRaw);
  await chooseTheme(current, "graphite"); await closeSettings(current); await waitBrightness(current.page, false);
  await current.page.emulateMedia({ colorScheme: "light" }); await chooseTheme(current, "system"); await closeSettings(current); await waitBrightness(current.page, true);
  await current.page.emulateMedia({ colorScheme: "dark" }); await waitBrightness(current.page, false);
  assert.equal((await settings(current.page)).theme, "system"); assert.equal(await raw(current.page), taskRaw);
  await current.context.close();

  progress("phone Settings touch target and persistent theme selection");
  current = await open("settings-mobile", { phone: true });
  await checkInfoPage(current, 4, "About"); await checkInfoPage(current, 5, "Help");
  await tapSettings(current);
  // A real tap must open Settings and focus Done. The overflowing phone
  // ScrollArea contributes one Tab stop before its Theme control.
  await tab(current.page, 2); await key(current.page, "Space"); await tab(current.page, 3); await key(current.page, "Space");
  await waitSettings(current.page, { theme: "light" }); await waitBrightness(current.page, true);
  assert.equal(await raw(current.page), taskRaw); await boot(current, true);
  assert.equal((await settings(current.page)).theme, "light"); await waitBrightness(current.page, true);
  await tapSettings(current); await capture(current.page, "settings-mobile");
  assert.equal(await raw(current.page), taskRaw); await current.context.close();

  progress("shortcut capture, reserved/conflicting keys, current guide and reset");
  current = await open("settings-shortcuts"); await calculate(current, "2 + 3", 2); const withCalculation = await raw(current.page);
  await openSettings(current, true); await tab(current.page, (await prefix(current.page)) + 7); await key(current.page, "Space");
  const beforeBinding = await raw(current.page, settingsKey);
  await key(current.page, "Control+z"); assert.equal(await raw(current.page, settingsKey), beforeBinding); assert.equal(await raw(current.page), withCalculation);
  await key(current.page, "Control+Alt+n"); assert.equal(await raw(current.page, settingsKey), beforeBinding); assert.equal(await raw(current.page), withCalculation);
  await key(current.page, "Control+Alt+5");
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).shortcuts?.calculator?.key === "5", settingsKey);
  assert.deepEqual((await settings(current.page)).shortcuts.calculator, { key: "5", ctrl: true, shift: false, alt: true, meta: false });
  await capture(current.page, "settings-shortcuts"); await closeSettings(current); await resetFocus(current.page);
  await key(current.page, "Control+Alt+4"); await current.page.waitForTimeout(200);
  assert.notEqual(await current.page.evaluate(() => document.activeElement?.tagName), "INPUT", "The replaced shortcut must stop opening Calculator");
  await calculate(current, "3 * 7", 3, "Control+Alt+5"); assert.equal((await state(current.page)).history.at(-1).result, 21);
  await focusSetting(current, (await prefix(current.page)) + 26); await key(current.page, "Space"); // 12 rows plus one enabled Calculator Reset, then Reset all.
  await current.page.waitForFunction(key => !Object.keys(JSON.parse(localStorage.getItem(key)).shortcuts ?? {}).length, settingsKey);
  await closeSettings(current); await resetFocus(current.page); await key(current.page, "Control+Alt+5"); await current.page.waitForTimeout(200);
  assert.notEqual(await current.page.evaluate(() => document.activeElement?.tagName), "INPUT");
  await calculate(current, "4 * 8", 4); assert.equal((await state(current.page)).history.at(-1).result, 32);
  await focusSetting(current, (await prefix(current.page)) + 7); await key(current.page, "Space"); await key(current.page, "Control+Alt+5");
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).shortcuts?.calculator?.key === "5", settingsKey);
  await focusSetting(current, (await prefix(current.page)) + 27); await key(current.page, "Space");
  await current.page.mouse.move(500, 900); await current.page.mouse.wheel(0, 1000); await paint(current.page);
  await capture(current.page, "settings-cheatsheet"); await current.context.close();

  progress("Undo level limits persist independently of task data");
  current = await open("settings-history"); await setLevels(current, 2); assert.equal(await raw(current.page), taskRaw);
  await calculate(current, "10 + 1", 2); await calculate(current, "10 + 2", 3); await calculate(current, "10 + 3", 4); const all = await state(current.page);
  await undo(current); assert.equal((await state(current.page)).history.length, 3);
  await undo(current); assert.equal((await state(current.page)).history.length, 2); const oldest = await state(current.page);
  await undo(current); assert.deepEqual(await state(current.page), oldest, "Undo must stop at the configured number of retained steps");
  await redo(current); assert.equal((await state(current.page)).history.length, 3); await redo(current); assert.deepEqual(await state(current.page), all);
  await setLevels(current, 0); await calculate(current, "10 + 4", 5); const disabled = await state(current.page);
  await undo(current); await redo(current); assert.deepEqual(await state(current.page), disabled, "Zero must disable application history");
  await boot(current, true); assert.equal((await settings(current.page)).undo_levels, 0); await undo(current); assert.deepEqual(await state(current.page), disabled);
  await current.context.close();

  progress("settings export/import validates separately from the document");
  current = await open("settings-portable"); const initialSettings = await raw(current.page, settingsKey);
  const exported = await downloadAt(current, 2); assert.equal(exported.name, "jig-settings.json"); assert.deepEqual(exported.data, defaults);
  for (const invalid of ["{broken", { ...defaults, version: 99 }, { ...defaults, undo_levels: 101 }, { ...defaults, theme: "neon" }, { ...defaults, shortcuts: { calculator: { key: "Z", ctrl: true } } }]) {
    await importSettings(current, invalid); assert.equal(await raw(current.page, settingsKey), initialSettings); assert.equal(await raw(current.page), taskRaw);
  }
  const imported = { ...defaults, theme: "light", undo_levels: 3, shortcuts: { calculator: { key: "5", ctrl: true, shift: false, alt: true, meta: false } } };
  await importSettings(current, imported); assert.equal(await raw(current.page, settingsKey), initialSettings, "Valid settings still wait for explicit Apply");
  await tab(current.page, 2, true); await key(current.page, "Space"); // Done -> Cancel import -> Apply imported settings.
  await waitSettings(current.page, { theme: "light", undo_levels: 3 }); assert.equal(await raw(current.page), taskRaw);
  await closeSettings(current); await waitBrightness(current.page, true); await boot(current, true);
  assert.deepEqual(await settings(current.page), imported); assert.equal(await raw(current.page), taskRaw);
  await current.context.close();

  progress("unsupported folder browsers retain explicit personal JSON downloads");
  current = await open("settings-download-fallback", { unsupported: true }); assert.equal((await backupStatus(current.page)).supported, false);
  const downloaded = await downloadAt(current, 6); assert.equal(downloaded.name, "jig.json"); assert.deepEqual(downloaded.data, fixture); assert.equal(await raw(current.page), taskRaw);
  await capture(current.page, "settings-download-fallback"); await current.context.close();

  progress("real directory streams, unique backups, debounce and remembered folder");
  current = await open("settings-folder", { directory: true, clock: true }); assert.equal((await backupStatus(current.page)).supported, true);
  await current.page.evaluate(async () => {
    const folder = await (await navigator.storage.getDirectory()).getDirectoryHandle("jig-settings-browser", { create: true });
    const file = await folder.getFileHandle("user-owned.txt", { create: true }), stream = await file.createWritable(); await stream.write("Sample: preserve this file"); await stream.close();
  });
  await focusSetting(current, 6); await key(current.page, "Space");
  await current.page.waitForFunction(() => { const s = JSON.parse(window.jigBackupStatus()); return !s.busy && s.folder === "jig-settings-browser" && s.permission === "granted"; });
  await focusSetting(current, 9); await key(current.page, "Space"); const first = await waitFiles(current.page, 1); assert.deepEqual(first[0].data, fixture);
  await focusSetting(current, 9); await key(current.page, "Space"); const second = await waitFiles(current.page, 2);
  assert.notEqual(second[0].name, second[1].name); assert.ok(second.every(file => /^jig-backup-.*\.json$/.test(file.name) && JSON.stringify(file.data) === JSON.stringify(first[0].data)));
  await focusSetting(current, 8); await key(current.page, "Space"); await waitSettings(current.page, { auto_backup: true });
  await calculate(current, "6 * 7", 2); const calculated = await state(current.page);
  await current.page.clock.fastForward(240000); assert.equal((await backupFiles(current.page)).length, 2, "Automatic backup must not run before five minutes");
  await current.page.clock.fastForward(70000); const automatic = await waitFiles(current.page, 3);
  const savedCalculation = automatic.find(file => file.data.history?.at(-1)?.expression === "6 * 7");
  assert.ok(savedCalculation, `Automatic backup must contain the saved calculation: ${JSON.stringify({ files: automatic, calculated, current: await state(current.page), settings: await settings(current.page), status: await backupStatus(current.page) })}`);
  assert.deepEqual(savedCalculation.data, calculated, `Automatic backup ${savedCalculation.name} must preserve every saved document field`);
  await current.page.clock.fastForward(310000); assert.equal((await backupFiles(current.page)).length, 3, "No saved changes means no repeated automatic backups");
  await boot(current, true);
  const restored = await backupStatus(current.page); assert.equal(restored.folder, "jig-settings-browser"); assert.equal(restored.permission, "granted");
  assert.equal(await current.page.evaluate(() => Number(sessionStorage.getItem("jig-test-picker"))), 1, "Reload must restore the handle without opening another chooser");
  assert.equal(await current.page.evaluate(() => Number(sessionStorage.getItem("jig-test-requestPermission"))), 0, "Reload must not prompt for permission");
  const settingsDownload = await downloadAt(current, 2); assert.equal(settingsDownload.name, "jig-settings.json"); assert.equal(JSON.stringify(settingsDownload.data).includes("jig-settings-browser"), false);
  await capture(current.page, "settings-folder");
  await closeSettings(current); await resetFocus(current.page); await key(current.page, "Control+Alt+2"); await key(current.page, "Control+Alt+d");
  await tab(current.page, 8); await key(current.page, "Space"); await key(current.page, "Control+Alt+d");
  assert.deepEqual(await state(current.page), calculated, "Sample mode must leave committed personal data intact");
  await current.page.clock.fastForward(310000); assert.equal((await backupFiles(current.page)).length, 3, "Temporary samples must never be written to the folder");
  await openSettings(current); await tab(current.page, 9);
  // With automatic backups already on, the next enabled control after it is
  // Restore. Both sample backup-writing buttons must be skipped by Tab.
  const chooserEvent = current.page.waitForEvent("filechooser"); await key(current.page, "Space"); const chooser = await chooserEvent;
  assert.equal(await chooser.element().getAttribute("id"), "jig-import"); await chooser.setFiles([]); current.settingsOpen = false;
  assert.equal((await backupFiles(current.page)).length, 3); assert.deepEqual(await state(current.page), calculated);
  assert.equal(await current.page.evaluate(async () => {
    const folder = await (await navigator.storage.getDirectory()).getDirectoryHandle("jig-settings-browser"); return (await (await folder.getFileHandle("user-owned.txt")).getFile()).text();
  }), "Sample: preserve this file");
  await current.context.close();
  assert.deepEqual(issues, [], "Settings, backups and documents must stay local without browser errors");
  console.log("Settings browser passed: stable header, persistent Graphite/Light/System themes, captured shortcuts/conflict rejection/reset/guide, configurable Undo limits, validated separate settings portability, personal download fallback, real OPFS backup streams/unique names/debounce/IDB restoration, and sample backup exclusion.");
} catch (error) {
  console.error(`Settings browser failed during: ${phase}`, error);
  if (current && !current.page.isClosed()) await capture(current.page, "settings-failure").catch(() => {});
  throw error;
} finally {
  clearTimeout(timer); let cleanupTimer;
  try { await Promise.race([Promise.all([...contexts.map(context => context.close().catch(() => {})), browser.close()]), new Promise((_, reject) => { cleanupTimer = setTimeout(() => reject(new Error("Settings browser shutdown exceeded 10 seconds")), 10000); })]); }
  catch (error) { console.error(error); process.exit(1); } finally { clearTimeout(cleanupTimer); }
}

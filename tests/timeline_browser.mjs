// Real browser checks for the day-based timeline. All records are synthetic.
// Run against a built preview served at /Jig/: node tests/timeline_browser.mjs
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
  tasks: [
    { id: 1, text: "Sample: casting studies", complete: false, deadline: 101, color: "rose", start: "2026-10-26T12:34:56Z" },
    { id: 2, text: "Sample: drying tests", complete: false, deadline: 102, color: "amber", start: "2026-10-28T00:00:00Z" },
    { id: 3, text: "Sample: finished glazing", complete: true, deadline: 103, color: "sage", start: "2026-10-25T00:00:00Z" },
    { id: 4, text: "Sample: photograph sculptures", complete: false, deadline: 104, color: "blue" },
    { id: 5, text: "Sample: exhibition opening", complete: false, deadline: 105, color: "lilac" },
    { id: 6, text: "Sample: explore a new form", complete: false },
  ],
  deadlines: [
    { id: 101, title: "Sample: casting studies", at: "2026-10-30T18:00:00Z" },
    { id: 102, title: "Sample: drying tests", at: "2026-10-30T18:00:00Z" },
    { id: 103, title: "Sample: finished glazing", at: "2026-10-29T18:00:00Z" },
    { id: 104, title: "Sample: photograph sculptures", at: "2026-10-31T18:00:00Z" },
    { id: 105, title: "Sample: exhibition opening", at: "2026-11-08T18:00:00Z" },
    { id: 107, title: "Sample: legacy delivery date", at: "2026-10-30T18:00:00Z" },
  ],
  entries: [{ id: 301, title: "Sample: hidden studio note", date: "2026-10-30" }],
  featured: 105,
};

let phase = "launch Chromium";
let current;
let originalError;
const contexts = [];
const issues = [];
// These maps track only the public layout commands sent by this driver.
const timelineMode = new WeakMap();
const timer = setTimeout(() => {
  console.error(`Timeline browser checks exceeded 360 seconds during: ${phase}`);
  if (originalError) console.error("Original timeline failure:", originalError);
  process.exit(1);
}, 360000);
function progress(message) { phase = message; console.log(`Timeline browser phase: ${message}`); }
async function paint(page) {
  await waitForAppPaint(page);
}
async function key(page, value) {
  await page.keyboard.press(value);
  if (value === "Control+Alt+t") timelineMode.set(page, !timelineMode.get(page));
  await paint(page);
}
async function tab(page, count = 1) { for (let index = 0; index < count; index++) await key(page, "Tab"); }
async function capture(page, name) {
  const png = await page.screenshot({ path: resolve(results, `${name}.png`), timeout: 60000, animations: "disabled", caret: "hide" });
  const allowed = new Set(["timeline-desktop", "timeline-dashboard", "timeline-divider", "timeline-label-divider", "timeline-range-drag", "timeline-mobile", "timeline-edit", "timeline-failure"]);
  const requested = new Set((process.env.JIG_RENDER_LOGS ?? "").split(","));
  const requestedDivider = ["timeline-divider", "timeline-label-divider", "timeline-range-drag"].includes(name) && requested.has("timeline-dashboard");
  if (allowed.has(name) && (requested.has(name) || requestedDivider) && png.length <= 1024 * 1024) {
    console.log(`JIG_RENDER_BEGIN_${name}`);
    const encoded = png.toString("base64");
    for (let offset = 0; offset < encoded.length; offset += 2048) console.log(encoded.slice(offset, offset + 2048));
    console.log(`JIG_RENDER_END_${name}`);
  }
  return png;
}
const browser = await chromium.launch({ headless: true, args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"] });
async function open(label, state = fixture, phone = false) {
  const context = await browser.newContext({ viewport: phone ? { width: 390, height: 844 } : { width: 1440, height: 1000 }, acceptDownloads: true, hasTouch: phone, timezoneId: phone ? "America/New_York" : "Etc/UTC" });
  contexts.push(context);
  await installFrameFence(context);
  await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin });
  await context.addInitScript(({ raw, storageKey, origin }) => {
    if (location.origin === origin && sessionStorage.getItem("jig-timeline-seeded") !== "yes") {
      localStorage.setItem(storageKey, raw);
      sessionStorage.setItem("jig-timeline-seeded", "yes");
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
  page.setDefaultNavigationTimeout(60000);
  await page.clock.setFixedTime(new Date("2026-10-30T12:00:00Z"));
  await boot(page);
  return { context, page };
}
async function boot(page, reload = false) {
  timelineMode.set(page, false);
  if (reload) await page.reload({ waitUntil: "load" });
  else await page.goto(base, { waitUntil: "load" });
  await page.bringToFront();
  await page.waitForFunction(() => window.jigReady === true || typeof window.jigBootError === "string", undefined, { timeout: 60000 });
  assert.equal(await page.evaluate(() => window.jigBootError), undefined);
  await page.waitForTimeout(500);
  // The spreadsheet is the launch workspace; exercise the retained Workbench.
  await page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
  await page.keyboard.press("Control+Alt+w");
  await paint(page);
}
async function state(page) { return page.evaluate(key => JSON.parse(localStorage.getItem(key)), storageKey); }
async function raw(page) { return page.evaluate(key => localStorage.getItem(key), storageKey); }
async function dividerBounds(page, timeout = 60000) {
  const png = await page.screenshot({ timeout, animations: "disabled", caret: "hide" });
  return page.evaluate(async encoded => {
    const picture = new Image(); picture.src = `data:image/png;base64,${encoded}`; await picture.decode();
    const canvas = document.createElement("canvas"); canvas.width = picture.width; canvas.height = picture.height;
    const context = canvas.getContext("2d"); context.drawImage(picture, 0, 0);
    const { data } = context.getImageData(0, 0, canvas.width, canvas.height);
    const matches = (pixel, color) => color.every((value, axis) => Math.abs(data[pixel * 4 + axis] - value) <= 3);
    const mask = new Uint8Array(canvas.width * canvas.height);
    for (let pixel = 0; pixel < mask.length; pixel++) mask[pixel] = matches(pixel, [113, 102, 93]) || matches(pixel, [244, 107, 74]) ? 1 : 0;
    const queue = new Int32Array(mask.length), lines = [];
    for (let pixel = 0; pixel < mask.length; pixel++) {
      if (!mask[pixel]) continue;
      let start = 0, end = 1, left = canvas.width, right = 0, top = canvas.height, bottom = 0;
      queue[0] = pixel; mask[pixel] = 0;
      while (start < end) {
        const current = queue[start++], x = current % canvas.width, y = Math.floor(current / canvas.width);
        left = Math.min(left, x); right = Math.max(right, x); top = Math.min(top, y); bottom = Math.max(bottom, y);
        for (const neighbour of [x ? current - 1 : -1, x + 1 < canvas.width ? current + 1 : -1, y ? current - canvas.width : -1, y + 1 < canvas.height ? current + canvas.width : -1]) {
          if (neighbour >= 0 && mask[neighbour]) { mask[neighbour] = 0; queue[end++] = neighbour; }
        }
      }
      if (right - left <= 14 && bottom - top >= 80 && left > canvas.width * 0.18 && right < canvas.width * 0.82) lines.push({ x: (left + right) / 2, top, bottom });
    }
    // The visible grip interrupts a tall separator line. Join its upper/lower
    // pieces, then require the grip's surface at the center to exclude pane edges.
    const joined = [];
    for (const line of lines) {
      const existing = joined.find(item => Math.abs(item.x - line.x) <= 2);
      if (existing) { existing.top = Math.min(existing.top, line.top); existing.bottom = Math.max(existing.bottom, line.bottom); }
      else joined.push({ ...line });
    }
    const grips = [];
    for (const item of joined) {
      if (item.bottom - item.top < 200) continue;
      let start;
      for (let y = item.top + 40; y <= item.bottom - 40; y++) {
        const surface = matches(y * canvas.width + Math.round(item.x), [41, 37, 34]);
        if (surface && start === undefined) start = y;
        if (!surface && start !== undefined) {
          const center = (start + y - 1) / 2;
          if (y - start >= 8 && y - start <= 30 && Math.abs(center - (item.top + item.bottom) / 2) <= 80) grips.push({ ...item, y: center });
          start = undefined;
        }
      }
    }
    return grips.sort((a, b) => (b.bottom - b.top) - (a.bottom - a.top))[0] ?? null;
  }, png.toString("base64"));
}
async function waitForDivider(page, expected = () => true) {
  const until = Date.now() + 60000;
  let bounds;
  do {
    bounds = await dividerBounds(page, Math.max(1, until - Date.now()));
  } while ((!bounds || !expected(bounds)) && Date.now() < until);
  assert.ok(bounds && expected(bounds), `The rendered Tasks/timeline divider must reach its expected position; found ${JSON.stringify(bounds)}`);
  return bounds;
}
async function expectState(page, expected, message) {
  const deadline = Date.now() + 5000;
  while (Date.now() < deadline) {
    const actual = await state(page);
    try { assert.deepEqual(actual, expected); return; } catch { await page.waitForTimeout(50); }
  }
  assert.deepEqual(await state(page), expected, message);
}
async function resetFocus(page) {
  await page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
  await key(page, "Escape");
}
async function timeline(page) {
  await resetFocus(page);
  if (timelineMode.get(page)) await key(page, "Control+Alt+t");
  await key(page, "Control+Alt+t");
  await key(page, "Control+m");
  // Re-enter after maximizing, so the public shortcut focuses Week in the
  // current layout rather than retaining an old tiled control identity.
  await key(page, "Control+Alt+t");
  await key(page, "Control+Alt+t");
  await key(page, "Space"); // Week.
  await tab(page, 4);
  await key(page, "Space"); // Today anchors this deterministic fixture.
  for (let index = 0; index < 4; index++) await key(page, "Shift+Tab");
  await key(page, "Space"); // Apply deferred Week focus.
}
async function tasks(page) {
  await resetFocus(page);
  await key(page, "Control+Alt+4");
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 50 });
  await key(page, "Escape");
  await key(page, "ArrowLeft");
  await key(page, "Control+m");
}
async function bar(page, color = "rose", timeout = 60000) {
  // Locate actual rendered task bars, independent of application data/hooks.
  const png = await page.screenshot({ timeout, animations: "disabled", caret: "hide" });
  return page.evaluate(async ({ encoded, color }) => {
    const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
    const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
    const context = canvas.getContext("2d");
    context.drawImage(bitmap, 0, 0);
    const pixels = context.getImageData(0, 0, bitmap.width, bitmap.height).data;
    const palette = { rose: [234, 133, 173], amber: [218, 185, 126] };
    const target = palette[color];
    const rows = [];
    for (let y = 110; y < Math.min(bitmap.height - 90, 650); y++) {
      const xs = [];
      for (let x = bitmap.width < 500 ? 130 : 205; x < bitmap.width - 25; x++) {
        const offset = (y * bitmap.width + x) * 4;
        if (target.every((value, axis) => Math.abs(pixels[offset + axis] - value) <= 3)) xs.push(x);
      }
      if (xs.length > 15) rows.push({ y, left: xs[0], right: xs.at(-1), count: xs.length });
    }
    const groups = [];
    for (const row of rows) {
      let group = groups.at(-1);
      if (!group || row.y > group.bottom + 1) { group = { top: row.y, bottom: row.y, left: row.left, right: row.right, count: 0 }; groups.push(group); }
      group.bottom = row.y; group.left = Math.min(group.left, row.left); group.right = Math.max(group.right, row.right); group.count += row.count;
    }
    bitmap.close();
    return groups.filter(group => group.bottom - group.top >= 5).sort((a, b) => b.count - a.count).map(group => ({ ...group, width: group.right - group.left + 1, height: group.bottom - group.top + 1 }))[0] ?? null;
  }, { encoded: png.toString("base64"), color });
}
async function waitForBar(page, expected = () => true) {
  const until = Date.now() + 60000;
  let bounds;
  do {
    bounds = await bar(page, "rose", Math.max(1, until - Date.now()));
  } while ((!bounds || !expected(bounds)) && Date.now() < until);
  assert.ok(bounds && expected(bounds), `The rendered task duration must reach its expected position; found ${JSON.stringify(bounds)}`);
  return bounds;
}
async function startBarResize(page, bounds, edge, distance) {
  const x = edge === "due" ? bounds.right - 1 : bounds.left + 1;
  const y = (bounds.top + bounds.bottom) / 2;
  await page.mouse.move(x, y);
  await page.waitForFunction(() => document.body.style.cursor === "ew-resize", undefined, { timeout: 10000, polling: 25 });
  await page.mouse.down();
  await paint(page);
  await page.mouse.move(x + distance, y, { steps: 5 });
  await paint(page);
}
async function openFirstRow(page, touch = false) {
  const bounds = await bar(page);
  assert.ok(bounds && bounds.width > 20, "The first task's colored duration must be visible in the timeline");
  const x = (bounds.left + bounds.right) / 2, y = (bounds.top + bounds.bottom) / 2;
  if (touch) await page.touchscreen.tap(x, y);
  else await page.mouse.click(x, y, { delay: 50 });
  await paint(page);
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 50 });
}
async function assertMilestone(page, firstBar) {
  const png = await page.screenshot({ timeout: 60000, animations: "disabled", caret: "hide" });
  const marker = await page.evaluate(async ({ encoded, centerY }) => {
    const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
    const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
    const context = canvas.getContext("2d");
    context.drawImage(bitmap, 0, 0);
    const pixels = context.getImageData(0, 0, bitmap.width, bitmap.height).data;
    const points = [];
    for (let y = Math.floor(centerY - 7); y <= Math.ceil(centerY + 7); y++) for (let x = 205; x < bitmap.width - 25; x++) {
      const offset = (y * bitmap.width + x) * 4;
      if ([109, 185, 238].every((value, axis) => Math.abs(pixels[offset + axis] - value) <= 3)) points.push([x, y]);
    }
    bitmap.close();
    if (!points.length) return null;
    return { count: points.length, width: Math.max(...points.map(point => point[0])) - Math.min(...points.map(point => point[0])) + 1, height: Math.max(...points.map(point => point[1])) - Math.min(...points.map(point => point[1])) + 1 };
  }, { encoded: png.toString("base64"), centerY: (firstBar.top + firstBar.bottom) / 2 + 3 * 36 });
  assert.ok(marker && marker.count >= 8 && marker.width <= 13 && marker.height >= 6, "A task with only a due date must render as a compact colored milestone rather than an invented duration");
}
async function assertCompactRows(page, png) {
  const visible = await page.evaluate(async encoded => {
    const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
    const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
    const context = canvas.getContext("2d");
    context.drawImage(bitmap, 0, 0);
    const pixels = context.getImageData(0, 0, bitmap.width, bitmap.height).data;
    // Confine this inspection to the actual left calendar/timeline panel.
    // Locate the first duration's 10px color band, then inspect the distinct
    // task/status marks at the left of each real 36px row beneath it.
    const bands = [];
    for (let y = 150; y < 500; y++) {
      let count = 0;
      for (let x = 100; x < 330; x++) {
        const offset = (y * bitmap.width + x) * 4;
        if ([234, 133, 173].every((value, axis) => Math.abs(pixels[offset + axis] - value) <= 3)) count++;
      }
      if (count > 30) {
        let band = bands.at(-1);
        if (!band || y > band.bottom + 1) { band = { top: y, bottom: y, count: 0 }; bands.push(band); }
        band.bottom = y; band.count += count;
      }
    }
    const band = bands.filter(value => value.bottom - value.top >= 5).sort((a, b) => b.count - a.count)[0];
    if (!band) { bitmap.close(); return 0; }
    const firstMarkY = (band.top + band.bottom) / 2 - 8;
    let visible = 0;
    for (let row = 0; row < 7; row++) {
      let marked = false;
      for (let y = Math.floor(firstMarkY + row * 36 - 4); y <= Math.ceil(firstMarkY + row * 36 + 4); y++) for (let x = 17; x <= 28; x++) {
        const offset = (y * bitmap.width + x) * 4;
        const channels = [pixels[offset], pixels[offset + 1], pixels[offset + 2]];
        if (Math.max(...channels) > 55 && Math.max(...channels) - Math.min(...channels) > 10) marked = true;
      }
      if (marked) visible++;
    }
    bitmap.close();
    return visible;
  }, png.toString("base64"));
  assert.ok(visible >= 5, `The fixed dashboard timeline must expose at least five task rows without expanding or scrolling; found ${visible}`);
}
async function schedule(page, due, start, submit = true) {
  await page.keyboard.press("Control+a");
  await page.keyboard.press("Backspace");
  await page.keyboard.type(due, { delay: 15 });
  // Browser event delivery can precede WASM text consumption. Give the last
  // character its own app frame before moving focus to another field.
  await paint(page);
  await key(page, "Tab");
  await page.keyboard.press("Control+a");
  await page.keyboard.press("Backspace");
  await page.keyboard.type(start, { delay: 15 });
  await paint(page);
  if (submit) await key(page, "Enter");
}
async function exportData(page) {
  await resetFocus(page);
  await key(page, "Control+Alt+d");
  await tab(page, 6); // Fresh/reloaded journal: Data through Export JSON.
  const downloadEvent = page.waitForEvent("download", { timeout: 5000 });
  await key(page, "Space");
  const download = await downloadEvent;
  assert.equal(download.suggestedFilename(), "jig.json");
  const exported = JSON.parse(await readFile(await download.path(), "utf8"));
  await key(page, "Control+Alt+d");
  return exported;
}
async function importData(page, imported, confirm = true) {
  await page.locator("#jig-import").setInputFiles({ name: "sample-timeline.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(imported)) });
  await page.waitForTimeout(1600);
  if (confirm) {
    await resetFocus(page);
    await tab(page, 2); // Data, then the real pending-import confirmation.
    await key(page, "Space");
  }
}

try {
  progress("widen timeline titles and retain the label preference across reload");
  current = await open("timeline-label-divider");
  await timeline(current.page);
  const labelInitialBar = await waitForBar(current.page);
  const labelRaw = await raw(current.page);
  // This fixture starts on Monday, so the duration's left edge locates the
  // visible day-lane boundary. The divider hit region is on its title side.
  const labelGrip = { x: labelInitialBar.left - 4, y: labelInitialBar.top - 20 };
  await current.page.mouse.move(labelGrip.x, labelGrip.y);
  await current.page.waitForFunction(() => document.body.style.cursor === "ew-resize", undefined, { timeout: 10000, polling: 25 });
  await current.page.mouse.down();
  await paint(current.page);
  await current.page.mouse.move(labelGrip.x + 160, labelGrip.y, { steps: 6 });
  await paint(current.page);
  const labelPreview = await waitForBar(current.page, item => item.left - labelInitialBar.left >= 50);
  assert.equal(await raw(current.page), labelRaw, "Changing the title-width preview must not save before the divider is released");
  await current.page.mouse.up();
  // The saved state below acknowledges release without depending on GL timing.
  await current.page.waitForFunction(key => Number.isInteger(JSON.parse(localStorage.getItem(key)).preferences.timeline_label_percent), storageKey);
  const labelState = await state(current.page);
  assert.ok(labelState.preferences.timeline_label_percent >= 10 && labelState.preferences.timeline_label_percent <= 80, "The title divider must save a bounded label-width percentage");
  const labelUnrelated = structuredClone(labelState); delete labelUnrelated.preferences.timeline_label_percent;
  assert.deepEqual(labelUnrelated, fixture, "Widening titles must preserve all task dates, identities, colors, completion and the featured countdown");
  await capture(current.page, "timeline-label-divider");
  await boot(current.page, true);
  await timeline(current.page);
  await expectState(current.page, labelState, "The title-width preference must survive reloading saved data");
  const labelRestored = await waitForBar(current.page, item => Math.abs(item.left - labelPreview.left) <= 12);
  await current.page.mouse.dblclick(labelRestored.left - 4, labelRestored.top - 20);
  await paint(current.page);
  await expectState(current.page, fixture, "Double-clicking the title divider must omit its preference and preserve all original records");
  await waitForBar(current.page, item => Math.abs(item.left - labelInitialBar.left) <= 3);

  progress("resize each visible duration end by one day and undo the complete schedule change");
  let rangeBar = await waitForBar(current.page);
  const dayPixels = (rangeBar.width + 1) / 5; // Five inclusive days, with the renderer's half-pixel inset at each end.
  const rangeRaw = await raw(current.page);
  await startBarResize(current.page, rangeBar, "due", dayPixels);
  await waitForBar(current.page, item => item.width - rangeBar.width >= dayPixels * 0.5);
  assert.equal(await raw(current.page), rangeRaw, "Dragging the due-date end must preview without changing saved dates");
  await capture(current.page, "timeline-range-drag");
  await current.page.mouse.up();
  const resizedDue = structuredClone(fixture); resizedDue.deadlines[0].at = "2026-10-31T18:00:00Z";
  await expectState(current.page, resizedDue, "Moving the due end one day must retain its 18:00 time and the exact opposite start instant");
  await key(current.page, "Escape");
  await key(current.page, "Control+z");
  await expectState(current.page, fixture, "One Undo must restore every record after dragging the due-date end");
  await timeline(current.page);
  rangeBar = await waitForBar(current.page);
  await startBarResize(current.page, rangeBar, "start", dayPixels);
  await waitForBar(current.page, item => item.left - rangeBar.left >= dayPixels * 0.5 && rangeBar.width - item.width >= dayPixels * 0.5);
  assert.equal(await raw(current.page), rangeRaw, "Dragging the start-date end must preview without changing saved dates");
  await current.page.mouse.up();
  const resizedStart = structuredClone(fixture); resizedStart.tasks[0].start = "2026-10-27T12:34:56Z";
  await expectState(current.page, resizedStart, "Moving the start end one day must preserve its 12:34:56 time and exact due instant");
  await key(current.page, "Escape");
  await key(current.page, "Control+z");
  await expectState(current.page, fixture, "One Undo must restore every record after dragging the start-date end");
  await current.context.close();

  progress("resize the Tasks and Gantt panes, persist their width, and restore the default");
  current = await open("timeline-divider");
  await key(current.page, "Control+Alt+2");
  const initialDivider = await waitForDivider(current.page);
  const beforeResize = await raw(current.page);
  await current.page.mouse.move(initialDivider.x, initialDivider.y);
  await current.page.waitForFunction(() => document.body.style.cursor === "ew-resize", undefined, { timeout: 10000, polling: 25 });
  await current.page.mouse.down();
  await paint(current.page);
  await current.page.mouse.move(initialDivider.x + 160, initialDivider.y, { steps: 6 });
  await paint(current.page);
  assert.equal(await raw(current.page), beforeResize, "Dragging the divider must wait until release before saving its preference");
  await current.page.mouse.up();
  await current.page.waitForFunction(key => Number.isInteger(JSON.parse(localStorage.getItem(key)).preferences.task_panel_percent), storageKey);
  const resized = await state(current.page);
  const percent = resized.preferences.task_panel_percent;
  assert.ok(percent >= 20 && percent <= 80, "A released divider must save a bounded Tasks width percentage");
  const unrelated = structuredClone(resized); delete unrelated.preferences.task_panel_percent;
  assert.deepEqual(unrelated, fixture, "Resizing must preserve every task, deadline, featured countdown and unrelated preference");
  const movedDivider = await waitForDivider(current.page, item => item.x - initialDivider.x >= 50);
  await capture(current.page, "timeline-divider");
  await key(current.page, "ArrowRight");
  const wider = structuredClone(resized); wider.preferences.task_panel_percent = percent + 1;
  await expectState(current.page, wider, "Right Arrow on the focused divider must widen Tasks by one percentage point");
  await key(current.page, "ArrowLeft");
  await expectState(current.page, resized, "Left Arrow must restore the previous divider preference without editing records");
  await boot(current.page, true);
  await key(current.page, "Control+Alt+2");
  await expectState(current.page, resized, "The divider preference must survive reloading the ordinary saved workbench");
  await waitForDivider(current.page, item => Math.abs(item.x - movedDivider.x) <= 12);
  await current.page.setViewportSize({ width: 390, height: 844 });
  await key(current.page, "Escape");
  await expectState(current.page, resized, "The narrow stacked layout must not overwrite the saved wide-panel preference");
  assert.equal(await current.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true, "The stacked Tasks workspace must remain within the narrow viewport");
  await current.page.setViewportSize({ width: 1440, height: 1000 });
  await key(current.page, "Escape");
  const restoredDivider = await waitForDivider(current.page, item => Math.abs(item.x - movedDivider.x) <= 12);
  await current.page.mouse.dblclick(restoredDivider.x, restoredDivider.y);
  await paint(current.page);
  await expectState(current.page, fixture, "Double-clicking the divider must omit the preference and restore the original data exactly");
  await waitForDivider(current.page, item => Math.abs(item.x - initialDivider.x) <= 3);
  await current.context.close();

  progress("inspect overlapping durations, completion and deadline-only work without changing records");
  current = await open("timeline-desktop");
  const originalRaw = JSON.stringify(fixture);
  assert.equal(await raw(current.page), originalRaw);
  await timeline(current.page);
  const rose = await bar(current.page), amber = await bar(current.page, "amber");
  assert.ok(rose && amber, "Both same-date tasks must have their own visible colored bars");
  assert.ok(Math.abs(rose.right - amber.right) <= 3, "Two tasks due on the same day must end at the same calendar position");
  assert.ok(Math.abs(rose.width / amber.width - 5 / 3) < 0.12, "Inclusive five-day and three-day spans must retain their actual relative duration");
  await assertMilestone(current.page, rose);
  assert.equal(await raw(current.page), originalRaw, "Viewing a timeline must not migrate legacy deadlines, add dates or change the featured task");
  await capture(current.page, "timeline-desktop");
  await key(current.page, "Control+m");
  await assertCompactRows(current.page, await capture(current.page, "timeline-dashboard"));
  await key(current.page, "Control+m");

  progress("navigate week and month windows with the keyboard");
  await tab(current.page); // Month follows the shortcut's Week anchor.
  await key(current.page, "Space");
  const monthRose = await bar(current.page);
  assert.ok(monthRose && monthRose.width / rose.width > 0.18 && monthRose.width / rose.width < 0.29, "Month view must change the real time scale from seven to 31 days");
  await tab(current.page, 2); // From Month to Next.
  await key(current.page, "Space");
  assert.equal(await bar(current.page), null, "The October task must leave the visible duration grid in November");
  await tab(current.page); // Today.
  await key(current.page, "Space");
  assert.ok(await bar(current.page), "Today must return to the current month");
  for (let index = 0; index < 4; index++) await key(current.page, "Shift+Tab");
  await key(current.page, "Space"); // Week.
  await tab(current.page, 2);
  await key(current.page, "Space"); // Previous week.
  assert.equal(await bar(current.page), null, "Week navigation must move the duration window");
  await tab(current.page);
  await key(current.page, "Space"); // Next returns to the current week.
  assert.ok(await bar(current.page));
  assert.equal(await raw(current.page), originalRaw, "Window navigation must leave all saved work and the countdown unchanged");

  progress("preserve an imported nonmidnight start when only the displayed due day changes");
  await timeline(current.page);
  await tab(current.page, 5);
  await key(current.page, "Enter");
  await current.page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 50 });
  await schedule(current.page, "2026-11-02", "2026-10-26");
  const dueOnly = structuredClone(fixture);
  dueOnly.deadlines[0].at = "2026-11-02T18:00:00Z";
  await expectState(current.page, dueOnly, "An unchanged start day must preserve its exact imported 12:34:56 instant");
  await key(current.page, "Escape");
  await key(current.page, "Control+z");
  await expectState(current.page, fixture, "Undo must restore the changed due day and exact nonmidnight start together");

  progress("edit the same task from its row and undo an atomic date-range change");
  const beforeOpening = await raw(current.page);
  await timeline(current.page);
  await tab(current.page, 5); // Week, Month, Previous, Next, Today, then first row.
  await key(current.page, "Enter");
  await current.page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 50 });
  assert.equal(await raw(current.page), beforeOpening, "Opening the inline editor must not edit or star the task");
  await schedule(current.page, "2026-11-02", "2026-10-27", false);
  await capture(current.page, "timeline-edit");
  await key(current.page, "Enter");
  const edited = structuredClone(fixture);
  edited.tasks[0].start = "2026-10-27T00:00:00Z";
  edited.deadlines[0].at = "2026-11-02T18:00:00Z";
  await expectState(current.page, edited, "Date-only editing must preserve the existing due time and reuse the same records");
  await key(current.page, "Escape");
  await key(current.page, "Control+z");
  await expectState(current.page, fixture, "A single Undo must restore both schedule fields and the original featured task");

  progress("reject reversed ranges, then preserve a corrected start and the exact original deadline");
  await timeline(current.page);
  await openFirstRow(current.page);
  await schedule(current.page, "2026-10-30", "2026-11-05");
  await current.page.waitForTimeout(500);
  assert.deepEqual(await state(current.page), fixture, "A start after the due date must not partially save either field");
  // The rejected Start field stays editable; explicitly correct it once.
  await current.page.keyboard.press("Control+a");
  await current.page.keyboard.type("2026-10-28", { delay: 15 });
  await key(current.page, "Enter");
  const withStart = structuredClone(fixture);
  withStart.tasks[0].start = "2026-10-28T00:00:00Z";
  await expectState(current.page, withStart, "Changing only Start must preserve the due instant, color, completion, links and featured countdown");
  await boot(current.page, true);
  await expectState(current.page, withStart, "Start dates must survive a reload in the ordinary task records");

  progress("clear an optional start into a milestone and undo the change");
  await timeline(current.page);
  await openFirstRow(current.page);
  await schedule(current.page, "2026-10-30", "");
  const withoutStart = structuredClone(withStart);
  delete withoutStart.tasks[0].start;
  await expectState(current.page, withoutStart, "Clearing Start must retain the exact existing due instant and linked record");
  await key(current.page, "Escape");
  await key(current.page, "Control+z");
  await expectState(current.page, withStart, "Undo must restore the cleared start in one step");
  await boot(current.page, true);

  progress("synchronize completion and undo it without losing the saved start");
  await timeline(current.page);
  assert.ok(await bar(current.page), "The incomplete task must retain a colored duration before completion");
  const incompleteRender = await current.page.screenshot({ timeout: 60000, animations: "disabled", caret: "hide" });
  await tasks(current.page);
  await tab(current.page, 4); // Fresh journal: first checklist checkbox.
  await key(current.page, "Space");
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).tasks[0].complete === true, storageKey);
  await timeline(current.page);
  assert.equal(await bar(current.page), null, "A completed task must use a subdued appearance rather than its original active-color fill");
  assert.notDeepEqual(await current.page.screenshot({ timeout: 60000, animations: "disabled", caret: "hide" }), incompleteRender, "Completing a task must visibly subdue its timeline row and identify it as Done");
  assert.equal((await state(current.page)).featured, fixture.featured);
  await key(current.page, "Escape");
  await key(current.page, "Control+z");
  await expectState(current.page, withStart, "Completion Undo must restore the start, color and linked due date together");
  await boot(current.page, true);

  progress("export and import starts using the existing JSON controls");
  const exported = await exportData(current.page);
  assert.deepEqual(exported, withStart, "JSON export must include starts alongside the existing task and deadline IDs");
  const imported = structuredClone(exported);
  imported.tasks[0].start = "2026-10-29T00:00:00Z";
  imported.deadlines[0].at = "2026-11-03T18:00:00Z";
  await importData(current.page, imported);
  await expectState(current.page, imported, "Confirmed JSON import must preserve dates, colors, status and record identity");
  await boot(current.page, true);
  await expectState(current.page, imported);
  const invalid = structuredClone(imported);
  invalid.tasks[0].start = "2026-11-04T00:00:00Z";
  await importData(current.page, invalid, false);
  assert.deepEqual(await state(current.page), imported, "An invalid imported range must preserve the previously saved workbench");
  await current.context.close();

  progress("load older task files and inspect a legacy deadline without creating records");
  const old = structuredClone(fixture);
  for (const task of old.tasks) delete task.start;
  current = await open("timeline-legacy", old);
  await timeline(current.page);
  assert.equal(await raw(current.page), JSON.stringify(old), "Older records without starts must open unchanged");
  await tab(current.page, 10); // The sixth dated row is the standalone legacy deadline.
  await key(current.page, "Space");
  await current.page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 50 });
  assert.equal(await raw(current.page), JSON.stringify(old), "Merely opening a legacy deadline editor must not create a linked task");
  await tab(current.page, 4); // Start, Save, Remove date, then Cancel.
  await key(current.page, "Space");
  assert.equal(await raw(current.page), JSON.stringify(old), "Cancelling the draft must leave legacy data intact");

  progress("give an existing undated task a range without duplicating its checklist row");
  await timeline(current.page);
  await tab(current.page, 11); // The seventh row is the ordinary undated task.
  await key(current.page, "Enter");
  await current.page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 50 });
  await schedule(current.page, "2026-11-06", "2026-11-01");
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).tasks.find(task => task.id === 6).deadline !== undefined, storageKey);
  const scheduled = await state(current.page);
  assert.equal(scheduled.tasks.length, old.tasks.length, "Scheduling an existing task must retain its single checklist record");
  assert.equal(scheduled.deadlines.length, old.deadlines.length + 1, "An undated task needs exactly one linked deadline");
  const scheduledTask = scheduled.tasks.find(task => task.id === 6);
  assert.equal(scheduledTask.start, "2026-11-01T00:00:00Z");
  assert.equal(scheduled.deadlines.find(deadline => deadline.id === scheduledTask.deadline).at, "2026-11-06T09:00:00Z", "A new date-only deadline uses the ordinary local 09:00 default");
  assert.equal(scheduled.featured, old.featured);
  await key(current.page, "Escape");
  await key(current.page, "Control+z");
  await expectState(current.page, old, "Undo must remove only the newly created link and range");
  await current.context.close();

  progress("tap a real duration on a phone and preserve local dates across daylight saving time");
  const phone = structuredClone(fixture);
  for (const task of phone.tasks) if (task.start) task.start = task.start.replace("T00:00:00Z", "T04:00:00Z");
  current = await open("timeline-mobile", phone, true);
  await timeline(current.page);
  await capture(current.page, "timeline-mobile");
  await openFirstRow(current.page, true);
  assert.deepEqual(await state(current.page), phone, "Touching the timeline must open the same task without changing the countdown");
  await schedule(current.page, "2026-11-04", "2026-11-02");
  phone.tasks[0].start = "2026-11-02T05:00:00Z"; // Midnight EST, after the clock change.
  phone.deadlines[0].at = "2026-11-04T19:00:00Z"; // Preserve the original 14:00 local due time.
  await expectState(current.page, phone, "Date-only fields must convert each selected local date using its own DST offset");
  assert.equal(await current.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true, "The phone timeline must stay within the existing panel width");
  await boot(current.page, true);
  await expectState(current.page, phone);
  assert.deepEqual(issues, [], "The local timeline must emit no app errors or network requests");
  console.log("Timeline browser checks passed: persistent timeline title widths and duration-end date resizing with live previews and atomic Undo; draggable Tasks/Gantt pane widths with keyboard adjustment, reload, narrow-layout preservation and default reset; overlapping colored durations and completion, real week/month navigation, keyboard and touch row editing, date-only atomic ranges and Undo, unchanged featured countdown and linked IDs, reload, JSON import/export and invalid-range preservation, older files and nonmutating legacy inspection, and DST-correct local dates on a phone.");
} catch (error) {
  originalError = error;
  console.error(`Timeline browser failed during: ${phase}`);
  if (current?.page && !current.page.isClosed()) {
    let diagnosticTimer;
    try {
      const diagnostics = await Promise.race([
        current.page.evaluate(key => ({
          frames: window.jigTestFrames ?? null,
          savedStateRaw: localStorage.getItem(key),
          focused: document.hasFocus(),
          visibility: document.visibilityState,
          activeElement: document.activeElement ? { tag: document.activeElement.tagName, id: document.activeElement.id } : null,
        }), storageKey),
        new Promise((_, reject) => { diagnosticTimer = setTimeout(() => reject(new Error("Timeline diagnostics exceeded 3 seconds")), 3000); }),
      ]);
      console.error("Timeline public browser diagnostics:", JSON.stringify(diagnostics));
    } catch (diagnosticError) {
      console.error("Timeline diagnostics unavailable:", String(diagnosticError));
    } finally { clearTimeout(diagnosticTimer); }
    await capture(current.page, "timeline-failure").catch(() => {});
  }
  throw error;
} finally {
  progress("close timeline contexts and Chromium");
  let cleanupTimer;
  let cleanupError;
  try {
    await Promise.race([
      Promise.all([...contexts.map(context => context.close().catch(() => {})), browser.close()]),
      new Promise((_, reject) => { cleanupTimer = setTimeout(() => reject(new Error("Timeline browser shutdown exceeded 10 seconds")), 10000); }),
    ]);
  } catch (error) { cleanupError = error; }
  finally { clearTimeout(cleanupTimer); clearTimeout(timer); }
  if (cleanupError) {
    if (originalError) console.error("Original timeline failure:", originalError);
    console.error("Timeline browser cleanup failed:", cleanupError);
    process.exit(1);
  }
}

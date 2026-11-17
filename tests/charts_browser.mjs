// Real Charts workspace and cell-linked controls. Every record is synthetic.
import assert from "node:assert/strict";
import { mkdir, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { inflateRawSync } from "node:zlib";
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
  tasks: [{ id: 1, text: "Sample: protect existing work", complete: false, deadline: 2, color: "rose" }],
  deadlines: [{ id: 2, title: "Sample: protect existing work", at: "2030-01-15T18:00:00Z" }],
  entries: [{ id: 3, title: "Sample: retained entry", date: "2030-01-15" }],
  notes: [{ id: 4, text: "Sample: retained note", pinned: true }],
  featured: 2,
  workbook: { version: 1, sheets: [{ id: 1, name: "Inputs", cells: {
    A1: "Material", B1: "Amount", A2: "Clay", B2: "10", A3: "Tools", B3: "=B2*2", A4: "Kiln", B4: "30",
  }, styles: { B3: { text_color: [179, 158, 207], bold: true } } }] },
};
let phase = "launch Chromium", current;
const contexts = [], issues = [];
const timer = setTimeout(() => { console.error(`Charts browser exceeded 300 seconds during: ${phase}`); process.exit(1); }, 300000);
function progress(value) { phase = value; console.log(`Charts browser phase: ${value}`); }
async function paint(page) { await waitForAppPaint(page); }
async function key(page, value) { await page.keyboard.press(value); await paint(page); }
async function tab(page, count = 1) { for (let index = 0; index < count; index++) await key(page, "Tab"); }
async function resetFocus(page, cancel = true) {
  const { width, height } = page.viewportSize();
  await page.locator("#jig_canvas").click({ position: { x: width - 6, y: height - 6 } });
  if (cancel) await key(page, "Escape"); else await paint(page);
}
async function text(page, value) {
  await key(page, "Control+a"); await page.keyboard.press("Backspace");
  await page.keyboard.type(value, { delay: 15 }); await paint(page);
}
async function raw(page) { return page.evaluate(key => localStorage.getItem(key), storageKey); }
async function state(page) { return JSON.parse(await raw(page)); }
function sheet(value) { return value.workbook.sheets[0]; }
function tools(value) { const { workbook, ...rest } = value; return rest; }
async function expectState(page, expected, message) {
  const until = Date.now() + 5000;
  do {
    const actual = await state(page);
    try { assert.deepEqual(actual, expected); return; } catch { await page.waitForTimeout(25); }
  } while (Date.now() < until);
  assert.deepEqual(await state(page), expected, message);
}
async function waitCell(page, value) {
  await page.waitForFunction(({ key, value }) => JSON.parse(localStorage.getItem(key)).workbook.sheets[0].cells.B2 === value,
    { key: storageKey, value });
}
async function focusedText(page, expected) {
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT");
  await page.evaluate(() => navigator.clipboard.writeText("Charts draft pending"));
  await key(page, "Control+a"); await key(page, "Control+c");
  await page.waitForFunction(expected => navigator.clipboard.readText().then(value => value === expected), expected);
  await key(page, "ArrowRight");
  return page.evaluate(() => ({ x: parseFloat(document.activeElement.style.left), y: parseFloat(document.activeElement.style.top) }));
}
async function select(page, address) {
  await key(page, "Control+g"); await page.waitForFunction(() => document.activeElement?.tagName === "INPUT");
  await text(page, address); await key(page, "Enter");
  await page.waitForFunction(() => document.activeElement?.id === "jig_canvas");
}
async function enter(page, address, value, submit = true) {
  await select(page, address); await key(page, "F2");
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT");
  await text(page, value);
  if (submit) { await key(page, "Enter"); await page.waitForFunction(() => document.activeElement?.id === "jig_canvas"); }
  else await focusedText(page, value);
}
async function cellValue(page, address, expected) {
  await select(page, address);
  await page.evaluate(() => navigator.clipboard.writeText("Charts cell pending"));
  await key(page, "Control+c");
  await page.waitForFunction(expected => navigator.clipboard.readText().then(value => value === expected), String(expected));
}
async function capture(page, name) {
  const png = await page.screenshot({ path: resolve(results, `${name}.png`), timeout: 60000, animations: "disabled", caret: "hide" });
  if (new Set((process.env.JIG_RENDER_LOGS ?? "").split(",")).has(name) && png.length <= 1024 * 1024) {
    console.log(`JIG_RENDER_BEGIN_${name}`);
    const encoded = png.toString("base64");
    for (let index = 0; index < encoded.length; index += 2048) console.log(encoded.slice(index, index + 2048));
    console.log(`JIG_RENDER_END_${name}`);
  }
}
async function colored(page, color, top = 0, bottom = page.viewportSize().height) {
  const png = await page.screenshot({ timeout: 10000, animations: "disabled", caret: "hide" });
  return page.evaluate(async ({ encoded, color, top, bottom }) => {
    const bitmap = await createImageBitmap(new Blob([Uint8Array.from(atob(encoded), c => c.charCodeAt(0))], { type: "image/png" }));
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height), context = canvas.getContext("2d"); context.drawImage(bitmap, 0, 0);
    const pixels = context.getImageData(0, 0, bitmap.width, bitmap.height).data;
    const seen = new Uint8Array(bitmap.width * bitmap.height), regions = [], points = [];
    top = Math.max(0, Math.floor(top)); bottom = Math.min(bitmap.height, Math.ceil(bottom));
    const near = index => color.every((value, axis) => Math.abs(pixels[index * 4 + axis] - value) <= 3);
    for (let y = top; y < bottom; y++) for (let x = 0; x < bitmap.width; x++) {
      const start = y * bitmap.width + x;
      if (near(start)) points.push(start);
      if (seen[start] || !near(start)) continue;
      const stack = [start], region = { left: x, right: x, top: y, bottom: y, count: 0 }; seen[start] = 1;
      while (stack.length) {
        const index = stack.pop(), px = index % bitmap.width, py = Math.floor(index / bitmap.width);
        region.left = Math.min(region.left, px); region.right = Math.max(region.right, px);
        region.top = Math.min(region.top, py); region.bottom = Math.max(region.bottom, py); region.count++;
        for (const [nx, ny] of [[px - 1, py], [px + 1, py], [px, py - 1], [px, py + 1]]) {
          if (nx < 0 || nx >= bitmap.width || ny < top || ny >= bottom) continue;
          const next = ny * bitmap.width + nx;
          if (!seen[next] && near(next)) { seen[next] = 1; stack.push(next); }
        }
      }
      if (region.count > 20) regions.push({ ...region, x: (region.left + region.right) / 2, y: (region.top + region.bottom) / 2,
        width: region.right - region.left + 1, height: region.bottom - region.top + 1 });
    }
    bitmap.close(); return { regions, count: points.length, signature: points.join(",") };
  }, { encoded: png.toString("base64"), color, top, bottom });
}
async function workspacePoint(page) {
  const { regions } = await colored(page, [67, 42, 34], 0, 66);
  const point = regions.find(r => r.width >= 40 && r.width <= 85 && r.height >= 18 && r.height <= 32);
  assert.ok(point, "The selected Sheets or Charts header tab must be visibly identifiable"); return point;
}
async function waitWorkspace(page, workspace) {
  await page.waitForFunction(workspace => localStorage.getItem("jig.workspace.v1") === workspace, workspace);
  // The action can run after this pass has already drawn the old header.
  // Observe the following actual app paints before locating its selected tab.
  const after = await page.evaluate(() => window.jigTestFrames.paints);
  await page.waitForFunction(after => window.jigTestFrames.paints >= after + 2
    && window.jigTestFrames.painted >= window.jigTestFrames.input, after);
}
async function clickWorkspace(page, point, workspace, touch = false) {
  if (touch) await page.touchscreen.tap(point.x, point.y); else await page.mouse.click(point.x, point.y);
  await paint(page);
  await waitWorkspace(page, workspace);
}
async function backToCells(page) {
  await resetFocus(page); await tab(page, 9); await key(page, "Enter");
  await waitWorkspace(page, "sheet");
}
async function menuItem(page, index) {
  await resetFocus(page); await tab(page, 10); await key(page, "Enter");
  await tab(page, index); await key(page, "Enter");
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT");
}
async function createChart(page, index, title) {
  const count = (await state(page)).workbook.charts?.length ?? 0;
  await menuItem(page, index); await text(page, title); await tab(page, 2); await text(page, "A1:B4"); await key(page, "Enter");
  await page.waitForFunction(({ key, count }) => JSON.parse(localStorage.getItem(key)).workbook.charts.length === count + 1, { key: storageKey, count });
}
async function createControl(page, index, label, cell) {
  const count = (await state(page)).workbook.controls?.length ?? 0;
  await menuItem(page, index); await text(page, label); await tab(page, 3); await text(page, cell); await key(page, "Enter");
  await page.waitForFunction(({ key, count }) => JSON.parse(localStorage.getItem(key)).workbook.controls.length === count + 1, { key: storageKey, count });
}
async function focusNumber(page, expected, preserveDraft = false) {
  await resetFocus(page, !preserveDraft); await tab(page, 12);
  // An overflowing gallery contributes its own focus stop before the cards.
  if (await page.evaluate(() => document.activeElement?.tagName !== "INPUT")) await tab(page);
  return focusedText(page, expected);
}
async function sliderThumb(page, field) {
  const until = Date.now() + 5000; let thumb;
  do {
    const { regions } = await colored(page, [77, 49, 39], field.y + 10, field.y + 85);
    thumb = regions.find(r => r.width >= 10 && r.width <= 24 && r.height >= 10 && r.height <= 24 && r.count > 45);
    if (thumb) return thumb;
  } while (Date.now() < until);
  assert.ok(thumb, "The focused Slider track must expose its actual visible thumb"); return thumb;
}
async function dragSlider(scene, thumb, touch = false) {
  const before = await state(scene.page);
  const distance = 40;
  let session;
  if (touch) {
    session = await scene.context.newCDPSession(scene.page);
    await session.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: thumb.x, y: thumb.y }] });
  } else { await scene.page.mouse.move(thumb.x, thumb.y); await scene.page.mouse.down(); }
  await paint(scene.page);
  for (let step = 1; step <= 4; step++) {
    if (touch) await session.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: thumb.x + distance * step / 4, y: thumb.y }] });
    else await scene.page.mouse.move(thumb.x + distance * step / 4, thumb.y);
    await paint(scene.page);
  }
  await expectState(scene.page, before, "Slider motion must remain a preview until pointer release");
  if (touch) { await session.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] }); await session.detach(); }
  else await scene.page.mouse.up();
  await paint(scene.page);
  await scene.page.waitForFunction(({ key, before }) => JSON.parse(localStorage.getItem(key)).workbook.sheets[0].cells.B2 !== before,
    { key: storageKey, before: sheet(before).cells.B2 });
  const after = await state(scene.page), value = Number(sheet(after).cells.B2);
  assert.ok(value > Number(sheet(before).cells.B2) && value <= 100 && Number.isInteger(value));
  const expected = structuredClone(before); sheet(expected).cells.B2 = String(value);
  await expectState(scene.page, expected, "Slider release must change only its bound literal cell");
  await key(scene.page, "Control+z");
  await expectState(scene.page, before, "One Undo must restore the entire multi-move slider gesture");
}
async function download(page, shortcut, extension) {
  const event = page.waitForEvent("download", { timeout: 15000 }); await key(page, shortcut);
  const item = await event; assert.ok(item.suggestedFilename().endsWith(extension)); return readFile(await item.path());
}
function zipEntries(bytes) {
  const end = bytes.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06])); assert.ok(end >= 0);
  const files = new Map(), count = bytes.readUInt16LE(end + 10); let offset = bytes.readUInt32LE(end + 16);
  for (let index = 0; index < count; index++) {
    assert.equal(bytes.readUInt32LE(offset), 0x02014b50);
    const method = bytes.readUInt16LE(offset + 10), size = bytes.readUInt32LE(offset + 20), local = bytes.readUInt32LE(offset + 42);
    const nameLength = bytes.readUInt16LE(offset + 28), extraLength = bytes.readUInt16LE(offset + 30), commentLength = bytes.readUInt16LE(offset + 32);
    const name = bytes.subarray(offset + 46, offset + 46 + nameLength).toString("utf8");
    const start = local + 30 + bytes.readUInt16LE(local + 26) + bytes.readUInt16LE(local + 28), data = bytes.subarray(start, start + size);
    assert.ok(method === 0 || method === 8); files.set(name, method === 8 ? inflateRawSync(data) : Buffer.from(data));
    offset += 46 + nameLength + extraLength + commentLength;
  }
  return files;
}
async function confirmImport(page) { await resetFocus(page); await tab(page, 6); await key(page, "Enter"); }
async function boot(page, reload = false) {
  if (reload) await page.reload({ waitUntil: "load" }); else await page.goto(base, { waitUntil: "load" });
  await page.bringToFront();
  await page.waitForFunction(() => window.jigReady === true || typeof window.jigBootError === "string", undefined, { timeout: 60000 });
  assert.equal(await page.evaluate(() => window.jigBootError), undefined);
  await paint(page);
  await waitWorkspace(page, await page.evaluate(() => localStorage.getItem("jig.workspace.v1")));
}
const browser = await chromium.launch({ headless: true, args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"] });
async function open(label, saved, phone = false, workspace = "sheet") {
  const context = await browser.newContext({ viewport: phone ? { width: 390, height: 844 } : { width: 1440, height: 1000 }, hasTouch: phone, acceptDownloads: true, timezoneId: "Etc/UTC" });
  contexts.push(context); await installFrameFence(context);
  await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin });
  await context.addInitScript(({ raw, workspace, origin, key }) => {
    if (location.origin === origin && sessionStorage.getItem("jig-charts-seeded") !== "yes") {
      localStorage.setItem(key, raw); localStorage.setItem("jig.workspace.v1", workspace); sessionStorage.setItem("jig-charts-seeded", "yes");
    }
  }, { raw: JSON.stringify(saved), workspace, origin, key: storageKey });
  await context.route("**/*", async route => {
    const url = new URL(route.request().url());
    if (["blob:", "data:", "about:"].includes(url.protocol) || url.origin === origin) await route.continue();
    else { issues.push(`${label}: unexpected request ${url.origin}${url.pathname}`); await route.abort(); }
  });
  const page = await context.newPage(); page.setDefaultTimeout(10000); page.setDefaultNavigationTimeout(60000);
  page.on("pageerror", error => issues.push(`${label}: ${error.message}`));
  await boot(page); return { context, page };
}

try {
  progress("send the selected sheet range to Charts without copying its cells");
  current = await open("charts-desktop", fixture);
  await select(current.page, "A1:B4"); await key(current.page, "Control+g");
  await key(current.page, "Shift+Tab"); await key(current.page, "Shift+Tab"); await key(current.page, "Enter");
  await tab(current.page, 10); await key(current.page, "Enter"); // Insert → Send selection to Charts.
  await waitWorkspace(current.page, "charts");
  const chartsHeader = await workspacePoint(current.page);
  await text(current.page, "Live bars"); await tab(current.page, 2); await focusedText(current.page, "A1:B4"); await key(current.page, "Enter");
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).workbook.charts?.length === 1, storageKey);
  assert.deepEqual(sheet(await state(current.page)).cells, sheet(fixture).cells, "Sending and chart creation must reference the original cells");

  progress("use the Charts header with a pending sheet edit and create Pie and Line directly");
  await backToCells(current.page); const sheetsHeader = await workspacePoint(current.page);
  await enter(current.page, "B2", "11", false);
  await clickWorkspace(current.page, chartsHeader, "charts"); await waitCell(current.page, "11");
  await createChart(current.page, 1, "Material split");
  await createChart(current.page, 2, "Input sensitivity");
  const created = await state(current.page);
  assert.deepEqual(created.workbook.charts.map(chart => chart.kind), ["bar", "pie", "line"]);
  assert.deepEqual(created.workbook.charts.map(chart => chart.title), ["Live bars", "Material split", "Input sensitivity"]);
  assert.equal(new Set(created.workbook.charts.map(chart => chart.id)).size, 3);
  const source = { type: "range", sheet: 1, range: { start: { row: 0, col: 0 }, end: { row: 3, col: 1 } } };
  for (const chart of created.workbook.charts) assert.deepEqual(chart.source, source);
  assert.deepEqual(tools(created), tools(fixture)); await capture(current.page, "charts-desktop");
  await current.page.mouse.move(700, 600); await current.page.mouse.wheel(0, 10000); await paint(current.page);
  const firstLine = await colored(current.page, [109, 185, 238], 650, 950);
  assert.ok(firstLine.count > 150, "The gallery must draw the actual Line series, not an empty card");

  progress("create Number, Slider and Result cards linked to the same workbook");
  await createControl(current.page, 5, "Input amount", "B2");
  await createControl(current.page, 6, "Amount slider", "B2");
  await createControl(current.page, 7, "Doubled result", "B3");
  const linked = await state(current.page);
  assert.equal(linked.workbook.controls.length, 3);
  assert.deepEqual(linked.workbook.controls.map(control => control.kind), ["number", { slider: { min: 0, max: 100, step: 1 } }, "result"]);
  assert.deepEqual(linked.workbook.controls.map(control => control.target), [
    { sheet: 1, cell: { row: 1, col: 1 } }, { sheet: 1, cell: { row: 1, col: 1 } }, { sheet: 1, cell: { row: 2, col: 1 } },
  ]);
  const numberPoint = await focusNumber(current.page, "11");
  const resultBefore = await colored(current.page, [179, 158, 207], 100, 650);
  assert.ok(resultBefore.count > 20, "Result must use its linked cell's visible number and text style");
  await text(current.page, "15"); await key(current.page, "Enter"); await waitCell(current.page, "15");
  await tab(current.page, 2); const sliderPoint = await focusedText(current.page, "15");
  assert.ok(Math.abs(sliderPoint.y - numberPoint.y) < 10, "Desktop controls must share a compact two-column row");
  const resultAfter = await colored(current.page, [179, 158, 207], 100, 650);
  assert.notEqual(resultAfter.signature, resultBefore.signature, "Changing the Number input must visibly recalculate the linked Result card");
  await tab(current.page, 2); const thumb = await sliderThumb(current.page, sliderPoint);
  await capture(current.page, "charts-controls"); await dragSlider(current, thumb);
  await current.page.mouse.move(700, 600); await current.page.mouse.wheel(0, 10000); await paint(current.page);
  const changedLine = await colored(current.page, [109, 185, 238], 650, 950);
  assert.notEqual(changedLine.signature, firstLine.signature, "Cell-linked inputs must redraw an existing Line chart from evaluated formulas");

  progress("preserve unfinished numeric drafts across header navigation and protect formula targets");
  await focusNumber(current.page, "15"); await text(current.page, "19"); await focusedText(current.page, "19");
  const beforeDraft = await state(current.page);
  await clickWorkspace(current.page, sheetsHeader, "sheet"); await key(current.page, "Control+z");
  await expectState(current.page, beforeDraft, "Application Undo must preserve an unfinished control draft and the committed workbook");
  await clickWorkspace(current.page, chartsHeader, "charts");
  await focusNumber(current.page, "19", true); await expectState(current.page, beforeDraft, "Header navigation must retain a numeric draft without silently applying it");
  await tab(current.page, 2); await key(current.page, "Enter"); // Reset the unfinished Number value.
  await backToCells(current.page); await cellValue(current.page, "B3", "30");
  await enter(current.page, "B2", "=B4/2"); await clickWorkspace(current.page, chartsHeader, "charts");
  const protectedState = await state(current.page);
  assert.ok((await colored(current.page, [237, 142, 132], 100, 500)).count > 20,
    "Formula-bound input cards must visibly explain that their source is read-only");
  await current.page.mouse.move(thumb.x, thumb.y); await current.page.mouse.down();
  await current.page.mouse.move(thumb.x + 40, thumb.y, { steps: 4 }); await current.page.mouse.up(); await paint(current.page);
  await expectState(current.page, protectedState, "A Number or Slider target that became a formula must remain read-only");
  assert.equal(sheet(await state(current.page)).cells.B2, "=B4/2");
  assert.equal(sheet(await state(current.page)).cells.B3, "=B2*2");
  await backToCells(current.page); await enter(current.page, "B2", "15"); await clickWorkspace(current.page, chartsHeader, "charts");

  progress("persist Charts, formulas and controls through reload, JSON and real ODS files");
  const portable = await state(current.page); await boot(current.page, true);
  assert.equal(await current.page.evaluate(() => localStorage.getItem("jig.workspace.v1")), "charts");
  await expectState(current.page, portable, "Reload must retain chart and control identities and the same underlying cells");
  await resetFocus(current.page); await key(current.page, "Control+Alt+d"); await tab(current.page, 9);
  const jsonEvent = current.page.waitForEvent("download", { timeout: 15000 }); await key(current.page, "Enter");
  const json = JSON.parse(await readFile(await (await jsonEvent).path(), "utf8"));
  assert.deepEqual(json, portable); await key(current.page, "Control+Alt+d");
  const ods = await download(current.page, "Control+s", ".ods"), files = zipEntries(ods);
  assert.deepEqual(JSON.parse(files.get("jig/state.json").toString("utf8")).state, portable);
  const pictures = [...files].filter(([name]) => /^Pictures\/chart-.*\.svg$/.test(name));
  assert.equal(pictures.length, 3);
  const lineSvg = pictures.find(([, bytes]) => bytes.toString("utf8").includes("Input sensitivity"))?.[1].toString("utf8");
  assert.ok(lineSvg); assert.equal((lineSvg.match(/<line [^>]*stroke="#6db9ee" stroke-width="2"\/>/g) ?? []).length, 2,
    "Portable Line data must contain two open data segments for three evaluated rows");
  await focusNumber(current.page, "15"); await text(current.page, "17"); await key(current.page, "Enter"); await waitCell(current.page, "17");
  const odsChooser = current.page.waitForEvent("filechooser"); await key(current.page, "Control+o");
  await (await odsChooser).setFiles({ name: "Sample-charts.ods", mimeType: "application/vnd.oasis.opendocument.spreadsheet", buffer: ods });
  await current.page.waitForTimeout(1200); await confirmImport(current.page);
  await expectState(current.page, portable, "ODS review must restore all chart/control definitions and their formula cells");
  await focusNumber(current.page, "15"); await text(current.page, "18"); await key(current.page, "Enter"); await waitCell(current.page, "18");
  await current.page.locator("#jig-import").setInputFiles({ name: "Sample-charts.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(json)) });
  await current.page.waitForTimeout(1200); await confirmImport(current.page); await expectState(current.page, portable);
  await current.context.close();

  progress("operate the same linked controls and workspace header by touch on a phone");
  current = await open("charts-mobile", portable, true, "charts");
  const phoneCharts = await workspacePoint(current.page);
  const first = await focusNumber(current.page, "15"); await tab(current.page, 2);
  const second = await focusedText(current.page, "15");
  assert.ok(second.y > first.y + 40, "Phone controls must stack in one readable column");
  await tab(current.page, 2); const phoneThumb = await sliderThumb(current.page, second);
  await capture(current.page, "charts-mobile"); await dragSlider(current, phoneThumb, true);
  await backToCells(current.page); const phoneSheets = await workspacePoint(current.page);
  await clickWorkspace(current.page, phoneCharts, "charts", true); await clickWorkspace(current.page, phoneSheets, "sheet", true);
  await clickWorkspace(current.page, phoneCharts, "charts", true);
  assert.equal(await current.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true);
  await boot(current.page, true); await expectState(current.page, portable);
  assert.equal(await current.page.evaluate(() => localStorage.getItem("jig.workspace.v1")), "charts");
  assert.deepEqual(tools(await state(current.page)), tools(fixture)); assert.deepEqual(issues, []);
  console.log("Charts browser checks passed: Charts header and reload, selection-linked Bar/Pie/Line creation, live formula-driven Line and styled Result rendering, shared Number/Slider cells, release-only slider gestures with one Undo, retained numeric drafts, formula-target protection, JSON/ODS definitions and open Line segments, and phone touch/layout without changing existing tasks or notes.");
} catch (error) {
  console.error(`Charts browser failed during: ${phase}`, error);
  if (current?.page && !current.page.isClosed()) {
    console.error("Charts public browser diagnostics:", await current.page.evaluate(key => ({ saved: localStorage.getItem(key), workspace: localStorage.getItem("jig.workspace.v1"), active: document.activeElement?.tagName, frames: window.jigTestFrames }), storageKey).catch(String));
    await capture(current.page, "charts-failure").catch(() => {});
  }
  throw error;
} finally {
  let cleanupTimer;
  try {
    await Promise.race([
      Promise.all([...contexts.map(context => context.close().catch(() => {})), browser.close()]),
      new Promise((_, reject) => { cleanupTimer = setTimeout(() => reject(new Error("Charts browser shutdown exceeded 10 seconds")), 10000); }),
    ]);
  } finally { clearTimeout(cleanupTimer); clearTimeout(timer); }
}

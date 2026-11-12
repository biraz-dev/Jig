// Real browser checks for Jig's Rust spreadsheet. All records are synthetic.
// Serve the built preview at /Jig/: node tests/spreadsheet_browser.mjs
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
  tasks: [{ id: 1, text: "Sample: photograph sculpture", complete: false, deadline: 101, color: "rose", start: "2026-11-09T00:00:00Z" }],
  deadlines: [{ id: 101, title: "Sample: photograph sculpture", at: "2026-11-15T18:00:00Z" }],
  entries: [{ id: 301, title: "Sample: hidden studio note", date: "2026-11-11" }],
  featured: 101,
};
const fixtureRaw = JSON.stringify(fixture);
let phase = "launch Chromium";
let interaction = "";
let current;
let originalError;
const contexts = [];
const issues = [];
const timer = setTimeout(() => {
  console.error(`Spreadsheet browser checks exceeded 900 seconds during: ${phase}; ${interaction}`);
  if (originalError) console.error("Original spreadsheet failure:", originalError);
  process.exit(1);
}, 900000);
function progress(message) { phase = message; interaction = ""; console.log(`Spreadsheet browser phase: ${message}`); }
async function paint(page) {
  await waitForAppPaint(page);
}
async function key(page, value) { await page.keyboard.press(value); await paint(page); }
async function tab(page, count = 1) { for (let index = 0; index < count; index++) await key(page, "Tab"); }
async function text(page, value) {
  await key(page, "Control+a");
  await page.keyboard.press("Backspace");
  await page.keyboard.type(value, { delay: 20 });
  // Text must reach the WASM editor before focus or submission changes.
  await paint(page);
}
async function confirmDraft(page, expected) {
  // A DOM animation frame need not include an egui update on a busy software
  // renderer. Observe the editor's actual draft before clicking another field;
  // coalescing typing and that click can deliver both to the new field.
  await page.evaluate(() => navigator.clipboard.writeText("Jig draft pending"));
  await key(page, "Control+a");
  await key(page, "Control+c");
  await page.waitForFunction(async expected => (await navigator.clipboard.readText()) === expected,
    expected, { timeout: 10000, polling: 50 });
  await key(page, "ArrowRight");
}
async function capture(page, name, timeout = 60000) {
  const png = await page.screenshot({ path: resolve(results, `${name}.png`), timeout, animations: "disabled", caret: "hide" });
  const allowed = new Set(["sheet-desktop", "sheet-mobile", "sheet-edit", "sheet-chart", "sheet-formats", "sheet-import", "sheet-failure"]);
  const requested = new Set((process.env.JIG_RENDER_LOGS ?? "").split(","));
  if (allowed.has(name) && requested.has(name) && png.length <= 1024 * 1024) {
    console.log(`JIG_RENDER_BEGIN_${name}`);
    const encoded = png.toString("base64");
    for (let offset = 0; offset < encoded.length; offset += 2048) console.log(encoded.slice(offset, offset + 2048));
    console.log(`JIG_RENDER_END_${name}`);
  }
  return png;
}

const browser = await chromium.launch({
  headless: true,
  executablePath: process.env.JIG_CHROMIUM_EXECUTABLE,
  args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"],
});
async function open(label, saved = fixture, phone = false) {
  const context = await browser.newContext({ viewport: phone ? { width: 390, height: 844 } : { width: 1440, height: 1000 }, acceptDownloads: true, hasTouch: phone, timezoneId: "Etc/UTC" });
  contexts.push(context);
  await installFrameFence(context);
  await context.addInitScript(() => {
    window.jigTestKeys = [];
    for (const type of ["keydown", "keyup", "focusin", "focusout", "pointerdown", "pointerup"]) {
      document.addEventListener(type, event => {
        const entry = { type, key: event.key, tag: event.target?.tagName, id: event.target?.id };
        if (type.startsWith("pointer")) Object.assign(entry, { x: event.clientX, y: event.clientY, time: performance.now() });
        queueMicrotask(() => { entry.prevented = event.defaultPrevented; });
        window.jigTestKeys.push(entry);
        if (window.jigTestKeys.length > 24) window.jigTestKeys.shift();
      }, true);
    }
  });
  await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin });
  if (saved !== null) await context.addInitScript(({ raw, storageKey, origin }) => {
    if (location.origin === origin && sessionStorage.getItem("jig-sheet-seeded") !== "yes") {
      localStorage.setItem(storageKey, raw);
      sessionStorage.setItem("jig-sheet-seeded", "yes");
    }
  }, { raw: JSON.stringify(saved), storageKey, origin });
  await context.route("**/*", async route => {
    const url = new URL(route.request().url());
    if (["blob:", "data:", "about:"].includes(url.protocol) || url.origin === origin) await route.continue();
    else { issues.push(`${label}: unexpected remote request ${url.origin}${url.pathname}`); await route.abort(); }
  });
  const page = await context.newPage();
  page.on("pageerror", error => { issues.push(`${label}: ${error.message}`); console.error("Sheet page error:", error.message); });
  page.on("console", message => { if (message.type() === "error") console.error("Sheet browser console:", message.text().slice(0, 3000)); });
  page.setDefaultTimeout(15000);
  await page.clock.setFixedTime(new Date("2026-11-11T12:00:00Z"));
  await boot(page);
  return { context, page };
}
async function boot(page, reload = false) {
  if (reload) await page.reload({ waitUntil: "load" });
  else await page.goto(base, { waitUntil: "load" });
  await page.bringToFront();
  await page.waitForFunction(() => window.jigReady === true || typeof window.jigBootError === "string", undefined, { timeout: 60000 });
  assert.equal(await page.evaluate(() => window.jigBootError), undefined, "The Rust spreadsheet must start successfully");
  assert.equal(await page.locator("#jig_canvas").isVisible(), true);
  await page.waitForTimeout(500);
  await page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
  await paint(page);
}
async function raw(page) { return page.evaluate(key => localStorage.getItem(key), storageKey); }
async function state(page) { const value = await raw(page); return value === null ? null : JSON.parse(value); }
async function expectState(page, expected, message) {
  const until = Date.now() + 5000;
  while (Date.now() < until) {
    const value = await state(page);
    try { assert.deepEqual(value, expected); return; } catch { await page.waitForTimeout(50); }
  }
  assert.deepEqual(await state(page), expected, message);
}
function toolsOnly(value) { const { workbook, ...tools } = value; return tools; }
function sheet(value, id = 1) { return value.workbook.sheets.find(item => item.id === id); }
async function nameFocus(page) {
  await key(page, "Control+g");
  // Native focus can change between software-rendered animation frames.
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 50 });
}
async function select(page, address) {
  interaction = `select ${address}`;
  await nameFocus(page);
  await text(page, address);
  await key(page, "Enter");
  // The same hidden input serves every editor. Wait for the address editor to
  // close before F2 or later input can be delivered to the previously focused field.
  await page.waitForFunction(() => document.activeElement?.id === "jig_canvas", undefined, { timeout: 10000, polling: 50 });
}
async function enter(page, address, value, submit = true) {
  await select(page, address);
  interaction = `edit ${address}: ${JSON.stringify(value)}`;
  await key(page, "F2");
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 50 });
  await text(page, value);
  if (submit) {
    await key(page, "Enter");
    await page.waitForFunction(() => document.activeElement?.id === "jig_canvas", undefined, { timeout: 10000, polling: 50 });
  } else {
    await confirmDraft(page, value);
  }
}
async function copied(page, address) {
  if (address !== undefined) await select(page, address);
  await page.evaluate(() => navigator.clipboard.writeText("Jig spreadsheet clipboard pending"));
  await key(page, "Control+c");
  const until = Date.now() + 3000;
  while (Date.now() < until) {
    const value = await page.evaluate(() => navigator.clipboard.readText());
    if (value !== "Jig spreadsheet clipboard pending") return value;
    await page.waitForTimeout(50);
  }
  assert.fail("The selected spreadsheet cell/range must copy its evaluated value through the real clipboard");
}
async function value(page, address, expected) {
  const encoded = await copied(page, address);
  // A single TSV field containing quotes or newlines is itself CSV-quoted.
  const actual = encoded.startsWith('"') && encoded.endsWith('"')
    ? encoded.slice(1, -1).replace(/""/g, '"') : encoded;
  if (typeof expected === "number") assert.ok(Math.abs(Number(actual) - expected) <= 1e-10, `${address}: ${JSON.stringify(actual)} must equal ${expected}`);
  else assert.equal(actual, expected, `Evaluated value in ${address}`);
}
async function paste(page, address, source) {
  await select(page, address);
  await page.evaluate(value => navigator.clipboard.writeText(value), source);
  await key(page, "Control+v");
}
async function toolbar(page, index) { interaction = `toolbar control ${index}`; await nameFocus(page); await tab(page, index); await key(page, "Space"); }
// Tab order from the address field. Keep these names aligned with the visible
// toolbar, so a new control cannot silently turn a style test into another action.
const controls = { font: 5, bold: 7, strike: 10, left: 11, center: 12, right: 13, borders: 16, number: 17 };
async function undo(page) { await key(page, "Escape"); await key(page, "Control+z"); }
async function exportCsv(page) {
  await nameFocus(page);
  await tab(page, 4);
  const event = page.waitForEvent("download", { timeout: 5000 });
  await key(page, "Space");
  const download = await event;
  assert.match(download.suggestedFilename(), /\.csv$/i);
  return readFile(await download.path(), "utf8");
}
async function exportJson(page) {
  await boot(page, true); // A fresh journal gives a predictable Data toolbar.
  await key(page, "Control+Alt+d");
  await tab(page, 6);
  const event = page.waitForEvent("download", { timeout: 5000 });
  await key(page, "Space");
  const download = await event;
  assert.equal(download.suggestedFilename(), "jig.json");
  const exported = JSON.parse(await readFile(await download.path(), "utf8"));
  await key(page, "Control+Alt+d");
  return exported;
}
async function importJson(page, imported, confirm = true) {
  await page.locator("#jig-import").setInputFiles({ name: "sample-workbook.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(imported)) });
  await page.waitForTimeout(1600);
  if (confirm) {
    await page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
    await key(page, "Escape");
    await tab(page, 2);
    await key(page, "Space");
  }
}
async function shortcutDownload(page, shortcut, extension) {
  const event = page.waitForEvent("download", { timeout: 15000 });
  await key(page, shortcut);
  const download = await event;
  assert.ok(download.suggestedFilename().endsWith(extension));
  return readFile(await download.path());
}
function zipEntries(bytes) {
  const end = bytes.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]));
  assert.ok(end >= 0, "An ODS file must contain a ZIP central directory");
  const count = bytes.readUInt16LE(end + 10), files = new Map();
  let offset = bytes.readUInt32LE(end + 16);
  for (let index = 0; index < count; index++) {
    assert.equal(bytes.readUInt32LE(offset), 0x02014b50);
    const method = bytes.readUInt16LE(offset + 10), size = bytes.readUInt32LE(offset + 20);
    const nameLength = bytes.readUInt16LE(offset + 28), extraLength = bytes.readUInt16LE(offset + 30), commentLength = bytes.readUInt16LE(offset + 32);
    const name = bytes.subarray(offset + 46, offset + 46 + nameLength).toString("utf8");
    const local = bytes.readUInt32LE(offset + 42);
    assert.equal(bytes.readUInt32LE(local), 0x04034b50);
    const start = local + 30 + bytes.readUInt16LE(local + 26) + bytes.readUInt16LE(local + 28);
    const data = bytes.subarray(start, start + size);
    assert.ok(method === 0 || method === 8, "The exported ODS must use standard stored/deflated ZIP entries");
    files.set(name, method === 8 ? inflateRawSync(data) : Buffer.from(data));
    offset += 46 + nameLength + extraLength + commentLength;
  }
  return files;
}
function odsWithOnlyStandardWorkbook(bytes) {
  // Make the private backup unavailable without changing any compressed data
  // or CRC. The real file importer must reconstruct styles from ODF XML.
  const result = Buffer.from(bytes);
  const end = result.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]));
  let offset = result.readUInt32LE(end + 16), renamed = false;
  for (let index = 0; index < result.readUInt16LE(end + 10); index++) {
    const nameLength = result.readUInt16LE(offset + 28), extraLength = result.readUInt16LE(offset + 30), commentLength = result.readUInt16LE(offset + 32);
    const name = result.subarray(offset + 46, offset + 46 + nameLength).toString("utf8");
    if (name === "jig/state.json") {
      const local = result.readUInt32LE(offset + 42);
      result.write("jig/other.json", local + 30, "utf8");
      result.write("jig/other.json", offset + 46, "utf8");
      renamed = true;
    }
    offset += 46 + nameLength + extraLength + commentLength;
  }
  assert.ok(renamed, "The standard ODS import fixture must exclude the private state backup");
  assert.equal(zipEntries(result).has("jig/state.json"), false);
  return result;
}
async function importOds(page, bytes) {
  const event = page.waitForEvent("filechooser", { timeout: 5000 });
  await key(page, "Control+o");
  const chooser = await event;
  await chooser.setFiles({ name: "sample-roundtrip.ods", mimeType: "application/vnd.oasis.opendocument.spreadsheet", buffer: bytes });
  await page.waitForTimeout(1600);
  await paint(page);
}
async function topMenu(page, backwards) {
  await nameFocus(page);
  for (let index = 0; index < backwards; index++) await key(page, "Shift+Tab");
  await key(page, "Space");
}
function sampleStatementPdf() {
  // A real, self-contained text PDF: no remote fixture, private statement or parser hook.
  const stream = "BT /F1 12 Tf 40 760 Td (11/10/2026  Sample clay purchase  25.40) Tj 0 -25 Td (11/11/2026  Sample tools  -12.50) Tj ET";
  const objects = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    `<< /Length ${Buffer.byteLength(stream)} >>\nstream\n${stream}\nendstream`,
  ];
  let source = "%PDF-1.4\n";
  const offsets = [0];
  for (let index = 0; index < objects.length; index++) {
    offsets.push(Buffer.byteLength(source));
    source += `${index + 1} 0 obj\n${objects[index]}\nendobj\n`;
  }
  const directory = Buffer.byteLength(source);
  source += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  source += offsets.slice(1).map(offset => `${String(offset).padStart(10, "0")} 00000 n \n`).join("");
  source += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${directory}\n%%EOF\n`;
  return Buffer.from(source);
}
async function importStatementPdf(page) {
  await topMenu(page, 5); // File.
  await tab(page, 3); // Open ODS, Save ODS, then Import PDF or image.
  const event = page.waitForEvent("filechooser", { timeout: 5000 });
  await key(page, "Space");
  const chooser = await event;
  await chooser.setFiles({ name: "Sample-transactions.pdf", mimeType: "application/pdf", buffer: sampleStatementPdf() });
  await page.waitForTimeout(2500);
  await paint(page);
}
async function reviewControl(page, index) {
  await page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
  await key(page, "Escape");
  await tab(page, index);
  await key(page, "Space");
}
async function barChart(page) {
  await topMenu(page, 2); // Insert, immediately before View and the address field.
  await tab(page, 6); // Rows, columns, delete rows/columns, Pie, then Bar.
  await key(page, "Space");
}
async function showCharts(page) {
  await topMenu(page, 1); // View.
  await tab(page, 2); // Tasks, then Charts.
  await key(page, "Space");
}
async function backToCells(page) {
  // Chart gallery controls follow Data/Undo and the footer Source link.
  await page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
  await key(page, "Escape");
  await tab(page, 6); // Data, Undo, Source, Save ODS, Export PDF, Back to cells.
  await key(page, "Space");
}
async function renderedBarWidths(page) {
  const png = await page.screenshot({ timeout: 60000, animations: "disabled", caret: "hide" });
  return page.evaluate(async encoded => {
    const picture = new Image();
    picture.src = `data:image/png;base64,${encoded}`;
    await picture.decode();
    const canvas = document.createElement("canvas");
    canvas.width = picture.width; canvas.height = picture.height;
    const context = canvas.getContext("2d");
    context.drawImage(picture, 0, 0);
    const { data } = context.getImageData(0, 0, canvas.width, canvas.height);
    const groups = [];
    let group;
    for (let y = 0; y < Math.min(canvas.height, 650); y++) {
      let count = 0;
      for (let x = 0; x < canvas.width; x++) {
        const offset = (y * canvas.width + x) * 4;
        if (Math.abs(data[offset] - 109) <= 1 && Math.abs(data[offset + 1] - 185) <= 1 && Math.abs(data[offset + 2] - 238) <= 1) count++;
      }
      if (count >= 80) {
        if (!group) { group = { start: y, end: y, width: count }; groups.push(group); }
        else { group.end = y; group.width = Math.max(group.width, count); }
      } else group = undefined;
    }
    return groups.filter(item => item.end - item.start >= 10 && item.end - item.start <= 22).map(item => item.width);
  }, png.toString("base64"));
}

try {
  progress("open older Jig data in the main spreadsheet without changing any records");
  current = await open("sheet-desktop");
  assert.equal(await raw(current.page), fixtureRaw);
  await select(current.page, "C3");
  await key(current.page, "F2");
  await key(current.page, "Escape");
  assert.equal(await raw(current.page), fixtureRaw, "Selecting, opening, and cancelling an empty cell must not migrate older saved data");
  await capture(current.page, "sheet-desktop", 10000);

  progress("edit real cells and recalculate formulas through references and ranges");
  for (const [address, contents] of [["A1", "2"], ["A2", "3"], ["B1", "=SUM(A1:A2)"], ["C1", "=B1*1.5"], ["C2", "=AVERAGE(A1:A2)"], ["C3", "=MIN(A1:A2)+MAX(A1:A2)"], ["D1", "Sample sculpture"], ["B2", "=1/0"]]) await enter(current.page, address, contents);
  let saved = await state(current.page);
  assert.deepEqual(toolsOnly(saved), fixture, "Spreadsheet edits must preserve tasks, deadlines, calendar notes, preferences, history, and the featured countdown");
  assert.equal(saved.workbook.version, 1);
  assert.equal(sheet(saved).cells.B1, "=SUM(A1:A2)", "Formulas must be saved as user-owned source text");
  console.log("Sheet focus before copying:", await current.page.evaluate(() => ({ tag: document.activeElement?.tagName, id: document.activeElement?.id, focus: document.hasFocus(), keys: window.jigTestKeys })));
  await value(current.page, "B1", 5);
  await value(current.page, "C1", 7.5);
  await value(current.page, "C2", 2.5);
  await value(current.page, "C3", 5);
  await value(current.page, "D1", "Sample sculpture");
  await value(current.page, "B2", "#DIV/0!");

  progress("preserve pointer-selected controls and cancel cell/formula drafts with Escape");
  await enter(current.page, "A10", "11");
  await enter(current.page, "A10", "22", false);
  await current.page.mouse.click(40, 80); // The visible address field in the desktop formula bar.
  await paint(current.page);
  await text(current.page, "B10");
  await key(current.page, "Enter");
  await current.page.waitForFunction(() => document.activeElement?.id === "jig_canvas");
  assert.equal(sheet(await state(current.page)).cells.A10, "22", "Leaving a cell must commit it while keeping the chosen address control focused");
  assert.equal(sheet(await state(current.page)).cells.B10, undefined, "Typing a destination address must not become cell content");
  await enter(current.page, "B10", "33");
  await enter(current.page, "A10", "44", false);
  await key(current.page, "Escape");
  await current.page.waitForFunction(() => document.activeElement?.id === "jig_canvas");
  assert.equal(sheet(await state(current.page)).cells.A10, "22", "Escape must cancel an in-cell draft");
  await select(current.page, "A10");
  await current.page.mouse.click(220, 80); // The visible formula field.
  await paint(current.page);
  await pageTextInput(current.page, "55");
  await confirmDraft(current.page, "55");
  await key(current.page, "Escape");
  await current.page.waitForFunction(() => document.activeElement?.id === "jig_canvas");
  assert.equal(sheet(await state(current.page)).cells.A10, "22", "Escape must cancel a formula-bar draft");
  await value(current.page, "A10", 22);

  progress("recalculate edits and restore their values with Undo");
  await enter(current.page, "A1", "4");
  await value(current.page, "B1", 7);
  await value(current.page, "C1", 10.5);
  await undo(current.page);
  assert.equal(sheet(await state(current.page)).cells.A1, "2", "One global Undo must restore the previous cell input");
  await value(current.page, "C1", 7.5);
  await enter(current.page, "E1", "=E2+1");
  await enter(current.page, "E2", "=E1+1");
  await value(current.page, "E1", "#CYCLE!");
  await enter(current.page, "E2", "10");
  await value(current.page, "E1", 11);
  await capture(current.page, "sheet-desktop");

  progress("paste and clear a rectangular range as single undoable actions");
  const beforePaste = await state(current.page);
  await paste(current.page, "F1", "10\t20\n30\t=SUM(F1:F2)");
  saved = await state(current.page);
  assert.equal(sheet(saved).cells.F1, "10");
  assert.equal(sheet(saved).cells.G2, "=SUM(F1:F2)");
  await select(current.page, "F1");
  await key(current.page, "Shift+ArrowRight");
  await key(current.page, "Shift+ArrowDown");
  assert.equal((await copied(current.page)).replace(/\r\n/g, "\n"), "10\t20\n30\t40", "A selected block must copy evaluated TSV values");
  await key(current.page, "Delete");
  for (const address of ["F1", "F2", "G1", "G2"]) assert.equal(sheet(await state(current.page)).cells[address], undefined);
  await undo(current.page);
  await expectState(current.page, saved, "A single Undo must restore the whole cleared rectangle");
  await undo(current.page);
  await expectState(current.page, beforePaste, "A second Undo must remove the entire paste while preserving all previous work");
  await enter(current.page, "A10", "5");
  await enter(current.page, "B10", "=A10*2");
  assert.equal(await copied(current.page, "A10:B10"), "5\t10");
  await select(current.page, "A11");
  await key(current.page, "Control+v");
  assert.equal(sheet(await state(current.page)).cells.B11, "=A11*2", "Pasting copied formulas inside Jig must relocate relative references");
  await enter(current.page, "A11", "7");
  await value(current.page, "B11", 14);
  await value(current.page, "B10", 10);

  progress("import quoted CSV at the selected cell and export calculated values");
  await select(current.page, "J1");
  await pageImportCsv(current.page, 'label,value\n"Sample, clay",7\n"two ""quoted"" words",=K2*2\n');
  await value(current.page, "J2", "Sample, clay");
  await value(current.page, "J3", 'two "quoted" words');
  await value(current.page, "K3", 14);
  saved = await state(current.page);
  assert.equal(sheet(saved).cells.K3, "=K2*2");
  const csv = await exportCsv(current.page);
  assert.ok(csv.includes('"Sample, clay",7'), "CSV export must quote commas without losing the numeric value");
  assert.ok(csv.includes('"two ""quoted"" words",14'), "CSV export must escape quotes and output evaluated formulas");
  assert.ok(!csv.includes("=K2*2"));
  const beforeInvalid = await raw(current.page);
  await pageImportCsv(current.page, '"unterminated quoted cell');
  assert.equal(await raw(current.page), beforeInvalid, "Malformed CSV must preserve every existing cell and workbench record");

  progress("add, rename and delete sheets while preserving cross-sheet references and Undo");
  await key(current.page, "Shift+F11");
  saved = await state(current.page);
  assert.equal(saved.workbook.sheets.length, 2, "The public new-sheet shortcut must create one worksheet");
  const secondId = saved.workbook.sheets.find(item => item.id !== 1).id;
  await enter(current.page, "A1", "42");
  assert.equal(sheet(await state(current.page), secondId).cells.A1, "42", "New sheets must become active for normal cell editing");
  await renameSheet(current.page, "Studio estimates");
  assert.equal(sheet(await state(current.page), secondId).name, "Studio estimates");
  await key(current.page, "Alt+PageUp");
  await enter(current.page, "H1", "='Studio estimates'!A1*2");
  await value(current.page, "H1", 84);
  await key(current.page, "Alt+PageDown");
  await renameSheet(current.page, "Firing estimates");
  assert.ok(sheet(await state(current.page)).cells.H1.includes("Firing estimates"), "Renaming a sheet must update its saved formula references");
  await key(current.page, "Alt+PageUp");
  await value(current.page, "H1", 84);
  await key(current.page, "Alt+PageDown");
  await sheetControl(current.page, "minus");
  assert.equal((await state(current.page)).workbook.sheets.length, 2, "Deleting a populated worksheet must wait for its inline confirmation");
  await key(current.page, "Space"); // The focused Delete sheet confirmation.
  assert.equal((await state(current.page)).workbook.sheets.length, 1);
  await value(current.page, "H1", "#REF!");
  await undo(current.page);
  assert.equal((await state(current.page)).workbook.sheets.length, 2, "One Undo must restore the removed worksheet and its cells");
  await value(current.page, "H1", 84);
  assert.deepEqual(toolsOnly(await state(current.page)), fixture);

  progress("format financial values while preserving their stored precision");
  await enter(current.page, "A15", "0.125");
  await select(current.page, "A15");
  const plainColumn = await current.page.screenshot({ clip: { x: 45, y: 200, width: 120, height: 550 }, timeout: 60000, animations: "disabled", caret: "hide" });
  await numberFormat(current.page, 2); // Percent.
  saved = await state(current.page);
  assert.equal(sheet(saved).styles.A15.number_format, "percent");
  assert.equal(sheet(saved).cells.A15, "0.125", "A percentage format must not rewrite its underlying value");
  await value(current.page, "A15", 0.125);
  assert.notDeepEqual(await current.page.screenshot({ clip: { x: 45, y: 200, width: 120, height: 550 }, timeout: 60000, animations: "disabled", caret: "hide" }), plainColumn, "Percentage formatting must visibly change the cells, not just save a style flag");
  await enter(current.page, "A16", "1234.56");
  await select(current.page, "A16");
  await numberFormat(current.page, 3); // USD currency.
  assert.deepEqual(sheet(await state(current.page)).styles.A16.number_format, { currency: "USD" });
  await value(current.page, "A16", 1234.56);
  const serial = (Date.UTC(2026, 10, 11) - Date.UTC(1899, 11, 30)) / 86400000;
  await enter(current.page, "A17", String(serial));
  await select(current.page, "A17");
  await numberFormat(current.page, 4); // Date, immediately after the only currency choice, USD.
  assert.equal(sheet(await state(current.page)).styles.A17.number_format, "date");
  assert.equal(sheet(await state(current.page)).cells.A17, String(serial));
  await value(current.page, "A17", serial);
  await select(current.page, "A15");
  await toolbar(current.page, controls.font);
  await tab(current.page, 3); // Mono, Sans, then Serif.
  await key(current.page, "Space");
  assert.equal(sheet(await state(current.page)).styles.A15.font_family, "serif");
  await toolbar(current.page, controls.bold);
  assert.equal(sheet(await state(current.page)).styles.A15.bold, true);

  progress("align and strike actual cell text, restore Auto, and preserve independent styles");
  await enter(current.page, "D15", "IIII");
  await select(current.page, "D15");
  await toolbar(current.page, controls.left);
  const styledCell = await selectedCellBounds(current.page);
  const leftInk = await cellInk(current.page, styledCell);
  assert.equal(sheet(await state(current.page)).styles.D15.alignment, "left");
  await toolbar(current.page, controls.center);
  const centerInk = await cellInk(current.page, styledCell);
  assert.equal(sheet(await state(current.page)).styles.D15.alignment, "center");
  assert.ok(centerInk.left - leftInk.left >= 20, "Center must move the visible text inside the cell");
  await toolbar(current.page, controls.right);
  const rightInk = await cellInk(current.page, styledCell);
  assert.equal(sheet(await state(current.page)).styles.D15.alignment, "right");
  assert.ok(rightInk.left - centerInk.left >= 20, "Right alignment must move the visible text past center");
  await autoAlignment(current.page);
  assert.equal(sheet(await state(current.page)).styles?.D15?.alignment, undefined, "Auto must reset the saved override to the omitted default");
  assert.ok(Math.abs((await cellInk(current.page, styledCell)).left - leftInk.left) <= 1, "Auto must return ordinary text to the left");
  await toolbar(current.page, controls.center);
  const unstruck = await cellInk(current.page, styledCell);
  await toolbar(current.page, controls.strike);
  saved = await state(current.page);
  assert.equal(sheet(saved).styles.D15.strike, true);
  assert.equal(sheet(saved).styles.D15.alignment, "center", "Strikethrough must preserve the chosen alignment");
  assert.ok((await cellInk(current.page, styledCell)).maxRow >= unstruck.maxRow + 6, "Strikethrough must draw a line across the real glyphs");
  await toolbar(current.page, controls.strike);
  assert.equal(sheet(await state(current.page)).styles.D15.strike, undefined, "The same S control must toggle strikethrough off");
  await undo(current.page);
  await expectState(current.page, saved, "One Undo must restore the complete struck, centered style");
  for (const [address, alignment] of [["E15", "left"], ["F15", "right"]]) {
    await enter(current.page, address, "12");
    await select(current.page, address);
    await toolbar(current.page, controls[alignment]);
  }

  progress("apply every border preset to a rectangle through the real Borders menu");
  const borderNames = ["none", "all", "outer", "inner", "horizontal", "vertical", "top", "bottom", "left", "right"];
  for (let index = 0; index < borderNames.length; index++) {
    await select(current.page, "H15:J17");
    await borderFormat(current.page, 0); // Edge commands compose, so start each independent check without borders.
    await borderFormat(current.page, index);
    const styles = sheet(await state(current.page)).styles ?? {};
    for (let row = 0; row < 3; row++) for (let col = 0; col < 3; col++) {
      const address = `${String.fromCharCode(72 + col)}${15 + row}`;
      const expected = borderEdges(borderNames[index], row, col);
      assert.deepEqual(styles[address]?.borders ?? {}, expected, `${borderNames[index]} on ${address} must set the requested rectangle edges`);
    }
    assert.equal(styles.A15.bold, true, "Border actions must preserve unrelated formatting");
    assert.equal(styles.D15.strike, true);
  }
  await borderFormat(current.page, 2); // Keep an outside border in the document round trip.
  await capture(current.page, "sheet-formats");

  progress("create a bar chart from selected cells and redraw it after editing source values");
  await paste(current.page, "A20", "Sample phase\tHours\nMaking\t8\nDrying\t24\nFiring\t10");
  await select(current.page, "A20:B23");
  await barChart(current.page);
  saved = await state(current.page);
  assert.equal(saved.workbook.charts.length, 1, "Insert Chart must create exactly one saved chart");
  const chart = saved.workbook.charts[0];
  assert.equal(chart.kind, "bar");
  assert.deepEqual(chart.source, { type: "range", sheet: 1, range: { start: { row: 19, col: 0 }, end: { row: 22, col: 1 } } });
  const firstBars = await renderedBarWidths(current.page);
  assert.equal(firstBars.length, 3, "The chart gallery must draw all three horizontal bars in the actual viewport");
  assert.ok(Math.abs(firstBars[0] / firstBars[1] - 8 / 24) < 0.015, "The visible chart must use the selected numeric data");
  await backToCells(current.page);
  await enter(current.page, "B21", "16");
  await showCharts(current.page);
  const updatedBars = await renderedBarWidths(current.page);
  assert.equal(updatedBars.length, 3);
  assert.ok(Math.abs(updatedBars[0] / updatedBars[1] - 16 / 24) < 0.015, "Editing source cells must redraw the existing chart with its new values");
  assert.equal((await state(current.page)).workbook.charts.length, 1, "Reopening a live chart must preserve its identity");
  assert.deepEqual(toolsOnly(await state(current.page)), fixture);
  await capture(current.page, "sheet-chart");
  await backToCells(current.page);
  await key(current.page, "Control+Alt+2");
  await current.page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
  await key(current.page, "Escape");
  await tab(current.page, 5); // Data, Undo, Source, dashboard scroll area, then Gantt chart.
  await key(current.page, "Space");
  saved = await state(current.page);
  assert.equal(saved.workbook.charts.length, 2, "Existing tasks must be available as a live Gantt chart from the retained task workspace");
  assert.equal(saved.workbook.charts[1].kind, "gantt");
  assert.deepEqual(saved.workbook.charts[1].source, { type: "tasks" });
  assert.deepEqual(toolsOnly(saved), fixture, "Creating a task chart must reference the existing task rather than duplicate or migrate it");
  await backToCells(current.page);

  progress("retain existing workbench tools and share the same Undo journal");
  const workbookBeforeTools = structuredClone((await state(current.page)).workbook);
  await key(current.page, "Control+Alt+4");
  await pageTextInput(current.page, "6 * 7");
  await key(current.page, "Enter");
  saved = await state(current.page);
  assert.equal(saved.history.at(-1).result, 42, "The existing calculator must remain operational");
  assert.deepEqual(saved.workbook, workbookBeforeTools);
  await undo(current.page);
  assert.deepEqual(toolsOnly(await state(current.page)), fixture);
  await key(current.page, "Control+Alt+w");
  await value(current.page, "K3", 14);

  progress("reload and move spreadsheet formulas through the existing JSON import/export");
  saved = await state(current.page);
  const exported = await exportJson(current.page);
  assert.deepEqual(exported, saved, "JSON backups must include formulas and all existing Jig records");
  assert.deepEqual(sheet(exported).styles.D15, { strike: true, alignment: "center" }, "JSON backups must serialize explicit alignment and strike together");
  await value(current.page, "C1", 7.5);
  const imported = structuredClone(exported);
  sheet(imported).cells.A1 = "8";
  await importJson(current.page, imported);
  await expectState(current.page, imported, "JSON import must restore the same workbook and workbench records");
  await value(current.page, "B1", 11);
  const invalid = structuredClone(imported);
  sheet(invalid).cells.AAAA999999 = "bad address";
  await importJson(current.page, invalid, false);
  await expectState(current.page, imported, "An invalid workbook import must preserve the previously saved data");
  await boot(current.page, true);
  await expectState(current.page, imported, "The complete workbook must survive restart");
  await value(current.page, "B1", 11);
  progress("save a real ODS document and restore its formulas and Jig records");
  const beforeOds = await state(current.page);
  const ods = await shortcutDownload(current.page, "Control+s", ".ods");
  const files = zipEntries(ods);
  assert.equal(files.get("mimetype")?.toString("utf8"), "application/vnd.oasis.opendocument.spreadsheet");
  assert.ok(files.get("content.xml")?.toString("utf8").includes("table:formula="), "ODS must contain standard spreadsheet formulas, not merely an embedded JSON backup");
  assert.ok(files.get("content.xml")?.toString("utf8").includes("number:percentage-style"), "ODS must include standard numeric formatting");
  const contentXml = files.get("content.xml").toString("utf8");
  for (const alignment of ["left", "center", "right"]) assert.ok(contentXml.includes(`fo:text-align="${alignment}"`), `ODS must expose standard ${alignment} paragraph alignment`);
  assert.ok(contentXml.includes('style:text-line-through-style="solid"'), "ODS must expose standard strikethrough formatting to other spreadsheet apps");
  const chartPictures = [...files].filter(([name]) => /^Pictures\/chart-.*\.svg$/.test(name));
  assert.equal(chartPictures.length, 2, "A saved ODS must carry both real charts as portable SVG images");
  assert.ok(chartPictures.some(([, bytes]) => bytes.toString("utf8").includes("Making")), "The portable bar chart must contain its actual data labels");
  assert.ok(chartPictures.some(([, bytes]) => bytes.toString("utf8").includes("Sample: photograph sculpture")), "The portable Gantt chart must render the existing task");
  for (const [name] of chartPictures) assert.ok(files.get("content.xml")?.toString("utf8").includes(name), "The ODS sheet must reference each chart image");
  assert.ok(files.has("jig/state.json"), "ODS must include Jig's portable application records");
  assert.deepEqual(JSON.parse(files.get("jig/state.json").toString("utf8")).state, beforeOds, "The portable ODS must retain the existing Jig records and exact workbook source");
  await enter(current.page, "A1", "99");
  const beforeOdsReview = await raw(current.page);
  await importOds(current.page, ods);
  assert.equal(await raw(current.page), beforeOdsReview, "Opening ODS must wait for the user's review before replacing current data");
  await current.page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
  await key(current.page, "Escape");
  await tab(current.page, 3); // Data, active Undo, then Confirm import.
  await key(current.page, "Space");
  await expectState(current.page, beforeOds, "Confirming a locally saved ODS must restore formulas and all original Jig records");
  assert.deepEqual(sheet(await state(current.page)).styles.D15, { strike: true, alignment: "center" }, "ODS import must restore the actual alignment and strike style");
  await value(current.page, "B1", 11);
  const beforePdf = await raw(current.page);
  const pdf = await shortcutDownload(current.page, "Control+p", ".pdf");
  assert.equal(pdf.subarray(0, 5).toString("ascii"), "%PDF-", "Export PDF must produce an actual portable document");
  assert.ok(pdf.includes(Buffer.from("/FontFile2")) && pdf.includes(Buffer.from("/ToUnicode")), "PDF text must carry embedded font and Unicode mappings");
  assert.equal(await raw(current.page), beforePdf, "Document exports must leave the safely saved state intact");

  progress("review a local text PDF before importing rows as one undoable change");
  await select(current.page, "M1");
  const beforeStatement = await state(current.page);
  await importStatementPdf(current.page);
  await expectState(current.page, beforeStatement, "Reading a PDF must preserve the entire workbook until reviewed rows are applied");
  await capture(current.page, "sheet-import");
  await reviewControl(current.page, 3); // Data, Source, then Cancel import; ODS restore starts a fresh Undo journal.
  await value(current.page, "M1", "");
  await expectState(current.page, beforeStatement, "Cancelling document review must preserve every original record");
  await select(current.page, "M1");
  await importStatementPdf(current.page);
  await reviewControl(current.page, 15); // Apply reviewed rows, including the footer Source tab stop.
  saved = await state(current.page);
  assert.equal(sheet(saved).cells.M1, "Column 1");
  assert.equal(sheet(saved).cells.M2, "11/10/2026");
  assert.equal(sheet(saved).cells.N2, "Sample clay purchase");
  assert.equal(sheet(saved).cells.O2, "'25.40", "Keep text must preserve the statement amount's exact spelling");
  assert.equal(sheet(saved).cells.N3, "Sample tools");
  assert.equal(sheet(saved).cells.O3, "'-12.50", "Keep text must preserve the original transaction signs and trailing zero");
  assert.equal(sheet(saved).cells.P2, "Sample-transactions.pdf", "Reviewed rows must retain their source filename");
  assert.equal(sheet(saved).cells.Q2, "1", "Reviewed rows must retain their PDF page number");
  assert.equal(sheet(saved).cells.R2, "11/10/2026  Sample clay purchase  25.40");
  assert.deepEqual(toolsOnly(saved), fixture);
  await value(current.page, "O2", "25.40");
  await value(current.page, "O3", "-12.50");
  await undo(current.page);
  await expectState(current.page, beforeStatement, "One global Undo must restore all cells and Jig data before a multirow PDF import");
  await current.context.close();

  progress("keep legacy foreign currency readable while offering the five USD-only number formats");
  const legacy = structuredClone(fixture);
  legacy.workbook = {
    version: 1, charts: [], sheets: [{ id: 1, name: "Legacy formats", cells: { A1: "12.5", B1: "100", C4: "7", D5: "9", E6: "5", F1: "=C4+D5+E6" }, styles: { A1: { number_format: { currency: "EUR" } } } }],
  };
  current = await open("sheet-legacy-formats", legacy);
  await value(current.page, "A1", 12.5);
  await value(current.page, "F1", 21);
  const offeredFormats = [undefined, "number", "percent", { currency: "USD" }, "date"];
  for (let index = 0; index < offeredFormats.length; index++) {
    await select(current.page, "B1");
    await numberFormat(current.page, index);
    saved = await state(current.page);
    assert.deepEqual(sheet(saved).styles?.B1?.number_format, offeredFormats[index], "The menu must offer General, Number, Percent, USD, then Date without foreign currency entries");
    assert.deepEqual(sheet(saved).styles.A1.number_format, { currency: "EUR" }, "Changing another cell must preserve a loaded legacy foreign currency format");
    assert.equal(sheet(saved).cells.B1, "100");
  }
  await boot(current.page, true);
  assert.deepEqual(sheet(await state(current.page)).styles.A1.number_format, { currency: "EUR" }, "Legacy foreign currency must also survive restart");

  progress("use the bottom plus and minus controls and double-click the sheet name");
  await sheetControl(current.page, "plus");
  saved = await state(current.page);
  assert.equal(saved.workbook.sheets.length, 2, "Clicking the bottom + must add and activate a sheet");
  const pointerSheet = saved.workbook.sheets.at(-1).id;
  await enter(current.page, "A1", "31");
  assert.equal(sheet(await state(current.page), pointerSheet).cells.A1, "31");
  await renameSheet(current.page, "Pointer sheet");
  assert.equal(sheet(await state(current.page), pointerSheet).name, "Pointer sheet", "Double-clicking the tab must open its real rename editor");
  await sheetControl(current.page, "minus");
  assert.equal((await state(current.page)).workbook.sheets.length, 2, "The bottom - must show confirmation before deleting the active sheet");
  await key(current.page, "Space");
  assert.equal((await state(current.page)).workbook.sheets.length, 1);
  await undo(current.page);
  assert.equal(sheet(await state(current.page), pointerSheet).cells.A1, "31", "Undo must restore the sheet deleted with the bottom -");
  // Removing the active sheet selected the first sheet; Undo restores the data
  // while the visible first sheet remains active for the header tests below.
  await value(current.page, "F1", 21);

  progress("right-click row and column headers to insert or delete whole selected spans and Undo");
  await select(current.page, "A1");
  const firstCell = await selectedCellBounds(current.page);
  const beforeHeaders = await state(current.page);
  for (const [axis, header, beforeFormula, afterFormula, deletedFormula] of [
    ["row", { x: firstCell.left - 22, y: firstCell.top + 3 * 27 + 13 }, "=C6+D7+E8", "=C4+D5+E8", "=#REF!+#REF!+E4"],
    ["column", { x: firstCell.left + 2 * 112 + 56, y: firstCell.top - 13 }, "=E4+F5+G6", "=C4+D5+G6", "=#REF!+#REF!+C6"],
  ]) {
    for (const [index, expected] of [[0, beforeFormula], [1, afterFormula], [2, deletedFormula]]) {
      await select(current.page, "A1");
      await current.page.mouse.click(header.x, header.y);
      await paint(current.page);
      await current.page.keyboard.down("Shift");
      await paint(current.page);
      await current.page.mouse.click(header.x + (axis === "column" ? 112 : 0), header.y + (axis === "row" ? 27 : 0));
      await paint(current.page);
      await current.page.keyboard.up("Shift");
      await paint(current.page);
      interaction = `right-click ${axis} header, action ${index}`;
      await current.page.mouse.click(header.x, header.y, { button: "right" });
      await paint(current.page);
      await tab(current.page, index + 1); // Insert before, Insert after, Delete.
      await key(current.page, "Space");
      const formulaAddress = axis === "column" ? (index === 2 ? "D1" : "H1") : "F1";
      assert.equal(sheet(await state(current.page)).cells[formulaAddress], expected, `${axis} header action must update formula references for the whole two-header selection`);
      await value(current.page, formulaAddress, index === 2 ? "#REF!" : 21);
      await undo(current.page);
      await expectState(current.page, beforeHeaders, `One Undo must restore every cell, style, and reference after ${axis} header action ${index}`);
    }
  }
  assert.deepEqual(toolsOnly(await state(current.page)), fixture);

  progress("restore alignment, strike and borders through standard ODS XML without the private backup");
  await importOds(current.page, odsWithOnlyStandardWorkbook(ods));
  await current.page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
  await key(current.page, "Escape");
  await tab(current.page, 3); // Data, active Undo, Confirm import.
  await key(current.page, "Space");
  saved = await state(current.page);
  assert.deepEqual(sheet(saved).styles.D15, { strike: true, alignment: "center" }, "Standard ODS XML must restore strikethrough and centered text without Jig metadata");
  assert.equal(sheet(saved).styles.E15.alignment, "left");
  assert.equal(sheet(saved).styles.F15.alignment, "right");
  assert.deepEqual(sheet(saved).styles.H15.borders, { top: true, left: true });
  assert.deepEqual(toolsOnly(saved), fixture, "Opening an ordinary ODS must preserve the existing workbench records");
  await value(current.page, "D15", "IIII");
  await current.context.close();

  progress("operate the spreadsheet grid by touch on a phone");
  current = await open("sheet-mobile", fixture, true);
  await enter(current.page, "A1", "12");
  await enter(current.page, "B1", "=A1*25.4");
  await value(current.page, "B1", 304.8);
  await select(current.page, "B1");
  await tapFirstCell(current.page);
  await key(current.page, "F2");
  await pageTextInput(current.page, "15");
  await key(current.page, "Enter");
  await value(current.page, "B1", 381);
  await capture(current.page, "sheet-mobile");
  assert.equal(await current.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true, "The phone spreadsheet must stay within the browser viewport");
  await enter(current.page, "A2", "Sample: cancelled edit", false);
  await capture(current.page, "sheet-edit");
  await key(current.page, "Escape");
  assert.equal(sheet(await state(current.page)).cells.A2, undefined, "Escape must discard a draft cell edit");
  await boot(current.page, true);
  await value(current.page, "B1", 381);
  assert.deepEqual(toolsOnly(await state(current.page)), fixture);
  await current.context.close();

  progress("keep edits to clearly labelled sample workbooks temporary");
  current = await open("sheet-samples", null);
  assert.equal(await raw(current.page), null);
  await enter(current.page, "D10", "Sample: temporary spreadsheet edit");
  await value(current.page, "D10", "Sample: temporary spreadsheet edit");
  assert.equal(await raw(current.page), null, "Sample spreadsheet edits must not create or replace personal data");
  assert.deepEqual(issues, [], "The local spreadsheet must emit no application errors or remote data requests");
  console.log("Spreadsheet browser checks passed: legacy-data preservation; cell editing, formulas, ranges, errors and cycle recovery; clipboard rectangles, formula relocation and global Undo; quoted CSV; sheet lifecycle and cross-sheet references; USD-only number choices with retained legacy currencies; visible alignment and strike, Auto reset, every border preset and style round trips; bottom sheet controls and double-click rename; header context-menu row/column span edits, reference updates and Undo; rendered live bar charts and existing-task Gantt charts; unchanged workbench tools; JSON backup/import and reload; real ODS documents with formulas, styles, charts and Jig data; embedded-font PDF export; local text-PDF review, cancellation, source provenance and atomic import/Undo; phone touch editing, draft cancellation and sample isolation.");
} catch (error) {
  originalError = error;
  console.error(`Spreadsheet browser failed during: ${phase}; ${interaction}`);
  console.error("Sheet application issues:", JSON.stringify(issues));
  if (current?.page && !current.page.isClosed()) {
    let deadline;
    await Promise.race([
      current.page.evaluate(() => ({ tag: document.activeElement?.tagName, id: document.activeElement?.id, focus: document.hasFocus(), keys: window.jigTestKeys, bootError: window.jigBootError })).then(info => console.error("Sheet focus at failure:", JSON.stringify(info))),
      new Promise(resolve => { deadline = setTimeout(resolve, 1500); }),
    ]).catch(() => {});
    clearTimeout(deadline);
  }
  if (current?.page && !current.page.isClosed()) await capture(current.page, "sheet-failure", 3000).catch(() => {});
  throw error;
} finally {
  progress("close spreadsheet contexts and Chromium");
  let cleanupTimer;
  let cleanupError;
  try {
    await Promise.race([
      Promise.all([...contexts.map(context => context.close().catch(() => {})), browser.close()]),
      new Promise((_, reject) => { cleanupTimer = setTimeout(() => reject(new Error("Spreadsheet browser shutdown exceeded 10 seconds")), 10000); }),
    ]);
  } catch (error) { cleanupError = error; }
  finally { clearTimeout(cleanupTimer); clearTimeout(timer); }
  if (cleanupError) {
    if (originalError) console.error("Original spreadsheet failure:", originalError);
    console.error("Spreadsheet browser cleanup failed:", cleanupError);
    process.exit(1);
  }
}

async function pageTextInput(page, contents) {
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 50 });
  await text(page, contents);
}
async function pageImportCsv(page, contents) {
  await nameFocus(page);
  await tab(page, 3); // Address, formula, Enter, then Import CSV.
  const event = page.waitForEvent("filechooser", { timeout: 5000 });
  await key(page, "Space");
  const chooser = await event;
  await chooser.setFiles({ name: "sample-sheet.csv", mimeType: "text/csv", buffer: Buffer.from(contents) });
  await page.waitForTimeout(1600);
  await paint(page);
}
async function renameSheet(page, name) {
  await select(page, "A1");
  const bounds = await selectedSheetBounds(page);
  interaction = `double-click sheet tab ${JSON.stringify(bounds)} to rename ${JSON.stringify(name)}`;
  // eframe runs the WASM UI synchronously on every down/up event. egui counts
  // releases less than 300 ms apart rather than reading the DOM dblclick event;
  // an artificial hold can exhaust that interval on the software renderer.
  await page.mouse.dblclick((bounds.left + bounds.right) / 2, (bounds.top + bounds.bottom) / 2);
  await paint(page);
  await pageTextInput(page, name);
  await key(page, "Enter");
  await page.waitForFunction(() => document.activeElement?.id === "jig_canvas", undefined, { timeout: 10000, polling: 50 });
}
async function numberFormat(page, index) {
  await toolbar(page, controls.number);
  await tab(page, index + 1);
  await key(page, "Space");
}
async function borderFormat(page, index) {
  await toolbar(page, controls.borders);
  await tab(page, index + 1);
  await key(page, "Space");
}
async function autoAlignment(page) {
  await topMenu(page, 3); // Format, before Insert and View.
  await tab(page, 7); // Font, size, B, I, U, S, Alignment.
  // egui 0.31 opens nested menus on pointer hover. Space activates top-level
  // menus but does not open this submenu. Its keyboard focus is visibly ACTIVE.
  const { shapes } = await selectedShapes(page, [77, 49, 39]);
  const alignment = shapes.find(item => item.top > 60 && item.right - item.left >= 90 && item.bottom - item.top <= 26);
  assert.ok(alignment, "Format must visibly focus its Alignment submenu before opening it");
  interaction = `open focused Alignment submenu ${JSON.stringify(alignment)} and choose Auto`;
  await page.mouse.click((alignment.left + alignment.right) / 2, (alignment.top + alignment.bottom) / 2);
  await paint(page);
  await tab(page); // Auto is the first choice.
  await key(page, "Space");
}
function borderEdges(preset, row, col) {
  const all = { top: true, right: true, bottom: true, left: true };
  const outer = { top: row === 0, right: col === 2, bottom: row === 2, left: col === 0 };
  const choices = {
    none: {}, all, outer,
    inner: Object.fromEntries(Object.entries(outer).map(([edge, enabled]) => [edge, !enabled])),
    horizontal: { top: true, bottom: true }, vertical: { left: true, right: true },
    top: { top: row === 0 }, bottom: { bottom: row === 2 }, left: { left: col === 0 }, right: { right: col === 2 },
  };
  return Object.fromEntries(Object.entries(choices[preset]).filter(([, enabled]) => enabled));
}
async function selectedShapes(page, color = [67, 42, 34]) {
  // Read rendered pixels, never application state or a synthetic UI hook. The
  // selected fill identifies both cells and the active sheet tab on any layout.
  const png = await page.screenshot({ timeout: 60000, animations: "disabled", caret: "hide" });
  return page.evaluate(async ({ encoded, color }) => {
    const picture = new Image(); picture.src = `data:image/png;base64,${encoded}`;
    await picture.decode();
    const canvas = document.createElement("canvas"); canvas.width = picture.width; canvas.height = picture.height;
    const context = canvas.getContext("2d"); context.drawImage(picture, 0, 0);
    const { data } = context.getImageData(0, 0, canvas.width, canvas.height);
    const mask = new Uint8Array(canvas.width * canvas.height);
    for (let pixel = 0; pixel < mask.length; pixel++) {
      const offset = pixel * 4;
      mask[pixel] = Math.abs(data[offset] - color[0]) <= 1 && Math.abs(data[offset + 1] - color[1]) <= 1 && Math.abs(data[offset + 2] - color[2]) <= 1 ? 1 : 0;
    }
    const queue = new Int32Array(mask.length), shapes = [];
    for (let pixel = 0; pixel < mask.length; pixel++) {
      if (!mask[pixel]) continue;
      let start = 0, end = 1;
      queue[0] = pixel; mask[pixel] = 0;
      let left = canvas.width, right = 0, top = canvas.height, bottom = 0;
      while (start < end) {
        const current = queue[start++], x = current % canvas.width, y = Math.floor(current / canvas.width);
        left = Math.min(left, x); right = Math.max(right, x); top = Math.min(top, y); bottom = Math.max(bottom, y);
        for (const neighbour of [x ? current - 1 : -1, x + 1 < canvas.width ? current + 1 : -1, y ? current - canvas.width : -1, y + 1 < canvas.height ? current + canvas.width : -1]) {
          if (neighbour >= 0 && mask[neighbour]) { mask[neighbour] = 0; queue[end++] = neighbour; }
        }
      }
      if (right - left >= 10 && bottom - top >= 12) shapes.push({ left, right, top, bottom, pixels: end });
    }
    return { shapes, height: canvas.height };
  }, { encoded: png.toString("base64"), color });
}
async function selectedCellBounds(page) {
  const { shapes } = await selectedShapes(page);
  const bounds = shapes.filter(item => item.right - item.left >= 85 && item.right - item.left <= 115 && item.bottom - item.top >= 20 && item.bottom - item.top <= 30 && item.pixels > 1500).sort((a, b) => b.bottom - a.bottom)[0];
  assert.ok(bounds, "The real viewport must show one selected cell before measuring its text or headers");
  return bounds;
}
async function selectedSheetBounds(page) {
  await page.mouse.move(8, 8);
  await key(page, "Escape");
  const { shapes, height } = await selectedShapes(page);
  const bounds = shapes.filter(item => item.top > height * 0.65 && item.bottom - item.top <= 32).sort((a, b) => b.bottom - a.bottom)[0];
  assert.ok(bounds, "The bottom tab strip must visibly mark the active sheet");
  return bounds;
}
async function sheetControl(page, control) {
  // These operations use the last tab, so the next two rendered button fills
  // belong to + and -. Find their complete rectangles: a scan through one
  // pixel row can be split into narrow fragments by the + or - glyph itself.
  await select(page, "A1");
  const bounds = await selectedSheetBounds(page);
  const { shapes } = await selectedShapes(page, [41, 37, 34]);
  const buttons = shapes.filter(item => item.left > bounds.right && item.right - item.left <= 100
    && Math.abs(item.top - bounds.top) <= 4 && Math.abs(item.bottom - bounds.bottom) <= 4)
    .sort((a, b) => a.left - b.left).slice(0, 2);
  const target = control === "plus" ? 0 : 1;
  interaction = `click bottom sheet ${control}; tab ${JSON.stringify(bounds)}; buttons ${JSON.stringify(buttons)}`;
  assert.ok(buttons.length > target, `The visible bottom tab strip must offer an enabled ${control} button after the last sheet`);
  const button = buttons[target];
  await page.mouse.click((button.left + button.right) / 2, (button.top + button.bottom) / 2);
  await paint(page);
}
async function cellInk(page, bounds) {
  const png = await page.screenshot({ clip: { x: bounds.left + 3, y: bounds.top + 3, width: bounds.right - bounds.left - 6, height: bounds.bottom - bounds.top - 6 }, timeout: 60000, animations: "disabled", caret: "hide" });
  return page.evaluate(async encoded => {
    const picture = new Image(); picture.src = `data:image/png;base64,${encoded}`; await picture.decode();
    const canvas = document.createElement("canvas"); canvas.width = picture.width; canvas.height = picture.height;
    const context = canvas.getContext("2d"); context.drawImage(picture, 0, 0);
    const { data } = context.getImageData(0, 0, canvas.width, canvas.height);
    let left = canvas.width, maxRow = 0;
    for (let y = 0; y < canvas.height; y++) {
      let count = 0;
      for (let x = 0; x < canvas.width; x++) {
        const offset = (y * canvas.width + x) * 4;
        if (data[offset] > 180 && data[offset + 1] > 175 && data[offset + 2] > 170) { left = Math.min(left, x); count++; }
      }
      maxRow = Math.max(maxRow, count);
    }
    return { left, maxRow };
  }, png.toString("base64"));
}
async function tapFirstCell(page) {
  // Locate the visibly selected B1 cell in the real screenshot, then tap its
  // left neighbour. Toolbar wrapping can move the grid on narrow screens.
  // B1 is selected first, so a missed tap cannot silently edit A1 anyway.
  const png = await page.screenshot({ timeout: 60000, animations: "disabled", caret: "hide" });
  const bounds = await page.evaluate(async encoded => {
    const picture = new Image(); picture.src = `data:image/png;base64,${encoded}`;
    await picture.decode();
    const canvas = document.createElement("canvas");
    canvas.width = picture.width; canvas.height = picture.height;
    const context = canvas.getContext("2d"); context.drawImage(picture, 0, 0);
    const { data } = context.getImageData(0, 0, canvas.width, canvas.height);
    const mask = new Uint8Array(canvas.width * canvas.height);
    for (let index = 0; index < mask.length; index++) {
      const offset = index * 4;
      mask[index] = Math.abs(data[offset] - 67) <= 1 && Math.abs(data[offset + 1] - 42) <= 1 && Math.abs(data[offset + 2] - 34) <= 1 ? 1 : 0;
    }
    const queue = new Int32Array(mask.length), shapes = [];
    for (let index = 0; index < mask.length; index++) {
      if (!mask[index]) continue;
      let start = 0, end = 1;
      queue[0] = index; mask[index] = 0;
      let left = canvas.width, right = 0, top = canvas.height, bottom = 0;
      while (start < end) {
        const pixel = queue[start++], x = pixel % canvas.width, y = Math.floor(pixel / canvas.width);
        left = Math.min(left, x); right = Math.max(right, x); top = Math.min(top, y); bottom = Math.max(bottom, y);
        for (const neighbour of [x ? pixel - 1 : -1, x + 1 < canvas.width ? pixel + 1 : -1, y ? pixel - canvas.width : -1, y + 1 < canvas.height ? pixel + canvas.width : -1]) {
          if (neighbour >= 0 && mask[neighbour]) { mask[neighbour] = 0; queue[end++] = neighbour; }
        }
      }
      if (right - left >= 85 && right - left <= 115 && bottom - top >= 20 && bottom - top <= 30 && end > 1500) shapes.push({ left, right, top, bottom });
    }
    // The selected B header has the same fill; its cell is the lower rectangle.
    return shapes.sort((a, b) => b.bottom - a.bottom)[0];
  }, png.toString("base64"));
  assert.ok(bounds, "The phone viewport must visibly show the selected B1 cell before touch navigation");
  await page.touchscreen.tap((bounds.left + bounds.right) / 2 - 112, (bounds.top + bounds.bottom) / 2);
  await paint(page);
}

// Draft is a real browser editor; every document in this suite is synthetic.
import assert from "node:assert/strict";
import { mkdir, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { inflateRawSync } from "node:zlib";
import { isDeepStrictEqual } from "node:util";
import { chromium } from "playwright";
import { installFrameFence, waitForAppPaint } from "./browser_frames.mjs";

const base = process.env.JIG_PREVIEW_URL ?? "http://127.0.0.1:4173/Jig/";
const origin = new URL(base).origin;
const storageKey = "jig.personal.v1";
const results = resolve("playwright-results");
await mkdir(results, { recursive: true });
const fixture = {
  version: 1, preferences: { decimals: 2, focus: 3, weather: null }, history: [],
  tasks: [{ id: 1, text: "Sample: preserve the checklist", complete: false }],
  notes: [{ id: 2, text: "Sample: preserve the note", pinned: true }],
  deadlines: [], entries: [], featured: null,
  workbook: { version: 1, sheets: [{ id: 1, name: "Sample inputs", cells: { A1: "Material", B1: "Amount", A2: "Clay", B2: "10" } }] },
};
const textOf = draft => (draft?.blocks ?? []).map(block => (block.runs ?? []).map(run => run.text).join("")).join("\n");
const runsOf = draft => (draft?.blocks ?? []).flatMap(block => block.runs ?? []).filter(run => run.text);
const allRuns = (draft, predicate) => { const runs = runsOf(draft); return runs.length > 0 && runs.every(predicate); };
const withoutDraft = value => { const { draft, ...other } = value; return other; };
let phase = "launch Chromium", current;
const contexts = [], issues = [];
const timer = setTimeout(() => { console.error(`Draft browser exceeded 480 seconds during: ${phase}`); process.exit(1); }, 480000);
function progress(value) { phase = value; console.log(`Draft browser phase: ${value}`); }
async function paint(page) { await waitForAppPaint(page); }
async function key(page, value) { await page.keyboard.press(value); await paint(page); }
async function tab(page, count = 1) { for (let index = 0; index < count; index++) await key(page, "Tab"); }
async function state(page) { return page.evaluate(key => JSON.parse(localStorage.getItem(key)), storageKey); }
async function readClipboard(page, expected, exact = false) {
  await page.waitForFunction(({ expected, exact }) => navigator.clipboard.readText().then(text => exact ? text === expected : text.includes(expected)), { expected, exact });
  return page.evaluate(async () => {
    const items = await navigator.clipboard.read(), output = {};
    for (const item of items) for (const type of item.types) if (["text/plain", "text/html"].includes(type)) output[type] = await (await item.getType(type)).text();
    return output;
  });
}
async function saved(page, predicate, message) {
  const until = Date.now() + 10000;
  do { const value = await state(page); if (predicate(value)) return value; await page.waitForTimeout(25); } while (Date.now() < until);
  assert.ok(predicate(await state(page)), message);
}
async function expectState(page, expected, message) {
  await saved(page, value => isDeepStrictEqual(value, expected), message);
  assert.deepEqual(await state(page), expected, message);
}
async function resetFocus(page) {
  const { width, height } = page.viewportSize();
  await page.locator("#jig_canvas").click({ position: { x: width - 6, y: height - 6 } });
  await key(page, "Escape");
}
async function button(page, name) { await page.getByRole("button", { name, exact: true }).click(); }
async function selectAll(page) { await page.locator("#jig-draft-editor").focus(); await page.keyboard.press("Control+a"); }
async function selectText(page, text) {
  await page.locator("#jig-draft-editor").focus();
  await page.evaluate(text => {
    const editor = document.getElementById("jig-draft-editor"), nodes = [];
    const walker = document.createTreeWalker(editor, NodeFilter.SHOW_TEXT);
    while (walker.nextNode()) nodes.push(walker.currentNode);
    const all = nodes.map(node => node.textContent).join(""), start = all.indexOf(text);
    if (start < 0) throw new Error(`Selection text not found: ${text}`);
    let offset = 0, first, last;
    for (const node of nodes) {
      const end = offset + node.textContent.length;
      if (!first && start >= offset && start < end) first = [node, start - offset];
      if (start + text.length > offset && start + text.length <= end) { last = [node, start + text.length - offset]; break; }
      offset = end;
    }
    if (!first || !last) throw new Error("Selection endpoints were not found");
    const range = document.createRange(); range.setStart(...first); range.setEnd(...last);
    const selection = window.getSelection(); selection.removeAllRanges(); selection.addRange(range);
  }, text);
  await page.waitForFunction(text => window.getSelection()?.toString() === text, text);
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
async function draftHeader(page) {
  // Locate the actual selected canvas button; the DOM editor begins below it.
  const panel = await page.locator("#jig-draft-panel").boundingBox();
  assert.ok(panel);
  const png = await page.screenshot({ animations: "disabled", caret: "hide" });
  const regions = await page.evaluate(async ({ encoded, bottom }) => {
    const image = await createImageBitmap(new Blob([Uint8Array.from(atob(encoded), c => c.charCodeAt(0))], { type: "image/png" }));
    const canvas = new OffscreenCanvas(image.width, image.height), ctx = canvas.getContext("2d"); ctx.drawImage(image, 0, 0);
    const pixels = ctx.getImageData(0, 0, image.width, image.height).data, seen = new Uint8Array(image.width * image.height), regions = [];
    const near = index => [67, 42, 34].every((channel, axis) => Math.abs(pixels[index * 4 + axis] - channel) <= 3);
    for (let y = 0; y < Math.min(image.height, bottom); y++) for (let x = 0; x < image.width; x++) {
      const start = y * image.width + x; if (seen[start] || !near(start)) continue;
      const stack = [start]; seen[start] = 1; let left = x, right = x, top = y, end = y, count = 0;
      while (stack.length) {
        const index = stack.pop(), px = index % image.width, py = Math.floor(index / image.width); count++;
        left = Math.min(left, px); right = Math.max(right, px); top = Math.min(top, py); end = Math.max(end, py);
        for (const [nx, ny] of [[px - 1, py], [px + 1, py], [px, py - 1], [px, py + 1]]) {
          if (nx < 0 || nx >= image.width || ny < 0 || ny >= bottom) continue;
          const next = ny * image.width + nx; if (!seen[next] && near(next)) { seen[next] = 1; stack.push(next); }
        }
      }
      if (count > 20) regions.push({ x: (left + right) / 2, y: (top + end) / 2, width: right - left + 1, height: end - top + 1 });
    }
    image.close(); return regions;
  }, { encoded: png.toString("base64"), bottom: Math.floor(panel.y) });
  const point = regions.find(region => region.width >= 40 && region.width <= 85 && region.height >= 16 && region.height <= 32);
  assert.ok(point, "The actual Draft header button must be visibly selected"); return point;
}
async function showDraft(page, point, touch = false) {
  if (touch) await page.touchscreen.tap(point.x, point.y); else await page.mouse.click(point.x, point.y);
  await page.locator("#jig-draft-editor").waitFor({ state: "visible" });
  await page.waitForFunction(() => localStorage.getItem("jig.workspace.v1") === "draft");
}
async function boot(page, reload = false) {
  if (reload) await page.reload({ waitUntil: "load" }); else await page.goto(base, { waitUntil: "load" });
  await page.bringToFront();
  await page.waitForFunction(() => window.jigReady === true || typeof window.jigBootError === "string", undefined, { timeout: 60000 });
  assert.equal(await page.evaluate(() => window.jigBootError), undefined);
  await paint(page);
}
const browser = await chromium.launch({ headless: true, args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"] });
async function open(label, document = fixture, phone = false, light = false, recovery = null) {
  const context = await browser.newContext({ viewport: phone ? { width: 390, height: 844 } : { width: 1440, height: 1000 }, hasTouch: phone, acceptDownloads: true, timezoneId: "Etc/UTC" });
  contexts.push(context); await installFrameFence(context);
  await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin });
  await context.addInitScript(({ origin, storageKey, document, light, recovery }) => {
    if (location.origin !== origin) return;
    if (sessionStorage.getItem("jig-draft-suite-seeded") !== "yes") {
      localStorage.setItem(storageKey, JSON.stringify(document)); localStorage.setItem("jig.workspace.v1", "draft");
      if (light) localStorage.setItem("jig.settings.v1", JSON.stringify({ version: 1, theme: "light", undo_levels: 20, auto_backup: false }));
      if (recovery !== null) sessionStorage.setItem("jig.draft.recovery.v1", recovery);
      sessionStorage.setItem("jig-draft-suite-seeded", "yes");
    }
    const write = Storage.prototype.setItem;
    Storage.prototype.setItem = function (key, value) {
      if (this === localStorage && key === storageKey && window.failDraftSave === true) {
        window.draftFailedSaves = (window.draftFailedSaves ?? 0) + 1;
        throw new DOMException("Synthetic Draft quota failure", "QuotaExceededError");
      }
      return write.call(this, key, value);
    };
  }, { origin, storageKey, document, light, recovery });
  await context.route("**/*", async route => {
    const url = new URL(route.request().url());
    if (["blob:", "data:", "about:"].includes(url.protocol) || url.origin === origin) await route.continue();
    else { issues.push(`${label}: unexpected request ${url.origin}${url.pathname}`); await route.abort(); }
  });
  const page = await context.newPage(); page.setDefaultTimeout(10000); page.setDefaultNavigationTimeout(60000);
  page.on("pageerror", error => issues.push(`${label}: ${error.message}`));
  page.on("dialog", dialog => dialog.accept());
  await boot(page); await page.locator("#jig-draft-editor").waitFor({ state: "visible" });
  return { context, page };
}
async function download(page, shortcut, extension) {
  const pending = page.waitForEvent("download", { timeout: 15000 }); await key(page, shortcut);
  const item = await pending; assert.ok(item.suggestedFilename().endsWith(extension)); return readFile(await item.path());
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

try {
  progress("load an older document without Draft and format real typed text");
  current = await open("draft-desktop");
  const header = await draftHeader(current.page), editor = current.page.locator("#jig-draft-editor");
  assert.equal(await editor.getAttribute("contenteditable"), "true");
  assert.equal(textOf((await state(current.page)).draft), "");
  await editor.click(); await current.page.keyboard.type("Sample installation", { delay: 15 });
  await current.page.keyboard.press("Enter"); await current.page.keyboard.type("Confirm dimensions before sending.", { delay: 15 });
  const words = "Sample installation\nConfirm dimensions before sending.";
  await saved(current.page, value => textOf(value.draft) === words, "Typed paragraphs must save to the shared document");
  for (const [name, flag] of [["Bold", "bold"], ["Italic", "italic"], ["Underline", "underline"]]) {
    await selectAll(current.page); await button(current.page, name);
    await saved(current.page, value => allRuns(value.draft, run => run[flag] === true), `${name} must format the selected text`);
  }
  await editor.press("Control+z");
  await saved(current.page, value => allRuns(value.draft, run => run.bold && run.italic && !run.underline), "Shared Undo must remove the last formatting action");
  await editor.press("Control+Shift+z");
  await saved(current.page, value => allRuns(value.draft, run => run.underline), "Shared Redo must restore formatting");
  await selectAll(current.page); await current.page.getByLabel("Text size", { exact: true }).selectOption("20");
  await saved(current.page, value => allRuns(value.draft, run => run.size === 20), "Text size must persist on all selected runs");
  await selectAll(current.page); await button(current.page, "Align center");
  await saved(current.page, value => value.draft.blocks.every(block => block.alignment === "center"), "Alignment must belong to paragraphs");
  await selectAll(current.page); await button(current.page, "Bullets");
  await saved(current.page, value => value.draft.blocks.every(block => block.list === "bullet"), "Bullets must persist as list items");
  await selectAll(current.page); await button(current.page, "Numbered list");
  await saved(current.page, value => value.draft.blocks.every(block => block.list === "number"), "Numbered items must replace the selected bullet list");
  await selectAll(current.page); await button(current.page, "Link");
  await current.page.getByLabel("Link URL", { exact: true }).fill("https://example.com/installation"); await button(current.page, "Apply link");
  await saved(current.page, value => allRuns(value.draft, run => run.link === "https://example.com/installation"), "Links must preserve selected text and formatting");
  await selectAll(current.page); await button(current.page, "Remove link");
  await saved(current.page, value => allRuns(value.draft, run => !run.link), "Remove link must keep the text");
  await selectAll(current.page); await button(current.page, "Link");
  await current.page.getByLabel("Link URL", { exact: true }).fill("https://example.com/installation"); await button(current.page, "Apply link");
  const formatted = (await saved(current.page, value => allRuns(value.draft, run => Boolean(run.link)), "Reapplied links must save")).draft;
  assert.equal(textOf(formatted), words);
  assert.deepEqual(withoutDraft(await state(current.page)), fixture, "Formatting must not touch other Jig tools");

  progress("copy rich text and send a selected part to Notes and Tasks without removing Draft");
  await selectAll(current.page); await button(current.page, "Copy");
  const clipboard = await readClipboard(current.page, "Sample installation");
  assert.ok(clipboard["text/plain"]?.includes("Sample installation") && clipboard["text/plain"]?.includes("Confirm dimensions"));
  assert.match(clipboard["text/html"] ?? "", /<(?:b|strong)\b|font-weight\s*:\s*(?:bold|[6-9]\d\d)/i);
  assert.match(clipboard["text/html"] ?? "", /<ol\b/i); assert.ok(clipboard["text/html"].includes("https://example.com/installation"));
  const excerpt = "Confirm dimensions";
  await selectText(current.page, excerpt); await button(current.page, "Copy");
  const partial = await readClipboard(current.page, excerpt, true);
  assert.match(partial["text/html"] ?? "", /<(?:b|strong)\b|font-weight\s*:\s*(?:bold|[6-9]\d\d)/i,
    "Copying inside a formatted text node must retain inherited bold");
  assert.match(partial["text/html"] ?? "", /font-size\s*:\s*20px/i, "Partial Copy must keep the selected font size");
  assert.ok(partial["text/html"].includes("https://example.com/installation"), "Partial Copy must retain its enclosing link");
  await selectText(current.page, excerpt); await button(current.page, "To note");
  await saved(current.page, value => value.notes.length === 2 && value.notes.some(note => note.text === excerpt), "To note must copy the selected text");
  await selectText(current.page, excerpt); await button(current.page, "To task");
  const transferred = await saved(current.page, value => value.tasks.length === 2 && value.tasks.some(task => task.text === excerpt), "To task must copy the selected text");
  assert.deepEqual(transferred.draft, formatted);
  assert.deepEqual(transferred.workbook, fixture.workbook); assert.deepEqual(transferred.notes[0], fixture.notes[0]);
  await editor.press("Control+z");
  await saved(current.page, value => value.tasks.length === 1 && value.notes.length === 2, "Draft Undo must use Jig history, not a separate native editor history");
  await editor.press("Control+Shift+z"); await expectState(current.page, transferred, "Redo must restore only the copied task");
  await resetFocus(current.page); await key(current.page, "Control+Alt+n");
  await current.page.locator("#jig-draft-panel").waitFor({ state: "hidden" });
  await current.page.waitForFunction(() => localStorage.getItem("jig.workspace.v1") === "notes");
  await key(current.page, "Control+z");
  await saved(current.page, value => value.tasks.length === 1 && value.notes.length === 2, "Undo from Notes must share the same history");
  await key(current.page, "Control+Alt+r");
  await current.page.locator("#jig-draft-editor").waitFor({ state: "visible" });
  await current.page.waitForFunction(() => localStorage.getItem("jig.workspace.v1") === "draft");
  await editor.press("Control+Shift+z");
  await expectState(current.page, transferred, "Returning to Draft must preserve its exact document and shared Redo");
  await current.page.getByRole("button", { name: "Copy", exact: true }).focus();
  await current.page.keyboard.press("Escape");
  await current.page.waitForFunction(() => document.activeElement?.id === "jig_canvas");
  await key(current.page, "Control+Alt+r");
  await current.page.waitForFunction(() => document.activeElement?.id === "jig-draft-editor");
  await selectAll(current.page); await current.page.keyboard.press("ArrowRight"); await capture(current.page, "draft-desktop");

  progress("restore Draft through refresh, JSON and a real Jig ODS file");
  const portable = await state(current.page); await boot(current.page, true);
  await current.page.locator("#jig-draft-editor").waitFor({ state: "visible" });
  await expectState(current.page, portable, "Reload must retain formatted Draft and all other tools");
  await resetFocus(current.page); await key(current.page, "Control+Alt+d"); await tab(current.page, 9);
  const jsonDownload = current.page.waitForEvent("download", { timeout: 15000 }); await key(current.page, "Enter");
  const json = JSON.parse(await readFile(await (await jsonDownload).path(), "utf8")); assert.deepEqual(json, portable);
  await key(current.page, "Control+Alt+d"); await key(current.page, "Control+Alt+w");
  await current.page.waitForFunction(() => localStorage.getItem("jig.workspace.v1") === "sheet");
  const ods = await download(current.page, "Control+s", ".ods"), entries = zipEntries(ods);
  assert.deepEqual(JSON.parse(entries.get("jig/state.json").toString("utf8")).state, portable);
  await current.page.locator("#jig-import").setInputFiles({ name: "Sample-before-Draft.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(fixture)) });
  await current.page.waitForTimeout(1200); await confirmImport(current.page);
  await expectState(current.page, fixture, "Older documents without Draft must load as an empty holding zone");
  const chooser = current.page.waitForEvent("filechooser"); await key(current.page, "Control+o");
  await (await chooser).setFiles({ name: "Sample-Draft.ods", mimeType: "application/vnd.oasis.opendocument.spreadsheet", buffer: ods });
  await current.page.waitForTimeout(1200); await confirmImport(current.page);
  await expectState(current.page, portable, "ODS must restore the same Draft formatting and source workbook");
  await current.page.locator("#jig-import").setInputFiles({ name: "Sample-Draft.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(json)) });
  await current.page.waitForTimeout(1200); await confirmImport(current.page); await expectState(current.page, portable, "JSON must retain formatted Draft");
  await showDraft(current.page, header); await current.context.close();

  progress("sanitize actual rich clipboard paste without executing or loading pasted content");
  current = await open("draft-paste");
  const html = '<p onclick="window.draftExecuted=true">Sample <strong>safe paste</strong> <span style="font-size:20px;color:red">size</span><img src="https://draft-never-fetch.invalid/pixel"><script>window.draftExecuted=true</script><a href="javascript:window.draftExecuted=true">bad link</a> <a href="https://example.com/source">source</a></p><ul><li>Useful item</li></ul>';
  await current.page.evaluate(async html => navigator.clipboard.write([new ClipboardItem({ "text/html": new Blob([html], { type: "text/html" }), "text/plain": new Blob(["Sample safe paste size bad link source\nUseful item"], { type: "text/plain" }) })]), html);
  await current.page.locator("#jig-draft-editor").click(); await current.page.keyboard.press("Control+v");
  const pasted = await saved(current.page, value => textOf(value.draft).includes("Useful item"), "Rich paste must save safe text and structure");
  assert.ok(runsOf(pasted.draft).some(run => run.bold && run.text.includes("safe paste")));
  assert.ok(runsOf(pasted.draft).some(run => run.link === "https://example.com/source"));
  assert.ok(pasted.draft.blocks.some(block => block.list === "bullet"));
  assert.ok(runsOf(pasted.draft).every(run => !run.link || /^(https?:\/\/|mailto:)/i.test(run.link)));
  assert.equal(await current.page.evaluate(() => window.draftExecuted), undefined);
  assert.equal(textOf(pasted.draft).includes("draftExecuted"), false, "Pasted script bodies must not become editor content");
  assert.equal(await current.page.locator("#jig-draft-editor img, #jig-draft-editor script, #jig-draft-editor iframe").count(), 0);
  assert.deepEqual(withoutDraft(pasted), fixture); await current.context.close();

  progress("recover the latest text across an immediate refresh and a failed main save");
  current = await open("draft-recovery");
  const refreshText = "Sample: café notes keep the last keystroke through refresh.";
  await current.page.locator("#jig-draft-editor").click(); await current.page.keyboard.insertText(refreshText);
  await boot(current.page, true);
  await saved(current.page, value => textOf(value.draft) === refreshText, "Immediate refresh must retain the latest text");
  const beforeFailure = await state(current.page);
  await current.page.evaluate(() => { window.failDraftSave = true; });
  await current.page.locator("#jig-draft-editor").focus(); await current.page.keyboard.press("Control+End");
  await current.page.keyboard.insertText(" Sample: recover even when saving fails.");
  await current.page.waitForFunction(() => window.draftFailedSaves > 0
    && Object.values(sessionStorage).some(value => value.includes("recover even when saving fails")));
  await expectState(current.page, beforeFailure, "A failed save must not overwrite the existing stored document");
  await boot(current.page, true);
  await saved(current.page, value => textOf(value.draft) === refreshText + " Sample: recover even when saving fails.", "Tab-local recovery must survive a main storage failure and save after reload");
  assert.deepEqual(withoutDraft(await state(current.page)), fixture); await current.context.close();

  progress("preserve an unreadable recovery journal and the current saved document");
  const unreadable = '{"version":1,"draft":';
  current = await open("draft-malformed-recovery", fixture, false, false, unreadable);
  await current.page.locator("#jig-draft-panel [role=status]").waitFor({ state: "visible" });
  const afterRecoveryPaint = await current.page.evaluate(() => window.jigTestFrames.paints);
  await current.page.waitForFunction(after => window.jigTestFrames.paints > after, afterRecoveryPaint);
  assert.equal(await current.page.evaluate(() => sessionStorage.getItem("jig.draft.recovery.v1")), unreadable,
    "An unreadable recovery copy must never be erased or rewritten by initial synchronization");
  await expectState(current.page, fixture, "Unreadable recovery must leave the saved document intact");
  await current.context.close();

  progress("use Draft on a light-theme phone without clipping the toolbar or editor");
  current = await open("draft-mobile", portable, true, true);
  const panel = current.page.locator("#jig-draft-panel"), phoneEditor = current.page.locator("#jig-draft-editor");
  const box = await panel.boundingBox(), field = await phoneEditor.boundingBox();
  assert.ok(box && field && box.x >= 0 && box.x + box.width <= 391 && field.width > 200 && field.height > 200);
  assert.equal(await current.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true);
  assert.equal(await panel.getAttribute("data-theme"), "light");
  assert.equal(await phoneEditor.evaluate(node => getComputedStyle(node).color), "rgb(49, 43, 39)",
    "The native editor must inherit the same warm Light theme as the rest of Jig");
  await phoneEditor.tap(); await current.page.keyboard.press("Control+End"); await current.page.keyboard.insertText(" Sample: phone edit.");
  const phoneSaved = await saved(current.page, value => textOf(value.draft).includes("Sample: phone edit."), "Touch-focused text must save");
  await selectAll(current.page); await current.page.keyboard.press("ArrowRight"); await capture(current.page, "draft-mobile");
  await boot(current.page, true); await expectState(current.page, phoneSaved, "Phone reload must retain formatting and text");
  assert.deepEqual(issues, [], "Draft must not execute pasted content, issue external requests, or raise page errors");
  console.log("Draft browser checks passed: older saved files, typed formatting/size/alignment/lists/links, rich copy, selected text to Notes/Tasks, shared Undo/Redo across workspaces, refresh and failed-save recovery, malformed-journal preservation, JSON/ODS round trips, safe rich paste without network requests, and light-theme phone layout/touch editing.");
} catch (error) {
  console.error(`Draft browser failed during: ${phase}`, error);
  if (current?.page && !current.page.isClosed()) {
    console.error("Draft browser diagnostics:", await current.page.evaluate(key => ({ saved: localStorage.getItem(key), workspace: localStorage.getItem("jig.workspace.v1"), active: document.activeElement?.id, html: document.getElementById("jig-draft-editor")?.innerHTML, frames: window.jigTestFrames }), storageKey).catch(String));
    await capture(current.page, "draft-failure").catch(() => {});
  }
  throw error;
} finally {
  let cleanupTimer;
  try {
    await Promise.race([
      Promise.all([...contexts.map(context => context.close().catch(() => {})), browser.close()]),
      new Promise((_, reject) => { cleanupTimer = setTimeout(() => reject(new Error("Draft browser shutdown exceeded 10 seconds")), 10000); }),
    ]);
  } finally { clearTimeout(cleanupTimer); clearTimeout(timer); }
}

// Real Chromium checks for the shared plain-text note board. All data is synthetic.
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
  version: 1, preferences: { decimals: 2, focus: 3, weather: null },
  history: [], tasks: [], deadlines: [], entries: [], featured: null,
  notes: [
    { id: 401, text: "Sample: clay tests\nRecord fired measurements", pinned: false },
    { id: 402, text: "Sample: exhibition\nPhotograph the installation", pinned: false },
  ],
};
const fullBody = "\n  Sample: fired test\nMeasure after cone 6\nKeep the full result\n";
const taskFixture = {
  ...structuredClone(fixture), notes: undefined,
  task_lists: [{ id: 1, name: "Studio" }, { id: 2, name: "Show" }],
  tasks: [
    { id: 1, text: "Sample: fired test", details: fullBody, complete: true, deadline: 101, start: "2026-10-26T12:34:56Z", color: "rose" },
    { id: 2, text: "Sample: exhibition opening", complete: false, list_id: 2, deadline: 102, color: "sage" },
  ],
  deadlines: [
    { id: 101, title: "Sample: fired test", at: "2026-10-30T18:00:00Z" },
    { id: 102, title: "Sample: exhibition opening", at: "2026-11-08T18:00:00Z" },
  ],
  featured: 102,
};
delete taskFixture.notes;
const issues = [], contexts = [];
const headerTargets = 6; // About, Data, Settings, Help, Calculator, Source.
let current, phase = "launch Chromium";
const timer = setTimeout(() => { console.error(`Notes browser exceeded 360 seconds during: ${phase}`); process.exit(1); }, 360000);
function progress(message) { phase = message; console.log(`Notes browser phase: ${message}`); }
async function paint(page) { await waitForAppPaint(page); }
async function key(page, value) { await page.keyboard.press(value); await paint(page); }
async function tab(page, count = 1) { for (let index = 0; index < count; index++) await key(page, "Tab"); }
async function canvasFocus(page) {
  const viewport = page.viewportSize();
  await page.locator("#jig_canvas").click({ position: { x: viewport.width - 6, y: viewport.height - 6 } });
}
async function state(page) { return page.evaluate(key => JSON.parse(localStorage.getItem(key)), storageKey); }
async function expectState(page, expected, message) {
  const until = Date.now() + 10000; let actual;
  do {
    actual = await state(page);
    try { assert.deepEqual(actual, expected); return; } catch { await page.waitForTimeout(50); }
  } while (Date.now() < until);
  assert.deepEqual(actual, expected, message);
}
async function expectNoNoteDraft(page) {
  await page.waitForFunction(() => !JSON.parse(sessionStorage.getItem("jig.drafts.v1"))?.note,
    undefined, { timeout: 10000, polling: 25 });
}
async function capture(page, name) {
  const png = await page.screenshot({ path: resolve(results, `${name}.png`), timeout: 60000, animations: "disabled", caret: "hide" });
  if (new Set((process.env.JIG_RENDER_LOGS ?? "").split(",")).has(name) && png.length <= 1024 * 1024) {
    console.log(`JIG_RENDER_BEGIN_${name}`);
    const encoded = png.toString("base64");
    for (let offset = 0; offset < encoded.length; offset += 2048) console.log(encoded.slice(offset, offset + 2048));
    console.log(`JIG_RENDER_END_${name}`);
  }
  return png;
}
const browser = await chromium.launch({ headless: true, args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"] });
async function open(label, initial = fixture, phone = false, launchNotes = true, hasTouch = phone) {
  const context = await browser.newContext({ viewport: phone ? { width: 390, height: 844 } : { width: 1440, height: 1000 }, hasTouch, acceptDownloads: true, timezoneId: "Etc/UTC" });
  contexts.push(context);
  await installFrameFence(context);
  await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin });
  await context.addInitScript(() => {
    window.jigNotesInputEvents = [];
    for (const type of ["keydown", "keyup", "beforeinput", "input", "pointerdown", "pointerup", "focusin", "focusout"]) {
      document.addEventListener(type, event => {
        window.jigNotesInputEvents.push({ type, key: event.key ?? null, data: event.data ?? null,
          inputType: event.inputType ?? null, target: event.target?.tagName ?? null,
          id: event.target?.id ?? null, time: Math.round(performance.now()) });
        if (window.jigNotesInputEvents.length > 32) window.jigNotesInputEvents.shift();
      }, true);
    }
  });
  await context.addInitScript(({ raw, key, origin }) => {
    const NativeDate = Date, fixed = NativeDate.parse("2026-10-30T12:00:00Z"), now = () => fixed;
    globalThis.Date = new Proxy(NativeDate, {
      apply: () => new NativeDate(fixed).toString(),
      construct: (target, args, newTarget) => Reflect.construct(target, args.length ? args : [fixed], newTarget),
      get: (target, property, receiver) => property === "now" ? now : Reflect.get(target, property, receiver),
    });
    if (location.origin === origin && sessionStorage.getItem("jig-notes-seeded") !== "yes") {
      if (raw !== null) localStorage.setItem(key, raw);
      sessionStorage.setItem("jig-notes-seeded", "yes");
    }
  }, { raw: initial === null ? null : JSON.stringify(initial), key: storageKey, origin });
  await context.route("**/*", async route => {
    const url = new URL(route.request().url());
    if (["blob:", "data:", "about:"].includes(url.protocol) || url.origin === origin) await route.continue();
    else { issues.push(`${label}: unexpected request ${url.origin}${url.pathname}`); await route.abort(); }
  });
  const page = await context.newPage();
  page.on("pageerror", error => issues.push(`${label}: ${error.message}`));
  page.on("dialog", async dialog => {
    assert.equal(dialog.type(), "beforeunload", "Only the expected unsaved-draft refresh warning may open a browser dialog");
    await dialog.accept();
  });
  page.setDefaultTimeout(15000);
  const scene = { context, page };
  await boot(scene, false, launchNotes); return scene;
}
async function boot(scene, reload = false, launchNotes = true) {
  if (reload) await scene.page.reload({ waitUntil: "load" }); else await scene.page.goto(base, { waitUntil: "load" });
  await scene.page.bringToFront();
  await scene.page.waitForFunction(() => window.jigReady === true || typeof window.jigBootError === "string", undefined, { timeout: 60000 });
  assert.equal(await scene.page.evaluate(() => window.jigBootError), undefined);
  await scene.page.waitForTimeout(500);
  if (launchNotes) await notes(scene);
}
async function notes(scene) {
  await canvasFocus(scene.page);
  await key(scene.page, "Escape"); await key(scene.page, "Control+Alt+n");
}
async function focusNote(scene, index, control) {
  await notes(scene);
  await tab(scene.page, headerTargets + 1 + index * 6 + control); // Add, then grip/pin/copy/task/del/body.
}
async function typeBody(page, text) {
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 25 });
  await page.keyboard.press("Control+a"); await page.keyboard.press("Backspace");
  for (const [index, line] of text.split("\n").entries()) {
    if (index) await key(page, "Enter");
    if (line) await page.keyboard.type(line, { delay: 10 });
    await paint(page);
  }
}
async function typeTaskDetails(page, text) {
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 25 });
  await page.keyboard.press("Control+a"); await page.keyboard.press("Backspace");
  for (const [index, line] of text.split("\n").entries()) {
    if (index) await key(page, "Shift+Enter");
    if (line) await page.keyboard.type(line, { delay: 10 });
    await paint(page);
  }
}
async function expectClipboard(page, expected) {
  const until = Date.now() + 10000;
  let actual;
  do {
    actual = await page.evaluate(() => navigator.clipboard.readText());
    if (actual === expected) return;
    await page.waitForTimeout(50);
  } while (Date.now() < until);
  assert.equal(actual, expected, "The real clipboard must contain the complete expected text");
}
async function expectEditorText(page, text) {
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 25 });
  await page.evaluate(() => navigator.clipboard.writeText("Working editor read pending"));
  await key(page, "Control+a"); await key(page, "Control+c");
  await expectClipboard(page, text);
}
async function attemptOpenLocal(scene) {
  // Data follows About in this known desktop viewport.
  // An existing Task editor treats this pointer click as an explicit cancellation.
  await scene.page.mouse.click(80, 14); await paint(scene.page);
  await tab(scene.page, 5); // Settings, Help, Calculator, Start empty, Open local.
  await key(scene.page, "Space"); await key(scene.page, "Control+Alt+d");
}
async function focusedButtonRect(page) {
  const png = await page.screenshot({ timeout: 60000, animations: "disabled", caret: "hide" });
  const rect = await page.evaluate(async encoded => {
    const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
    const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height), ctx = canvas.getContext("2d"); ctx.drawImage(bitmap, 0, 0);
    const pixels = ctx.getImageData(0, 0, bitmap.width, bitmap.height).data, seen = new Uint8Array(bitmap.width * bitmap.height), regions = [];
    const match = index => [77, 49, 39].every((value, axis) => Math.abs(pixels[index * 4 + axis] - value) <= 3);
    for (let y = 42; y < Math.min(700, bitmap.height); y++) for (let x = 5; x < bitmap.width - 5; x++) {
      const start = y * bitmap.width + x;
      if (seen[start] || !match(start)) continue;
      const queue = [start], r = { left: x, right: x, top: y, bottom: y, count: 0 }; seen[start] = 1;
      while (queue.length) {
        const p = queue.pop(), px = p % bitmap.width, py = Math.floor(p / bitmap.width);
        r.left = Math.min(r.left, px); r.right = Math.max(r.right, px); r.top = Math.min(r.top, py); r.bottom = Math.max(r.bottom, py); r.count++;
        for (const [nx, ny] of [[px - 1, py], [px + 1, py], [px, py - 1], [px, py + 1]]) {
          if (nx < 0 || nx >= bitmap.width || ny < 42 || ny >= Math.min(700, bitmap.height)) continue;
          const next = ny * bitmap.width + nx;
          if (!seen[next] && match(next)) { seen[next] = 1; queue.push(next); }
        }
      }
      if (r.count >= 40 && r.bottom - r.top >= 8 && r.bottom - r.top <= 35 && r.right - r.left < 180) regions.push(r);
    }
    const r = regions.sort((a, b) => a.top - b.top || b.count - a.count)[0] ?? null;
    if (r) {
      // A wrapped task row extends below its star. Its left graphite gutter
      // exposes the complete row height without depending on toolbar wrapping.
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
    bitmap.close(); return r;
  }, png.toString("base64"));
  assert.ok(rect, "The keyboard-focused note control must be visible for the real pointer interaction");
  return { ...rect, x: (rect.left + rect.right) / 2, y: (rect.top + rect.bottom) / 2 };
}
async function grip(scene, index) {
  await focusNote(scene, index, 2);
  const pin = await focusedButtonRect(scene.page);
  return { x: pin.left - 12, y: pin.y }; // Visible 18px grip and 3px gap immediately before pin.
}
async function assertReadableNoteBody(scene) {
  await focusNote(scene, 0, 2);
  const pin = await focusedButtonRect(scene.page);
  const png = await scene.page.screenshot({ timeout: 60000, animations: "disabled", caret: "hide" });
  const body = await scene.page.evaluate(async ({ encoded, pin }) => {
    const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
    const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height), ctx = canvas.getContext("2d"); ctx.drawImage(bitmap, 0, 0);
    const pixels = ctx.getImageData(0, 0, bitmap.width, bitmap.height).data;
    const cardLeft = pin.left - 27;
    let best = { span: 0, ink: 0 };
    // Inspect saved text beneath the toolbar, rather than counting toolbar
    // labels. A body squeezed into one vertical character column cannot pass.
    for (let y = Math.ceil(pin.y + 36); y < Math.min(bitmap.height, pin.y + 150); y++) {
      let left = bitmap.width, right = 0, ink = 0;
      for (let x = Math.ceil(cardLeft + 5); x < cardLeft + 145; x++) {
        const offset = (y * bitmap.width + x) * 4;
        if ([230, 225, 219].every((value, axis) => Math.abs(pixels[offset + axis] - value) <= 8)) {
          left = Math.min(left, x); right = x; ink++;
        }
      }
      // The 12px mono glyphs are heavily antialiased: require several solid
      // glyph samples across a real line, rather than an arbitrary ink density.
      if (ink >= 4 && right - left > best.span) best = { span: right - left, ink };
    }
    bitmap.close(); return best;
  }, { encoded: png.toString("base64"), pin });
  assert.ok(body.span >= 70, `A saved note must use readable horizontal text across its card body below the toolbar; found ${JSON.stringify(body)}`);
}
async function undo(scene, expected) {
  await notes(scene); await key(scene.page, "Control+z");
  await expectState(scene.page, expected, "One Undo must restore the complete note/task operation");
}
async function exportData(scene) {
  await notes(scene); await key(scene.page, "Control+Alt+d"); await tab(scene.page, 9);
  const event = scene.page.waitForEvent("download"); await key(scene.page, "Space");
  const download = await event; assert.equal(download.suggestedFilename(), "jig.json");
  const result = JSON.parse(await readFile(await download.path(), "utf8"));
  await key(scene.page, "Control+Alt+d"); return result;
}
async function taskGanttButtonPoint(page, control = "gantt") {
  const until = Date.now() + 10000; let point;
  do {
    const png = await page.screenshot({ timeout: 60000, animations: "disabled", caret: "hide" });
    point = await page.evaluate(async ({ encoded, control }) => {
      const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
      const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
      const canvas = new OffscreenCanvas(bitmap.width, bitmap.height), ctx = canvas.getContext("2d"); ctx.drawImage(bitmap, 0, 0);
      const pixels = ctx.getImageData(0, 0, bitmap.width, bitmap.height).data, runs = [];
      const rule = (x, y) => [113, 102, 93].every((value, axis) => Math.abs(pixels[(y * bitmap.width + x) * 4 + axis] - value) <= 3);
      // The Gantt toolbar is directly beneath the task-list strip. Find its
      // actual button frame rather than counting optional scroll-area tab stops.
      const input = control !== "gantt", details = control === "details", right = input ? Math.min(Math.floor(bitmap.width * 0.46), 640) : 150;
      for (let y = input ? 110 : 60; y < (input ? 540 : 180); y++) {
        let start;
        for (let x = 5; x <= right; x++) {
          if (rule(x, y) && start === undefined) start = x;
          if ((!rule(x, y) || x === right) && start !== undefined) {
            const end = x - 1;
            if (start <= (input ? 30 : 20) && end - start >= (input ? 150 : 70) && end - start <= (input ? 640 : 120)) runs.push({ left: start, right: end, y });
            start = undefined;
          }
        }
      }
      bitmap.close();
      for (const top of runs) for (const bottom of runs) {
        if (bottom.y - top.y >= (details ? 35 : 12) && bottom.y - top.y <= (details ? 180 : input ? 30 : 22) && Math.abs(top.left - bottom.left) <= 2 && Math.abs(top.right - bottom.right) <= 2)
          return { x: (top.left + top.right) / 2, y: (top.y + bottom.y) / 2 };
      }
      return null;
    }, { encoded: png.toString("base64"), control });
  } while (!point && Date.now() < until);
  assert.ok(point, `The Tasks workspace must visibly expose its ${control === "gantt" ? "Gantt chart button" : control === "details" ? "task details editor" : "task input"}`);
  return point;
}

async function taskToNote(scene, listCount, complete) {
  await canvasFocus(scene.page);
  await key(scene.page, "Escape"); await key(scene.page, "Control+Alt+2");
  const strip = listCount + (listCount > 1 ? 3 : 1);
  await tab(scene.page, headerTargets + strip + 2 + (complete ? 5 : 6));
  await key(scene.page, "Space"); // Dated first row's Edit opens the unified Name field.
  await tab(scene.page, (listCount > 1 ? listCount : 0) + (complete ? 9 : 10)); // Due/picker, Start/picker, Remove, Details, lists, optional Featured, Save, Cancel, To note.
  await key(scene.page, "Space");
}

async function noteCardFrames(page, minimumY = 80) {
  // Read the saved card's ordinary graphite frame in the shared Tasks board.
  // The Tasks panel rules use a different color; no widget IDs are exposed.
  const png = await page.screenshot({ timeout: 60000, animations: "disabled", caret: "hide" });
  return page.evaluate(async ({ encoded, minimumY }) => {
    const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
    const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height), ctx = canvas.getContext("2d"); ctx.drawImage(bitmap, 0, 0);
    const pixels = ctx.getImageData(0, 0, bitmap.width, bitmap.height).data;
    const edge = (x, y) => [80, 72, 66].every((value, axis) => Math.abs(pixels[(y * bitmap.width + x) * 4 + axis] - value) <= 2);
    const cards = [];
    for (let y = minimumY; y < bitmap.height - 20; y++) {
      for (let x = 5; x < Math.min(220, bitmap.width - 150); x++) {
        if (edge(x, y) && edge(x, y + 2) && Array.from({ length: 130 }, (_, offset) => x + offset).every(px => edge(px, y))) {
          cards.push({ left: x, top: y }); break;
        }
      }
    }
    bitmap.close(); return cards;
  }, { encoded: png.toString("base64"), minimumY });
}
async function sharedNotePoints(page) {
  const [card] = await noteCardFrames(page, 250);
  assert.ok(card, "The shared Tasks board must expose the saved note card for a real pointer drop");
  return { body: { x: card.left + 45, y: card.top + 85 }, grip: { x: card.left + 16, y: card.top + 16 } };
}

try {
  progress("start with real personal Tasks, save a plan, and recover work after refresh");
  current = await open("notes-first-use", null, false, false);
  assert.equal(await current.page.evaluate(key => localStorage.getItem(key), storageKey), null,
    "A fresh personal launch may remain empty until the first real edit");
  await taskGanttButtonPoint(current.page); // No preference fixture: Tasks is the actual first-use workspace.
  const freshInput = await taskGanttButtonPoint(current.page, "input");
  await current.page.mouse.click(freshInput.x, freshInput.y); await paint(current.page);
  await typeBody(current.page, "Sample: plan exhibition"); await key(current.page, "Enter");
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key))?.tasks.length === 1, storageKey);
  const firstTask = (await state(current.page)).tasks[0];
  assert.equal(firstTask.text, "Sample: plan exhibition", "First-use work must save as personal data");
  await canvasFocus(current.page);
  await key(current.page, "Escape"); await key(current.page, "Control+Alt+2");
  await tab(current.page, headerTargets + 8); // Header, tabs2, commands2, star/checkbox/×/date.
  await key(current.page, "Space");
  await typeBody(current.page, "2026-11-02"); await tab(current.page, 2); // Due picker, then Start.
  await typeBody(current.page, "2026-10-30"); await key(current.page, "Enter");
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).tasks[0].start != null, storageKey);
  const plan = await state(current.page);
  assert.equal(plan.deadlines.length, 1); assert.equal(plan.tasks[0].deadline, plan.deadlines[0].id);
  await boot(current, true, false);
  await taskGanttButtonPoint(current.page);
  await expectState(current.page, plan, "A saved task and its Gantt dates must survive an ordinary Tasks refresh");

  progress("recover an unfinished Start/Due form, cancel it, then explicitly save the recovered range");
  for (const cancel of [true, false]) {
    await canvasFocus(current.page);
    await key(current.page, "Escape"); await key(current.page, "Control+Alt+2");
    await tab(current.page, headerTargets + 9); await key(current.page, "Space");
    await typeBody(current.page, "2026-11-05"); await tab(current.page, 2);
    await typeBody(current.page, "2026-11-01");
    await expectState(current.page, plan, "Changing a date form must not partially change the saved schedule");
    await boot(current, true, false);
    await expectEditorText(current.page, "2026-11-05"); await tab(current.page, 2);
    await expectEditorText(current.page, "2026-11-01");
    await expectState(current.page, plan, "Refresh must restore both unfinished date fields without committing them");
    if (cancel) {
      await key(current.page, "Escape"); // Cancel the entire recovered editor, including its dates.
      await expectState(current.page, plan, "Cancelling a recovered date form must preserve the original dates");
    } else {
      await key(current.page, "Enter");
      await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).deadlines[0].at.startsWith("2026-11-05"), storageKey);
      const rescheduled = await state(current.page);
      assert.ok(rescheduled.tasks[0].start.startsWith("2026-11-01"));
      assert.equal(rescheduled.tasks[0].id, plan.tasks[0].id);
      assert.equal(rescheduled.tasks[0].deadline, plan.tasks[0].deadline);
      await undo(current, plan);
    }
  }

  progress("cancel unfinished dates on a color click, then recover a fresh editor and reject external conflicts");
  const firstUse = current;
  current = await open("notes-date-metadata-recovery", plan, false, false);
  await canvasFocus(current.page); await key(current.page, "Escape"); await key(current.page, "Control+Alt+2");
  await tab(current.page, headerTargets + 8); // Dated row's color control, after the boxed ×.
  const colorPoint = await focusedButtonRect(current.page);
  await tab(current.page); await key(current.page, "Space"); // Date opens the unified editor on Due.
  await typeBody(current.page, "2026-11-06");
  await current.page.mouse.click(colorPoint.x, colorPoint.y); await paint(current.page);
  await expectState(current.page, plan, "Clicking a row control outside the editor must cancel its unfinished date without saving");
  await current.page.waitForFunction(() => !JSON.parse(sessionStorage.getItem("jig.drafts.v1"))?.schedule);
  // The dismissed field owned keyboard focus. Re-enter the color control
  // through the normal Tasks tab order rather than assuming a pointer click
  // transfers egui keyboard focus from an editor that has just disappeared.
  await canvasFocus(current.page); await key(current.page, "Escape"); await key(current.page, "Control+Alt+2");
  await tab(current.page, headerTargets + 8); await key(current.page, "Space");
  await tab(current.page, 4); await key(current.page, "Space"); // date, Edit, None, Rose.
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).tasks[0].color === "rose", storageKey);
  const recolored = structuredClone(plan); recolored.tasks[0].color = "rose";
  await expectState(current.page, recolored, "A color change must retain the committed dates and every other record");
  await canvasFocus(current.page); await key(current.page, "Escape"); await key(current.page, "Control+Alt+2");
  await tab(current.page, headerTargets + 9); await key(current.page, "Space");
  await typeBody(current.page, "2026-11-06");
  await current.page.waitForFunction(() => JSON.parse(sessionStorage.getItem("jig.drafts.v1"))?.schedule?.due === "2026-11-06");
  await boot(current, true, false);
  await expectEditorText(current.page, "2026-11-06");
  await expectState(current.page, recolored, "Refresh must restore unfinished dates while preserving the committed color");

  const external = structuredClone(recolored);
  external.tasks[0].text += " (changed elsewhere)";
  external.deadlines[0].title = external.tasks[0].text;
  const preservedDraft = await current.page.evaluate(() => sessionStorage.getItem("jig.drafts.v1"));
  await current.page.evaluate(({ key, raw }) => localStorage.setItem(key, raw), { key: storageKey, raw: JSON.stringify(external) });
  await boot(current, true, false);
  assert.notEqual(await current.page.evaluate(() => document.activeElement?.tagName), "INPUT",
    "A date draft belonging to an externally changed task must not reopen against the new record");
  await expectState(current.page, external, "Draft recovery must leave the externally updated task and deadline intact");
  assert.equal(await current.page.evaluate(() => sessionStorage.getItem("jig.drafts.v1")), preservedDraft,
    "A conflicting date draft must remain available for recovery without overwriting its raw backup");
  await current.context.close(); current = firstUse; await current.page.bringToFront();

  await key(current.page, "Control+Alt+n"); await tab(current.page, headerTargets + 1); await key(current.page, "Space");
  await typeBody(current.page, "Sample: installation reminder\nBring measuring tape");
  await key(current.page, "Control+Enter");
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).notes?.length === 1, storageKey);
  const savedNote = await state(current.page);
  assert.equal(await current.page.evaluate(() => localStorage.getItem("jig.workspace.v1")), "notes");
  await boot(current, true, false); // Do not reopen Notes: the remembered workspace must do that.
  await expectState(current.page, savedNote, "Committed Tasks and Notes must reload together");
  await tab(current.page, headerTargets + 1); await key(current.page, "Space");
  const recoveredBody = "Sample: unfinished reminder\nMeasure twice before packing";
  await typeBody(current.page, recoveredBody);
  await expectState(current.page, savedNote, "An unfinished note must remain a draft until saved");
  assert.ok(await current.page.evaluate(() => sessionStorage.getItem("jig.drafts.v1")?.includes("Measure twice")),
    "The current tab must retain its unfinished note before refresh");
  await boot(current, true, false);
  await current.page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 25 });
  await current.page.evaluate(() => navigator.clipboard.writeText("Draft recovery pending"));
  await key(current.page, "Control+a"); await key(current.page, "Control+c");
  await expectClipboard(current.page, recoveredBody);
  await key(current.page, "Control+Enter");
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).notes.length === 2, storageKey);
  const recoveredNotes = await state(current.page);
  await canvasFocus(current.page);
  await key(current.page, "Control+Alt+2");
  const taskInput = await taskGanttButtonPoint(current.page, "input");
  await current.page.mouse.click(taskInput.x, taskInput.y); await paint(current.page);
  const unsent = "Sample: unfinished studio task Z";
  await typeBody(current.page, unsent.slice(0, -1));
  await current.page.keyboard.type("Z");
  await current.page.keyboard.press("Control+Alt+n"); // No paint: the old Task field must consume its last character first.
  await paint(current.page);
  await current.page.waitForFunction(() => localStorage.getItem("jig.workspace.v1") === "notes", undefined, { timeout: 10000 });
  assert.equal(await current.page.evaluate(() => JSON.parse(sessionStorage.getItem("jig.drafts.v1")).task.text), unsent);
  await key(current.page, "Control+Alt+2");
  if (!await current.page.evaluate(() => document.activeElement?.tagName === "INPUT")) {
    const input = await taskGanttButtonPoint(current.page, "input");
    await current.page.mouse.click(input.x, input.y); await paint(current.page);
  }
  await expectEditorText(current.page, unsent);
  await expectState(current.page, recoveredNotes, "Typing a new task must not create a partial checklist record");
  await key(current.page, "Control+Alt+o");
  await expectEditorText(current.page, unsent);
  await attemptOpenLocal(current);
  const guardedInput = await taskGanttButtonPoint(current.page, "input");
  await current.page.mouse.click(guardedInput.x, guardedInput.y); await paint(current.page);
  await expectEditorText(current.page, unsent);
  await expectState(current.page, recoveredNotes, "Keyboard and Data-menu Open local must preserve unsent task text and saved records");
  assert.equal(await current.page.evaluate(() => JSON.parse(sessionStorage.getItem("jig.drafts.v1")).task.text), unsent);
  await boot(current, true, false);
  await current.page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 25 });
  await current.page.evaluate(() => navigator.clipboard.writeText("Task draft recovery pending"));
  await key(current.page, "Control+a"); await key(current.page, "Control+c");
  await expectClipboard(current.page, unsent);
  await key(current.page, "Enter");
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).tasks.length === 2, storageKey);
  const kept = await state(current.page);
  assert.equal(kept.tasks[1].text, unsent); assert.deepEqual(kept.notes, recoveredNotes.notes);
  assert.deepEqual(kept.deadlines, plan.deadlines);
  await capture(current.page, "notes-recovered");
  assert.deepEqual(await exportData(current), kept, "A portable JSON backup must contain the recovered tasks, dates and notes");
  await current.context.close();

  progress("ignore legacy temporary drafts without changing the saved personal notes");
  current = await open("notes-legacy-temporary-draft");
  await tab(current.page, headerTargets + 1); await key(current.page, "Space");
  await typeBody(current.page, "Sample: legacy temporary draft must remain separate");
  await current.page.waitForFunction(() => sessionStorage.getItem("jig.drafts.v1")?.includes("legacy temporary draft"));
  await current.page.evaluate(() => {
    const draft = JSON.parse(sessionStorage.getItem("jig.drafts.v1"));
    draft.kind = "demo";
    sessionStorage.setItem("jig.drafts.v1", JSON.stringify(draft));
  });
  await boot(current, true, false);
  assert.notEqual(await current.page.evaluate(() => document.activeElement?.tagName), "INPUT",
    "An old temporary draft must not reopen as a personal editor");
  await expectState(current.page, fixture, "Ignoring a legacy temporary draft must preserve every saved personal record");
  assert.equal(await current.page.evaluate(key => localStorage.getItem(key), storageKey), JSON.stringify(fixture),
    "Loading an obsolete temporary draft must not rewrite the personal file");
  await current.context.close();

  progress("create and edit multiline text without a separate title or premature save");
  current = await open("notes-desktop");
  await tab(current.page, headerTargets + 1); const add = await focusedButtonRect(current.page);
  await key(current.page, "Space");
  const body = "Sample: shelf tests\nStandard 104\nMeasure after firing";
  await typeBody(current.page, body);
  await expectState(current.page, fixture, "Plain Enter must add note lines without saving an unfinished draft");
  await key(current.page, "Control+Enter");
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).notes.length === 3, storageKey);
  const added = await state(current.page);
  assert.equal(added.notes[2].text, body); assert.equal(added.notes[2].pinned, false);
  await focusNote(current, 0, 6); await key(current.page, "Space");
  const updated = "Sample: clay tests\nKeep the full multiline result\nNo title field\nOne\nTwo\nThree\nFour\nFive\nSix\nEnd of full note\n"
    + "Ω glazing record ".repeat(40) + "\nTAIL Ω: the complete note remains available.";
  await typeBody(current.page, updated); await key(current.page, "Control+Enter");
  const edited = structuredClone(added); edited.notes[0].text = updated;
  await expectState(current.page, edited, "Editing a note must preserve its ID and every other record");

  progress("copy the full clipped body and explicitly save the final queued character");
  await current.page.evaluate(() => navigator.clipboard.writeText("Notes copy pending"));
  await focusNote(current, 0, 3); await key(current.page, "Space");
  await expectClipboard(current.page, updated);
  await expectState(current.page, edited, "Copy must preserve the note and all other records");
  await focusNote(current, 0, 6); await key(current.page, "Space");
  const quickBody = updated + "\nFinal character: ";
  const beforeFinalSave = structuredClone(edited);
  await typeBody(current.page, quickBody);
  await current.page.keyboard.type("Z");
  await current.page.keyboard.press("Control+Enter"); // Intentionally no paint after the final character.
  await paint(current.page);
  edited.notes[0].text = quickBody + "Z";
  await expectState(current.page, edited, "Explicit Ctrl-Enter must save the full note, including the final queued character");
  await expectNoNoteDraft(current.page);

  progress("cancel an outside-click edit without changing saved text or the existing Undo/Redo history");
  await undo(current, beforeFinalSave);
  await focusNote(current, 0, 6); await key(current.page, "Space");
  await typeBody(current.page, "Sample: discard this existing-note draft");
  const blankNotes = { x: current.page.viewportSize().width - 30, y: 400 };
  await current.page.mouse.click(blankNotes.x, blankNotes.y); await paint(current.page);
  await expectState(current.page, beforeFinalSave, "Clicking blank board space must cancel the edit and retain the saved note");
  await expectNoNoteDraft(current.page);
  await key(current.page, "Control+Shift+z");
  await expectState(current.page, edited, "Cancelling an edit must retain the Redo of the preceding explicit Save");
  await key(current.page, "Control+z");
  await expectState(current.page, beforeFinalSave, "Cancellation must create no Undo entry above the real Save");
  await key(current.page, "Control+Shift+z"); await expectState(current.page, edited);

  progress("cancel a pending note edit when immediately changing workspace");
  await focusNote(current, 0, 6); await key(current.page, "Space");
  await typeBody(current.page, "Sample: discard on workspace switch ");
  await current.page.keyboard.type("Z");
  await current.page.keyboard.press("Control+Alt+2"); // No paint: the queued character must not turn Cancel into Save.
  await paint(current.page);
  await expectState(current.page, edited, "A quick workspace change must cancel the note edit without committing any text");
  await expectNoNoteDraft(current.page);
  await taskGanttButtonPoint(current.page); // The actual Tasks toolbar proves the switch occurred.

  progress("cancel the old edit on Add note, then discard a new note by clicking outside");
  await notes(current); await tab(current.page, headerTargets + 1);
  const addAfterEdit = await focusedButtonRect(current.page);
  await focusNote(current, 0, 6); await key(current.page, "Space");
  const pointerBody = "Sample: discard on Add note Q";
  await typeBody(current.page, pointerBody.slice(0, -1));
  await current.page.keyboard.type("Q");
  await current.page.mouse.click(addAfterEdit.x, addAfterEdit.y); // No paint: cancel the old field before starting the new card.
  await paint(current.page);
  await expectState(current.page, edited, "Clicking Add note must discard the old draft and leave all saved notes intact");
  await expectNoNoteDraft(current.page); // The new editor is empty; no queued old character may leak into it.
  await typeBody(current.page, "Sample: new notes also require an explicit Save");
  await current.page.waitForFunction(() => JSON.parse(sessionStorage.getItem("jig.drafts.v1"))?.note?.id === null);
  await current.page.mouse.click(blankNotes.x, blankNotes.y); await paint(current.page);
  await expectState(current.page, edited, "Clicking outside a new unsaved note must discard it without allocating a saved record");
  await expectNoNoteDraft(current.page);

  progress("cancel the current draft when clicking another note, while retaining editor-local Copy and Save");
  const otherGrip = await grip(current, 1);
  const otherBody = { x: otherGrip.x + 40, y: otherGrip.y + 65 };
  await focusNote(current, 0, 6); await key(current.page, "Space");
  await typeBody(current.page, "Sample: discard before opening another note");
  await current.page.mouse.click(otherBody.x, otherBody.y); await paint(current.page);
  await expectEditorText(current.page, edited.notes[1].text);
  await expectState(current.page, edited, "Clicking another note must open its saved text and discard the old edit");
  await expectNoNoteDraft(current.page); // The newly opened, unchanged editor has no recovery text.
  await key(current.page, "Escape");
  await focusNote(current, 0, 3); const copyInEditor = await focusedButtonRect(current.page);
  await focusNote(current, 0, 6); await key(current.page, "Space");
  const copyDraft = "Sample: Copy keeps this working note ";
  await typeBody(current.page, copyDraft);
  // The short draft can shrink the editor's inner scroller relative to its
  // original long body. Measure Save only after that layout has settled.
  await tab(current.page); const explicitSave = await focusedButtonRect(current.page);
  await key(current.page, "Shift+Tab");
  await current.page.evaluate(() => navigator.clipboard.writeText("Working-note Copy pending"));
  await current.page.mouse.click(copyInEditor.x, copyInEditor.y); await paint(current.page);
  await expectClipboard(current.page, copyDraft);
  await current.page.waitForFunction(text => JSON.parse(sessionStorage.getItem("jig.drafts.v1"))?.note?.text === text, copyDraft);
  await expectState(current.page, edited, "Copy inside the edited card must retain the draft without committing it");
  await current.page.mouse.click(explicitSave.x + 35, explicitSave.top - 35); await paint(current.page);
  await current.page.waitForFunction(() => document.activeElement?.tagName === "INPUT");
  await expectEditorText(current.page, copyDraft);
  await key(current.page, "Control+End"); // Refocusing by pointer can select a word; append at an explicit document end.
  await current.page.keyboard.type("Z");
  await current.page.mouse.click(explicitSave.x, explicitSave.y); // No paint: explicit Save must include the last character.
  await current.page.mouse.click(blankNotes.x, blankNotes.y); // A rapid later outside click must not cancel that Save.
  await paint(current.page);
  const pointerSaved = structuredClone(edited); pointerSaved.notes[0].text = copyDraft + "Z";
  await expectState(current.page, pointerSaved, "Explicit Save must persist the final queued character even when an outside click immediately follows it");
  await expectNoNoteDraft(current.page);
  await undo(current, edited);

  progress("explicitly save a note with Ctrl-S without dropping queued input");
  await focusNote(current, 0, 6); await key(current.page, "Space");
  const shortcutBody = "Sample: Ctrl-S explicitly saves this note ";
  await typeBody(current.page, shortcutBody);
  await current.page.keyboard.type("Z");
  await current.page.keyboard.press("Control+s"); await paint(current.page);
  const shortcutSaved = structuredClone(edited); shortcutSaved.notes[0].text = shortcutBody + "Z";
  await expectState(current.page, shortcutSaved, "Ctrl-S must explicitly save the complete note rather than merely closing it");
  await expectNoNoteDraft(current.page);
  await undo(current, edited);

  progress("cancel a settled note edit and clear its unfinished recovery text");
  await focusNote(current, 0, 6); await key(current.page, "Space");
  const cancelledBody = "Sample: Escape should discard this unfinished note edit.";
  await typeBody(current.page, cancelledBody);
  await current.page.waitForFunction(text => JSON.parse(sessionStorage.getItem("jig.drafts.v1"))?.note?.text === text, cancelledBody);
  await key(current.page, "Escape");
  await current.page.waitForFunction(() => !JSON.parse(sessionStorage.getItem("jig.drafts.v1"))?.note);
  await expectState(current.page, edited, "Escape must close the note edit, clear its recovery and preserve its saved body");

  progress("pin, keyboard reorder and drag notes as atomic operations");
  await focusNote(current, 1, 2); await key(current.page, "Space");
  const pinned = structuredClone(edited); const pin = pinned.notes.splice(1, 1)[0]; pin.pinned = true; pinned.notes.unshift(pin);
  await expectState(current.page, pinned, "Pinned notes must appear first without altering their body");
  await undo(current, edited);
  await focusNote(current, 0, 1); await key(current.page, "Alt+ArrowRight");
  const reordered = structuredClone(edited); [reordered.notes[0], reordered.notes[1]] = [reordered.notes[1], reordered.notes[0]];
  await expectState(current.page, reordered, "Alt+arrow on the visible grip must reorder within the unpinned group");
  await undo(current, edited);
  const from = await grip(current, 0), to = await grip(current, 1);
  await current.page.mouse.move(from.x, from.y); await current.page.mouse.down(); await paint(current.page);
  await current.page.mouse.move(to.x + 110, to.y + 55, { steps: 6 }); await paint(current.page);
  await expectState(current.page, edited, "A drag preview must not save before release");
  await current.page.mouse.up(); await paint(current.page);
  await expectState(current.page, reordered, "Dropping on a note must reorder the same records");
  await undo(current, edited);
  await focusNote(current, 2, 5); await key(current.page, "Space");
  const deleted = structuredClone(edited); deleted.notes.pop();
  await expectState(current.page, deleted, "Delete must remove only the chosen note");
  await undo(current, edited);
  await assertReadableNoteBody(current);
  await capture(current.page, "notes-desktop");
  await key(current.page, "Control+Alt+2"); await capture(current.page, "notes-tasks");
  await expectState(current.page, edited, "The board beneath Tasks and the Notes workspace must share the same records");
  await boot(current, true); await expectState(current.page, edited, "Note bodies, order and pins must survive restart");
  assert.deepEqual(await exportData(current), edited, "JSON export must include the same plain-text note records");
  const imported = structuredClone(edited); imported.notes[1].text += "\nImported locally";
  await current.page.locator("#jig-import").setInputFiles({ name: "sample-notes.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(imported)) });
  await current.page.waitForTimeout(1200); await expectState(current.page, edited, "Import must wait for confirmation");
  await notes(current); await tab(current.page, 6); await key(current.page, "Space"); // About, Data, Settings, Help, Calculator, Confirm import.
  await expectState(current.page, imported, "Confirmed JSON import must preserve complete note text and identities");
  await boot(current, true); await expectState(current.page, imported);
  await current.context.close();

  progress("cancel a Task edit when immediately clicking another task's Edit");
  const race = structuredClone(taskFixture);
  race.tasks[0].complete = false; delete race.tasks[1].list_id;
  current = await open("notes-task-edit-safety", race);
  await canvasFocus(current.page);
  await key(current.page, "Escape"); await key(current.page, "Control+Alt+2");
  await tab(current.page, headerTargets + 13); await key(current.page, "Space"); // First row Edit, after ×.
  const details = await taskGanttButtonPoint(current.page, "details");
  await current.page.mouse.click(details.x, details.y); await paint(current.page);
  const reviewedBody = "Sample: fired test\nReviewed details\nFinal character: Z";
  await typeTaskDetails(current.page, reviewedBody.slice(0, -1));
  // Changing multiline text can shrink the inline editor and move later rows.
  // Measure the actual second Edit after the draft has reached its final height.
  await tab(current.page, 12); // Details -> lists2, Star/Save/Cancel/To note, then row2 Edit.
  const otherEdit = await focusedButtonRect(current.page);
  const settledDetails = await taskGanttButtonPoint(current.page, "details");
  await current.page.mouse.click(settledDetails.x, settledDetails.y); await paint(current.page);
  await expectEditorText(current.page, reviewedBody.slice(0, -1)); await key(current.page, "Control+End");
  console.log("Unified task edit-switch pointer target:", JSON.stringify(otherEdit));
  await current.page.keyboard.type("Z");
  await current.page.mouse.click(otherEdit.x, otherEdit.y); // No paint after the final old-field character.
  await paint(current.page);
  await expectEditorText(current.page, race.tasks[1].text);
  await expectState(current.page, race, "Editing another task must discard the old draft without changing either saved record");
  await current.page.waitForFunction(() => {
    const draft = JSON.parse(sessionStorage.getItem("jig.drafts.v1"));
    return !draft?.task && !draft?.schedule;
  });
  await key(current.page, "Escape");

  progress("route each queued character once within the unified Task editor and recover its full details");
  await canvasFocus(current.page); await key(current.page, "Control+Alt+2");
  await tab(current.page, headerTargets + 13); await key(current.page, "Space");
  const firstDetails = await taskGanttButtonPoint(current.page, "details");
  await current.page.mouse.click(firstDetails.x, firstDetails.y); await paint(current.page);
  await typeTaskDetails(current.page, reviewedBody);
  const earlierTitle = await taskGanttButtonPoint(current.page, "input");
  await current.page.keyboard.type("R");
  await current.page.mouse.click(earlierTitle.x, earlierTitle.y); // Route queued Details text before focusing Name.
  await paint(current.page);
  await current.page.waitForFunction(({ key, details, title }) => {
    const task = JSON.parse(sessionStorage.getItem(key))?.task;
    return task?.id === 1 && task.details === details && task.text === title;
  }, { key: "jig.drafts.v1", details: reviewedBody + "R", title: race.tasks[0].text }, { timeout: 10000 });
  await key(current.page, "Control+End");
  await current.page.keyboard.type("Q"); await paint(current.page);
  await current.page.waitForFunction(({ key, details, title }) => {
    const task = JSON.parse(sessionStorage.getItem(key))?.task;
    return task?.id === 1 && task.details === details && task.text === title;
  }, { key: "jig.drafts.v1", details: reviewedBody + "R", title: race.tasks[0].text + "Q" }, { timeout: 10000 });
  await typeBody(current.page, race.tasks[0].text);
  const previousDetails = await taskGanttButtonPoint(current.page, "details");
  await current.page.mouse.click(previousDetails.x, previousDetails.y); await paint(current.page);
  await typeTaskDetails(current.page, reviewedBody);
  await expectState(current.page, race, "All fields must remain staged until the unified editor is explicitly saved");
  await key(current.page, "Control+Alt+o"); await expectEditorText(current.page, reviewedBody);
  await boot(current, true, false);
  const restoredDetails = await taskGanttButtonPoint(current.page, "details");
  await current.page.mouse.click(restoredDetails.x, restoredDetails.y); await paint(current.page);
  await expectEditorText(current.page, reviewedBody);
  await expectState(current.page, race, "Refresh must restore the working details without modifying the saved task");
  await key(current.page, "Enter"); // Enter saves Details; Shift+Enter above inserts its line breaks.
  const reviewed = structuredClone(race); reviewed.tasks[0].details = reviewedBody;
  await expectState(current.page, reviewed, "Explicit Save must commit the complete original Task edit without changing the other task");
  await undo(current, race);
  await canvasFocus(current.page); await key(current.page, "Control+Alt+2");
  await tab(current.page, headerTargets + 13); await key(current.page, "Space");
  await typeBody(current.page, "Sample: cancelled Data-navigation name");
  await attemptOpenLocal(current);
  await expectState(current.page, race, "Clicking Data must cancel an existing Task edit without silently saving or removing records");
  await current.page.waitForFunction(() => !JSON.parse(sessionStorage.getItem("jig.drafts.v1"))?.task);
  await current.context.close();

  progress("move a dated completed task into Notes and restore its full metadata");
  current = await open("notes-task-conversion", taskFixture);
  assert.equal(await current.page.evaluate(key => localStorage.getItem(key), storageKey), JSON.stringify(taskFixture),
    "Opening an existing personal file must not rewrite its tasks, notes or preferences");
  await taskToNote(current, 2, true);
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).notes?.length === 1, storageKey);
  const archived = await state(current.page), note = archived.notes[0];
  assert.equal(note.id, 1); assert.equal(note.text, fullBody);
  assert.equal(archived.tasks.length, 1); assert.equal(archived.tasks[0].id, 2);
  assert.deepEqual(archived.deadlines, [taskFixture.deadlines[1]], "A converted task must leave no active calendar/Gantt deadline behind");
  assert.equal(archived.featured, 102, "Moving another task into Notes must preserve the featured countdown");
  assert.deepEqual(note.task_origin, { complete: true, list_id: 1, color: "rose", start: "2026-10-26T12:34:56Z", deadline: taskFixture.deadlines[0], was_featured: false });
  await notes(current); await focusNote(current, 0, 4); await key(current.page, "Space");
  await tab(current.page, 5); // del, body, Studio, Show, Convert.
  await key(current.page, "Space");
  const restored = structuredClone(taskFixture); restored.tasks.reverse(); restored.deadlines.reverse();
  await expectState(current.page, restored, "Returning a note to Tasks must restore the same ID, full body, date, completion, color and list");
  await undo(current, archived); await undo(current, taskFixture);
  await current.context.close();

  progress("drag a note grip onto a task row without losing text, dates or the featured countdown");
  current = await open("notes-task-grip-drop", archived, false, false);
  // Keep Show visible while Studio is active: a row drop must use the
  // hovered task's list, rather than blindly using the active-list setting.
  await canvasFocus(current.page);
  await key(current.page, "Control+Alt+2"); await tab(current.page, headerTargets + 2); await key(current.page, "Space");
  await key(current.page, "Shift+Tab"); await key(current.page, "Shift+Space");
  let notePoints = await sharedNotePoints(current.page);
  const outsideTaskInput = await taskGanttButtonPoint(current.page, "input");
  await current.page.mouse.click(notePoints.body.x, notePoints.body.y); await paint(current.page);
  await current.page.waitForFunction(() => document.activeElement?.tagName === "INPUT");
  await typeBody(current.page, "Sample: this cancelled note character stays out of Tasks ");
  await current.page.keyboard.type("Z");
  await current.page.mouse.click(outsideTaskInput.x, outsideTaskInput.y); await paint(current.page);
  await expectState(current.page, archived, "Clicking the Task input must cancel the note edit before that input takes focus");
  await expectNoNoteDraft(current.page);
  await current.page.waitForFunction(() => document.activeElement?.tagName === "INPUT");
  const cleanTaskText = "Sample: fresh task after cancelling a note";
  // Do not select-all or clear: a leaked note character must remain detectable.
  await current.page.keyboard.type(cleanTaskText, { delay: 10 }); await key(current.page, "Enter");
  await current.page.waitForFunction(({ key, count }) => JSON.parse(localStorage.getItem(key)).tasks.length === count,
    { key: storageKey, count: archived.tasks.length + 1 });
  const afterOutsideTask = await state(current.page);
  const cleanTask = afterOutsideTask.tasks.find(task => !archived.tasks.some(original => original.id === task.id));
  assert.ok(cleanTask, "The new Task input must create exactly the separate requested task");
  assert.equal(cleanTask.text, cleanTaskText, "The old note's queued character must never leak into the newly focused Task input");
  assert.equal(cleanTask.complete, false); assert.equal(cleanTask.list_id ?? 1, 1);
  const precedingRecords = structuredClone(afterOutsideTask);
  precedingRecords.tasks = precedingRecords.tasks.filter(task => task.id !== cleanTask.id);
  assert.deepEqual(precedingRecords, archived, "The cancelled note and its archived schedule must survive adding the separate fresh task");
  await undo(current, archived);
  await canvasFocus(current.page);
  await key(current.page, "Control+Alt+2"); await tab(current.page, headerTargets + 2); await key(current.page, "Space");
  await key(current.page, "Shift+Tab"); await key(current.page, "Shift+Space");
  notePoints = await sharedNotePoints(current.page);
  await current.page.mouse.click(notePoints.body.x, notePoints.body.y); await paint(current.page);
  await current.page.waitForFunction(() => document.activeElement?.tagName === "INPUT");
  await typeBody(current.page, "Sample: changing task lists cancels this note draft");
  await current.page.mouse.click(110, 55); await paint(current.page); // Show tab, outside the note editor.
  await expectState(current.page, archived, "Selecting a task list must cancel the shared-board note edit without changing its archived task");
  await expectNoNoteDraft(current.page);
  // Restore Studio-active combined lists before targeting the Show row below.
  await canvasFocus(current.page);
  await key(current.page, "Control+Alt+2"); await tab(current.page, headerTargets + 2); await key(current.page, "Space");
  await key(current.page, "Shift+Tab"); await key(current.page, "Shift+Space");
  await canvasFocus(current.page);
  await key(current.page, "Control+Alt+2"); await tab(current.page, headerTargets + 8); // Header, strip5, commands2, first star.
  const rowStar = await focusedButtonRect(current.page);
  assert.ok(Number.isFinite(rowStar.rowBottom), "The combined task row must expose its full shaded frame");
  const rowDrop = { x: rowStar.x + 110, y: rowStar.rowBottom - 2 }; // Below the full row midpoint, including wrapped controls.
  notePoints = await sharedNotePoints(current.page);
  const beginNoteDrop = async () => {
    await current.page.mouse.move(notePoints.grip.x, notePoints.grip.y); await current.page.mouse.down(); await paint(current.page);
    await current.page.mouse.move(rowDrop.x, rowDrop.y, { steps: 10 }); await paint(current.page);
  };
  await beginNoteDrop(); await expectState(current.page, archived, "A note-to-task drag preview must not save before release");
  await key(current.page, "Escape"); await current.page.mouse.up(); await paint(current.page);
  await key(current.page, "Control+z");
  await expectState(current.page, archived, "Cancelling a note drop must preserve its archive and create no Undo entry");
  await beginNoteDrop(); await expectState(current.page, archived);
  await current.page.mouse.up(); await paint(current.page);
  const rowConverted = structuredClone(restored); rowConverted.tasks[1].list_id = 2;
  await expectState(current.page, rowConverted, "A row drop must move the same note ID into that list with its full body, dates, color and completion intact");
  assert.equal((await state(current.page)).featured, taskFixture.featured, "Dropping a note must not replace the existing featured countdown");
  await undo(current, archived);
  await key(current.page, "Control+z"); await expectState(current.page, archived, "A note conversion must be one atomic Undo step");
  await current.context.close();

  progress("drop a note onto the active Tasks list with real touch input and cancel safely");
  // A touch-capable wide viewport keeps the source board and target visible;
  // the separate phone checks exercise the narrow responsive layout.
  current = await open("notes-task-touch-drop", archived, false, false, true);
  notePoints = await sharedNotePoints(current.page);
  await current.page.mouse.click(notePoints.body.x, notePoints.body.y); await paint(current.page);
  const sharedSaveBody = "Sample: Ctrl-S saves the shared Tasks note ";
  await typeBody(current.page, sharedSaveBody);
  await current.page.keyboard.type("Z");
  const sharedDownloads = [];
  const recordSharedDownload = download => sharedDownloads.push(download.suggestedFilename());
  current.page.on("download", recordSharedDownload);
  await current.page.keyboard.press("Control+s"); await paint(current.page);
  const sharedSaved = structuredClone(archived); sharedSaved.notes[0].text = sharedSaveBody + "Z";
  await expectState(current.page, sharedSaved, "Ctrl-S in the shared Tasks board must save its active note, including queued text");
  await expectNoNoteDraft(current.page);
  assert.deepEqual(sharedDownloads, [], "Saving an active shared note must not also download the Tasks ODS backup");
  await undo(current, archived);
  await canvasFocus(current.page); await key(current.page, "Control+Alt+2");
  notePoints = await sharedNotePoints(current.page);
  const taskToolbar = await taskGanttButtonPoint(current.page);
  const blankDrop = { x: 220, y: taskToolbar.y + 120 };
  const dropSession = await current.context.newCDPSession(current.page);
  const noteTouch = (type, point) => dropSession.send("Input.dispatchTouchEvent", {
    type, touchPoints: type === "touchEnd" || type === "touchCancel" ? [] : [{ ...point, radiusX: 1, radiusY: 1, force: 1, id: 1 }],
  });
  const moveTouchDrop = async () => {
    for (let step = 1; step <= 10; step++) {
      await noteTouch("touchMove", { x: notePoints.grip.x + (blankDrop.x - notePoints.grip.x) * step / 10, y: notePoints.grip.y + (blankDrop.y - notePoints.grip.y) * step / 10 });
      await current.page.waitForTimeout(25);
    }
    await paint(current.page);
  };
  const beginTouchDrop = async () => {
    await noteTouch("touchStart", notePoints.grip); await paint(current.page);
    await moveTouchDrop();
  };
  await beginTouchDrop(); await expectState(current.page, archived, "A touch note drop must remain a preview until release");
  await noteTouch("touchCancel"); await paint(current.page);
  await key(current.page, "Control+z"); await expectState(current.page, archived, "Touch cancellation must not convert a note or create Undo history");
  await beginTouchDrop(); await expectState(current.page, archived);
  // Send a new press directly after cancellation, without an app-paint fence.
  // Chromium may paint between these events, so this checks a rapid real
  // gesture rather than guaranteeing that both arrive in the same app frame.
  await noteTouch("touchCancel");
  await noteTouch("touchStart", notePoints.grip); await paint(current.page);
  await moveTouchDrop();
  await expectState(current.page, archived, "A rapid cancel/restart on the same note grip must remain a preview until release");
  await noteTouch("touchEnd"); await paint(current.page);
  await expectState(current.page, restored, "A blank-pane touch drop must use the active list and preserve the original note/task metadata");
  await undo(current, archived);
  // Keep observing through subsequent app frames and the complete touch flow,
  // so a delayed wrong ODS dispatch cannot escape an early absence check.
  assert.deepEqual(sharedDownloads, [], "Explicit note Save must never schedule a delayed Tasks backup download");
  current.page.off("download", recordSharedDownload);
  await current.context.close();

  progress("explicitly Save by touch, cancel a touched-outside draft, and reorder the same notes");
  current = await open("notes-mobile", fixture, true);
  await focusNote(current, 0, 6); await key(current.page, "Space");
  const touchBody = "Sample: explicit touch Save retains this note";
  await typeBody(current.page, touchBody);
  await tab(current.page); const touchSave = await focusedButtonRect(current.page);
  await current.page.touchscreen.tap(touchSave.x, touchSave.y); await paint(current.page);
  const touchSaved = structuredClone(fixture); touchSaved.notes[0].text = touchBody;
  await expectState(current.page, touchSaved, "Tapping Save must explicitly persist the edited note");
  await expectNoNoteDraft(current.page);
  await undo(current, fixture);
  await notes(current); await tab(current.page, headerTargets + 1); await key(current.page, "Space");
  await typeBody(current.page, "Sample: tapping outside discards this new note");
  await current.page.waitForFunction(() => JSON.parse(sessionStorage.getItem("jig.drafts.v1"))?.note?.id === null);
  await current.page.touchscreen.tap(350, 600); await paint(current.page);
  await expectState(current.page, fixture, "A real touch outside a new unsaved note must discard it without creating a record");
  await expectNoNoteDraft(current.page);
  const first = await grip(current, 0), second = await grip(current, 1);
  const session = await current.context.newCDPSession(current.page);
  const touch = (type, x, y) => session.send("Input.dispatchTouchEvent", { type, touchPoints: type === "touchEnd" ? [] : [{ x, y, radiusX: 1, radiusY: 1, force: 1, id: 1 }] });
  await touch("touchStart", first.x, first.y); await paint(current.page);
  for (let step = 1; step <= 6; step++) {
    await touch("touchMove", first.x + (second.x + 110 - first.x) * step / 6, first.y + (second.y + 55 - first.y) * step / 6);
    await current.page.waitForTimeout(30);
  }
  await expectState(current.page, fixture, "A touch drag must not save its order until release");
  await touch("touchEnd"); await paint(current.page);
  const touchOrder = structuredClone(fixture); touchOrder.notes.reverse();
  await expectState(current.page, touchOrder, "Touch must reorder the existing note records");
  assert.equal(await current.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true, "The note grid must fit a phone without horizontal scrolling");
  await capture(current.page, "notes-mobile"); await current.context.close();

  progress("respect the 81-note limit without opening an extra draft");
  const capped = { ...structuredClone(fixture), notes: Array.from({ length: 81 }, (_, index) => ({ id: 401 + index, text: `Sample: note ${index + 1}`, pinned: false })) };
  current = await open("notes-cap", capped);
  await current.page.mouse.click(add.x, add.y); await paint(current.page);
  await expectState(current.page, capped, "The disabled Add note must preserve all 81 records");
  assert.equal(await current.page.evaluate(() => document.activeElement?.tagName === "INPUT"), false, "The 81-note board must not open an unsavable extra draft");
  await capture(current.page, "notes-cap"); await current.context.close();

  progress("swipe a note body to scroll the phone board without editing or moving records");
  current = await open("notes-body-scroll", capped, true);
  const beforeSwipe = await noteCardFrames(current.page);
  assert.ok(beforeSwipe.length >= 2, "The phone must display several note rows before the real body swipe");
  const swipeSession = await current.context.newCDPSession(current.page);
  const swipe = (type, point) => swipeSession.send("Input.dispatchTouchEvent", {
    type, touchPoints: type === "touchEnd" ? [] : [{ ...point, radiusX: 1, radiusY: 1, force: 1, id: 1 }],
  });
  const swipeFrom = { x: beforeSwipe[0].left + 55, y: beforeSwipe[0].top + 85 };
  const swipeTo = { x: swipeFrom.x, y: swipeFrom.y - 70 };
  await swipe("touchStart", swipeFrom); await paint(current.page);
  for (let step = 1; step <= 8; step++) {
    await swipe("touchMove", { x: swipeFrom.x, y: swipeFrom.y - 70 * step / 8 });
    await current.page.waitForTimeout(25);
  }
  await current.page.waitForTimeout(100); await swipe("touchMove", swipeTo); await paint(current.page);
  await swipe("touchEnd"); await paint(current.page);
  const afterSwipe = await noteCardFrames(current.page);
  assert.ok(afterSwipe.some(after => beforeSwipe.every(before => Math.abs(after.top - before.top) > 15)),
    `A body swipe must visibly scroll note rows; before=${JSON.stringify(beforeSwipe)}, after=${JSON.stringify(afterSwipe)}`);
  await expectState(current.page, capped, "Touch body scrolling must not reorder, convert or edit any of the 81 saved notes");
  assert.equal(await current.page.evaluate(() => document.activeElement?.tagName === "INPUT"), false, "A scrolling body swipe must not open the editor");
  await current.context.close();
  assert.deepEqual(issues, [], "The shared note board must stay local and emit no app errors");
  console.log("Notes browser checks passed: explicit Ctrl-Enter/Ctrl-S/button/touch Save including queued input; outside-click, workspace, other-note, Add-note and task-list cancellation without autosave or Undo/Redo mutation; cancelled note input cannot leak into Tasks; editor-local Copy and keyboard focus; titleless multiline add/edit/delete, pin and keyboard/pointer/touch reorder, shared Notes/Tasks board, release-only grip conversion and cancellation/rapid restart, full metadata/featured preservation, reload, JSON import/export and 81-note limit.");
} catch (error) {
  console.error(`Notes browser failed during: ${phase}`, error);
  if (current && !current.page.isClosed()) {
    // This suite owns every synthetic record; report public persisted data and
    // DOM events so a lost last character can be distinguished from focus.
    let diagnosticTimer;
    await Promise.race([
      current.page.evaluate(key => ({ saved: localStorage.getItem(key), drafts: sessionStorage.getItem("jig.drafts.v1"),
        active: document.activeElement?.tagName, events: window.jigNotesInputEvents, frames: window.jigTestFrames }), storageKey)
        .then(value => console.error("Notes public failure diagnostics:", JSON.stringify(value))),
      new Promise(resolve => { diagnosticTimer = setTimeout(resolve, 1500); }),
    ]).catch(() => {});
    clearTimeout(diagnosticTimer);
    await capture(current.page, "notes-failure").catch(() => {});
  }
  throw error;
} finally {
  clearTimeout(timer);
  for (const context of contexts) await context.close().catch(() => {});
  await browser.close();
}

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
let current, phase = "launch Chromium";
const timer = setTimeout(() => { console.error(`Notes browser exceeded 360 seconds during: ${phase}`); process.exit(1); }, 360000);
function progress(message) { phase = message; console.log(`Notes browser phase: ${message}`); }
async function paint(page) { await waitForAppPaint(page); }
async function key(page, value) { await page.keyboard.press(value); await paint(page); }
async function tab(page, count = 1) { for (let index = 0; index < count; index++) await key(page, "Tab"); }
async function state(page) { return page.evaluate(key => JSON.parse(localStorage.getItem(key)), storageKey); }
async function expectState(page, expected, message) {
  const until = Date.now() + 10000; let actual;
  do {
    actual = await state(page);
    try { assert.deepEqual(actual, expected); return; } catch { await page.waitForTimeout(50); }
  } while (Date.now() < until);
  assert.deepEqual(actual, expected, message);
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
async function open(label, initial = fixture, phone = false, launchNotes = true) {
  const context = await browser.newContext({ viewport: phone ? { width: 390, height: 844 } : { width: 1440, height: 1000 }, hasTouch: phone, acceptDownloads: true, timezoneId: "Etc/UTC" });
  contexts.push(context);
  await installFrameFence(context);
  await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin });
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
  const scene = { context, page, undo: false };
  await boot(scene, false, launchNotes); return scene;
}
async function boot(scene, reload = false, launchNotes = true) {
  if (reload) await scene.page.reload({ waitUntil: "load" }); else await scene.page.goto(base, { waitUntil: "load" });
  await scene.page.bringToFront();
  await scene.page.waitForFunction(() => window.jigReady === true || typeof window.jigBootError === "string", undefined, { timeout: 60000 });
  assert.equal(await scene.page.evaluate(() => window.jigBootError), undefined);
  await scene.page.waitForTimeout(500); scene.undo = false;
  if (launchNotes) await notes(scene);
}
async function notes(scene) {
  await scene.page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
  await key(scene.page, "Escape"); await key(scene.page, "Control+Alt+n");
}
function header(scene) { return scene.undo ? 3 : 2; } // Data, optional Undo, Source.
async function focusNote(scene, index, control) {
  await notes(scene);
  await tab(scene.page, header(scene) + 1 + index * 6 + control); // Add, then grip/pin/copy/task/del/body.
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
async function expectEditorText(page, text) {
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 25 });
  await page.evaluate(() => navigator.clipboard.writeText("Working editor read pending"));
  await key(page, "Control+a"); await key(page, "Control+c");
  await page.waitForFunction(async expected => (await navigator.clipboard.readText()) === expected, text, { timeout: 10000, polling: 50 });
}
async function attemptSample(scene) {
  // Data is fixed at the top right of this known 1440px desktop viewport.
  // Clicking it transfers focus without cancelling the working Task editor.
  await scene.page.mouse.click(scene.page.viewportSize().width - 22, 18); await paint(scene.page);
  await tab(scene.page, scene.undo ? 4 : 3); // Optional Undo, Start empty, Open local, Sample mode.
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
    for (let y = 42; y < Math.min(480, bitmap.height); y++) for (let x = 5; x < bitmap.width - 5; x++) {
      const start = y * bitmap.width + x;
      if (seen[start] || !match(start)) continue;
      const queue = [start], r = { left: x, right: x, top: y, bottom: y, count: 0 }; seen[start] = 1;
      while (queue.length) {
        const p = queue.pop(), px = p % bitmap.width, py = Math.floor(p / bitmap.width);
        r.left = Math.min(r.left, px); r.right = Math.max(r.right, px); r.top = Math.min(r.top, py); r.bottom = Math.max(r.bottom, py); r.count++;
        for (const [nx, ny] of [[px - 1, py], [px + 1, py], [px, py - 1], [px, py + 1]]) {
          if (nx < 0 || nx >= bitmap.width || ny < 42 || ny >= Math.min(480, bitmap.height)) continue;
          const next = ny * bitmap.width + nx;
          if (!seen[next] && match(next)) { seen[next] = 1; queue.push(next); }
        }
      }
      if (r.count >= 40 && r.bottom - r.top >= 8 && r.bottom - r.top <= 35 && r.right - r.left < 180) regions.push(r);
    }
    bitmap.close(); return regions.sort((a, b) => a.top - b.top || b.count - a.count)[0] ?? null;
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
      if (ink >= 28 && right - left > best.span) best = { span: right - left, ink };
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
  await notes(scene); await key(scene.page, "Control+Alt+d"); await tab(scene.page, scene.undo ? 7 : 6);
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
      for (let y = input ? 110 : 78; y < (input ? 540 : 145); y++) {
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
        if (bottom.y - top.y >= (details ? 35 : 12) && bottom.y - top.y <= (details ? 180 : 30) && Math.abs(top.left - bottom.left) <= 2 && Math.abs(top.right - bottom.right) <= 2)
          return { x: (top.left + top.right) / 2, y: (top.y + bottom.y) / 2 };
      }
      return null;
    }, { encoded: png.toString("base64"), control });
  } while (!point && Date.now() < until);
  assert.ok(point, `The Tasks workspace must visibly expose its ${control === "gantt" ? "Gantt chart button" : control === "details" ? "task details editor" : "task input"}`);
  return point;
}

async function taskToNote(scene, listCount, complete) {
  await scene.page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
  await key(scene.page, "Escape"); await key(scene.page, "Control+Alt+2");
  const strip = listCount + (listCount > 1 ? 3 : 1);
  await tab(scene.page, header(scene) + strip + 2 + (complete ? 4 : 5));
  await key(scene.page, "Space"); // Dated first row's edit; no other tasks in this list.
  await tab(scene.page, (listCount > 1 ? listCount : 0) + 6); // del, choices, input, Save, full body, Cancel, To note.
  await key(scene.page, "Space"); scene.undo = true;
}

try {
  progress("start with real personal Tasks, save a plan, and recover work after refresh");
  current = await open("notes-first-use", null, false, false);
  assert.equal(await current.page.evaluate(key => localStorage.getItem(key), storageKey), null,
    "A fresh personal launch may remain empty until the first real edit");
  await taskGanttButtonPoint(current.page); // No preference fixture: Tasks is the actual first-use workspace.
  const freshInput = await taskGanttButtonPoint(current.page, "input");
  await current.page.mouse.click(freshInput.x, freshInput.y); await paint(current.page);
  await typeBody(current.page, "Sample: plan exhibition"); await key(current.page, "Enter"); current.undo = true;
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key))?.tasks.length === 1, storageKey);
  const firstTask = (await state(current.page)).tasks[0];
  assert.equal(firstTask.text, "Sample: plan exhibition", "First-use work must save as personal data, rather than disappear into sample mode");
  await current.page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
  await key(current.page, "Escape"); await key(current.page, "Control+Alt+2");
  await tab(current.page, 10); // Header3, tabs2, commands2, star/checkbox/date.
  await key(current.page, "Space");
  await typeBody(current.page, "2026-11-02"); await tab(current.page);
  await typeBody(current.page, "2026-10-30"); await key(current.page, "Enter");
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).tasks[0].start != null, storageKey);
  const plan = await state(current.page);
  assert.equal(plan.deadlines.length, 1); assert.equal(plan.tasks[0].deadline, plan.deadlines[0].id);
  await boot(current, true, false);
  await taskGanttButtonPoint(current.page);
  await expectState(current.page, plan, "A saved task and its Gantt dates must survive an ordinary Tasks refresh");

  progress("recover an unfinished Start/Due form, cancel it, then explicitly save the recovered range");
  for (const cancel of [true, false]) {
    await current.page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
    await key(current.page, "Escape"); await key(current.page, "Control+Alt+2");
    await tab(current.page, header(current) + 8); await key(current.page, "Space");
    await typeBody(current.page, "2026-11-05"); await tab(current.page);
    await typeBody(current.page, "2026-11-01");
    await expectState(current.page, plan, "Changing a date form must not partially change the saved schedule");
    await boot(current, true, false);
    await expectEditorText(current.page, "2026-11-05"); await tab(current.page);
    await expectEditorText(current.page, "2026-11-01");
    await expectState(current.page, plan, "Refresh must restore both unfinished date fields without committing them");
    if (cancel) {
      await tab(current.page, 3); await key(current.page, "Space"); // Save, Remove date, Cancel.
      await expectState(current.page, plan, "Cancelling a recovered date form must preserve the original dates");
    } else {
      await key(current.page, "Enter"); current.undo = true;
      await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).deadlines[0].at.startsWith("2026-11-05"), storageKey);
      const rescheduled = await state(current.page);
      assert.ok(rescheduled.tasks[0].start.startsWith("2026-11-01"));
      assert.equal(rescheduled.tasks[0].id, plan.tasks[0].id);
      assert.equal(rescheduled.tasks[0].deadline, plan.tasks[0].deadline);
      await undo(current, plan); current.undo = false;
    }
  }

  await key(current.page, "Control+Alt+n"); await tab(current.page, 3); await key(current.page, "Space");
  await typeBody(current.page, "Sample: installation reminder\nBring measuring tape");
  await key(current.page, "Control+Enter"); current.undo = true;
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).notes?.length === 1, storageKey);
  const savedNote = await state(current.page);
  assert.equal(await current.page.evaluate(() => localStorage.getItem("jig.workspace.v1")), "notes");
  await boot(current, true, false); // Do not reopen Notes: the remembered workspace must do that.
  await expectState(current.page, savedNote, "Committed Tasks and Notes must reload together");
  await tab(current.page, 3); await key(current.page, "Space");
  const recoveredBody = "Sample: unfinished reminder\nMeasure twice before packing";
  await typeBody(current.page, recoveredBody);
  await expectState(current.page, savedNote, "An unfinished note must remain a draft until saved");
  assert.ok(await current.page.evaluate(() => sessionStorage.getItem("jig.drafts.v1")?.includes("Measure twice")),
    "The current tab must retain its unfinished note before refresh");
  await boot(current, true, false);
  await current.page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 25 });
  await current.page.evaluate(() => navigator.clipboard.writeText("Draft recovery pending"));
  await key(current.page, "Control+a"); await key(current.page, "Control+c");
  await current.page.waitForFunction(async expected => (await navigator.clipboard.readText()) === expected, recoveredBody, { timeout: 10000, polling: 50 });
  await key(current.page, "Control+Enter"); current.undo = true;
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).notes.length === 2, storageKey);
  const recoveredNotes = await state(current.page);
  await current.page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
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
  await attemptSample(current);
  const guardedInput = await taskGanttButtonPoint(current.page, "input");
  await current.page.mouse.click(guardedInput.x, guardedInput.y); await paint(current.page);
  await expectEditorText(current.page, unsent);
  await expectState(current.page, recoveredNotes, "Open local and Sample mode must preserve unsent personal task text and saved records");
  assert.equal(await current.page.evaluate(() => JSON.parse(sessionStorage.getItem("jig.drafts.v1")).task.text), unsent);
  await boot(current, true, false);
  await current.page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 25 });
  await current.page.evaluate(() => navigator.clipboard.writeText("Task draft recovery pending"));
  await key(current.page, "Control+a"); await key(current.page, "Control+c");
  await current.page.waitForFunction(async expected => (await navigator.clipboard.readText()) === expected, unsent, { timeout: 10000, polling: 50 });
  await key(current.page, "Enter"); current.undo = true;
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).tasks.length === 2, storageKey);
  const kept = await state(current.page);
  assert.equal(kept.tasks[1].text, unsent); assert.deepEqual(kept.notes, recoveredNotes.notes);
  assert.deepEqual(kept.deadlines, plan.deadlines);
  await capture(current.page, "notes-recovered");
  assert.deepEqual(await exportData(current), kept, "A portable JSON backup must contain the recovered tasks, dates and notes");
  await current.context.close();

  progress("create and edit multiline text without a separate title or premature save");
  current = await open("notes-desktop");
  await tab(current.page, 3); const add = await focusedButtonRect(current.page);
  await key(current.page, "Space");
  const body = "Sample: shelf tests\nStandard 104\nMeasure after firing";
  await typeBody(current.page, body);
  await expectState(current.page, fixture, "Plain Enter must add note lines without saving an unfinished draft");
  await key(current.page, "Control+Enter"); current.undo = true;
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).notes.length === 3, storageKey);
  const added = await state(current.page);
  assert.equal(added.notes[2].text, body); assert.equal(added.notes[2].pinned, false);
  await focusNote(current, 0, 6); await key(current.page, "Space");
  const updated = "Sample: clay tests\nKeep the full multiline result\nNo title field\nOne\nTwo\nThree\nFour\nFive\nSix\nEnd of full note\n"
    + "Ω glazing record ".repeat(40) + "\nTAIL Ω: the complete note remains available.";
  await typeBody(current.page, updated); await key(current.page, "Control+Enter");
  const edited = structuredClone(added); edited.notes[0].text = updated;
  await expectState(current.page, edited, "Editing a note must preserve its ID and every other record");

  progress("copy the full clipped body and keep the final character during a quick workspace change");
  await current.page.evaluate(() => navigator.clipboard.writeText("Notes copy pending"));
  await focusNote(current, 0, 3); await key(current.page, "Space");
  await current.page.waitForFunction(async expected => (await navigator.clipboard.readText()) === expected, updated, { timeout: 10000, polling: 50 });
  await expectState(current.page, edited, "Copy must preserve the note and all other records");
  await focusNote(current, 0, 6); await key(current.page, "Space");
  const quickBody = updated + "\nFinal character: ";
  await typeBody(current.page, quickBody);
  await current.page.keyboard.type("Z");
  await current.page.keyboard.press("Control+Alt+2"); // Intentionally no paint after the final character.
  await paint(current.page);
  edited.notes[0].text = quickBody + "Z";
  await expectState(current.page, edited, "A quick workspace change must save the full note, including the final input character");
  await taskGanttButtonPoint(current.page); // The actual Tasks toolbar proves the deferred switch occurred.

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
  await notes(current); await tab(current.page, 2); await key(current.page, "Space");
  await expectState(current.page, imported, "Confirmed JSON import must preserve complete note text and identities");
  await boot(current, true); await expectState(current.page, imported);
  await current.context.close();

  progress("retain edited Task details when switching data or quickly clicking another task's Edit");
  const race = structuredClone(taskFixture);
  race.tasks[0].complete = false; delete race.tasks[1].list_id;
  current = await open("notes-task-edit-safety", race);
  await current.page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
  await key(current.page, "Escape"); await key(current.page, "Control+Alt+2");
  await tab(current.page, 21); // Header2, two-list strip5, commands2, row1 controls7, row2 Edit5.
  const otherEdit = await focusedButtonRect(current.page);
  await current.page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
  await key(current.page, "Escape"); await key(current.page, "Control+Alt+2");
  await tab(current.page, 14); await key(current.page, "Space"); // First row Edit.
  const details = await taskGanttButtonPoint(current.page, "details");
  await current.page.mouse.click(details.x, details.y); await paint(current.page);
  const reviewedBody = "Sample: fired test\nReviewed details\nFinal character: Z";
  await typeBody(current.page, reviewedBody.slice(0, -1));
  await current.page.keyboard.type("Z");
  await current.page.mouse.click(otherEdit.x, otherEdit.y); // Intentionally no paint after the final input.
  await paint(current.page);
  await current.page.waitForFunction(({ key, text }) => {
    const draft = JSON.parse(sessionStorage.getItem(key));
    return draft?.task?.id === 1 && draft.task.details === text;
  }, { key: "jig.drafts.v1", text: reviewedBody }, { timeout: 10000 });
  await expectState(current.page, race, "Edit another task must wait for Save/Clear and leave both records unchanged");
  await expectEditorText(current.page, reviewedBody);
  await key(current.page, "Control+Alt+o"); await expectEditorText(current.page, reviewedBody);
  await attemptSample(current);
  const guardedDetails = await taskGanttButtonPoint(current.page, "details");
  await current.page.mouse.click(guardedDetails.x, guardedDetails.y); await paint(current.page);
  await expectEditorText(current.page, reviewedBody);
  await expectState(current.page, race, "Data switches must not discard or silently commit an existing Task's edited details");
  await boot(current, true, false);
  const restoredDetails = await taskGanttButtonPoint(current.page, "details");
  await current.page.mouse.click(restoredDetails.x, restoredDetails.y); await paint(current.page);
  await expectEditorText(current.page, reviewedBody);
  await expectState(current.page, race, "Refreshing must restore the working details without modifying the saved task");
  const titleInput = await taskGanttButtonPoint(current.page, "input");
  await current.page.mouse.click(titleInput.x, titleInput.y); await paint(current.page);
  await key(current.page, "Enter"); current.undo = true;
  const reviewed = structuredClone(race); reviewed.tasks[0].details = reviewedBody;
  await expectState(current.page, reviewed, "Explicit Save must commit the complete original Task edit without changing the other task");
  await undo(current, race); await current.context.close();

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

  progress("reorder the same notes using a real touch drag");
  current = await open("notes-mobile", fixture, true);
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
  await expectState(current.page, touchOrder, "Touch must reorder the existing note records"); current.undo = true;
  assert.equal(await current.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true, "The note grid must fit a phone without horizontal scrolling");
  await capture(current.page, "notes-mobile"); await current.context.close();

  progress("respect the 81-note limit without opening an extra draft");
  const capped = { ...structuredClone(fixture), notes: Array.from({ length: 81 }, (_, index) => ({ id: 401 + index, text: `Sample: note ${index + 1}`, pinned: false })) };
  current = await open("notes-cap", capped);
  await current.page.mouse.click(add.x, add.y); await paint(current.page);
  await expectState(current.page, capped, "The disabled Add note must preserve all 81 records");
  assert.equal(await current.page.evaluate(() => document.activeElement?.tagName === "INPUT"), false, "The 81-note board must not open an unsavable extra draft");
  await capture(current.page, "notes-cap"); await current.context.close();
  assert.deepEqual(issues, [], "The shared note board must stay local and emit no app errors");
  console.log("Notes browser checks passed: titleless multiline add/edit/delete, pin and keyboard/pointer/touch reorder, atomic Undo, shared Notes/Tasks board, safe task conversion preserving text/metadata and featured selection, reload, JSON import/export and 81-note limit.");
} catch (error) {
  console.error(`Notes browser failed during: ${phase}`, error);
  if (current && !current.page.isClosed()) await capture(current.page, "notes-failure").catch(() => {});
  throw error;
} finally {
  clearTimeout(timer);
  for (const context of contexts) await context.close().catch(() => {});
  await browser.close();
}

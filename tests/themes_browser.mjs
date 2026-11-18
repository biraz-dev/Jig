// Real theme controls and paint output. All documents and settings are synthetic.
import assert from "node:assert/strict";
import { mkdir, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { isDeepStrictEqual } from "node:util";
import { chromium } from "playwright";
import { installFrameFence, waitForAppPaint } from "./browser_frames.mjs";

const base = process.env.JIG_PREVIEW_URL ?? "http://127.0.0.1:4173/Jig/";
const origin = new URL(base).origin;
const stateKey = "jig.personal.v1", settingsKey = "jig.settings.v1";
const results = resolve("playwright-results");
await mkdir(results, { recursive: true });
const fixture = {
  version: 1, preferences: { decimals: 2, focus: 3, weather: null }, history: [],
  tasks: [{ id: 1, text: "Sample: preserve task colors", complete: false, deadline: 101, color: "rose" }],
  deadlines: [{ id: 101, title: "Sample: preserve task colors", at: "2030-01-15T18:00:00Z" }],
  entries: [], notes: [{ id: 201, text: "Sample: preserve note text", pinned: true }], featured: 101,
  draft: { blocks: [{ runs: [{ text: "Sample: gather work here." }] }] },
  workbook: { version: 1, sheets: [{ id: 1, name: "Sample inputs", cells: { A1: "Material", B1: "Amount", A2: "Clay", B2: "10", A3: "Tools", B3: "5" },
    styles: { B2: { text_color: [201, 167, 120] } } }],
    charts: [{ id: 1, title: "Sample costs", kind: "bar", source: { type: "range", sheet: 1, range: { start: { row: 0, col: 0 }, end: { row: 2, col: 1 } } } }],
    controls: [{ id: 1, label: "Sample total", target: { sheet: 1, cell: { row: 1, col: 1 } }, kind: "result" }] },
};
const rawDocument = JSON.stringify(fixture);
const defaults = { version: 2, theme: "ember", undo_levels: 20, auto_backup: false };
const palettes = {
  ember: { background: [30, 27, 25], text: [230, 225, 219], accent: [244, 107, 74] },
  paper: { background: [246, 242, 236], text: [49, 43, 39], accent: [175, 64, 38] },
  nord: { background: [46, 52, 64], text: [236, 239, 244], accent: [136, 192, 208] },
  dracula: { background: [40, 42, 54], text: [248, 248, 242], accent: [189, 147, 249] },
  graphite: { background: [32, 34, 37], text: [226, 228, 232], accent: [128, 185, 176] },
  catppuccin: { background: [30, 30, 46], text: [205, 214, 244], accent: [203, 166, 247] },
  gruvbox: { background: [40, 40, 40], text: [235, 219, 178], accent: [250, 189, 47] },
  tokyo_night: { background: [26, 27, 38], text: [192, 202, 245], accent: [122, 162, 247] },
  dark: { background: [16, 16, 16], text: [216, 216, 216], accent: [192, 192, 192] },
};
const custom = { background: [18, 24, 30], surface: [28, 36, 44], text: [224, 231, 238], accent: [233, 151, 91] };
const contexts = [], issues = [];
let phase = "launch Chromium", current;
const timer = setTimeout(() => { console.error(`Themes browser exceeded 480 seconds during: ${phase}`); process.exit(1); }, 480000);
function progress(value) { phase = value; console.log(`Themes browser phase: ${value}`); }
async function paint(page) { await waitForAppPaint(page); }
async function key(page, chord) { await page.keyboard.press(chord); await paint(page); }
async function tab(page, count = 1, reverse = false) { for (let index = 0; index < count; index++) await key(page, reverse ? "Shift+Tab" : "Tab"); }
async function raw(page, key = stateKey) { return page.evaluate(key => localStorage.getItem(key), key); }
async function settings(page) { return JSON.parse(await raw(page, settingsKey)); }
async function waitSettings(page, predicate, message) {
  const until = Date.now() + 10000;
  do { const value = await settings(page); if (predicate(value)) return value; await page.waitForTimeout(25); } while (Date.now() < until);
  assert.ok(predicate(await settings(page)), message); return settings(page);
}
async function preserved(page) {
  assert.equal(await raw(page), rawDocument, "Changing appearance must not rewrite the task, note, sheet or Draft document");
}
async function followingPaints(page, count = 2) {
  const before = await page.evaluate(() => window.jigTestFrames.paints);
  await page.waitForFunction(({ before, count }) => window.jigTestFrames.paints >= before + count
    && window.jigTestFrames.painted >= window.jigTestFrames.input, { before, count });
}
async function resetFocus(page) {
  const { width, height } = page.viewportSize();
  await page.locator("#jig_canvas").click({ position: { x: width - 6, y: height - 6 } });
  await key(page, "Escape");
}
async function openSettings(scene) {
  if (scene.settingsOpen) await closeSettings(scene);
  await resetFocus(scene.page); await key(scene.page, "Control+Comma");
  scene.settingsOpen = true; await followingPaints(scene.page);
  // The custom editor can leave Settings scrolled down. Scrolling the real
  // panel back to its top keeps the Theme control visible for this action.
  await scene.page.mouse.move(scene.phone ? 250 : 600, 250); await scene.page.mouse.wheel(0, -10000); await paint(scene.page);
}
async function closeSettings(scene) {
  if (!scene.settingsOpen) return;
  await key(scene.page, "Control+Comma"); scene.settingsOpen = false; await followingPaints(scene.page);
}
async function chooseTheme(scene, name) {
  await openSettings(scene); await tab(scene.page, scene.phone ? 2 : 1); await key(scene.page, "Space");
  // The parent panel and scrolling popup each precede the theme options.
  await tab(scene.page, { ember: 3, paper: 4, system: 5, nord: 6, dracula: 7, graphite: 8, catppuccin: 9, gruvbox: 10, tokyo_night: 11, dark: 12, custom: 13 }[name]);
  await key(scene.page, "Space");
  await waitSettings(scene.page, value => value.version === 2 && value.theme === name, `Selecting ${name} must save its canonical v2 preference`);
  await preserved(scene.page);
}
async function canvasColor(page, x = 2, y = page.viewportSize().height / 2) {
  const png = await page.screenshot({ animations: "disabled", caret: "hide" });
  return page.evaluate(async ({ encoded, x, y }) => {
    const image = await createImageBitmap(new Blob([Uint8Array.from(atob(encoded), character => character.charCodeAt(0))], { type: "image/png" }));
    const canvas = new OffscreenCanvas(image.width, image.height), ctx = canvas.getContext("2d"); ctx.drawImage(image, 0, 0);
    const color = [...ctx.getImageData(Math.floor(x), Math.floor(y), 1, 1).data].slice(0, 3); image.close(); return color;
  }, { encoded: png.toString("base64"), x, y });
}
async function paintedBackground(page, expected) {
  const until = Date.now() + 10000; let actual;
  do { actual = await canvasColor(page); if (actual.every((value, index) => Math.abs(value - expected[index]) <= 2)) return;
    await page.waitForTimeout(25); } while (Date.now() < until);
  assert.deepEqual(actual, expected, "The actual canvas chrome must use the selected background");
}
async function draftPalette(scene, expected) {
  await scene.page.locator("#jig-draft-editor").waitFor({ state: "visible" });
  await scene.page.waitForFunction(expected => {
    const panel = document.getElementById("jig-draft-panel"), editor = document.getElementById("jig-draft-editor");
    const color = channels => `rgb(${channels.join(", ")})`;
    const probe = document.createElement("span"); probe.style.color = getComputedStyle(panel).getPropertyValue("--draft-accent"); panel.append(probe);
    const accent = getComputedStyle(probe).color; probe.remove();
    return getComputedStyle(panel).backgroundColor === color(expected.background)
      && getComputedStyle(editor).color === color(expected.text) && accent === color(expected.accent);
  }, expected);
  await paintedBackground(scene.page, expected.background); await preserved(scene.page);
}
async function capture(page, name) {
  const png = await page.screenshot({ path: resolve(results, `${name}.png`), timeout: 60000, animations: "disabled", caret: "hide" });
  if (new Set((process.env.JIG_RENDER_LOGS ?? "").split(",")).has(name) && png.length <= 1024 * 1024) {
    console.log(`JIG_RENDER_BEGIN_${name}`); const encoded = png.toString("base64");
    for (let offset = 0; offset < encoded.length; offset += 2048) console.log(encoded.slice(offset, offset + 2048));
    console.log(`JIG_RENDER_END_${name}`);
  }
}
async function selectedDraftHeader(page) {
  const panel = await page.locator("#jig-draft-panel").boundingBox();
  const png = await page.screenshot({ animations: "disabled", caret: "hide" });
  const point = await page.evaluate(async ({ encoded, bottom }) => {
    const image = await createImageBitmap(new Blob([Uint8Array.from(atob(encoded), character => character.charCodeAt(0))], { type: "image/png" }));
    const canvas = new OffscreenCanvas(image.width, image.height), ctx = canvas.getContext("2d"); ctx.drawImage(image, 0, 0);
    const pixels = ctx.getImageData(0, 0, image.width, image.height).data, seen = new Uint8Array(image.width * image.height);
    const matches = index => [67, 42, 34].every((value, axis) => Math.abs(pixels[index * 4 + axis] - value) <= 3);
    for (let y = 0; y < bottom; y++) for (let x = 0; x < image.width; x++) {
      const start = y * image.width + x; if (seen[start] || !matches(start)) continue;
      const stack = [start]; seen[start] = 1; let left = x, right = x, top = y, end = y, count = 0;
      while (stack.length) {
        const index = stack.pop(), px = index % image.width, py = Math.floor(index / image.width); count++;
        left = Math.min(left, px); right = Math.max(right, px); top = Math.min(top, py); end = Math.max(end, py);
        for (const [nx, ny] of [[px - 1, py], [px + 1, py], [px, py - 1], [px, py + 1]]) {
          if (nx < 0 || nx >= image.width || ny < 0 || ny >= bottom) continue;
          const next = ny * image.width + nx; if (!seen[next] && matches(next)) { seen[next] = 1; stack.push(next); }
        }
      }
      if (count > 100 && right - left >= 40 && right - left <= 85 && end - top >= 15 && end - top <= 31) {
        image.close(); return { x: (left + right) / 2, y: (top + end) / 2 };
      }
    }
    image.close(); return null;
  }, { encoded: png.toString("base64"), bottom: Math.floor(panel.y) });
  assert.ok(point, "The initial Draft header must be visibly selected"); return point;
}
async function showDraft(scene, point) {
  if (scene.phone) await scene.page.touchscreen.tap(point.x, point.y); else await scene.page.mouse.click(point.x, point.y);
  await scene.page.locator("#jig-draft-editor").waitFor({ state: "visible" });
  await scene.page.waitForFunction(() => localStorage.getItem("jig.workspace.v1") === "draft");
}
async function boot(scene, reload = false) {
  if (reload) await scene.page.reload({ waitUntil: "load" }); else await scene.page.goto(base, { waitUntil: "load" });
  await scene.page.bringToFront();
  await scene.page.waitForFunction(() => window.jigReady === true || typeof window.jigBootError === "string", undefined, { timeout: 60000 });
  assert.equal(await scene.page.evaluate(() => window.jigBootError), undefined);
  scene.settingsOpen = false; await paint(scene.page);
}
const browser = await chromium.launch({ headless: true, args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"] });
async function open(label, saved = defaults, { phone = false, workspace = "draft", colorScheme = "dark" } = {}) {
  const context = await browser.newContext({ viewport: phone ? { width: 390, height: 844 } : { width: 1440, height: 1400 }, hasTouch: phone, acceptDownloads: true, timezoneId: "Etc/UTC", colorScheme });
  contexts.push(context); await installFrameFence(context); await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin });
  await context.addInitScript(({ origin, stateKey, settingsKey, document, saved, workspace }) => {
    if (location.origin === origin && sessionStorage.getItem("jig-theme-suite-seeded") !== "yes") {
      localStorage.setItem(stateKey, document); localStorage.setItem(settingsKey, JSON.stringify(saved));
      localStorage.setItem("jig.workspace.v1", workspace); sessionStorage.setItem("jig-theme-suite-seeded", "yes");
    }
  }, { origin, stateKey, settingsKey, document: rawDocument, saved, workspace });
  await context.route("**/*", async route => {
    const url = new URL(route.request().url());
    if (["blob:", "data:", "about:"].includes(url.protocol) || url.origin === origin) await route.continue();
    else { issues.push(`${label}: unexpected request ${url.origin}${url.pathname}`); await route.abort(); }
  });
  const page = await context.newPage(); page.setDefaultTimeout(10000); page.setDefaultNavigationTimeout(60000);
  page.on("pageerror", error => issues.push(`${label}: ${error.message}`));
  const scene = { context, page, settingsOpen: false, phone }; await boot(scene); return scene;
}
async function copyFocusedHex(page) {
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT");
  await page.evaluate(() => navigator.clipboard.writeText("Theme input pending"));
  await key(page, "Control+a"); await key(page, "Control+c");
  await page.waitForFunction(() => navigator.clipboard.readText().then(text => /^#[0-9a-f]{6}$/i.test(text)));
  const value = await page.evaluate(() => navigator.clipboard.readText()); await key(page, "ArrowRight"); return value.toUpperCase();
}
async function nextHex(page, expected) {
  for (let index = 0; index < 4; index++) {
    await tab(page);
    if (await page.evaluate(() => document.activeElement?.tagName === "INPUT")) {
      const value = await copyFocusedHex(page); assert.equal(value, expected); return;
    }
  }
  assert.fail("The next custom color HEX field must be keyboard reachable");
}
async function editHex(scene, field, value) {
  await key(scene.page, "Control+a"); await scene.page.keyboard.type(value, { delay: 15 }); await paint(scene.page);
  await waitSettings(scene.page, data => isDeepStrictEqual(data.custom_theme?.[field], custom[field]), `${field} must save the typed HEX color`);
  await preserved(scene.page);
}
async function exportSettings(scene) {
  await openSettings(scene); await tab(scene.page, 2);
  const pending = scene.page.waitForEvent("download", { timeout: 15000 }); await key(scene.page, "Space");
  const item = await pending; assert.equal(item.suggestedFilename(), "jig-settings.json"); return JSON.parse(await readFile(await item.path(), "utf8"));
}
async function importSettings(scene, value) {
  await openSettings(scene); await tab(scene.page, 3);
  const pending = scene.page.waitForEvent("filechooser", { timeout: 15000 }); await key(scene.page, "Space");
  await (await pending).setFiles({ name: "Sample-custom-theme.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(value)) });
  await scene.page.waitForFunction(() => document.activeElement?.id === "jig_canvas");
  await followingPaints(scene.page);
  // Import focuses Done; reverse traversal reaches Cancel then Apply.
  await tab(scene.page, 2, true); await key(scene.page, "Space");
  await waitSettings(scene.page, data => isDeepStrictEqual(data, value), "Imported theme settings must round-trip exactly after Apply");
  await preserved(scene.page);
}

try {
  progress("preserve legacy warm and light preferences without rewriting documents");
  current = await open("theme-legacy-warm", { ...defaults, version: 1, theme: "graphite" });
  await draftPalette(current, palettes.ember);
  assert.equal((await settings(current.page)).theme, "graphite", "Reading v1 preferences must not silently rewrite their storage");
  await current.context.close();
  current = await open("theme-legacy-light", { ...defaults, version: 1, theme: "light" });
  await draftPalette(current, palettes.paper); assert.equal((await settings(current.page)).theme, "light"); await current.context.close();

  progress("select every named preset through the real Settings menu and verify canvas and Draft colors");
  current = await open("theme-presets"); const header = await selectedDraftHeader(current.page);
  for (const name of ["nord", "dracula", "graphite", "paper", "ember", "catppuccin", "gruvbox", "tokyo_night", "dark"]) {
    await chooseTheme(current, name); await closeSettings(current); await draftPalette(current, palettes[name]);
    await capture(current.page, `theme-${name.replaceAll("_", "-")}`); await boot(current, true); await draftPalette(current, palettes[name]);
    assert.equal((await settings(current.page)).theme, name);
  }
  for (const name of ["tokyo_night", "dark"]) {
    await chooseTheme(current, name); await closeSettings(current); await draftPalette(current, palettes[name]);
    const presetSettings = await exportSettings(current);
    assert.equal(presetSettings.theme, name, "Named presets must use their canonical name in portable settings");
    await closeSettings(current); await chooseTheme(current, "ember"); await closeSettings(current);
    await importSettings(current, presetSettings); await closeSettings(current); await draftPalette(current, palettes[name]);
    await boot(current, true); await draftPalette(current, palettes[name]);
    assert.deepEqual(await settings(current.page), presetSettings, "Imported preset settings must survive reload exactly");
  }

  progress("carry a selected preset through Tasks, Sheets and live Charts without changing document colors");
  await chooseTheme(current, "nord"); await closeSettings(current);
  await resetFocus(current.page); await key(current.page, "Control+Alt+2");
  await current.page.locator("#jig-draft-panel").waitFor({ state: "hidden" }); await paintedBackground(current.page, palettes.nord.background); await preserved(current.page);
  await key(current.page, "Control+Alt+w"); await current.page.waitForFunction(() => localStorage.getItem("jig.workspace.v1") === "sheet");
  await paintedBackground(current.page, palettes.nord.background); await preserved(current.page); await showDraft(current, header); await draftPalette(current, palettes.nord);
  await current.context.close();
  current = await open("theme-charts", { ...defaults, theme: "dracula" }, { workspace: "charts" });
  await paintedBackground(current.page, palettes.dracula.background); await preserved(current.page);
  // The user-specified cell ink and the first series color must survive chrome changes.
  const png = await current.page.screenshot({ animations: "disabled", caret: "hide" });
  const colors = await current.page.evaluate(async encoded => {
    const image = await createImageBitmap(new Blob([Uint8Array.from(atob(encoded), character => character.charCodeAt(0))], { type: "image/png" }));
    const canvas = new OffscreenCanvas(image.width, image.height), ctx = canvas.getContext("2d"); ctx.drawImage(image, 0, 0);
    const pixels = ctx.getImageData(0, 0, image.width, image.height).data, counts = [0, 0, 0];
    const expected = [[201, 167, 120], [109, 185, 238], [30, 27, 25]];
    for (let y = 90; y < 650; y++) for (let x = 10; x < image.width - 10; x++) {
      const at = (y * image.width + x) * 4;
      expected.forEach((color, index) => { if (color.every((value, axis) => Math.abs(pixels[at + axis] - value) <= 3)) counts[index]++; });
    }
    image.close(); return counts;
  }, png.toString("base64"));
  assert.ok(colors[0] > 15, "Result cards must retain their explicit cell text color");
  assert.ok(colors[1] > 80, "Chart series colors must retain their semantic palette");
  assert.ok(colors[2] < 20, "The Chart card must not retain a warm rectangle under Dracula");
  await current.context.close();

  progress("edit custom HEX colors with atomic validation, retain the palette across presets, and round-trip settings");
  current = await open("theme-custom"); await chooseTheme(current, "custom");
  assert.equal(await copyFocusedHex(current.page), "#1E1B19", "Selecting Custom must reveal and focus its Background HEX field");
  const beforePartial = await settings(current.page);
  await key(current.page, "Control+a"); await current.page.keyboard.type("#12", { delay: 15 }); await paint(current.page);
  assert.deepEqual(await settings(current.page), beforePartial, "A partial HEX value must remain a draft without changing settings");
  await editHex(current, "background", "#12181E");
  await nextHex(current.page, "#292522"); await editHex(current, "surface", "#1C242C");
  await nextHex(current.page, "#E6E1DB"); await editHex(current, "text", "#E0E7EE");
  await nextHex(current.page, "#F46B4A"); await editHex(current, "accent", "#E9975B");
  await closeSettings(current); await draftPalette(current, custom); await capture(current.page, "theme-custom");
  await chooseTheme(current, "paper"); await closeSettings(current); await draftPalette(current, palettes.paper);
  assert.deepEqual((await settings(current.page)).custom_theme, custom, "Preset selection must retain the user's custom palette");
  await chooseTheme(current, "custom"); await closeSettings(current); await draftPalette(current, custom);
  const exported = await exportSettings(current); assert.deepEqual(exported.custom_theme, custom); assert.equal(exported.theme, "custom");
  await closeSettings(current); await chooseTheme(current, "ember"); await closeSettings(current);
  await importSettings(current, exported); await closeSettings(current); await draftPalette(current, custom);
  await boot(current, true); await draftPalette(current, custom); assert.deepEqual(await settings(current.page), exported);

  progress("follow the operating system light/dark preference and respect native accent availability");
  await chooseTheme(current, "system"); await closeSettings(current); await draftPalette(current, palettes.ember);
  await current.page.emulateMedia({ colorScheme: "light" }); await draftPalette(current, palettes.paper);
  await current.page.emulateMedia({ colorScheme: "dark" }); await draftPalette(current, palettes.ember);
  await chooseTheme(current, "custom");
  await copyFocusedHex(current.page); await tab(current.page, 2, true); await key(current.page, "Space");
  await waitSettings(current.page, value => value.system_accent === true, "The system accent choice must be saved independently");
  await closeSettings(current);
  const nativeAccent = await current.page.evaluate(() => window.jigSystemAccent());
  const expectedAccent = nativeAccent ? [1, 3, 5].map(offset => parseInt(nativeAccent.slice(offset, offset + 2), 16)) : custom.accent;
  await draftPalette(current, { ...custom, accent: expectedAccent });
  assert.deepEqual((await settings(current.page)).custom_theme.accent, custom.accent, "Native accent adoption must never overwrite the manual fallback color");
  await current.context.close();

  progress("fit a custom theme on a touch phone and restore it on reload");
  current = await open("theme-mobile", { ...exported, system_accent: false }, { phone: true });
  await draftPalette(current, custom);
  for (const name of ["tokyo_night", "dark"]) {
    await chooseTheme(current, name); await closeSettings(current); await draftPalette(current, palettes[name]);
    await boot(current, true); await draftPalette(current, palettes[name]);
  }
  await capture(current.page, "theme-dark-mobile");
  await chooseTheme(current, "custom"); await closeSettings(current); await draftPalette(current, custom);
  const panel = await current.page.locator("#jig-draft-panel").boundingBox();
  assert.ok(panel && panel.x >= 0 && panel.x + panel.width <= 391 && panel.height > 400);
  assert.equal(await current.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true);
  await current.page.locator("#jig-draft-editor").tap(); await capture(current.page, "theme-mobile");
  await boot(current, true); await draftPalette(current, custom);
  assert.deepEqual((await settings(current.page)).custom_theme, custom); await preserved(current.page);
  assert.deepEqual(issues, [], "Theme changes must not issue external requests or cause browser errors");
  console.log("Themes browser checks passed: v1 appearance migration; actual preset selection, canvas/Draft colors and reload including Catppuccin, Gruvbox, Tokyo Night and Dark; unchanged task/sheet/chart data and semantic ink; custom HEX editing/validation and retained palettes; preset/custom settings file round trips; OS light/dark and real native-accent fallback; and phone menu selection, fit and reload.");
} catch (error) {
  console.error(`Themes browser failed during: ${phase}`, error);
  if (current?.page && !current.page.isClosed()) {
    console.error("Themes browser diagnostics:", await current.page.evaluate(({ stateKey, settingsKey }) => ({ state: localStorage.getItem(stateKey), settings: localStorage.getItem(settingsKey), workspace: localStorage.getItem("jig.workspace.v1"), active: { tag: document.activeElement?.tagName, id: document.activeElement?.id }, frames: window.jigTestFrames }), { stateKey, settingsKey }).catch(String));
    await capture(current.page, "theme-failure").catch(() => {});
  }
  throw error;
} finally {
  let cleanupTimer;
  try {
    await Promise.race([Promise.all([...contexts.map(context => context.close().catch(() => {})), browser.close()]),
      new Promise((_, reject) => { cleanupTimer = setTimeout(() => reject(new Error("Themes browser shutdown exceeded 10 seconds")), 10000); })]);
  } finally { clearTimeout(cleanupTimer); clearTimeout(timer); }
}

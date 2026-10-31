// Real Chromium checks for local STL files. Every model and dataset is synthetic.
// Serve the built preview at /Jig/, then: node tests/stl_browser.mjs
import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium } from "playwright";

const base = process.env.JIG_PREVIEW_URL ?? "http://127.0.0.1:4173/Jig/";
const origin = new URL(base).origin;
const storageKey = "jig.personal.v1";
const results = resolve("playwright-results");
await mkdir(results, { recursive: true });
const fixture = {
  version: 1, preferences: { decimals: 2, focus: 3 },
  history: [{ expression: "2 + 2", result: 4 }],
  tasks: [{ id: 1, text: "Sample: keep artist checklist", complete: false }],
  deadlines: [{ id: 2, title: "Sample: future opening", at: "2030-01-15T18:00:00Z" }],
  entries: [], featured: 2,
};
const fixtureRaw = JSON.stringify(fixture);
const vertices = [[0, 0, 0], [10, 0, 0], [10, 10, 0], [0, 10, 0], [0, 0, 10], [10, 0, 10], [10, 10, 10], [0, 10, 10]];
const faces = [[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [3, 7, 6], [3, 6, 2], [0, 4, 7], [0, 7, 3], [1, 2, 6], [1, 6, 5]];
function binaryCube(scale = 1) {
  const bytes = Buffer.alloc(84 + faces.length * 50);
  bytes.write("solid Sample: local-only cube; binary despite its header");
  bytes.writeUInt32LE(faces.length, 80);
  faces.forEach((face, index) => face.forEach((vertex, point) => vertices[vertex].forEach((coordinate, axis) => {
    bytes.writeFloatLE(coordinate * scale, 84 + index * 50 + 12 + point * 12 + axis * 4);
  })));
  return bytes;
}
const asciiCube = Buffer.from(`solid Sample: ASCII cube\n${faces.map(face => `facet normal 0 0 0\nouter loop\n${face.map(index => `vertex ${vertices[index].join(" ")}`).join("\n")}\nendloop\nendfacet`).join("\n")}\nendsolid Sample: ASCII cube\n`);
let phase = "launch Chromium";
let originalError;
const issues = [];
const contexts = [];
// Track only the layout commands sent by this driver. No application state is
// exposed: repeated report reads can keep the already maximized GPU view open.
const expanded = new WeakMap();
const timer = setTimeout(() => {
  console.error(`STL browser checks exceeded 360 seconds during: ${phase}`);
  if (originalError) console.error("Original STL browser failure:", originalError);
  process.exit(1);
}, 360000);
function progress(message) { phase = message; console.log(`STL browser phase: ${message}`); }
async function paint(page) {
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  // Browser RAF can precede WASM input consumption under software-rendered 3D.
  await page.waitForTimeout(250);
}
async function key(page, value) {
  await page.keyboard.press(value);
  if (value === "Escape") expanded.set(page, false);
  else if (value === "Control+m" || value === "F2") expanded.set(page, !expanded.get(page));
  await paint(page);
}
async function tab(page, count) { for (let index = 0; index < count; index++) await key(page, "Tab"); }
async function bounded(promise, milliseconds, description) {
  let deadline;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        deadline = setTimeout(() => reject(new Error(`${description} exceeded ${milliseconds}ms`)), milliseconds);
      }),
    ]);
  } finally { clearTimeout(deadline); }
}
function renderLog(name, png) {
  const allow = new Set([
    "stl-desktop", "stl-maximized", "stl-mobile", "stl-hidpi", "stl-small", "stl-tiny", "stl-failure", "stl-failure-canvas", "stl-reloaded", "stl-open-mesh",
    "stl-focus-open", "stl-focus-tab1", "stl-focus-tab2", "stl-focus-tab3",
  ]);
  const requested = new Set((process.env.JIG_RENDER_LOGS ?? "").split(","));
  if (allow.has(name) && requested.has(name) && png.length <= 1024 * 1024) {
    console.log(`JIG_RENDER_BEGIN_${name}`);
    const encoded = png.toString("base64");
    for (let offset = 0; offset < encoded.length; offset += 2048) console.log(encoded.slice(offset, offset + 2048));
    console.log(`JIG_RENDER_END_${name}`);
  }
}
async function capture(page, name, timeout = 60000) {
  const png = await page.screenshot({
    path: resolve(results, `${name}.png`), timeout, animations: "disabled", caret: "hide",
  });
  renderLog(name, png);
}
async function failureDiagnostics(page) {
  const details = await bounded(page.evaluate(() => {
    const canvas = document.getElementById("jig_canvas");
    const gl = canvas?.getContext("webgl2") ?? canvas?.getContext("webgl");
    return {
      visibility: document.visibilityState, focused: document.hasFocus(), ready: window.jigReady,
      activeElement: document.activeElement ? { tag: document.activeElement.tagName, id: document.activeElement.id } : null,
      viewport: { width: innerWidth, height: innerHeight, pixelRatio: devicePixelRatio },
      canvas: canvas ? { width: canvas.width, height: canvas.height, contextLost: gl?.isContextLost() ?? null } : null,
    };
  }), 3000, "Public STL browser diagnostics").catch(error => ({ unavailable: String(error) }));
  console.error("STL public browser diagnostics:", JSON.stringify(details));
  try { await capture(page, "stl-failure", 3000); }
  catch (error) {
    console.error("STL failure screenshot unavailable:", String(error));
    const data = await bounded(page.evaluate(() => document.getElementById("jig_canvas")?.toDataURL("image/png") ?? ""), 3000, "Canvas PNG fallback").catch(() => "");
    if (data.startsWith("data:image/png;base64,")) {
      const png = Buffer.from(data.slice("data:image/png;base64,".length), "base64");
      await writeFile(resolve(results, "stl-failure-canvas.png"), png);
      renderLog("stl-failure-canvas", png);
    }
  }
}
const browser = await chromium.launch({ headless: true, args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"] });
async function newPage(label, viewport = { width: 1440, height: 1000 }, seed = true, deviceScaleFactor = 1) {
  const context = await browser.newContext({ viewport, deviceScaleFactor, acceptDownloads: true, hasTouch: viewport.width < 500, timezoneId: "Etc/UTC" });
  contexts.push(context);
  await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin });
  if (seed) await context.addInitScript(({ raw, storageKey, origin }) => {
    if (location.origin === origin && sessionStorage.getItem("jig-stl-seeded") !== "yes") {
      localStorage.setItem(storageKey, raw);
      sessionStorage.setItem("jig-stl-seeded", "yes");
    }
  }, { raw: fixtureRaw, storageKey, origin });
  await context.route("**/*", async route => {
    const url = new URL(route.request().url());
    if (["blob:", "data:", "about:"].includes(url.protocol) || url.origin === origin) await route.continue();
    else { issues.push(`${label}: unexpected remote request ${url.origin}${url.pathname}`); await route.abort(); }
  });
  const page = await context.newPage();
  page.on("pageerror", error => issues.push(`${label}: ${error.message}`));
  page.setDefaultTimeout(15000);
  await page.clock.setFixedTime(new Date("2026-10-30T12:00:00Z"));
  return { context, page };
}
async function boot(page, reload = false) {
  if (reload) await page.reload({ waitUntil: "load" });
  else await page.goto(base, { waitUntil: "load" });
  expanded.set(page, false);
  await page.bringToFront();
  await page.waitForFunction(() => window.jigReady === true || typeof window.jigBootError === "string", undefined, { timeout: 60000 });
  assert.equal(await page.evaluate(() => window.jigBootError), undefined);
  await page.waitForTimeout(500);
}
async function maximize(page) {
  if (!expanded.get(page)) {
    await page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
    await key(page, "Escape");
    await key(page, "Control+Alt+5");
    await key(page, "Control+m");
  }
  // Re-enter through the public Open STL shortcut after the expanded layout is
  // built, so file errors, scrolling and global Undo cannot change our anchor.
  await key(page, "Control+Alt+5");
  await page.waitForTimeout(250);
}
async function saved(page) { return page.evaluate(key => localStorage.getItem(key), storageKey); }
async function upload(page, bytes, name) {
  await page.locator("#jig-stl-import").setInputFiles({ name, mimeType: "model/stl", buffer: bytes });
  // STL I/O is polled by the same low-frequency idle repaint as other applets.
  await page.waitForTimeout(1300);
  await paint(page);
}

async function copyReport(page, captureFocus = false) {
  const sentinel = "Jig STL report pending";
  let copied = "";
  for (let attempt = 0; attempt < 2; attempt++) {
    await page.evaluate(value => navigator.clipboard.writeText(value), sentinel);
    await maximize(page);
    if (attempt) await page.waitForTimeout(250);
    // Ctrl-Alt-5 focuses Open STL. Its next controls are Sample cube, Clear and
    // Copy report, independent of the header's changing global Undo state.
    if (captureFocus && attempt === 0) {
      await capture(page, "stl-focus-open");
      for (let index = 1; index <= 3; index++) {
        await key(page, "Tab");
        await capture(page, `stl-focus-tab${index}`);
      }
    } else await tab(page, 3);
    await key(page, "Space");
    const deadline = Date.now() + 3000;
    while (Date.now() < deadline) {
      copied = await page.evaluate(() => navigator.clipboard.readText());
      if (copied.startsWith("Jig STL report\n")) return copied;
      await page.waitForTimeout(50);
    }
    if (attempt === 0) console.warn("STL report read unavailable; retrying the read-only Copy report action once after reanchoring.");
  }
  assert.fail(`Copy report did not produce a Jig STL report after two bounded read attempts; clipboard began ${JSON.stringify(copied.slice(0, 180))}`);
}

async function control(page, position) {
  await maximize(page);
  if (position === 2) {
    // From Open STL, reverse past Source to the real global Undo button. This
    // still works after Clear disables all mesh-dependent applet controls.
    await key(page, "Shift+Tab");
    await key(page, "Shift+Tab");
  } else {
    assert.ok(position >= 4, "Loaded STL control positions start at Open STL");
    await tab(page, position - 4);
  }
  await key(page, "Space");
}

async function undo(page) { await key(page, "Control+z"); }

async function modelView(page) {
  // The maximized desktop viewport's interior excludes control focus outlines,
  // footer text and the fixed axis widget. A restored camera must draw the same
  // cube pixels here; the supplied clock is fixed for deterministic captures.
  return page.screenshot({
    clip: { x: 50, y: 180, width: 700, height: 600 }, timeout: 60000, animations: "disabled", caret: "hide",
  });
}

async function setScale(page, percent) {
  await maximize(page);
  await tab(page, 16); // Physical scale, starting from the Open STL anchor.
  // DragValue enters its text editor on Tab focus. Enter would commit and close
  // it before typing; wait for that real editor instead, then commit once below.
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 3000 });
  await page.keyboard.press("Control+a");
  await page.keyboard.type(String(percent), { delay: 30 });
  await key(page, "Enter");
}

function measurements(report) {
  const number = "([-+]?\\d+(?:\\.\\d+)?(?:[eE][-+]?\\d+)?)";
  const area = report.match(new RegExp(`Surface area:\\s*${number}\\s*mm`));
  const volume = report.match(new RegExp(`Total volume:\\s*${number}\\s*mm`));
  const dimensions = report.split("\n").find(line => line.startsWith("Dimensions:"));
  assert.ok(area, "Copied report must expose surface area with explicit mm² units");
  assert.ok(dimensions, "Copied report must expose physical dimensions");
  return {
    area: Number(area[1]), volume: volume ? Number(volume[1]) : null,
    dimensions: [...dimensions.matchAll(new RegExp(number, "g"))].slice(0, 3).map(match => Number(match[1])),
  };
}

function exportedDimensions(bytes) {
  assert.ok(bytes.length >= 84, "Scaled export must be binary STL");
  const count = bytes.readUInt32LE(80);
  assert.equal(bytes.length, 84 + count * 50);
  const minimum = [Infinity, Infinity, Infinity], maximum = [-Infinity, -Infinity, -Infinity];
  for (let face = 0; face < count; face++) for (let point = 0; point < 3; point++) for (let axis = 0; axis < 3; axis++) {
    const value = bytes.readFloatLE(84 + face * 50 + 12 + point * 12 + axis * 4);
    assert.ok(Number.isFinite(value));
    minimum[axis] = Math.min(minimum[axis], value);
    maximum[axis] = Math.max(maximum[axis], value);
  }
  return maximum.map((value, index) => value - minimum[index]);
}

function close(actual, expected, message) {
  assert.ok(Math.abs(actual - expected) <= Math.max(0.00001, Math.abs(expected) * 1e-9), `${message}: ${actual} != ${expected}`);
}

async function bridgeChecks() {
  progress("local file bridge limits, races, clear and export");
  const bridge = await newPage("stl-file-bridge", { width: 800, height: 600 }, false);
  // Run the production bridge on an isolated same-origin page without WASM
  // consuming pending files. These are ordinary public file-bridge functions.
  const address = new URL("__stl_bridge_test__", base).href;
  await bridge.page.route(address, route => route.fulfill({
    contentType: "text/html", body: '<input id="jig-stl-import" type="file">',
  }));
  await bridge.page.goto(address);
  await bridge.page.addScriptTag({ url: new URL("stl.js", base).href });
  const result = await bridge.page.evaluate(async () => {
    const input = document.getElementById("jig-stl-import");
    const choose = file => {
      Object.defineProperty(input, "files", { configurable: true, value: file ? [file] : [] });
      input.dispatchEvent(new Event("change"));
    };
    let finishSlow;
    const slow = new File([new Uint8Array([1, 2, 3])], "slow.stl");
    slow.arrayBuffer = () => new Promise(resolve => { finishSlow = resolve; });
    choose(slow);
    choose(new File([new Uint8Array([4, 5, 6])], "fast.stl"));
    await new Promise(resolve => setTimeout(resolve, 20));
    const latest = Array.from(window.jigStlTakeBytes());
    const latestName = window.jigStlTakeName();
    finishSlow(new Uint8Array([1, 2, 3]).buffer);
    await Promise.resolve();
    const stale = window.jigStlTakeBytes().length;
    let finishCleared;
    const cleared = new File([new Uint8Array([7])], "cleared.stl");
    cleared.arrayBuffer = () => new Promise(resolve => { finishCleared = resolve; });
    choose(cleared);
    window.jigStlClear();
    finishCleared(new Uint8Array([7]).buffer);
    await Promise.resolve();
    const afterClear = window.jigStlTakeBytes().length;
    choose(new File([], "empty.stl"));
    const empty = window.jigStlTakeError();
    const oversized = new File([new Uint8Array([9])], "oversized.stl");
    Object.defineProperty(oversized, "size", { value: 20 * 1024 * 1024 + 1 });
    choose(oversized);
    const large = window.jigStlTakeError();
    const noRepeatedError = window.jigStlTakeError();
    const stored = Object.keys(localStorage);
    return { latest, latestName, stale, afterClear, empty, large, noRepeatedError, stored };
  });
  assert.deepEqual(result.latest, [4, 5, 6]);
  assert.equal(result.latestName, "fast.stl");
  assert.equal(result.stale, 0, "An old file read must not replace a newer selection");
  assert.equal(result.afterClear, 0, "A late read must not restore a cleared model");
  assert.match(result.empty, /empty/i);
  assert.match(result.large, /20 MiB/);
  assert.equal(result.noRepeatedError, "");
  assert.deepEqual(result.stored, [], "The STL bridge must not persist filenames or model bytes");
  const downloadEvent = bridge.page.waitForEvent("download");
  await bridge.page.evaluate(() => window.jigStlExport(new Uint8Array([1, 2, 3]), "sample-scaled.stl"));
  const download = await downloadEvent;
  assert.equal(download.suggestedFilename(), "sample-scaled.stl");
  assert.deepEqual(await readFile(await download.path()), Buffer.from([1, 2, 3]));
  await bridge.context.close();
}

let current;
try {
  await bridgeChecks();
  progress("open a closed binary cube and copy physical measurements");
  current = await newPage("stl-desktop");
  await boot(current.page);
  assert.equal(await saved(current.page), fixtureRaw);
  await maximize(current.page);
  const emptyViewer = await modelView(current.page);
  // The shortcut has focused the actual Open STL button; activate it directly.
  const chooserEvent = current.page.waitForEvent("filechooser", { timeout: 5000 });
  await key(current.page, "Space");
  const chooser = await chooserEvent;
  await chooser.setFiles({ name: "Sample-local-cube.stl", mimeType: "model/stl", buffer: binaryCube() });
  await current.page.waitForTimeout(1300);
  await paint(current.page);
  const binaryReport = await copyReport(current.page, true);
  assert.match(binaryReport, /Sample-local-cube\.stl/);
  assert.deepEqual(measurements(binaryReport), { area: 600, volume: 1000, dimensions: [10, 10, 10] });
  await capture(current.page, "stl-maximized");
  assert.equal(await saved(current.page), fixtureRaw, "Opening an STL must not rewrite the workbench dataset");

  progress("export the loaded model using the visible STL control");
  const exportEvent = current.page.waitForEvent("download");
  await tab(current.page, 1); // Copy report -> Export STL.
  await key(current.page, "Space");
  const exported = await exportEvent;
  assert.match(exported.suggestedFilename(), /\.stl$/i);
  assert.deepEqual(exportedDimensions(await readFile(await exported.path())), [10, 10, 10]);

  progress("open ASCII STL and preserve it after a malformed replacement");
  await upload(current.page, asciiCube, "Sample-ASCII-cube.stl");
  const asciiReport = await copyReport(current.page);
  assert.match(asciiReport, /Sample-ASCII-cube\.stl/);
  assert.deepEqual(measurements(asciiReport), { area: 600, volume: 1000, dimensions: [10, 10, 10] });
  await upload(current.page, Buffer.from("solid broken\nfacet normal 0 0 0\nouter loop\nvertex 0 0 0\n"), "Sample-broken.stl");
  const preservedReport = await copyReport(current.page);
  assert.match(preservedReport, /Sample-ASCII-cube\.stl/);
  assert.deepEqual(measurements(preservedReport), measurements(asciiReport), "Rejected STL must keep the last usable geometry");
  assert.equal(await saved(current.page), fixtureRaw);

  progress("reject invalid STL without inserting an undo step");
  await undo(current.page);
  const beforeAsciiReport = await copyReport(current.page);
  assert.match(beforeAsciiReport, /Sample-local-cube\.stl/, "One Undo must skip the failed import and restore the preceding valid model");
  assert.deepEqual(measurements(beforeAsciiReport), measurements(binaryReport));
  await upload(current.page, asciiCube, "Sample-ASCII-cube.stl");
  assert.deepEqual(measurements(await copyReport(current.page)), measurements(asciiReport));

  progress("change view without changing physical measurements");
  // From Copy report: Export STL, Iso, Front. The cube's front projection is
  // visibly different from its initial isometric view while measurements agree.
  const beforeView = await modelView(current.page);
  await tab(current.page, 3);
  await key(current.page, "Space");
  assert.notDeepEqual(await modelView(current.page), beforeView, "A view preset must visibly rotate the model");
  assert.deepEqual(measurements(await copyReport(current.page)), measurements(asciiReport));
  await undo(current.page);
  assert.deepEqual(measurements(await copyReport(current.page)), measurements(asciiReport));
  assert.deepEqual(await modelView(current.page), beforeView, "Undo must restore the preceding camera view");

  progress("orbit by dragging without changing geometry measurements");
  const beforeOrbit = await modelView(current.page);
  await current.page.mouse.move(400, 400);
  await current.page.mouse.down();
  await current.page.mouse.move(470, 430, { steps: 8 });
  await current.page.mouse.up();
  await paint(current.page);
  assert.notDeepEqual(await modelView(current.page), beforeOrbit, "Dragging the model viewport must change its view");
  assert.deepEqual(measurements(await copyReport(current.page)), measurements(asciiReport));
  await undo(current.page);
  assert.deepEqual(measurements(await copyReport(current.page)), measurements(asciiReport));
  assert.deepEqual(await modelView(current.page), beforeOrbit, "One Undo must restore an entire orbit gesture");

  progress("interpret unitless STL coordinates using explicit source units");
  await control(current.page, 18); // Source units: cm.
  const centimeterReport = await copyReport(current.page);
  assert.match(centimeterReport, /Source units: cm/);
  assert.deepEqual(measurements(centimeterReport), { area: 60000, volume: 1000000, dimensions: [100, 100, 100] });
  await undo(current.page);
  const restoredUnits = await copyReport(current.page);
  assert.match(restoredUnits, /Source units: mm/);
  assert.deepEqual(measurements(restoredUnits), measurements(asciiReport), "Undo must restore the former source unit");

  progress("scale the actual model and distinguish that from camera zoom");
  await setScale(current.page, 200);
  const scaledReport = await copyReport(current.page);
  assert.match(scaledReport, /Model scale: 200\.00%/);
  assert.deepEqual(measurements(scaledReport), { area: 2400, volume: 8000, dimensions: [20, 20, 20] });
  await undo(current.page);
  const unscaledReport = await copyReport(current.page);
  assert.match(unscaledReport, /Model scale: 100\.00%/);
  assert.deepEqual(measurements(unscaledReport), measurements(asciiReport), "One Undo must restore a whole physical-scale edit");
  await setScale(current.page, 200);
  await control(current.page, 13); // Fit reframes without changing physical scale.
  assert.deepEqual(measurements(await copyReport(current.page)), measurements(scaledReport));

  progress("apply Standard 104 cone 06 linear shrinkage and compensation");
  await control(current.page, 22); // Estimate.
  const estimatedReport = await copyReport(current.page);
  assert.match(estimatedReport, /Standard #104/);
  assert.match(estimatedReport, /Cone: 06\b/);
  assert.match(estimatedReport, /Mode: Estimate/);
  assert.match(estimatedReport, /Linear shrinkage: 6\.000000%/);
  const estimated = measurements(estimatedReport);
  estimated.dimensions.forEach(value => close(value, 20 * 0.94, "Estimated length"));
  close(estimated.area, 2400 * 0.94 ** 2, "Estimated surface area");
  close(estimated.volume, 8000 * 0.94 ** 3, "Estimated enclosed volume");
  await undo(current.page);
  const beforeEstimateReport = await copyReport(current.page);
  assert.match(beforeEstimateReport, /Mode: Off/);
  assert.deepEqual(measurements(beforeEstimateReport), measurements(scaledReport), "Undo must restore the former shrinkage mode");
  await control(current.page, 22); // Reapply Estimate.
  await control(current.page, 23); // Compensate.
  const compensatedReport = await copyReport(current.page);
  assert.match(compensatedReport, /Mode: Compensate/);
  const compensated = measurements(compensatedReport);
  compensated.dimensions.forEach(value => close(value, 20 / 0.94, "Compensated length"));
  close(compensated.area, 2400 / 0.94 ** 2, "Compensated surface area");
  close(compensated.volume, 8000 / 0.94 ** 3, "Compensated enclosed volume");
  await undo(current.page);
  const beforeCompensationReport = await copyReport(current.page);
  assert.match(beforeCompensationReport, /Mode: Estimate/);
  assert.deepEqual(measurements(beforeCompensationReport), estimated, "Undo must restore the former estimate");
  await control(current.page, 23); // Reapply Compensate for the local export.
  assert.equal(await saved(current.page), fixtureRaw, "Mesh transforms and camera changes must not save model data");

  progress("export compensated geometry using the selected source units");
  await maximize(current.page);
  await tab(current.page, 4); // Open STL -> Sample, Clear, Copy, Export.
  const compensatedExportEvent = current.page.waitForEvent("download");
  await key(current.page, "Space");
  const compensatedExport = await compensatedExportEvent;
  for (const dimension of exportedDimensions(await readFile(await compensatedExport.path()))) close(dimension, 20 / 0.94, "Exported compensated geometry");

  await key(current.page, "Escape");
  await capture(current.page, "stl-desktop");
  assert.equal(await saved(current.page), fixtureRaw);

  progress("clear and restore a model using the visible Undo button");
  await control(current.page, 6); // Clear drops the current model.
  assert.deepEqual(await modelView(current.page), emptyViewer, "Clear must empty the model before testing Undo");
  // The global header Undo remains available after the applet is empty.
  await control(current.page, 2);
  const restoredAfterClear = await copyReport(current.page);
  assert.match(restoredAfterClear, /Sample-ASCII-cube\.stl/);
  assert.deepEqual(measurements(restoredAfterClear), compensated, "Undo Clear must restore the model and its physical transforms");
  assert.equal(await saved(current.page), fixtureRaw);

  progress("select an exact manufacturer cone point and save only preferences");
  await control(current.page, 24); // Material opens its inline choices.
  // Manufacturer data link, Standard 104/cone06, Standard 104/cone4.
  await tab(current.page, 3);
  await key(current.page, "Space");
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).preferences.clay?.selected === "standard-104-4", storageKey);
  const coneFourReport = await copyReport(current.page);
  assert.match(coneFourReport, /Cone: 4\b/);
  assert.match(coneFourReport, /Linear shrinkage: 11\.000000%/);
  const coneFour = measurements(coneFourReport);
  coneFour.dimensions.forEach(value => close(value, 20 / 0.89, "Cone4 compensated length"));
  close(coneFour.area, 2400 / 0.89 ** 2, "Cone4 compensated area");
  close(coneFour.volume, 8000 / 0.89 ** 3, "Cone4 compensated volume");

  progress("undo globally in order across camera and saved clay preferences");
  const materialCamera = await modelView(current.page);
  await control(current.page, 10); // Front view is newer than the cone4 choice.
  await undo(current.page);
  const stillConeFourReport = await copyReport(current.page);
  assert.match(stillConeFourReport, /Cone: 4\b/);
  assert.deepEqual(measurements(stillConeFourReport), coneFour, "Undoing a camera edit must preserve the selected material");
  assert.deepEqual(await modelView(current.page), materialCamera, "The first global Undo must restore only the camera");
  assert.equal(JSON.parse(await saved(current.page)).preferences.clay?.selected, "standard-104-4");

  progress("undo a material selection and restore its saved cone preference");
  await undo(current.page);
  const restoredMaterialReport = await copyReport(current.page);
  assert.match(restoredMaterialReport, /Cone: 06\b/);
  assert.match(restoredMaterialReport, /Linear shrinkage: 6\.000000%/);
  assert.deepEqual(measurements(restoredMaterialReport), compensated);
  await current.page.waitForFunction(key => (JSON.parse(localStorage.getItem(key)).preferences.clay?.selected ?? "standard-104-06") === "standard-104-06", storageKey);
  const materialUndoState = JSON.parse(await saved(current.page));
  for (const field of ["tasks", "deadlines", "entries", "history", "featured"]) assert.deepEqual(materialUndoState[field], fixture[field]);
  // The inline material list remains open; choose the second published point
  // again so restart testing covers an explicitly saved, nondefault selection.
  await control(current.page, 27);
  await current.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).preferences.clay?.selected === "standard-104-4", storageKey);
  const savedWithClay = await saved(current.page);
  const clayState = JSON.parse(savedWithClay);
  for (const field of ["tasks", "deadlines", "entries", "history", "featured"]) assert.deepEqual(clayState[field], fixture[field]);
  assert.ok(!savedWithClay.includes("Sample-ASCII-cube.stl"), "Material preferences must not contain model filenames");

  progress("report an open surface without inventing a solid volume");
  const triangle = Buffer.from("solid Sample: open triangle\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 10 0 0\nvertex 0 10 0\nendloop\nendfacet\nendsolid Sample: open triangle\n");
  await upload(current.page, triangle, "Sample-open-triangle.stl");
  const openReport = await copyReport(current.page);
  const open = measurements(openReport);
  assert.equal(open.area, 50);
  assert.equal(open.volume, null, "An open surface has no trustworthy enclosed volume");
  assert.match(openReport, /Cone: 4\b/);
  await capture(current.page, "stl-open-mesh");

  progress("restart without retaining STL file bytes or filenames");
  assert.equal(await saved(current.page), savedWithClay);
  await boot(current.page, true);
  await maximize(current.page);
  await capture(current.page, "stl-reloaded");
  const stored = await current.page.evaluate(() => Object.fromEntries(Object.entries(localStorage)));
  assert.ok(!JSON.stringify(stored).includes("Sample-open-triangle.stl"));
  assert.ok(!JSON.stringify(stored).includes("Sample-ASCII-cube.stl"));
  assert.equal(await saved(current.page), savedWithClay);
  await upload(current.page, binaryCube(), "Sample-after-restart.stl");
  const restartedReport = await copyReport(current.page);
  assert.match(restartedReport, /Cone: 4\b/, "Selected clay and cone must survive a restart");
  assert.deepEqual(measurements(restartedReport), { area: 600, volume: 1000, dimensions: [10, 10, 10] });
  assert.equal(await saved(current.page), savedWithClay);
  await current.context.close();

  progress("open STL on a touch-size viewport");
  current = await newPage("stl-mobile", { width: 390, height: 844 });
  await boot(current.page);
  await upload(current.page, binaryCube(), "Sample-mobile-cube.stl");
  await maximize(current.page);
  assert.equal(await current.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true);
  await capture(current.page, "stl-mobile");
  assert.equal(await saved(current.page), fixtureRaw);
  await current.context.close();

  progress("keep small and subprecision mesh measurements readable");
  current = await newPage("stl-small");
  await boot(current.page);
  await upload(current.page, binaryCube(0.1), "Sample-one-mm-cube.stl");
  assert.deepEqual(measurements(await copyReport(current.page)), { area: 6, volume: 1, dimensions: [1, 1, 1] });
  await capture(current.page, "stl-small");
  await upload(current.page, binaryCube(0.01), "Sample-tiny-cube.stl");
  assert.deepEqual(measurements(await copyReport(current.page)), { area: 0.06, volume: 0.001, dimensions: [0.1, 0.1, 0.1] });
  await capture(current.page, "stl-tiny");
  assert.equal(await saved(current.page), fixtureRaw, "Measurement labels must not change saved data");
  await current.context.close();

  progress("keep model proportions on a large high-DPI drawing surface");
  current = await newPage("stl-hidpi", { width: 2200, height: 1000 }, true, 2);
  await boot(current.page);
  await upload(current.page, binaryCube(), "Sample-retina-cube.stl");
  const highDpiReport = await copyReport(current.page);
  assert.deepEqual(measurements(highDpiReport), { area: 600, volume: 1000, dimensions: [10, 10, 10] });
  await tab(current.page, 3); // Copy report -> Export, Iso, Front.
  await key(current.page, "Space");
  await capture(current.page, "stl-hidpi");
  // The first action row used to scroll above the maximized panel after Tab
  // navigation. Activate its visible position without reanchoring the view.
  const visibleOpenEvent = current.page.waitForEvent("filechooser", { timeout: 5000 });
  await current.page.locator("#jig_canvas").click({ position: { x: 80, y: 64 }, delay: 50 });
  const visibleOpen = await visibleOpenEvent;
  await visibleOpen.setFiles([]);
  assert.equal(await saved(current.page), fixtureRaw);

  assert.deepEqual(issues, [], "STL errors or unexpected requests");
  console.log("STL browser checks passed: local bridge bounds/races/clear, binary/ASCII cube measurements, unit interpretation, physical scale versus camera, Standard104/cone06 estimate and compensation, compensated local export, cone4 preference persistence, Undo for files/clear/camera gestures/units/scale/shrinkage/material settings, invalid replacement preservation, unavailable volume for open surfaces, memory-only models on restart, desktop/mobile/high-DPI layouts, and workbench data isolation.");
} catch (error) {
  originalError = error;
  console.error(`STL browser failed during: ${phase}`);
  if (current?.page && !current.page.isClosed()) await failureDiagnostics(current.page).catch(() => {});
  if (issues.length) console.error(issues.join("\n"));
  throw error;
} finally {
  progress("close STL browser");
  let cleanupTimer;
  let cleanupError;
  try {
    await Promise.race([
      (async () => {
        for (const context of contexts) await context.close().catch(() => {});
        await browser.close();
      })(),
      new Promise((_, reject) => {
        cleanupTimer = setTimeout(() => reject(new Error("Closing STL browser contexts and Chromium exceeded 10 seconds")), 10000);
      }),
    ]);
  } catch (error) {
    cleanupError = error;
  } finally {
    clearTimeout(cleanupTimer);
    clearTimeout(timer);
  }
  if (cleanupError) {
    // A timed-out browser still owns sockets and child processes. Terminate the
    // driver instead of letting those handles keep a failed CI step alive.
    if (originalError) console.error("Original STL browser failure:", originalError);
    console.error("STL browser cleanup failed:", cleanupError);
    process.exit(1);
  }
}

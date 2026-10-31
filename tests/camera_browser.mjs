// Real Chromium camera checks. All meshes and stored records are synthetic.
// Serve the built preview at /Jig/, then: node tests/camera_browser.mjs
import assert from "node:assert/strict";
import { mkdir, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium } from "playwright";

const base = process.env.JIG_PREVIEW_URL ?? "http://127.0.0.1:4173/Jig/";
const origin = new URL(base).origin;
const storageKey = "jig.personal.v1";
const results = resolve("playwright-results");
await mkdir(results, { recursive: true });
const fixture = {
  version: 1, preferences: { decimals: 2, focus: 3, weather: null },
  history: [{ expression: "2 + 2", result: 4 }],
  tasks: [{ id: 1, text: "Sample: keep camera work separate", complete: false }],
  deadlines: [], entries: [], featured: null,
};
const rawFixture = JSON.stringify(fixture);
const points = [[0, 0, 0], [10, 0, 0], [10, 20, 0], [0, 20, 0], [0, 0, 30], [10, 0, 30], [10, 20, 30], [0, 20, 30]];
const faces = [[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [3, 7, 6], [3, 6, 2], [0, 4, 7], [0, 7, 3], [1, 2, 6], [1, 6, 5]];
function box() {
  const bytes = Buffer.alloc(84 + faces.length * 50);
  bytes.write("Sample: asymmetric 10 x 20 x 30 mm camera box");
  bytes.writeUInt32LE(faces.length, 80);
  faces.forEach((face, index) => face.forEach((point, vertex) => points[point].forEach((value, axis) => {
    bytes.writeFloatLE(value, 84 + index * 50 + 12 + vertex * 12 + axis * 4);
  })));
  return bytes;
}

let phase = "launch Chromium";
let originalError;
let current;
const contexts = [];
const issues = [];
// Track only commands sent by this driver. No private app state is inspected.
const expanded = new WeakMap();
const watchdog = setTimeout(() => {
  console.error(`Camera browser checks exceeded 240 seconds during: ${phase}`);
  if (originalError) console.error("Original camera failure:", originalError);
  process.exit(1);
}, 240000);
function progress(message) { phase = message; console.log(`Camera browser phase: ${message}`); }
async function paint(page) {
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  // Software WebGL may consume input after the browser animation callback.
  await page.waitForTimeout(250);
}
async function key(page, value) {
  await page.keyboard.press(value);
  if (value === "Escape") expanded.set(page, false);
  else if (value === "Control+m" || value === "F2") expanded.set(page, !expanded.get(page));
  await paint(page);
}
async function tab(page, count) { for (let index = 0; index < count; index++) await key(page, "Tab"); }
function renderLog(name, png) {
  const allowed = new Set(["camera-orthographic", "camera-perspective", "camera-axis-x", "camera-axis-y", "camera-axis-z", "camera-mobile", "camera-failure"]);
  const requested = new Set((process.env.JIG_RENDER_LOGS ?? "").split(","));
  if (!allowed.has(name) || !requested.has(name) || png.length > 1024 * 1024) return;
  console.log(`JIG_RENDER_BEGIN_${name}`);
  const encoded = png.toString("base64");
  for (let offset = 0; offset < encoded.length; offset += 2048) console.log(encoded.slice(offset, offset + 2048));
  console.log(`JIG_RENDER_END_${name}`);
}
async function capture(page, name, timeout = 60000) {
  const png = await page.screenshot({ path: resolve(results, `${name}.png`), timeout, animations: "disabled", caret: "hide" });
  renderLog(name, png);
  return png;
}
async function modelPixels(page) {
  // Exclude the toolbar, statistics, and the gnomon's bottom-left corner.
  return page.screenshot({ clip: { x: 120, y: 200, width: 620, height: 540 }, timeout: 60000, animations: "disabled", caret: "hide" });
}
const browser = await chromium.launch({ headless: true, args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"] });
async function open(label, viewport = { width: 1440, height: 1000 }) {
  const context = await browser.newContext({ viewport, acceptDownloads: true, hasTouch: viewport.width < 500, timezoneId: "Etc/UTC" });
  contexts.push(context);
  await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin });
  await context.addInitScript(({ raw, storageKey, origin }) => {
    if (location.origin === origin && sessionStorage.getItem("jig-camera-seeded") !== "yes") {
      localStorage.setItem(storageKey, raw);
      sessionStorage.setItem("jig-camera-seeded", "yes");
    }
  }, { raw: rawFixture, storageKey, origin });
  await context.route("**/*", async route => {
    const url = new URL(route.request().url());
    if (["blob:", "data:", "about:"].includes(url.protocol) || url.origin === origin) await route.continue();
    else { issues.push(`${label}: unexpected remote request ${url.origin}${url.pathname}`); await route.abort(); }
  });
  const page = await context.newPage();
  page.on("pageerror", error => issues.push(`${label}: ${error.message}`));
  page.setDefaultTimeout(15000);
  await page.clock.setFixedTime(new Date("2026-10-30T12:00:00Z"));
  await page.goto(base, { waitUntil: "load" });
  await page.bringToFront();
  await page.waitForFunction(() => window.jigReady === true || typeof window.jigBootError === "string", undefined, { timeout: 60000 });
  assert.equal(await page.evaluate(() => window.jigBootError), undefined);
  await page.waitForTimeout(500);
  expanded.set(page, false);
  await page.locator("#jig-stl-import").setInputFiles({ name: "Sample-camera-box.stl", mimeType: "model/stl", buffer: box() });
  await page.waitForTimeout(1300);
  await paint(page);
  await maximize(page);
  return { context, page };
}
async function maximize(page) {
  if (!expanded.get(page)) {
    await page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
    await key(page, "Escape");
    await key(page, "Control+Alt+5");
    await key(page, "Control+m");
  }
  await key(page, "Control+Alt+5");
  await page.waitForTimeout(250);
}
async function control(page, position) {
  await maximize(page);
  await tab(page, position);
  await key(page, "Space");
}
async function report(page) {
  let value = "";
  for (let attempt = 0; attempt < 2; attempt++) {
    await page.evaluate(() => navigator.clipboard.writeText("Camera report pending"));
    await maximize(page);
    if (attempt) await page.waitForTimeout(250);
    await tab(page, 3);
    await key(page, "Space");
    const deadline = Date.now() + 3000;
    while (Date.now() < deadline) {
      value = await page.evaluate(() => navigator.clipboard.readText());
      if (value.startsWith("Jig STL report\n")) return value;
      await page.waitForTimeout(50);
    }
    if (!attempt) console.warn("Camera report unavailable; retrying only the read action after reanchoring.");
  }
  assert.fail(`Copy report did not produce an STL report; clipboard began ${JSON.stringify(value.slice(0, 150))}`);
}
function measurements(value) {
  const number = "([-+]?\\d+(?:\\.\\d+)?(?:[eE][-+]?\\d+)?)";
  const dimensions = value.split("\n").find(line => line.startsWith("Dimensions:"));
  const area = value.match(new RegExp(`Surface area:\\s*${number}\\s*mm`));
  const volume = value.match(new RegExp(`Total volume:\\s*${number}\\s*mm`));
  assert.ok(dimensions && area && volume, "Closed camera fixture must report physical measurements");
  return { dimensions: [...dimensions.matchAll(new RegExp(number, "g"))].slice(0, 3).map(match => Number(match[1])), area: Number(area[1]), volume: Number(volume[1]) };
}
function camera(value) {
  const yaw = value.match(/^Camera yaw:\s*([-+\d.eE]+) rad$/m);
  const pitch = value.match(/^Camera pitch:\s*([-+\d.eE]+) rad$/m);
  const projection = value.match(/^Projection:\s*(.+)$/m);
  assert.ok(yaw && pitch && projection, "Copied report must expose the actual camera and projection");
  return { yaw: Number(yaw[1]), pitch: Number(pitch[1]), projection: projection[1] };
}
function close(actual, expected, message) { assert.ok(Math.abs(actual - expected) <= 0.000002, `${message}: ${actual} != ${expected}`); }
async function exported(page) {
  await maximize(page);
  await tab(page, 4);
  const downloadEvent = page.waitForEvent("download", { timeout: 5000 });
  await key(page, "Space");
  const download = await downloadEvent;
  assert.match(download.suggestedFilename(), /-mm\.stl$/i, "The default export must identify millimeters");
  return readFile(await download.path());
}

async function clickAxis(page, axis) {
  // Locate the visible, colored label in actual rendered pixels. This reads a
  // screenshot, not application state, and adapts to phone/desktop clipping.
  const png = await page.screenshot({ timeout: 60000, animations: "disabled", caret: "hide" });
  const point = await page.evaluate(async ({ encoded, axis }) => {
    const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
    const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
    const context = canvas.getContext("2d");
    context.drawImage(bitmap, 0, 0);
    const pixels = context.getImageData(0, 0, bitmap.width, bitmap.height).data;
    const colors = { X: [234, 133, 173], Y: [148, 188, 160], Z: [109, 185, 238] };
    const near = (offset, target) => target.every((value, index) => Math.abs(pixels[offset + index] - value) <= 4);
    const colored = { X: [], Y: [], Z: [] };
    for (let y = 200; y < bitmap.height - 45; y++) for (let x = 15; x < Math.min(130, bitmap.width); x++) {
      const offset = (y * bitmap.width + x) * 4;
      for (const [name, color] of Object.entries(colors)) if (near(offset, color)) colored[name].push({ x, y });
    }
    const all = Object.values(colored).flat();
    if (!all.length || !colored[axis].length) return { error: `Visible ${axis} axis color was absent` };
    const left = Math.max(0, Math.min(...all.map(point => point.x)) - 8), right = Math.min(bitmap.width - 1, Math.max(...all.map(point => point.x)) + 8);
    const top = Math.max(0, Math.min(...all.map(point => point.y)) - 8), bottom = Math.min(bitmap.height - 1, Math.max(...all.map(point => point.y)) + 8);
    const centers = [];
    for (let y = top; y <= bottom; y++) for (let x = left; x <= right; x++) {
      if (near((y * bitmap.width + x) * 4, [246, 218, 174])) centers.push({ x, y });
    }
    if (!centers.length) return { error: "The gnomon's visible cream pivot was absent" };
    const pivot = { x: centers.reduce((sum, point) => sum + point.x, 0) / centers.length, y: centers.reduce((sum, point) => sum + point.y, 0) / centers.length };
    const distance = point => (point.x - pivot.x) ** 2 + (point.y - pivot.y) ** 2;
    const label = colored[axis].reduce((farthest, point) => distance(point) > distance(farthest) ? point : farthest);
    return { x: label.x, y: label.y, radius: Math.sqrt(distance(label)) };
  }, { encoded: png.toString("base64"), axis });
  assert.equal(point.error, undefined, point.error);
  assert.ok(point.radius >= 18 && point.radius <= 45, `The ${axis} click must target its visible outer label, not the center`);
  const viewport = page.viewportSize();
  if (viewport.width < 500) await page.touchscreen.tap(point.x, point.y);
  else await page.mouse.click(point.x, point.y, { delay: 50 });
  await paint(page);
}

try {
  progress("switch orthographic and perspective views without changing physical geometry");
  current = await open("camera-desktop");
  const baseline = await report(current.page);
  assert.deepEqual(measurements(baseline), { dimensions: [10, 20, 30], area: 2200, volume: 6000 });
  assert.equal(camera(baseline).projection, "Orthographic");
  const initialExport = await exported(current.page);
  const orthographic = await modelPixels(current.page);
  await capture(current.page, "camera-orthographic");
  await control(current.page, 12); // Projection follows the + view control.
  const perspectiveReport = await report(current.page);
  assert.equal(camera(perspectiveReport).projection, "Perspective (45°)");
  assert.deepEqual(measurements(perspectiveReport), measurements(baseline));
  const perspective = await modelPixels(current.page);
  assert.notDeepEqual(perspective, orthographic, "Perspective must visibly change the model projection");
  await capture(current.page, "camera-perspective");
  await key(current.page, "Control+z");
  assert.deepEqual(camera(await report(current.page)), camera(baseline), "One Undo restores the prior projection");
  assert.deepEqual(await modelPixels(current.page), orthographic, "Undo restores the complete orthographic view");
  await control(current.page, 12);
  assert.equal(camera(await report(current.page)).projection, "Perspective (45°)");

  progress("click the actual X, Y and Z gnomon labels and undo each snap");
  for (const [axis, expectedYaw, expectedPitch] of [["X", -Math.PI / 2, 0], ["Y", Math.PI, 0], ["Z", 0, Math.PI / 2]]) {
    const before = camera(await report(current.page));
    const beforePixels = await modelPixels(current.page);
    await clickAxis(current.page, axis);
    const snappedReport = await report(current.page);
    const snapped = camera(snappedReport);
    close(snapped.yaw, expectedYaw, `${axis} snap yaw`);
    close(snapped.pitch, expectedPitch, `${axis} snap pitch`);
    assert.equal(snapped.projection, "Perspective (45°)", "Axis snaps preserve the selected projection");
    assert.deepEqual(measurements(snappedReport), measurements(baseline));
    assert.notDeepEqual(await modelPixels(current.page), beforePixels, `${axis} snap changes the visible orientation`);
    await capture(current.page, `camera-axis-${axis.toLowerCase()}`);
    await key(current.page, "Control+z");
    assert.deepEqual(camera(await report(current.page)), before, `${axis} snap Undo restores the complete previous camera`);
    assert.deepEqual(await modelPixels(current.page), beforePixels, `${axis} snap Undo restores model pixels`);
  }

  progress("keep camera changes out of exported STL vertices and persistent state");
  assert.deepEqual(await exported(current.page), initialExport, "Projection and orientation must never bake into exported geometry");
  assert.equal(await current.page.evaluate(key => localStorage.getItem(key), storageKey), rawFixture);
  await current.context.close();

  progress("tap a visible axis on a touch viewport");
  current = await open("camera-mobile", { width: 390, height: 844 });
  await clickAxis(current.page, "X");
  const mobile = await report(current.page);
  close(camera(mobile).yaw, -Math.PI / 2, "Touch X snap yaw");
  close(camera(mobile).pitch, 0, "Touch X snap pitch");
  assert.deepEqual(measurements(mobile), measurements(baseline));
  await capture(current.page, "camera-mobile");
  assert.equal(await current.page.evaluate(key => localStorage.getItem(key), storageKey), rawFixture);
  assert.deepEqual(issues, [], "Camera interactions must not emit app errors or contact remote services");
  console.log("Camera browser checks passed: real orthographic/perspective45 controls, visible XYZ label mouse/touch snaps, camera Undo, invariant physical measurements and exported vertices, asymmetric geometry, and saved-data isolation.");
} catch (error) {
  originalError = error;
  console.error(`Camera browser failed during: ${phase}`);
  if (current?.page && !current.page.isClosed()) await capture(current.page, "camera-failure", 3000).catch(() => {});
  throw error;
} finally {
  progress("close camera contexts and Chromium");
  let cleanupTimer;
  let cleanupError;
  try {
    await Promise.race([
      Promise.all([...contexts.map(context => context.close().catch(() => {})), browser.close()]),
      new Promise((_, reject) => { cleanupTimer = setTimeout(() => reject(new Error("Camera browser shutdown exceeded 10 seconds")), 10000); }),
    ]);
  } catch (error) { cleanupError = error; }
  finally { clearTimeout(cleanupTimer); clearTimeout(watchdog); }
  if (cleanupError) {
    if (originalError) console.error("Original camera browser failure:", originalError);
    console.error("Camera browser cleanup failed:", cleanupError);
    process.exit(1);
  }
}

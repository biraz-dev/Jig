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

function greyball() {
  // Closed, slightly grooved sphere: its changing face normals exercise a
  // matcap more meaningfully than a box with three uniformly shaded planes.
  const segments = 48, rings = 32, radius = 20;
  const vertices = [[0, 0, radius]];
  for (let latitude = 1; latitude < rings; latitude++) {
    const phi = Math.PI * latitude / rings;
    const radial = radius * Math.sin(phi) * (1 + 0.07 * Math.cos(8 * phi));
    for (let longitude = 0; longitude < segments; longitude++) {
      const theta = 2 * Math.PI * longitude / segments;
      vertices.push([radial * Math.cos(theta), radial * Math.sin(theta), radius * Math.cos(phi)]);
    }
  }
  const bottom = vertices.push([0, 0, -radius]) - 1;
  const ring = (latitude, longitude) => 1 + (latitude - 1) * segments + longitude % segments;
  const triangles = [];
  for (let longitude = 0; longitude < segments; longitude++) {
    triangles.push([0, ring(1, longitude), ring(1, longitude + 1)]);
    for (let latitude = 1; latitude < rings - 1; latitude++) {
      const a = ring(latitude, longitude), b = ring(latitude + 1, longitude);
      const c = ring(latitude + 1, longitude + 1), d = ring(latitude, longitude + 1);
      triangles.push([a, b, c], [a, c, d]);
    }
    triangles.push([bottom, ring(rings - 1, longitude + 1), ring(rings - 1, longitude)]);
  }
  const bytes = Buffer.alloc(84 + triangles.length * 50);
  bytes.write("Sample: grooved greyball material test");
  bytes.writeUInt32LE(triangles.length, 80);
  triangles.forEach((triangle, index) => triangle.forEach((vertex, corner) => vertices[vertex].forEach((value, axis) => {
    bytes.writeFloatLE(value, 84 + index * 50 + 12 + corner * 12 + axis * 4);
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
  const allowed = new Set(["camera-orthographic", "camera-perspective", "camera-axis-x", "camera-axis-y", "camera-axis-z", "camera-mobile", "camera-matcap", "camera-matcap-mobile", "camera-failure"]);
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
    const near = (offset, target, tolerance = 4) => target.every((value, index) => Math.abs(pixels[offset + index] - value) <= tolerance);
    const colored = { X: [], Y: [], Z: [] };
    for (let y = 200; y < bitmap.height - 45; y++) for (let x = 15; x < Math.min(130, bitmap.width); x++) {
      const offset = (y * bitmap.width + x) * 4;
      // Thin font strokes blend with the dark plate. Keep the pivot strict,
      // but include the visible anti-aliased axis glyph as well as its line.
      for (const [name, color] of Object.entries(colors)) if (near(offset, color, 20)) colored[name].push({ x, y });
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

async function matcapVisual(page, kind) {
  const png = await page.screenshot({ timeout: 60000, animations: "disabled", caret: "hide" });
  return page.evaluate(async ({ encoded, kind }) => {
    const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
    const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
    const context = canvas.getContext("2d");
    context.drawImage(bitmap, 0, 0);
    const pixels = context.getImageData(0, 0, bitmap.width, bitmap.height).data;
    const phone = bitmap.width < 500;
    const bounds = kind === "button"
      ? { left: 20, right: 150, top: phone ? 180 : 115, bottom: phone ? 650 : 235 }
      : { left: phone ? 100 : 130, right: phone ? bitmap.width - 20 : 780, top: phone ? 350 : 190, bottom: phone ? 610 : 850 };
    const near = (offset, target) => target.every((value, axis) => Math.abs(pixels[offset + axis] - value) <= 3);
    const accepts = offset => kind === "button"
      ? [[21, 31, 43], [38, 54, 73], [43, 66, 90], [40, 72, 101], [60, 60, 60]].some(color => near(offset, color))
      : pixels[offset] >= 45 && pixels[offset] <= 245 && Math.abs(pixels[offset] - pixels[offset + 1]) <= 2 && Math.abs(pixels[offset] - pixels[offset + 2]) <= 2;
    const seen = new Uint8Array(bitmap.width * bitmap.height), regions = [];
    for (let y = bounds.top; y < bounds.bottom; y++) for (let x = bounds.left; x < bounds.right; x++) {
      const start = y * bitmap.width + x;
      if (seen[start] || !accepts(start * 4)) continue;
      const stack = [start];
      seen[start] = 1;
      const region = { left: x, right: x, top: y, bottom: y, count: 0, minimum: 255, maximum: 0 };
      const shades = new Set();
      while (stack.length) {
        const index = stack.pop(), py = Math.floor(index / bitmap.width), px = index % bitmap.width, shade = pixels[index * 4];
        region.left = Math.min(region.left, px); region.right = Math.max(region.right, px);
        region.top = Math.min(region.top, py); region.bottom = Math.max(region.bottom, py); region.count++;
        region.minimum = Math.min(region.minimum, shade); region.maximum = Math.max(region.maximum, shade); shades.add(Math.floor(shade / 5));
        for (const [nx, ny] of [[px - 1, py], [px + 1, py], [px, py - 1], [px, py + 1]]) {
          if (nx < bounds.left || nx >= bounds.right || ny < bounds.top || ny >= bounds.bottom) continue;
          const next = ny * bitmap.width + nx;
          if (!seen[next] && accepts(next * 4)) { seen[next] = 1; stack.push(next); }
        }
      }
      regions.push({ ...region, width: region.right - region.left + 1, height: region.bottom - region.top + 1, shades: shades.size });
    }
    bitmap.close();
    const candidates = kind === "button"
      ? regions.filter(region => region.width >= 100 && region.width <= 125 && region.height >= 18 && region.height <= 26 && region.count > 600).sort((a, b) => a.top - b.top)
      : regions.filter(region => region.width > 80 && region.height > 80 && region.count > 2000).sort((a, b) => b.count - a.count);
    return candidates[0] ?? null;
  }, { encoded: png.toString("base64"), kind });
}
async function clickMatcap(page) {
  if (page.viewportSize().width >= 500) { await page.mouse.move(12, 12); await paint(page); }
  const button = await matcapVisual(page, "button");
  assert.ok(button, "The grey matcap toggle must actually be visible inside the viewport");
  const x = (button.left + button.right) / 2, y = (button.top + button.bottom) / 2;
  if (page.viewportSize().width < 500) await page.touchscreen.tap(x, y);
  else { await page.mouse.click(x, y, { delay: 50 }); await page.mouse.move(12, 12); }
  await paint(page);
}
async function curvedFront(page) {
  await page.locator("#jig-stl-import").setInputFiles({ name: "Sample-grooved-greyball.stl", mimeType: "model/stl", buffer: greyball() });
  await page.waitForTimeout(1300);
  await paint(page);
  await control(page, 5); // Iso restores orthographic projection.
  await control(page, 6); // Front looks directly onto the XZ plane.
}
async function assertMatcap(page) {
  const body = await matcapVisual(page, "body");
  assert.ok(body, "The curved model must render a substantial neutral-grey surface");
  assert.ok(body.maximum - body.minimum >= 35 && body.shades >= 6, `The greyball must show its changing surface normals through varied studio shading, not a flat grey fill: ${JSON.stringify(body)}`);
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

  progress("inspect a curved front-facing object with neutral grey matcap and Undo");
  await curvedFront(current.page);
  const warmReport = await report(current.page);
  assert.match(warmReport, /^Viewport material: Warm clay$/m);
  assert.deepEqual(camera(warmReport), { yaw: 0, pitch: 0, projection: "Orthographic" });
  const warmPixels = await modelPixels(current.page);
  const curvedExport = await exported(current.page);
  await clickMatcap(current.page);
  const greyReport = await report(current.page);
  assert.match(greyReport, /^Viewport material: Grey matcap$/m);
  assert.deepEqual(camera(greyReport), camera(warmReport), "Changing viewport material must keep the front view");
  assert.deepEqual(measurements(greyReport), measurements(warmReport), "Matcap must not change any physical measurements");
  await assertMatcap(current.page);
  assert.notDeepEqual(await modelPixels(current.page), warmPixels, "Neutral matcap must visibly replace the warm material");
  await capture(current.page, "camera-matcap");
  assert.deepEqual(await exported(current.page), curvedExport, "A viewport material must never change exported STL bytes");
  await key(current.page, "Control+z");
  assert.match(await report(current.page), /^Viewport material: Warm clay$/m, "One Undo restores the previous viewport material");
  assert.deepEqual(await modelPixels(current.page), warmPixels, "Material Undo restores the warm render exactly");
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

  progress("tap the grey matcap toggle on a phone and use its keyboard shortcut");
  await curvedFront(current.page);
  const mobileWarm = await report(current.page);
  await clickMatcap(current.page);
  const mobileGrey = await report(current.page);
  assert.match(mobileGrey, /^Viewport material: Grey matcap$/m);
  assert.deepEqual(measurements(mobileGrey), measurements(mobileWarm));
  assert.deepEqual(camera(mobileGrey), camera(mobileWarm));
  await assertMatcap(current.page);
  await key(current.page, "Control+Alt+g");
  assert.match(await report(current.page), /^Viewport material: Warm clay$/m, "The material shortcut must also work without a pointer");
  await key(current.page, "Control+z");
  assert.match(await report(current.page), /^Viewport material: Grey matcap$/m, "Undo restores the material changed by keyboard");
  await capture(current.page, "camera-matcap-mobile");
  assert.equal(await current.page.evaluate(key => localStorage.getItem(key), storageKey), rawFixture);
  assert.deepEqual(issues, [], "Camera interactions must not emit app errors or contact remote services");
  console.log("Camera browser checks passed: real orthographic/perspective45 controls, visible XYZ label mouse/touch snaps, camera Undo, curved front-view grey matcap mouse/touch/keyboard controls with neutral varied shading and Undo, invariant physical measurements and exported vertices, asymmetric geometry, and saved-data isolation.");
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

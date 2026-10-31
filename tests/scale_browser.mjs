// End-to-end physical STL scaling. All models and saved records are synthetic.
// Serve the built preview at /Jig/, then: node tests/scale_browser.mjs
import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium } from "playwright";

const base = process.env.JIG_PREVIEW_URL ?? "http://127.0.0.1:4173/Jig/";
const origin = new URL(base).origin;
const storageKey = "jig.personal.v1";
const results = resolve("playwright-results");
const screenshotOptions = { timeout: 60000, animations: "disabled", caret: "hide" };
await mkdir(results, { recursive: true });
const fixture = {
  version: 1, preferences: { decimals: 2, focus: 3, weather: null },
  history: [{ expression: "2 + 2", result: 4 }],
  tasks: [{ id: 1, text: "Sample: keep sculpture checklist", complete: false }],
  deadlines: [], entries: [], featured: null,
};
const fixtureRaw = JSON.stringify(fixture);
const points = [[0, 0, 0], [10, 0, 0], [10, 10, 0], [0, 10, 0], [0, 0, 10], [10, 0, 10], [10, 10, 10], [0, 10, 10]];
const faces = [[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [3, 7, 6], [3, 6, 2], [0, 4, 7], [0, 7, 3], [1, 2, 6], [1, 6, 5]];
function cube() {
  const bytes = Buffer.alloc(84 + faces.length * 50);
  bytes.write("Sample: physical-scale cube");
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
const expanded = new WeakMap();
const watchdog = setTimeout(() => {
  console.error(`Scale browser exceeded 360 seconds during: ${phase}`);
  if (originalError) console.error(originalError);
  process.exit(1);
}, 360000);
function progress(message) { phase = message; console.log(`Scale browser phase: ${message}`); }
async function paint(page) {
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  // Software WebGL may consume input after the browser's first animation frame.
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
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(`${description} exceeded ${milliseconds}ms`)), milliseconds);
      }),
    ]);
  } finally { clearTimeout(timer); }
}
async function capture(page, name, timeout = 5000) {
  // Named images are review artifacts. Pixel assertions below use independent,
  // strict screenshots; a stalled compositor readback must not skip later tests.
  let png;
  try {
    png = await page.screenshot({ ...screenshotOptions, timeout: Math.min(timeout, 5000), path: resolve(results, `${name}.png`) });
  } catch (error) {
    console.warn(`Optional scale review capture ${name} unavailable: ${String(error).split("\n")[0]}`);
    const data = await bounded(page.evaluate(async () => {
      const source = document.getElementById("jig_canvas");
      if (!source) return "";
      const encoded = source.toDataURL("image/png");
      if (!encoded.startsWith("data:image/png;base64,")) return "";
      const bytes = Uint8Array.from(atob(encoded.split(",")[1]), value => value.charCodeAt(0));
      const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
      const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
      const context = canvas.getContext("2d");
      context.drawImage(bitmap, 0, 0);
      const pixels = context.getImageData(0, 0, bitmap.width, bitmap.height).data;
      bitmap.close();
      const step = Math.max(4, Math.floor(pixels.length / 4096 / 4) * 4);
      let visible = 0, varied = 0;
      for (let offset = 0; offset < pixels.length; offset += step) {
        if (pixels[offset + 3] === 0) continue;
        if (pixels[offset] + pixels[offset + 1] + pixels[offset + 2] > 100) visible++;
        if (Math.abs(pixels[offset] - pixels[0]) + Math.abs(pixels[offset + 1] - pixels[1]) + Math.abs(pixels[offset + 2] - pixels[2]) > 20) varied++;
      }
      // Discard transparent or uniform WebGL buffers instead of saving a blank.
      return visible >= 16 && varied >= 8 ? encoded : "";
    }), 3000, "Scale canvas review fallback").catch(() => "");
    if (!data) {
      console.warn(`No nonblank canvas fallback was available for ${name}; continuing functional checks.`);
      return;
    }
    png = Buffer.from(data.slice("data:image/png;base64,".length), "base64");
    await writeFile(resolve(results, `${name}.png`), png);
    console.warn(`Saved scale review capture ${name} through the public canvas fallback.`);
  }
  const requested = new Set((process.env.JIG_RENDER_LOGS ?? "").split(","));
  if (["scale-axes", "scale-target", "scale-mobile", "scale-failure"].includes(name) && requested.has(name) && png.length <= 1024 * 1024) {
    console.log(`JIG_RENDER_BEGIN_${name}`);
    const encoded = png.toString("base64");
    for (let offset = 0; offset < encoded.length; offset += 2048) console.log(encoded.slice(offset, offset + 2048));
    console.log(`JIG_RENDER_END_${name}`);
  }
}
const browser = await chromium.launch({ headless: true, args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"] });
async function open(label, viewport = { width: 1440, height: 1000 }) {
  const context = await browser.newContext({ viewport, acceptDownloads: true, hasTouch: viewport.width < 500, timezoneId: "Etc/UTC" });
  contexts.push(context);
  await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin });
  await context.addInitScript(({ origin, storageKey, raw }) => {
    if (location.origin === origin && !sessionStorage.getItem("jig-scale-seeded")) {
      localStorage.setItem(storageKey, raw);
      sessionStorage.setItem("jig-scale-seeded", "yes");
    }
  }, { origin, storageKey, raw: fixtureRaw });
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
  expanded.set(page, false);
  await page.waitForTimeout(500);
  return { context, page };
}
async function stl(page) {
  if (!expanded.get(page)) {
    await page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
    await key(page, "Escape");
    await key(page, "Control+Alt+5");
    await key(page, "Control+m");
  }
  await key(page, "Control+Alt+5");
  await page.waitForTimeout(250);
}
async function size(page) {
  await stl(page);
  await key(page, "Control+Alt+s");
  await page.waitForTimeout(250);
}
async function upload(page, bytes, name) {
  await page.locator("#jig-stl-import").setInputFiles({ name, mimeType: "model/stl", buffer: bytes });
  await page.waitForTimeout(1300);
  await paint(page);
}
async function raw(page) { return page.evaluate(key => localStorage.getItem(key), storageKey); }
async function readReport(page) {
  let report = "";
  for (let attempt = 0; attempt < 2; attempt++) {
    await page.evaluate(() => navigator.clipboard.writeText("Scale report pending"));
    await stl(page);
    if (attempt) await page.waitForTimeout(250);
    await tab(page, 3); // Open STL -> Sample cube, Clear, Copy report.
    await key(page, "Space");
    const deadline = Date.now() + 3000;
    while (Date.now() < deadline) {
      report = await page.evaluate(() => navigator.clipboard.readText());
      if (report.startsWith("Jig STL report\n")) return report;
      await page.waitForTimeout(50);
    }
    if (!attempt) console.warn("Scale report unavailable; reanchoring the read-only Copy report action once.");
  }
  assert.fail(`Scale Copy report did not produce a report: ${JSON.stringify(report.slice(0, 160))}`);
}
async function editFocused(page, value) {
  // DragValue already enters text editing when it receives Tab focus.
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 3000 });
  await page.keyboard.press("Control+a");
  await page.keyboard.type(String(value), { delay: 30 });
  await key(page, "Enter");
}
async function master(page, value) { await stl(page); await tab(page, 17); await editFocused(page, value); }
async function axis(page, index, value) { await size(page); await tab(page, 1 + index); await editFocused(page, value); }
async function target(page, index, value) { await size(page); await tab(page, 4 + index); await editFocused(page, value); }
async function toggleLinked(page) { await size(page); await key(page, "Space"); }
async function resetScaling(page) { await size(page); await tab(page, 7); await key(page, "Space"); }
async function exportUnit(page, index) { await size(page); await tab(page, 8 + index); await key(page, "Space"); }
async function sourceUnit(page, index) { await stl(page); await tab(page, 14 + index); await key(page, "Space"); }
async function shrinkMode(page, index) { await stl(page); await tab(page, 18 + index); await key(page, "Space"); }
async function fit(page) { await stl(page); await tab(page, 9); await key(page, "Space"); }
async function front(page) { await stl(page); await tab(page, 6); await key(page, "Space"); }
async function undo(page) { await stl(page); await key(page, "Control+z"); }
async function download(page) {
  await stl(page);
  await tab(page, 4); // Open STL -> Sample, Clear, Copy, Export.
  const event = page.waitForEvent("download", { timeout: 10000 });
  await key(page, "Space");
  const file = await event;
  return { name: file.suggestedFilename(), bytes: await readFile(await file.path()) };
}
async function pixels(page) {
  return page.screenshot({ ...screenshotOptions, clip: { x: 100, y: 200, width: 600, height: 500 } });
}
async function modelAspect(page) {
  // Read the public rendered pixels, excluding the labels and corner gnomon.
  // The warm model fill is distinct from Jig's blue/charcoal background.
  const png = await page.screenshot({ ...screenshotOptions, clip: { x: 50, y: 190, width: 750, height: 690 } });
  return page.evaluate(async encoded => {
    const bytes = Uint8Array.from(atob(encoded), value => value.charCodeAt(0));
    const image = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const surface = new OffscreenCanvas(image.width, image.height);
    const context = surface.getContext("2d");
    context.drawImage(image, 0, 0);
    const data = context.getImageData(0, 0, image.width, image.height).data;
    let left = image.width, right = -1, top = image.height, bottom = -1;
    for (let y = 0; y < image.height; y++) for (let x = 0; x < image.width; x++) {
      const offset = (y * image.width + x) * 4;
      const [red, green, blue] = data.subarray(offset, offset + 3);
      if (red > 40 && red > green + 5 && green > blue + 5) {
        left = Math.min(left, x); right = Math.max(right, x); top = Math.min(top, y); bottom = Math.max(bottom, y);
      }
    }
    image.close();
    return { width: right - left + 1, height: bottom - top + 1 };
  }, png.toString("base64"));
}
function close(actual, expected, message) {
  assert.ok(Math.abs(actual - expected) <= Math.max(0.00001, Math.abs(expected) * 0.000002), `${message}: ${actual} != ${expected}`);
}
function values(report) {
  const number = "([-+]?\\d+(?:\\.\\d+)?(?:[eE][-+]?\\d+)?)";
  const area = report.match(new RegExp(`Surface area:\\s*${number}\\s*mm`));
  const volume = report.match(new RegExp(`Total volume:\\s*${number}\\s*mm`));
  const line = report.split("\n").find(line => line.startsWith("Dimensions:"));
  assert.ok(area && volume && line, "A closed cube must report dimensions, surface area and total volume");
  return { dimensions: [...line.matchAll(new RegExp(number, "g"))].slice(0, 3).map(match => Number(match[1])), area: Number(area[1]), volume: Number(volume[1]) };
}
function dimensions(actual, expected, message) { actual.forEach((value, index) => close(value, expected[index], `${message} ${"XYZ"[index]}`)); }
function box(report, sides) {
  const measured = values(report);
  dimensions(measured.dimensions, sides, "Physical dimension");
  close(measured.area, 2 * (sides[0] * sides[1] + sides[0] * sides[2] + sides[1] * sides[2]), "Box surface area");
  close(measured.volume, sides[0] * sides[1] * sides[2], "Box enclosed volume");
  return measured;
}
function geometry(bytes) {
  const count = bytes.readUInt32LE(80);
  assert.equal(count, 12, "Cube export must retain its twelve triangles");
  assert.equal(bytes.length, 84 + count * 50);
  const minimum = [Infinity, Infinity, Infinity], maximum = [-Infinity, -Infinity, -Infinity];
  let area = 0, signed = 0;
  for (let face = 0; face < count; face++) {
    const offset = 84 + face * 50;
    const normal = [0, 1, 2].map(axis => bytes.readFloatLE(offset + axis * 4));
    const triangle = [0, 1, 2].map(point => [0, 1, 2].map(axis => bytes.readFloatLE(offset + 12 + point * 12 + axis * 4)));
    for (const point of triangle) point.forEach((value, axis) => {
      assert.ok(Number.isFinite(value)); minimum[axis] = Math.min(minimum[axis], value); maximum[axis] = Math.max(maximum[axis], value);
    });
    const [a, b, c] = triangle;
    const u = b.map((value, axis) => value - a[axis]), v = c.map((value, axis) => value - a[axis]);
    const cross = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    const length = Math.hypot(...cross);
    assert.ok(length > 0, "Export must not collapse a face");
    close(Math.hypot(...normal), 1, "Exported normal length");
    normal.forEach((value, axis) => close(value, cross[axis] / length, "Exported normal direction"));
    area += length / 2;
    const bc = [b[1] * c[2] - b[2] * c[1], b[2] * c[0] - b[0] * c[2], b[0] * c[1] - b[1] * c[0]];
    signed += a.reduce((sum, value, axis) => sum + value * bc[axis], 0) / 6;
  }
  return { dimensions: maximum.map((value, axis) => value - minimum[axis]), area, volume: Math.abs(signed) };
}
function exportedBox(file, sides, unit = "mm") {
  assert.match(file.name, new RegExp(`[-_]${unit}\\.stl$`, "i"), "Downloaded filename must make its units explicit");
  const measured = geometry(file.bytes);
  dimensions(measured.dimensions, sides, "Exported vertex bounds");
  close(measured.area, 2 * (sides[0] * sides[1] + sides[0] * sides[2] + sides[1] * sides[2]), "Exported surface area");
  close(measured.volume, sides[0] * sides[1] * sides[2], "Exported volume");
}

try {
  progress("uniform scaling and independent XYZ percentages");
  current = await open("scale-desktop");
  await stl(current.page);
  const empty = await pixels(current.page);
  await upload(current.page, cube(), "Sample-scale-cube.stl");
  box(await readReport(current.page), [10, 10, 10]);
  await master(current.page, 200);
  box(await readReport(current.page), [20, 20, 20]);
  await undo(current.page);
  box(await readReport(current.page), [10, 10, 10]);
  await toggleLinked(current.page);
  await axis(current.page, 0, 200);
  await axis(current.page, 1, 300);
  await axis(current.page, 2, 400);
  let report = await readReport(current.page);
  box(report, [20, 30, 40]);
  assert.match(report, /Scale edits: Independent/);
  await fit(current.page);
  await front(current.page);
  box(await readReport(current.page), [20, 30, 40]);
  const projected = await modelAspect(current.page);
  assert.ok(projected.width > 50 && projected.height > 50, "Scaled model must remain visible");
  assert.ok(Math.abs(projected.width / projected.height - 0.5) < 0.03, "Front preview must show X:Z dimensions of 20:40, not a uniformly scaled cube");
  await capture(current.page, "scale-axes");
  exportedBox(await download(current.page), [20, 30, 40]);
  await master(current.page, 150);
  box(await readReport(current.page), [30, 45, 60]);
  await undo(current.page);
  box(await readReport(current.page), [20, 30, 40]);

  progress("shrinkage and compensation use all three physical scale factors");
  await shrinkMode(current.page, 1);
  box(await readReport(current.page), [20, 30, 40].map(value => value * 0.94));
  await undo(current.page);
  await shrinkMode(current.page, 2);
  const compensated = [20, 30, 40].map(value => value / 0.94);
  box(await readReport(current.page), compensated);
  exportedBox(await download(current.page), compensated);
  await undo(current.page);
  box(await readReport(current.page), [20, 30, 40]);

  progress("undo each axis edit as one complete action");
  await undo(current.page); // Front changed only the camera after Fit.
  box(await readReport(current.page), [20, 30, 40]);
  await undo(current.page); // Fit changed only the camera and is newer than XYZ.
  box(await readReport(current.page), [20, 30, 40]);
  await undo(current.page);
  box(await readReport(current.page), [20, 30, 10]);
  await undo(current.page);
  box(await readReport(current.page), [20, 10, 10]);
  await undo(current.page);
  box(await readReport(current.page), [10, 10, 10]);
  await undo(current.page);
  assert.match(await readReport(current.page), /Scale edits: Linked/);

  progress("set independent target dimensions and link proportional resizing");
  await toggleLinked(current.page);
  await target(current.page, 0, 25);
  await target(current.page, 1, 35);
  await target(current.page, 2, 45);
  box(await readReport(current.page), [25, 35, 45]);
  await fit(current.page);
  await capture(current.page, "scale-target");
  exportedBox(await download(current.page), [25, 35, 45]);
  await toggleLinked(current.page);
  await target(current.page, 0, 50);
  box(await readReport(current.page), [50, 70, 90]);
  await undo(current.page);
  box(await readReport(current.page), [25, 35, 45]);

  progress("explicit export units and Undo after Clear restore the complete scene");
  await exportUnit(current.page, 2); // Inches.
  report = await readReport(current.page);
  assert.match(report, /Export units: in\b/);
  box(report, [25, 35, 45]);
  exportedBox(await download(current.page), [25, 35, 45].map(value => value / 25.4), "in");
  await stl(current.page);
  await tab(current.page, 2);
  await key(current.page, "Space"); // Clear.
  assert.deepEqual(await pixels(current.page), empty, "Clear must empty the viewer before Undo");
  await undo(current.page);
  report = await readReport(current.page);
  box(report, [25, 35, 45]);
  assert.match(report, /Export units: in\b/);
  assert.match(report, /Scale edits: Linked/);
  await undo(current.page);
  assert.match(await readReport(current.page), /Export units: mm\b/);
  await resetScaling(current.page);
  box(await readReport(current.page), [10, 10, 10]);
  await undo(current.page);
  box(await readReport(current.page), [25, 35, 45]);
  const resized = await download(current.page);
  exportedBox(resized, [25, 35, 45]);
  await upload(current.page, resized.bytes, "Sample-resized-mm.stl");
  report = await readReport(current.page);
  box(report, [25, 35, 45]);
  assert.match(report, /Source units: mm\b/);
  assert.equal(await raw(current.page), fixtureRaw, "Scene scaling and export settings must not rewrite portable workbench data");
  await current.context.close();

  progress("convert source centimeters and inches into millimeter export geometry");
  current = await open("scale-source-units");
  await upload(current.page, cube(), "Sample-centimeter-source.stl");
  await sourceUnit(current.page, 1);
  report = await readReport(current.page);
  assert.match(report, /Source units: cm\b/);
  box(report, [100, 100, 100]);
  const centimeters = await download(current.page);
  exportedBox(centimeters, [100, 100, 100]);
  await upload(current.page, centimeters.bytes, "Sample-cm-export-mm.stl");
  box(await readReport(current.page), [100, 100, 100]);
  await upload(current.page, cube(), "Sample-inch-source.stl");
  await sourceUnit(current.page, 2);
  report = await readReport(current.page);
  assert.match(report, /Source units: in\b/);
  box(report, [254, 254, 254]);
  const inches = await download(current.page);
  exportedBox(inches, [254, 254, 254]);
  await upload(current.page, inches.bytes, "Sample-in-export-mm.stl");
  box(await readReport(current.page), [254, 254, 254]);
  assert.equal(await raw(current.page), fixtureRaw);
  await current.context.close();

  progress("target sizing remains accessible on a touch-sized display");
  current = await open("scale-mobile", { width: 390, height: 844 });
  await upload(current.page, cube(), "Sample-mobile-scale.stl");
  await target(current.page, 0, 20);
  box(await readReport(current.page), [20, 20, 20]);
  await size(current.page);
  await capture(current.page, "scale-mobile");
  assert.equal(await current.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true);
  assert.equal(await raw(current.page), fixtureRaw);
  assert.deepEqual(issues, [], "Scale tests must stay local and emit no page errors");
  console.log("Scale browser passed: uniform and independent XYZ geometry, exact linked/unlinked targets, determinant volume and transformed area, shrinkage and compensated export, vertex bounds and normals in real mm/in downloads, scene Undo/reset/Clear, source cm/in to mm export and reimport, mobile sizing, and portable-data isolation.");
} catch (error) {
  originalError = error;
  console.error(`Scale browser failed during: ${phase}`);
  if (current?.page && !current.page.isClosed()) await capture(current.page, "scale-failure", 3000).catch(() => {});
  if (issues.length) console.error(issues.join("\n"));
  throw error;
} finally {
  progress("close scale contexts and Chromium");
  let cleanupTimer;
  let failed = false;
  try {
    await Promise.race([
      Promise.all([...contexts.map(context => context.close().catch(() => {})), browser.close()]),
      new Promise((_, reject) => {
        cleanupTimer = setTimeout(() => reject(new Error("Scale browser shutdown exceeded 10 seconds")), 10000);
      }),
    ]);
  } catch (error) {
    failed = true;
    if (originalError) console.error(originalError);
    console.error(error);
  } finally {
    clearTimeout(cleanupTimer);
    clearTimeout(watchdog);
  }
  if (failed) process.exit(1);
}

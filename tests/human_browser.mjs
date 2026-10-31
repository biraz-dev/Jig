// Physical human-reference checks. All meshes and saved records are synthetic.
// Serve the built preview at /Jig/, then: node tests/human_browser.mjs
import assert from "node:assert/strict";
import { mkdir, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium } from "playwright";

const base = process.env.JIG_PREVIEW_URL ?? "http://127.0.0.1:4173/Jig/";
const origin = new URL(base).origin;
const storageKey = "jig.personal.v1";
const results = resolve("playwright-results");
const screenshotOptions = { timeout: 60000, animations: "disabled", caret: "hide" };
const referenceHeight = 69 * 25.4;
const fixture = {
  version: 1, preferences: { decimals: 2, focus: 3, weather: null },
  history: [{ expression: "2 + 2", result: 4 }],
  tasks: [{ id: 1, text: "Sample: keep scale comparison separate", complete: false }],
  deadlines: [], entries: [], featured: null,
};
const fixtureRaw = JSON.stringify(fixture);
await mkdir(results, { recursive: true });

function box(sourceDivisor = 1) {
  // The source object's bottom is deliberately not at zero. The comparison
  // must align feet with its actual minimum Z, not the original coordinate zero.
  const points = [
    [-200, -150, 50], [200, -150, 50], [200, 150, 50], [-200, 150, 50],
    [-200, -150, 50 + referenceHeight], [200, -150, 50 + referenceHeight],
    [200, 150, 50 + referenceHeight], [-200, 150, 50 + referenceHeight],
  ];
  const faces = [[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [3, 7, 6], [3, 6, 2], [0, 4, 7], [0, 7, 3], [1, 2, 6], [1, 6, 5]];
  const bytes = Buffer.alloc(84 + faces.length * 50);
  bytes.write("Sample: human-height box with a raised source bottom");
  bytes.writeUInt32LE(faces.length, 80);
  faces.forEach((face, index) => face.forEach((point, vertex) => points[point].forEach((value, axis) => {
    bytes.writeFloatLE(value / sourceDivisor, 84 + index * 50 + 12 + vertex * 12 + axis * 4);
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
  console.error(`Human browser checks exceeded 360 seconds during: ${phase}`);
  if (originalError) console.error(originalError);
  process.exit(1);
}, 360000);
function progress(message) { phase = message; console.log(`Human browser phase: ${message}`); }
async function paint(page) {
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await page.waitForTimeout(250);
}
async function key(page, value) {
  await page.keyboard.press(value);
  if (value === "Escape") expanded.set(page, false);
  else if (value === "Control+m" || value === "F2") expanded.set(page, !expanded.get(page));
  await paint(page);
}
async function tab(page, count) { for (let index = 0; index < count; index++) await key(page, "Tab"); }
async function capture(page, name, timeout = 60000) {
  const png = await page.screenshot({ ...screenshotOptions, timeout, path: resolve(results, `${name}.png`) });
  const requested = new Set((process.env.JIG_RENDER_LOGS ?? "").split(","));
  if (["human-equal-height", "human-half-height", "human-centimeters", "human-mobile", "human-failure"].includes(name) && requested.has(name) && png.length <= 1024 * 1024) {
    console.log(`JIG_RENDER_BEGIN_${name}`);
    const encoded = png.toString("base64");
    for (let offset = 0; offset < encoded.length; offset += 2048) console.log(encoded.slice(offset, offset + 2048));
    console.log(`JIG_RENDER_END_${name}`);
  }
  return png;
}
const browser = await chromium.launch({ headless: true, args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"] });
async function open(label, viewport = { width: 1440, height: 1000 }, sourceDivisor = 1) {
  const context = await browser.newContext({ viewport, acceptDownloads: true, hasTouch: viewport.width < 500, timezoneId: "Etc/UTC" });
  contexts.push(context);
  await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin });
  await context.addInitScript(({ raw, storageKey, origin }) => {
    if (location.origin === origin && !sessionStorage.getItem("jig-human-seeded")) {
      localStorage.setItem(storageKey, raw);
      sessionStorage.setItem("jig-human-seeded", "yes");
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
  await page.goto(base, { waitUntil: "load" });
  await page.bringToFront();
  await page.waitForFunction(() => window.jigReady === true || typeof window.jigBootError === "string", undefined, { timeout: 60000 });
  assert.equal(await page.evaluate(() => window.jigBootError), undefined);
  expanded.set(page, false);
  await page.waitForTimeout(500);
  await page.locator("#jig-stl-import").setInputFiles({ name: `Sample-human-${sourceDivisor === 1 ? "millimeters" : "centimeters"}.stl`, mimeType: "model/stl", buffer: box(sourceDivisor) });
  await page.waitForTimeout(1300);
  await paint(page);
  await stl(page);
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
async function report(page) {
  let value = "";
  for (let attempt = 0; attempt < 2; attempt++) {
    await page.evaluate(() => navigator.clipboard.writeText("Human report pending"));
    await stl(page);
    if (attempt) await page.waitForTimeout(250);
    await tab(page, 3);
    await key(page, "Space");
    const deadline = Date.now() + 3000;
    while (Date.now() < deadline) {
      value = await page.evaluate(() => navigator.clipboard.readText());
      if (value.startsWith("Jig STL report\n")) return value;
      await page.waitForTimeout(50);
    }
    if (!attempt) console.warn("Human report unavailable; retrying only the read action after reanchoring.");
  }
  assert.fail(`Copy report did not produce a report: ${JSON.stringify(value.slice(0, 160))}`);
}
async function control(page, position) { await stl(page); await tab(page, position); await key(page, "Space"); }
async function exported(page) {
  await stl(page);
  await tab(page, 4);
  const downloadEvent = page.waitForEvent("download", { timeout: 10000 });
  await key(page, "Space");
  const file = await downloadEvent;
  assert.match(file.suggestedFilename(), /-mm\.stl$/i);
  const bytes = await readFile(await file.path());
  assert.equal(bytes.readUInt32LE(80), 12, "The reference person must not add triangles to exported STL");
  assert.equal(bytes.length, 84 + 12 * 50);
  return bytes;
}
function number(value, prefix) {
  const match = value.match(new RegExp(`^${prefix}:\\s*([-+\\d.eE]+)`, "m"));
  assert.ok(match, `Report must contain ${prefix}`);
  return Number(match[1]);
}
function geometry(value) {
  const line = value.split("\n").find(line => line.startsWith("Dimensions:"));
  assert.ok(line);
  return {
    dimensions: [...line.matchAll(/[-+]?\d+(?:\.\d+)?(?:[eE][-+]?\d+)?/g)].slice(0, 3).map(match => Number(match[0])),
    area: number(value, "Surface area"), volume: number(value, "Total volume"),
  };
}
function camera(value) {
  const projection = value.match(/^Projection:\s*(.+)$/m);
  assert.ok(projection);
  return { yaw: number(value, "Camera yaw"), pitch: number(value, "Camera pitch"), projection: projection[1] };
}
function comparison(value, enabled) {
  assert.match(value, new RegExp(`^Comparison: ${enabled ? "On" : "Off"}$`, "m"));
  close(number(value, "Reference height"), referenceHeight, "The human reference remains exactly69in");
}
function close(actual, expected, message, relative = 0.000002) {
  assert.ok(Math.abs(actual - expected) <= Math.max(0.00001, Math.abs(expected) * relative), `${message}: ${actual} != ${expected}`);
}

async function visual(page, kind) {
  const png = await page.screenshot(screenshotOptions);
  return page.evaluate(async ({ encoded, kind }) => {
    const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
    const image = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
    const surface = new OffscreenCanvas(image.width, image.height);
    const context = surface.getContext("2d");
    context.drawImage(image, 0, 0);
    const pixels = context.getImageData(0, 0, image.width, image.height).data;
    const phone = image.width < 500;
    const bounds = kind === "toggle"
      ? { left: phone ? image.width - 150 : Math.floor(image.width * 0.47), right: phone ? image.width - 12 : Math.floor(image.width * 0.58), top: phone ? 180 : 115, bottom: phone ? 650 : 235 }
      : { left: phone ? 95 : 130, right: phone ? image.width - 12 : Math.floor(image.width * 0.58) - 6, top: phone ? 270 : 160, bottom: image.height - 35 };
    const near = (offset, target, tolerance = 5) => target.every((value, axis) => Math.abs(pixels[offset + axis] - value) <= tolerance);
    const classes = kind === "toggle" ? ["button"] : ["model", "person"];
    const answer = {};
    for (const name of classes) {
      const accepts = offset => name === "model"
        ? pixels[offset] > 55 && pixels[offset] > pixels[offset + 1] + 8 && pixels[offset + 1] > pixels[offset + 2] + 8
        : name === "person" ? near(offset, [148, 188, 160])
          : [[21, 31, 43], [38, 54, 73], [43, 66, 90], [40, 72, 101], [60, 60, 60]].some(color => near(offset, color, 3));
      const seen = new Uint8Array(image.width * image.height);
      const components = [];
      for (let y = bounds.top; y < bounds.bottom; y++) for (let x = bounds.left; x < bounds.right; x++) {
        const start = y * image.width + x;
        if (seen[start] || !accepts(start * 4)) continue;
        const stack = [start];
        seen[start] = 1;
        const region = { left: x, right: x, top: y, bottom: y, count: 0 };
        while (stack.length) {
          const index = stack.pop(), py = Math.floor(index / image.width), px = index % image.width;
          region.left = Math.min(region.left, px); region.right = Math.max(region.right, px);
          region.top = Math.min(region.top, py); region.bottom = Math.max(region.bottom, py); region.count++;
          for (const [nx, ny] of [[px - 1, py], [px + 1, py], [px, py - 1], [px, py + 1]]) {
            if (nx < bounds.left || nx >= bounds.right || ny < bounds.top || ny >= bounds.bottom) continue;
            const next = ny * image.width + nx;
            if (!seen[next] && accepts(next * 4)) { seen[next] = 1; stack.push(next); }
          }
        }
        region.width = region.right - region.left + 1;
        region.height = region.bottom - region.top + 1;
        components.push(region);
      }
      if (name === "person") {
        // The silhouette is assembled from separate convex pieces. On a small
        // display, anti-aliasing can disconnect head/feet from the torso. Read
        // every visible sage piece beside the model, not just its largest part.
        const pieces = components.filter(region => region.count >= 2 && region.left > (answer.model?.right ?? bounds.left) + 3);
        if (pieces.length) {
          const union = pieces.reduce((result, region) => ({
            left: Math.min(result.left, region.left), right: Math.max(result.right, region.right),
            top: Math.min(result.top, region.top), bottom: Math.max(result.bottom, region.bottom), count: result.count + region.count,
          }), { left: image.width, right: -1, top: image.height, bottom: -1, count: 0 });
          answer.person = { ...union, width: union.right - union.left + 1, height: union.bottom - union.top + 1 };
        } else answer.person = null;
        continue;
      }
      const candidates = name === "button"
        ? components.filter(region => region.width >= 90 && region.width <= 125 && region.height >= 14 && region.height <= 28 && region.count > 450)
        : components.filter(region => region.count > 300 && region.height > 50);
      answer[name] = candidates.sort((a, b) => b.count - a.count)[0] ?? null;
    }
    answer.bounds = bounds;
    image.close();
    return answer;
  }, { encoded: png.toString("base64"), kind });
}
async function toggleHuman(page) {
  // Click the actual filled viewport button located in rendered pixels. This
  // adapts to the mobile tool's wrapped controls without reading private state.
  if (page.viewportSize().width >= 500) {
    await page.mouse.move(12, 12);
    await paint(page);
  }
  const { button } = await visual(page, "toggle");
  assert.ok(button, "The visual Human5′9″ toggle must be visible in the viewport");
  const x = (button.left + button.right) / 2, y = (button.top + button.bottom) / 2;
  if (page.viewportSize().width < 500) await page.touchscreen.tap(x, y);
  else await page.mouse.click(x, y, { delay: 50 });
  await paint(page);
}
async function bodyRatio(page, expectedRatio) {
  const bodies = await visual(page, "bodies");
  assert.ok(bodies.model && bodies.person, "Both the model and the physical reference must actually render");
  for (const [name, body] of [["model", bodies.model], ["person", bodies.person]]) {
    assert.ok(body.top > bodies.bounds.top + 3 && body.bottom < bodies.bounds.bottom - 3, `The ${name}'s top and feet must fit inside the visible view, not its crop boundary: ${JSON.stringify(bodies)}`);
  }
  assert.ok(bodies.model.right < bodies.person.left, "The person must stand beside the model");
  assert.ok(Math.abs(bodies.model.bottom - bodies.person.bottom) <= 3, `Model bottom and feet must share a Z plane: ${JSON.stringify(bodies)}`);
  assert.ok(Math.abs(bodies.model.height / bodies.person.height - expectedRatio) <= 0.025, `Rendered height ratio must be ${expectedRatio}: ${JSON.stringify(bodies)}`);
  return bodies;
}
async function setZPercent(page, percent) {
  await stl(page);
  await key(page, "Control+Alt+s");
  await page.waitForTimeout(250);
  // Default Linked is on. Turn it off, then edit the Z total percentage.
  await key(page, "Space");
  await tab(page, 3);
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 3000 });
  await page.keyboard.press("Control+a");
  await page.keyboard.type(String(percent), { delay: 30 });
  await key(page, "Enter");
}

try {
  progress("align a raised model bottom with a human reference of equal physical height");
  current = await open("human-equal");
  const baseline = await report(current.page);
  const measured = geometry(baseline);
  measured.dimensions.forEach((value, axis) => close(value, [400, 300, referenceHeight][axis], "Source box dimension"));
  comparison(baseline, false);
  const before = camera(baseline);
  const originalExport = await exported(current.page);
  await toggleHuman(current.page);
  const compared = await report(current.page);
  comparison(compared, true);
  assert.deepEqual(camera(compared), { yaw: 0, pitch: 0, projection: "Orthographic" });
  assert.deepEqual(geometry(compared), measured, "The reference must not alter model measurements");
  close(number(compared, "Object / human height"), 1, "Public physical height ratio");
  await bodyRatio(current.page, 1);
  await capture(current.page, "human-equal-height");
  assert.deepEqual(await exported(current.page), originalExport, "The person, baseline, and comparison camera must not enter the exported object");

  progress("undo the comparison camera and keep reference height fixed during Z scaling");
  await key(current.page, "Control+z");
  const undone = await report(current.page);
  comparison(undone, false);
  assert.deepEqual(camera(undone), before, "Undo must restore the camera from before comparison");
  await toggleHuman(current.page);
  await setZPercent(current.page, 50);
  const half = await report(current.page);
  comparison(half, true);
  close(number(half, "Object / human height"), 0.5, "Half-height physical ratio");
  close(geometry(half).dimensions[2], referenceHeight / 2, "Only the object height changes");
  await bodyRatio(current.page, 0.5);
  await capture(current.page, "human-half-height");
  await key(current.page, "Control+z");
  const restored = await report(current.page);
  comparison(restored, true);
  close(number(restored, "Object / human height"), 1, "One scale Undo restores the object height");
  await bodyRatio(current.page, 1);

  progress("leave comparison for perspective and undo back to the front reference");
  await control(current.page, 12);
  const perspective = await report(current.page);
  comparison(perspective, false);
  assert.equal(camera(perspective).projection, "Perspective (45°)");
  assert.deepEqual(geometry(perspective), measured);
  await key(current.page, "Control+z");
  comparison(await report(current.page), true);
  await bodyRatio(current.page, 1);
  assert.deepEqual(await exported(current.page), originalExport);
  assert.equal(await current.page.evaluate(key => localStorage.getItem(key), storageKey), fixtureRaw);
  await current.context.close();

  progress("keep the same physical human size when source coordinates use centimeters");
  current = await open("human-centimeters", { width: 1440, height: 1000 }, 10);
  await control(current.page, 15); // Source centimeters, after the projection toggle.
  await toggleHuman(current.page);
  const centimeters = await report(current.page);
  comparison(centimeters, true);
  geometry(centimeters).dimensions.forEach((value, axis) => close(value, measured.dimensions[axis], "Equivalent source-unit dimension"));
  close(number(centimeters, "Object / human height"), 1, "Centimeter input height ratio");
  await bodyRatio(current.page, 1);
  await capture(current.page, "human-centimeters");
  assert.equal(await current.page.evaluate(key => localStorage.getItem(key), storageKey), fixtureRaw);
  await current.context.close();

  progress("tap the visual comparison toggle on a touch-sized viewport");
  current = await open("human-mobile", { width: 390, height: 844 });
  await toggleHuman(current.page);
  const mobile = await report(current.page);
  comparison(mobile, true);
  close(number(mobile, "Object / human height"), 1, "Mobile physical height ratio");
  await bodyRatio(current.page, 1);
  await capture(current.page, "human-mobile");
  assert.equal(await current.page.evaluate(key => localStorage.getItem(key), storageKey), fixtureRaw);
  assert.deepEqual(issues, [], "Human-reference interactions must stay local and emit no application errors");
  console.log("Human browser checks passed: visible mouse/touch comparison toggle, fixed69in reference, front X/Z with Z up, actual shared feet/model baseline despite raised source coordinates, rendered equal/half height ratios, scaling and camera Undo, source-unit invariance, reference-free STL export, and saved-data isolation.");
} catch (error) {
  originalError = error;
  console.error(`Human browser failed during: ${phase}`);
  if (current?.page && !current.page.isClosed()) await capture(current.page, "human-failure", 3000).catch(() => {});
  throw error;
} finally {
  progress("close human contexts and Chromium");
  let cleanupTimer;
  let cleanupError;
  try {
    await Promise.race([
      Promise.all([...contexts.map(context => context.close().catch(() => {})), browser.close()]),
      new Promise((_, reject) => { cleanupTimer = setTimeout(() => reject(new Error("Human browser shutdown exceeded 10 seconds")), 10000); }),
    ]);
  } catch (error) { cleanupError = error; }
  finally { clearTimeout(cleanupTimer); clearTimeout(watchdog); }
  if (cleanupError) {
    if (originalError) console.error("Original human failure:", originalError);
    console.error("Human browser cleanup failed:", cleanupError);
    process.exit(1);
  }
}

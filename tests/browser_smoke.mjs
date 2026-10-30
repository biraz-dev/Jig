// Real Chromium checks for the built Rust/WASM preview. All datasets are synthetic.
// Serve dist at /Jig/ before running: node tests/browser_smoke.mjs
import assert from "node:assert/strict";
import { mkdir, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium } from "playwright";

const base = process.env.JIG_PREVIEW_URL ?? "http://127.0.0.1:4173/Jig/";
const origin = new URL(base).origin;
const key = "jig.personal.v1";
const results = resolve("playwright-results");
await mkdir(results, { recursive: true });

const fixture = {
  version: 1,
  preferences: { decimals: 2, focus: 3 },
  history: [{ expression: "2 + 2", result: 4 }],
  tasks: [{ id: 1, text: "Sample: browser persistence check", complete: false }],
  deadlines: [{ id: 2, title: "Sample: future opening", at: "2030-01-15T18:00:00Z" }],
  entries: [{ id: 3, title: "Sample: studio visit", date: "2030-01-15" }],
  featured: 2,
};
const fixtureRaw = JSON.stringify(fixture);
const issues = [];
const pages = [];
const browser = await chromium.launch({
  headless: true,
  args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"],
});

async function newPage(label, { raw, viewport = { width: 1440, height: 1100 } } = {}) {
  const context = await browser.newContext({ viewport, acceptDownloads: true });
  if (raw !== undefined) {
    // Seed once per tab; a reload must exercise the app's actual saved state.
    await context.addInitScript(({ key, raw, origin }) => {
      if (location.origin === origin && sessionStorage.getItem("jig-smoke-seeded") !== "yes") {
        localStorage.setItem(key, raw);
        sessionStorage.setItem("jig-smoke-seeded", "yes");
      }
    }, { key, raw, origin });
  }
  const page = await context.newPage();
  observe(page, label);
  return { context, page };
}

function observe(page, label) {
  pages.push({ page, label });
  page.setDefaultTimeout(15000);
  page.on("pageerror", error => issues.push(`${label}: ${error.message}`));
  page.on("request", request => {
    const url = new URL(request.url());
    if (!["blob:", "data:", "about:"].includes(url.protocol) && url.origin !== origin) {
      issues.push(`${label}: unexpected external request ${url.origin}${url.pathname}`);
    }
  });
}

async function boot(page, reload = false) {
  if (reload) await page.reload({ waitUntil: "load" });
  else await page.goto(base, { waitUntil: "load" });
  await page.waitForFunction(
    () => window.jigReady === true || typeof window.jigBootError === "string",
    undefined,
    { timeout: 60000 },
  );
  assert.equal(await page.evaluate(() => window.jigBootError), undefined, "Rust/WASM startup failed");
  assert.equal(await page.locator("#jig_canvas").isVisible(), true);
  assert.equal(await page.locator("#loading").isHidden(), true);
  // WebRunner startup precedes the first painted egui frame.
  await page.waitForTimeout(500);
}

async function saved(page) {
  return page.evaluate(key => localStorage.getItem(key), key);
}

async function calculate(page, expression) {
  await page.bringToFront();
  await page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
  await page.keyboard.press("Control+Alt+4");
  await page.waitForTimeout(300);
  await page.keyboard.type(expression, { delay: 15 });
  await page.keyboard.press("Enter");
}

async function expectCalculation(page, expression, result, length) {
  await page.waitForFunction(({ key, expression, result, length }) => {
    try {
      const state = JSON.parse(localStorage.getItem(key));
      const last = state.history.at(-1);
      return state.history.length === length && last.expression === expression && last.result === result;
    } catch {
      return false;
    }
  }, { key, expression, result, length }, { timeout: 15000 });
}

try {
  const demo = await newPage("sample-desktop");
  await boot(demo.page);
  assert.equal(await saved(demo.page), null, "Fresh preview should start with temporary sample data");
  await calculate(demo.page, "1 / 3");
  await demo.page.waitForTimeout(1200);
  assert.equal(await saved(demo.page), null, "Editing sample data must not create personal browser state");
  await demo.page.screenshot({ path: resolve(results, "sample-desktop.png") });

  const personal = await newPage("personal-desktop", { raw: fixtureRaw });
  await boot(personal.page);
  assert.equal(await saved(personal.page), fixtureRaw, "Opening personal state must not rewrite it");
  await calculate(personal.page, "1 / 3");
  await expectCalculation(personal.page, "1 / 3", 1 / 3, 2);
  await boot(personal.page, true);
  assert.equal(JSON.parse(await saved(personal.page)).history.at(-1).result, 1 / 3);
  await calculate(personal.page, "ans * 3");
  await expectCalculation(personal.page, "ans * 3", 1, 3);
  await personal.page.screenshot({ path: resolve(results, "personal-desktop.png") });

  // Exercise the public file bridge: export is a local browser download.
  const exportRaw = await saved(personal.page);
  const downloadEvent = personal.page.waitForEvent("download");
  await personal.page.evaluate(text => window.jigExport(text), exportRaw);
  const download = await downloadEvent;
  assert.equal(download.suggestedFilename(), "jig.json");
  const downloadPath = await download.path();
  assert.ok(downloadPath);
  assert.deepEqual(JSON.parse(await readFile(downloadPath, "utf8")), JSON.parse(exportRaw));

  // The actual hidden file input queues imports for the Rust UI to validate.
  for (const [name, contents] of [["broken.json", "{broken"], ["unrelated.json", "{}"]]) {
    await personal.page.locator("#jig-import").setInputFiles({
      name, mimeType: "application/json", buffer: Buffer.from(contents),
    });
    await personal.page.waitForTimeout(1600);
    assert.equal(await saved(personal.page), exportRaw, `Invalid import ${name} changed saved data`);
  }

  const malformedRaw = "{broken saved sample fixture";
  const malformed = await newPage("malformed-preserved", { raw: malformedRaw });
  await boot(malformed.page);
  await calculate(malformed.page, "10 + 5");
  await malformed.page.waitForTimeout(1200);
  assert.equal(await saved(malformed.page), malformedRaw, "Malformed saved state must remain available for recovery");
  await malformed.page.screenshot({ path: resolve(results, "malformed-preserved.png") });

  // A failed explicit reload in an already-personal session must not acknowledge
  // malformed raw data and then overwrite it on the next ordinary edit.
  const laterCorrupt = await newPage("later-corrupt-preserved", { raw: fixtureRaw });
  await boot(laterCorrupt.page);
  await laterCorrupt.page.evaluate(({ key, raw }) => localStorage.setItem(key, raw), { key, raw: malformedRaw });
  await laterCorrupt.page.bringToFront();
  await laterCorrupt.page.locator("#jig_canvas").click({ position: { x: 12, y: 12 } });
  await laterCorrupt.page.keyboard.press("Control+Alt+o");
  await laterCorrupt.page.waitForTimeout(500);
  await calculate(laterCorrupt.page, "10 + 5");
  await laterCorrupt.page.waitForTimeout(1200);
  assert.equal(await saved(laterCorrupt.page), malformedRaw, "Failed reload allowed a later edit to overwrite corrupt data");

  // Two tabs share one storage area; stale public bridge writes must be rejected.
  const shared = await newPage("cross-tab-first");
  await boot(shared.page);
  const second = await shared.context.newPage();
  observe(second, "cross-tab-second");
  await boot(second);
  assert.equal(await shared.page.evaluate(() => window.jigLoad()), null);
  assert.equal(await second.evaluate(() => window.jigLoad()), null);
  await shared.page.evaluate(text => window.jigSave(text), fixtureRaw);
  const conflict = await second.evaluate(text => {
    try { window.jigSave(text); return null; } catch (error) { return String(error); }
  }, JSON.stringify({ ...fixture, history: [] }));
  assert.match(conflict, /Another tab changed/);
  assert.equal(await saved(shared.page), fixtureRaw);
  const clearConflict = await second.evaluate(() => {
    try { window.jigClear(); return null; } catch (error) { return String(error); }
  });
  assert.match(clearConflict, /Another tab changed/);
  assert.equal(await saved(shared.page), fixtureRaw);

  // Independent browser contexts must not inherit another context's data.
  const compact = await newPage("sample-mobile", { viewport: { width: 390, height: 844 } });
  await boot(compact.page);
  assert.equal(await saved(compact.page), null);
  assert.equal(await compact.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true);
  await compact.page.screenshot({ path: resolve(results, "sample-mobile.png") });

  assert.deepEqual(issues, [], "Browser errors or unexpected remote requests");
  console.log("Browser smoke passed: WASM startup, sample isolation, real calculator input, precise restart persistence, local JSON export, invalid import preservation, corrupt state recovery, stale-tab rejection, independent contexts and compact layout.");
} catch (error) {
  for (const { page, label } of pages) {
    if (!page.isClosed()) {
      await page.screenshot({ path: resolve(results, `failure-${label}.png`) }).catch(() => {});
    }
  }
  if (issues.length) console.error("Browser diagnostics:", issues.join("\n"));
  throw error;
} finally {
  await browser.close();
}

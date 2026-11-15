// Real Chromium checks for the Rust/WASM Tasks, calendar and calculator UI.
// Every fixture is synthetic. Serve dist at /Jig/ before running this file.
import assert from "node:assert/strict";
import { mkdir, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium, errors } from "playwright";
import { installFrameFence, waitForAppPaint } from "./browser_frames.mjs";
import "./import_bridges.mjs";

const base = process.env.JIG_PREVIEW_URL ?? "http://127.0.0.1:4173/Jig/";
const origin = new URL(base).origin;
const sampleUrl = new URL(base); sampleUrl.searchParams.set("sample", "1"); sampleUrl.hash = "";
const key = "jig.personal.v1";
const weatherSettingsKey = "jig.weather.settings.v1", weatherCacheKey = "jig.weather.cache.v1";
const weatherOrigins = ["https://api.open-meteo.com", "https://geocoding-api.open-meteo.com"];
const results = resolve("playwright-results");
await mkdir(results, { recursive: true });
const fixture = {
  version: 1,
  preferences: { decimals: 2, focus: 3, weather: null },
  history: [{ expression: "2 + 2", result: 4 }],
  tasks: [{ id: 1, text: "Sample: browser persistence check", complete: false, deadline: 2, color: "rose" }],
  deadlines: [{ id: 2, title: "Sample: browser persistence check", at: "2030-01-15T18:00:00Z" }],
  entries: [{ id: 3, title: "Sample: hidden studio visit", date: "2030-01-15" }],
  notes: [{ id: 4, text: "Sample: useful studio note", pinned: true }],
  featured: 2,
};
const fixtureRaw = JSON.stringify(fixture);
const issues = [], pages = [];
let phase = "launch Chromium";
const suiteTimer = setTimeout(() => { console.error(`Browser smoke exceeded 300 seconds during: ${phase}`); process.exit(1); }, 300000);
function progress(message) { phase = message; console.log(`Browser phase: ${message}`); }
function bounded(promise, milliseconds, description) {
  let timer;
  return Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`${description} timed out after ${milliseconds}ms`)), milliseconds); })]).finally(() => clearTimeout(timer));
}
async function release(context, label) { progress(`close ${label}`); await bounded(context.close(), 5000, `Closing ${label}`); }
async function paint(page) { await waitForAppPaint(page); }
async function navigateKey(page, value) { await page.keyboard.press(value); await paint(page); }
async function tab(page, count = 1) { for (let index = 0; index < count; index++) await navigateKey(page, "Tab"); }
async function capture(page, name) {
  const png = await page.screenshot({ path: resolve(results, `${name}.png`), timeout: 60000, animations: "disabled", caret: "hide" });
  if (new Set((process.env.JIG_RENDER_LOGS ?? "").split(",")).has(name) && png.length <= 1024 * 1024) {
    console.log(`JIG_RENDER_BEGIN_${name}`); const encoded = png.toString("base64");
    for (let offset = 0; offset < encoded.length; offset += 2048) console.log(encoded.slice(offset, offset + 2048));
    console.log(`JIG_RENDER_END_${name}`);
  }
  return png;
}
const browser = await chromium.launch({ headless: true, args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"] });

async function newPage(label, {
  raw, viewport = { width: 1440, height: 1100 }, weather, weatherSettings, weatherCache, fixedTime, hasTouch = false, workspace = "tasks",
} = {}) {
  const context = await browser.newContext({ viewport, acceptDownloads: true, hasTouch, timezoneId: "Etc/UTC" });
  // Start in Tasks once; later reloads must preserve actual workspace choices.
  await context.addInitScript(({ path, workspace }) => {
    if (location.pathname === path && sessionStorage.getItem("jig-workspace-seeded") !== "yes") {
      localStorage.setItem("jig.workspace.v1", workspace);
      sessionStorage.setItem("jig-workspace-seeded", "yes");
    }
  }, { path: new URL(base).pathname, workspace });
  await installFrameFence(context);
  if (fixedTime) {
    // Keep calendar fixtures deterministic without replacing the native RAF,
    // performance clock or timers that drive egui's real rendering and inputs.
    await context.addInitScript(value => {
      const NativeDate = Date;
      const fixed = NativeDate.parse(value);
      const now = () => fixed;
      globalThis.Date = new Proxy(NativeDate, {
        apply: () => new NativeDate(fixed).toString(),
        construct: (target, args, newTarget) => Reflect.construct(target, args.length ? args : [fixed], newTarget),
        get: (target, key, receiver) => key === "now" ? now : Reflect.get(target, key, receiver),
      });
    }, fixedTime);
  }
  await context.addInitScript(() => {
    // Browser event diagnostics only; this does not expose or change Rust state.
    window.jigSmokeEvents = [];
    for (const type of ["keydown", "input"]) {
      document.addEventListener(type, event => {
        const sample = {
          type, time: Math.round(performance.now()),
          key: event.key ?? null, code: event.code ?? null,
          data: event.data ?? null,
          ctrl: event.ctrlKey ?? false, alt: event.altKey ?? false,
          target: event.target?.tagName ?? null, id: event.target?.id ?? null,
        };
        // Record prevention after the app has handled the event.
        queueMicrotask(() => {
          sample.prevented = event.defaultPrevented;
          window.jigSmokeEvents.push(sample);
          if (window.jigSmokeEvents.length > 40) window.jigSmokeEvents.shift();
        });
      }, true);
    }
  });
  const initial = {};
  if (raw !== undefined) initial[key] = raw;
  if (weatherSettings !== undefined) initial[weatherSettingsKey] = JSON.stringify(weatherSettings);
  if (weatherCache !== undefined) initial[weatherCacheKey] = JSON.stringify(weatherCache);
  if (Object.keys(initial).length) {
    // Seed once per tab; a reload must exercise the app's actual saved state.
    await context.addInitScript(({ initial, origin }) => {
      if (location.origin === origin && sessionStorage.getItem("jig-smoke-seeded") !== "yes") {
        for (const [key, raw] of Object.entries(initial)) localStorage.setItem(key, raw);
        sessionStorage.setItem("jig-smoke-seeded", "yes");
      }
    }, { initial, origin });
  }
  for (const provider of weatherOrigins) {
    await context.route(`${provider}/**`, async route => {
      // Always intercept providers, including unexpected requests from a disabled
      // context: these tests never contact a real weather or location service.
      if (!weather) { await route.abort(); return; }
      const request = route.request();
      const url = new URL(request.url());
      weather.requests.push(url.toString());
      assert.equal(request.method(), "GET");
      assert.equal(request.postData(), null, "Application state must not be posted to weather providers");
      if (url.origin === weatherOrigins[0]) {
        assert.equal(url.pathname, "/v1/forecast");
        const allowed = new Set(["latitude", "longitude", "current", "hourly", "daily", "timezone", "forecast_days", "temperature_unit"]);
        assert.ok([...url.searchParams.keys()].every(key => allowed.has(key)), `Unexpected forecast query fields: ${url.search}`);
        assert.equal(Number(url.searchParams.get("latitude")), weather.config.latitude);
        assert.equal(Number(url.searchParams.get("longitude")), weather.config.longitude);
        assert.equal(url.searchParams.get("temperature_unit"), weather.config.unit);
        assert.deepEqual(url.searchParams.get("current").split(",").sort(), ["temperature_2m", "relative_humidity_2m", "weather_code"].sort());
        assert.equal(url.searchParams.get("hourly"), "relative_humidity_2m");
        assert.deepEqual(url.searchParams.get("daily").split(",").sort(), ["temperature_2m_max", "temperature_2m_min", "precipitation_probability_max", "weather_code"].sort());
        assert.equal(url.searchParams.get("timezone"), "auto");
        assert.equal(Number(url.searchParams.get("forecast_days")), 16);
        if (weather.fail) {
          await route.fulfill({ status: 503, contentType: "application/json", headers: { "access-control-allow-origin": "*" }, body: '{"error":true,"reason":"Synthetic provider outage"}' });
        } else {
          await route.fulfill({ status: 200, contentType: "application/json", headers: { "access-control-allow-origin": "*" }, body: JSON.stringify(mockForecast(weather.config, weather.now)) });
        }
      } else {
        assert.equal(url.pathname, "/v1/search");
        const allowed = new Set(["name", "count", "language", "format"]);
        assert.ok([...url.searchParams.keys()].every(key => allowed.has(key)));
        await route.fulfill({ status: 200, contentType: "application/json", headers: { "access-control-allow-origin": "*" }, body: JSON.stringify({ results: [{ id: 12345, name: "Sample Harbor", country: "Example", country_code: "GB", admin1: "Synthetic Region", latitude: 51.5, longitude: -0.12, timezone: "Etc/UTC" }] }) });
      }
    });
  }
  const page = await context.newPage();
  observe(page, label, weather ? weatherOrigins : []);
  return { context, page };
}

function observe(page, label, allowedExternal = []) {
  pages.push({ page, label });
  page.setDefaultTimeout(15000);
  page.on("pageerror", error => issues.push(`${label}: ${error.message}`));
  page.on("request", request => {
    const url = new URL(request.url());
    if (!["blob:", "data:", "about:"].includes(url.protocol) && url.origin !== origin && !allowedExternal.includes(url.origin)) {
      issues.push(`${label}: unexpected external request ${url.origin}${url.pathname}`);
    }
  });
}

async function boot(page, reload = false, address = base) {
  const label = pages.find(item => item.page === page)?.label ?? address;
  progress(`${reload ? "reload" : "boot"} ${label}`);
  if (reload) await page.reload({ waitUntil: "load" }); else await page.goto(address, { waitUntil: "load" });
  await page.bringToFront();
  await page.waitForFunction(() => window.jigReady === true || typeof window.jigBootError === "string", undefined, { timeout: 60000, polling: 100 });
  assert.equal(await page.evaluate(() => window.jigBootError), undefined, "Rust/WASM startup failed");
  assert.equal(await page.locator("#jig_canvas").isVisible(), true);
  assert.equal(await page.locator("#loading").isHidden(), true);
  await page.waitForTimeout(500);
}
async function resetFocus(page) {
  const { width, height } = page.viewportSize();
  await page.locator("#jig_canvas").click({ position: { x: width - 6, y: height - 6 } });
  await navigateKey(page, "Escape");
}
async function saved(page) { return page.evaluate(key => localStorage.getItem(key), key); }
async function calculate(page, expression) {
  progress(`calculate ${expression}`);
  await page.bringToFront(); await resetFocus(page); await navigateKey(page, "Control+Alt+4");
  await typeCalculation(page, expression);
}
async function typeCalculation(page, expression) {
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 10000, polling: 25 });
  await page.keyboard.press("Control+a"); await page.keyboard.type(expression, { delay: 15 }); await navigateKey(page, "Enter");
}
async function expectCalculation(page, expression, result, length, unit) {
  await page.waitForFunction(({ key, expression, result, length, unit }) => {
    try { const state = JSON.parse(localStorage.getItem(key)), last = state.history.at(-1); return state.history.length === length && last.expression === expression && last.result === result && last.unit === unit; } catch { return false; }
  }, { key, expression, result, length, unit }, { timeout: 15000 });
}
async function openTaskDates(page) {
  await resetFocus(page); await navigateKey(page, "Control+Alt+2");
  // Data/Settings/Calculator/About/Help/Source, Tasks/+, Gantt/Save, then row date.
  await tab(page, 14); await navigateKey(page, "Space");
  await page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { polling: 25 });
}
async function openDuePicker(page) {
  await openTaskDates(page);
  await tab(page); await navigateKey(page, "Space");
}
async function imeCursor(page, minimumY = 0) {
  await page.waitForFunction(minimumY => {
    const input = document.activeElement;
    return input?.tagName === "INPUT" && Number.isFinite(parseFloat(input.style.left)) && parseFloat(input.style.top) > minimumY;
  }, minimumY, { timeout: 5000, polling: 25 });
  return page.evaluate(() => ({ x: parseFloat(document.activeElement.style.left), y: parseFloat(document.activeElement.style.top) }));
}
async function focusVisibleInput(page, point) {
  // eframe positions its public IME transport at the visible Rust caret. Its
  // CSS coordinates therefore identify the actual field for this real click.
  // Offset from the one-pixel transport itself so the pointer reaches canvas.
  await page.mouse.click(point.x + 6, point.y); await paint(page);
  await page.waitForFunction(point => document.activeElement?.tagName === "INPUT"
    && Math.abs(parseFloat(document.activeElement.style.top) - point.y) < 8, point, { timeout: 5000, polling: 25 });
}
async function focusPickerTime(page) {
  for (let index = 0; index < 100; index++) {
    await tab(page);
    try {
      await page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { timeout: 150, polling: 25 });
    } catch (error) { if (error instanceof errors.TimeoutError) continue; throw error; }
    // Observe the actual editor via normal Copy. The browser's IME input is a
    // transport, so its DOM value is not the Rust field's content.
    await page.evaluate(() => navigator.clipboard.writeText("Picker copy pending"));
    await navigateKey(page, "Control+a"); await navigateKey(page, "Control+c");
    const until = Date.now() + 3000; let copied = "Picker copy pending";
    while (copied === "Picker copy pending" && Date.now() < until) {
      let timer;
      try {
        copied = await Promise.race([page.evaluate(() => navigator.clipboard.readText()), new Promise((_, reject) => {
          timer = setTimeout(() => reject(new Error("Picker clipboard read exceeded its deadline")), Math.max(1, until - Date.now()));
        })]);
      } finally { clearTimeout(timer); }
      if (copied === "Picker copy pending") await new Promise(resolve => setTimeout(resolve, Math.min(25, Math.max(0, until - Date.now()))));
    }
    assert.notEqual(copied, "Picker copy pending", "Copy must expose the focused date-picker editor's actual text");
    if (/^\d{2}:\d{2}$/.test(copied)) return;
  }
  assert.fail("The date picker must expose a keyboard-editable HH:MM field");
}
async function exportData(page) {
  const raw = await saved(page), event = page.waitForEvent("download");
  await page.evaluate(text => window.jigExport(text), raw);
  const download = await event; assert.equal(download.suggestedFilename(), "jig.json");
  return JSON.parse(await readFile(await download.path(), "utf8"));
}
async function confirmImport(page) {
  await resetFocus(page);
  // Five header buttons, then the pending import's real Confirm control.
  await tab(page, 6); await navigateKey(page, "Space");
}
async function expectClipChange(page, before, clip, message) {
  const until = Date.now() + 5000; let actual;
  do {
    actual = await page.screenshot({ clip, timeout: 3000, animations: "disabled", caret: "hide" });
    if (!actual.equals(before)) return;
    await page.waitForTimeout(50);
  } while (Date.now() < until);
  assert.notDeepEqual(actual, before, message);
}
async function filledRegions(page, color, top, bottom) {
  const png = await page.screenshot({ animations: "disabled", caret: "hide" });
  return page.evaluate(async ({ encoded, color, top, bottom }) => {
    const bitmap = await createImageBitmap(new Blob([Uint8Array.from(atob(encoded), c => c.charCodeAt(0))], { type: "image/png" }));
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height), context = canvas.getContext("2d"); context.drawImage(bitmap, 0, 0);
    const pixels = context.getImageData(0, 0, bitmap.width, bitmap.height).data, seen = new Uint8Array(bitmap.width * bitmap.height), regions = [];
    bottom = Math.min(bottom, bitmap.height);
    const near = index => color.every((value, axis) => Math.abs(pixels[index * 4 + axis] - value) <= 3);
    for (let y = top; y < bottom; y++) for (let x = 6; x < bitmap.width - 6; x++) {
      const start = y * bitmap.width + x;
      if (seen[start] || !near(start)) continue;
      const stack = [start], r = { left: x, right: x, top: y, bottom: y, count: 0 }; seen[start] = 1;
      while (stack.length) {
        const index = stack.pop(), px = index % bitmap.width, py = Math.floor(index / bitmap.width);
        r.left = Math.min(r.left, px); r.right = Math.max(r.right, px); r.top = Math.min(r.top, py); r.bottom = Math.max(r.bottom, py); r.count++;
        for (const [nx, ny] of [[px - 1, py], [px + 1, py], [px, py - 1], [px, py + 1]]) {
          if (nx < 0 || nx >= bitmap.width || ny < top || ny >= bottom) continue;
          const next = ny * bitmap.width + nx;
          if (!seen[next] && near(next)) { seen[next] = 1; stack.push(next); }
        }
      }
      if (r.count > 100) regions.push({ ...r, x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2, width: r.right - r.left + 1, height: r.bottom - r.top + 1 });
    }
    bitmap.close(); return regions.sort((a, b) => b.count - a.count);
  }, { encoded: png.toString("base64"), color, top, bottom });
}
async function selectedDay(page) {
  const regions = await filledRegions(page, [67, 42, 34], 60, page.viewportSize().height - 40);
  const region = regions.find(r => r.count > 900 && r.height >= 55 && r.height < 80 && r.width >= 18 && r.width <= 98);
  assert.ok(region, "The continuous calendar's selected day must be visibly readable"); return region;
}
async function clickCalculatorIcon(page, touch = false) {
  await resetFocus(page);
  // Header order is stable: Data, Settings, Calculator; tabs are pointer controls.
  await tab(page, 3);
  const regions = await filledRegions(page, [77, 49, 39], 0, 90);
  const icon = regions.find(r => r.count > 200 && r.width >= 24 && r.width <= 34 && r.height >= 22 && r.height <= 32);
  assert.ok(icon, "The focused Calculator icon must be visible in the header");
  if (touch) await page.touchscreen.tap(icon.x, icon.y); else await page.mouse.click(icon.x, icon.y);
  await paint(page);
}

function mockForecast(config, at = new Date()) {
  const unit = config.unit;
  const today = new Date(at);
  today.setUTCHours(0, 0, 0, 0);
  const dates = [0, 1].map(offset => new Date(today.getTime() + offset * 86400000).toISOString().slice(0, 10));
  const fahrenheit = unit === "fahrenheit";
  return {
    latitude: config.latitude, longitude: config.longitude, generationtime_ms: 0.1,
    utc_offset_seconds: 0, timezone: "Etc/UTC", timezone_abbreviation: "UTC", elevation: 10,
    current_units: { time: "iso8601", interval: "seconds", temperature_2m: fahrenheit ? "°F" : "°C", relative_humidity_2m: "%", weather_code: "wmo code" },
    current: { time: `${dates[0]}T12:00`, interval: 900, temperature_2m: fahrenheit ? 70.7 : 21.5, relative_humidity_2m: 67, weather_code: 61 },
    hourly_units: { time: "iso8601", relative_humidity_2m: "%" },
    hourly: {
      time: dates.flatMap(date => Array.from({ length: 24 }, (_, hour) => `${date}T${String(hour).padStart(2, "0")}:00`)),
      relative_humidity_2m: dates.flatMap((_, day) => Array(24).fill(day === 0 ? 67 : 79)),
    },
    daily_units: { time: "iso8601", temperature_2m_max: fahrenheit ? "°F" : "°C", temperature_2m_min: fahrenheit ? "°F" : "°C", precipitation_probability_max: "%", weather_code: "wmo code" },
    daily: { time: dates, temperature_2m_max: fahrenheit ? [73.76, 77.72] : [23.2, 25.4], temperature_2m_min: fahrenheit ? [59.18, 62.96] : [15.1, 17.2], precipitation_probability_max: [78, 12], weather_code: [61, 2] },
  };
}

async function expectWeatherCache(page, config) {
  progress(`forecast cache for ${config.name} (${config.unit})`);
  await page.waitForFunction(({ key, config }) => {
    try {
      const cache = JSON.parse(localStorage.getItem(key));
      return cache.config.name === config.name && cache.config.unit === config.unit
        && cache.forecast.days.length === 2 && cache.forecast.current !== null;
    } catch { return false; }
  }, { key: weatherCacheKey, config }, { polling: 100 });
  return page.evaluate(key => JSON.parse(localStorage.getItem(key)), weatherCacheKey);
}

function checkWeather(cache, config, at) {
  const raw = mockForecast(config, at);
  assert.deepEqual(cache.config, config);
  assert.equal(cache.forecast.timezone, "Etc/UTC");
  assert.ok(Number.isFinite(Date.parse(cache.forecast.fetched_at)));
  assert.equal(cache.forecast.current.temperature, raw.current.temperature_2m);
  assert.equal(cache.forecast.current.humidity, 67);
  assert.equal(cache.forecast.current.code, 61);
  assert.deepEqual(cache.forecast.days.map(day => ({
    date: day.date, high: day.high, low: day.low,
    rain: day.rain_probability, humidity: day.humidity_mean, code: day.code,
  })), raw.daily.time.map((date, index) => ({
    date, high: raw.daily.temperature_2m_max[index], low: raw.daily.temperature_2m_min[index],
    rain: raw.daily.precipitation_probability_max[index], humidity: index === 0 ? 67 : 79,
    code: raw.daily.weather_code[index],
  })));
}
try {
  const demo = await newPage("sample-desktop");
  await boot(demo.page, false, `${sampleUrl.href}#weather=Sample%20Harbor`);
  assert.equal(await saved(demo.page), null, "Explicit sample data must remain temporary");
  await capture(demo.page, "consolidated-sample"); await clickCalculatorIcon(demo.page); await typeCalculation(demo.page, "1 / 3"); await demo.page.waitForTimeout(800);
  assert.equal(await saved(demo.page), null, "Sample calculator edits must not create personal state");
  await release(demo.context, "sample-desktop");

  const legacyWorkspace = await newPage("legacy-workspace", { raw: fixtureRaw, workspace: "workbench" });
  await boot(legacyWorkspace.page);
  assert.equal(await legacyWorkspace.page.evaluate(() => localStorage.getItem("jig.workspace.v1")), "tasks", "A removed Workbench preference must return to Tasks");
  assert.equal(await saved(legacyWorkspace.page), fixtureRaw);
  await release(legacyWorkspace.context, "legacy-workspace");

  const personal = await newPage("personal-desktop", { raw: fixtureRaw }); await boot(personal.page);
  assert.equal(await saved(personal.page), fixtureRaw, "Opening saved work must not migrate or rewrite it");
  await resetFocus(personal.page); await navigateKey(personal.page, "Control+Alt+d");
  assert.equal(await saved(personal.page), fixtureRaw); await capture(personal.page, "consolidated-data-open");
  await calculate(personal.page, "72 in mm"); await expectCalculation(personal.page, "72 in mm", 1828.8, 2, "mm");
  await navigateKey(personal.page, "Control+Alt+d");
  await calculate(personal.page, "1 1/2 in cm"); await expectCalculation(personal.page, "1 1/2 in cm", 3.81, 3, "cm");
  await calculate(personal.page, "1 / 3"); await expectCalculation(personal.page, "1 / 3", 1 / 3, 4);
  const millimeters = (1 / 3) * 25.4;
  await calculate(personal.page, "ans in mm"); await expectCalculation(personal.page, "ans in mm", millimeters, 5, "mm");
  await boot(personal.page, true);
  assert.deepEqual(JSON.parse(await saved(personal.page)).history.at(-1), { expression: "ans in mm", result: millimeters, unit: "mm" });
  await calculate(personal.page, "ans * 3"); await expectCalculation(personal.page, "ans * 3", millimeters * 3, 6);
  const beforeInvalid = await saved(personal.page);
  await calculate(personal.page, "72 in parsecs"); await personal.page.waitForTimeout(400);
  assert.equal(await saved(personal.page), beforeInvalid, "Invalid conversions must preserve history and the previous answer");
  await calculate(personal.page, "3 / 8"); await expectCalculation(personal.page, "3 / 8", 0.375, 7);
  const calculations = [1, 2, 3, 4].map(value => ({ expression: `7 + ${value}`, result: 7 + value }));
  for (const [index, item] of calculations.entries()) { await calculate(personal.page, item.expression); await expectCalculation(personal.page, item.expression, item.result, 8 + index); }
  const afterMath = JSON.parse(await saved(personal.page));
  for (const field of ["tasks", "deadlines", "entries", "notes", "featured"]) assert.deepEqual(afterMath[field], fixture[field], `Calculator popup changed ${field}`);
  await capture(personal.page, "calculator-popup-desktop");
  await navigateKey(personal.page, "Control+Alt+4");
  await personal.page.waitForFunction(() => document.activeElement?.tagName === "INPUT", undefined, { polling: 25 });
  await navigateKey(personal.page, "Control+Alt+w");
  await personal.page.waitForFunction(() => document.activeElement?.tagName !== "INPUT", undefined, { polling: 25 });
  assert.equal(await personal.page.evaluate(() => localStorage.getItem("jig.workspace.v1")), "sheet");
  await calculate(personal.page, "18 in cm"); await expectCalculation(personal.page, "18 in cm", 45.72, 12, "cm");
  assert.equal(await personal.page.evaluate(() => localStorage.getItem("jig.workspace.v1")), "sheet", "Opening calculator must preserve the current workspace");
  await resetFocus(personal.page); await navigateKey(personal.page, "Control+Alt+2");
  const exported = await exportData(personal.page), exportRaw = await saved(personal.page);
  assert.deepEqual(exported, JSON.parse(exportRaw)); assert.deepEqual(exported.entries, fixture.entries); assert.deepEqual(exported.notes, fixture.notes);
  const invalidColor = structuredClone(fixture); invalidColor.tasks[0].color = "neon";
  for (const [name, contents] of [["broken.json", "{broken"], ["unrelated.json", "{}"], ["invalid-color.json", JSON.stringify(invalidColor)]]) {
    await personal.page.locator("#jig-import").setInputFiles({ name, mimeType: "application/json", buffer: Buffer.from(contents) });
    await personal.page.waitForTimeout(1200); assert.equal(await saved(personal.page), exportRaw, `Invalid import ${name} changed saved work`);
  }
  await boot(personal.page, true);
  const imported = structuredClone(exported); imported.tasks[0].color = "blue";
  await personal.page.locator("#jig-import").setInputFiles({ name: "sample-import.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(imported)) });
  await personal.page.waitForTimeout(1200); assert.deepEqual(JSON.parse(await saved(personal.page)), exported, "Selecting an import must wait for confirmation");
  await confirmImport(personal.page);
  await personal.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).tasks[0].color === "blue", key);
  assert.deepEqual(JSON.parse(await saved(personal.page)), imported);
  await boot(personal.page, true); assert.deepEqual(JSON.parse(await saved(personal.page)), imported, "Imported task colors and notes must survive reload");
  await release(personal.context, "personal-desktop");

  progress("choose a task date and time without committing popup drafts");
  const picker = await newPage("task-date-time-picker", { raw: fixtureRaw, fixedTime: "2026-10-30T12:00:00Z" });
  await picker.context.grantPermissions(["clipboard-read", "clipboard-write"], { origin }); await boot(picker.page);
  await openDuePicker(picker.page); await capture(picker.page, "task-date-time-popup"); await navigateKey(picker.page, "ArrowRight");
  assert.equal(await saved(picker.page), fixtureRaw, "Browsing a picker must not save or move a task deadline");
  await navigateKey(picker.page, "Escape"); await navigateKey(picker.page, "Escape");
  assert.equal(await saved(picker.page), fixtureRaw, "Cancelling popup browsing must preserve original data");
  await openDuePicker(picker.page); await navigateKey(picker.page, "ArrowRight"); await focusPickerTime(picker.page);
  await picker.page.keyboard.type("14:30", { delay: 30 }); await navigateKey(picker.page, "Enter");
  assert.equal(await saved(picker.page), fixtureRaw, "Use date changes only the editor draft until Save date");
  await picker.page.waitForTimeout(100); await navigateKey(picker.page, "Enter");
  const dated = structuredClone(fixture); dated.deadlines[0].at = "2030-01-16T14:30:00Z";
  await picker.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).deadlines[0].at === "2030-01-16T14:30:00Z", key);
  assert.deepEqual(JSON.parse(await saved(picker.page)), dated, "Saving date/time must preserve task identity, color, featured selection and notes");
  await resetFocus(picker.page); await navigateKey(picker.page, "Control+z");
  await picker.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).deadlines[0].at === "2030-01-15T18:00:00Z", key);
  assert.deepEqual(JSON.parse(await saved(picker.page)), fixture, "One Undo must restore the exact original deadline");
  await navigateKey(picker.page, "Control+Shift+z");
  await picker.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).deadlines[0].at === "2030-01-16T14:30:00Z", key);
  await boot(picker.page, true); assert.deepEqual(JSON.parse(await saved(picker.page)), dated);
  await capture(picker.page, "task-date-time-picker"); await release(picker.context, "task-date-time-picker");

  progress("assign Start and Due drafts from the main Calendar and save once");
  const calendarPick = await newPage("calendar-task-picking", { raw: fixtureRaw, fixedTime: "2030-01-15T12:00:00Z" });
  await boot(calendarPick.page); await openTaskDates(calendarPick.page);
  const dueField = await imeCursor(calendarPick.page);
  await tab(calendarPick.page, 2); // Due picker icon, then Start text.
  const startField = await imeCursor(calendarPick.page, dueField.y + 16);
  assert.ok(startField.y > dueField.y + 16, "Start must be a distinct visible date field");
  await navigateKey(calendarPick.page, "Control+Alt+t");
  const startDay = await selectedDay(calendarPick.page);
  await calendarPick.page.mouse.click(startDay.x, startDay.y); await paint(calendarPick.page);
  assert.equal(await saved(calendarPick.page), fixtureRaw, "Selecting a Start day must only change the unsaved task draft");
  // Disarming keeps the chosen draft while ordinary Calendar browsing resumes.
  await navigateKey(calendarPick.page, "Escape"); const browseDay = await selectedDay(calendarPick.page);
  await calendarPick.page.mouse.click(browseDay.x + browseDay.width + 2, browseDay.y); await paint(calendarPick.page);
  assert.equal(await saved(calendarPick.page), fixtureRaw);
  await focusVisibleInput(calendarPick.page, dueField);
  const dueDay = await selectedDay(calendarPick.page); // Picking status moves this band down.
  await calendarPick.page.mouse.click(dueDay.x + dueDay.width + 2, dueDay.y); await paint(calendarPick.page);
  assert.equal(await saved(calendarPick.page), fixtureRaw, "Selecting a Due day must wait for the same Save date transaction");
  await capture(calendarPick.page, "calendar-task-picking");
  await focusVisibleInput(calendarPick.page, dueField); await navigateKey(calendarPick.page, "Enter");
  const calendarDates = structuredClone(fixture);
  calendarDates.tasks[0].start = "2030-01-15T00:00:00Z"; calendarDates.deadlines[0].at = "2030-01-17T18:00:00Z";
  await calendarPick.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).deadlines[0].at === "2030-01-17T18:00:00Z", key);
  assert.deepEqual(JSON.parse(await saved(calendarPick.page)), calendarDates, "Save must preserve the existing due time, default Start time, IDs, colors, featured deadline and notes");
  await resetFocus(calendarPick.page); await navigateKey(calendarPick.page, "Control+z");
  await calendarPick.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).tasks[0].start === undefined, key);
  assert.deepEqual(JSON.parse(await saved(calendarPick.page)), fixture, "One Undo must restore both original date fields together");
  await navigateKey(calendarPick.page, "Control+Shift+z");
  await calendarPick.page.waitForFunction(key => JSON.parse(localStorage.getItem(key)).tasks[0].start === "2030-01-15T00:00:00Z", key);
  assert.deepEqual(JSON.parse(await saved(calendarPick.page)), calendarDates);
  await boot(calendarPick.page, true); assert.deepEqual(JSON.parse(await saved(calendarPick.page)), calendarDates);
  await release(calendarPick.context, "calendar-task-picking");

  for (const [label, viewport, expression, result, unit] of [
    ["personal-medium", { width: 1024, height: 768 }, "24 in cm", 60.96, "cm"],
    ["personal-mobile", { width: 390, height: 844 }, "72 in mm", 1828.8, "mm"],
  ]) {
    const scene = await newPage(label, { raw: fixtureRaw, viewport, hasTouch: true }); await boot(scene.page);
    assert.equal(await scene.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true);
    if (label === "personal-mobile") { await clickCalculatorIcon(scene.page, true); await typeCalculation(scene.page, expression); }
    else await calculate(scene.page, expression);
    await expectCalculation(scene.page, expression, result, 2, unit);
    assert.equal(await scene.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true, "Calculator popup must fit the viewport");
    assert.deepEqual(JSON.parse(await saved(scene.page)).notes, fixture.notes); await capture(scene.page, label === "personal-mobile" ? "consolidated-mobile" : "consolidated-medium");
    await release(scene.context, label);
  }

  const malformedRaw = "{broken saved sample fixture";
  for (const later of [false, true]) {
    const scene = await newPage(later ? "later-corrupt-preserved" : "malformed-preserved", { raw: later ? fixtureRaw : malformedRaw }); await boot(scene.page);
    if (later) { await scene.page.evaluate(({ key, raw }) => localStorage.setItem(key, raw), { key, raw: malformedRaw }); await resetFocus(scene.page); await navigateKey(scene.page, "Control+Alt+o"); }
    await calculate(scene.page, "10 + 5"); await scene.page.waitForTimeout(800);
    assert.equal(await saved(scene.page), malformedRaw, "Corrupt saved data must remain available for recovery after ordinary edits");
    await release(scene.context, later ? "later-corrupt-preserved" : "malformed-preserved");
  }

  const shared = await newPage("cross-tab-first"); await boot(shared.page);
  const second = await shared.context.newPage(); observe(second, "cross-tab-second"); await boot(second);
  assert.equal(await shared.page.evaluate(() => window.jigLoad()), null); assert.equal(await second.evaluate(() => window.jigLoad()), null);
  await shared.page.evaluate(text => window.jigSave(text), fixtureRaw);
  const conflict = await second.evaluate(text => { try { window.jigSave(text); return null; } catch (error) { return String(error); } }, JSON.stringify({ ...fixture, history: [] }));
  assert.match(conflict, /Another tab changed/); assert.equal(await saved(shared.page), fixtureRaw);
  const clearConflict = await second.evaluate(() => { try { window.jigClear(); return null; } catch (error) { return String(error); } });
  assert.match(clearConflict, /Another tab changed/); assert.equal(await saved(shared.page), fixtureRaw);
  const independent = await newPage("independent-sample"); await boot(independent.page, false, sampleUrl.href);
  assert.equal(await saved(independent.page), null); assert.equal(await saved(shared.page), fixtureRaw);
  await release(shared.context, "cross-tab pair"); await release(independent.context, "independent-sample");

  const celsius = { name: "Sample Harbor", latitude: 51.5, longitude: -0.12, unit: "celsius" };
  const weatherFixture = { ...fixture, preferences: { ...fixture.preferences, weather: celsius } }, weatherDate = "2026-10-30T12:00:00Z";
  const weatherMock = { config: celsius, requests: [], now: weatherDate };
  const weather = await newPage("weather-calendar", { raw: JSON.stringify(weatherFixture), weatherSettings: celsius, weather: weatherMock, viewport: { width: 1440, height: 1000 }, hasTouch: true, fixedTime: weatherDate });
  await boot(weather.page); const cached = await expectWeatherCache(weather.page, celsius); checkWeather(cached, celsius, weatherDate);
  assert.equal(weatherMock.requests.length, 1); await boot(weather.page, true); await weather.page.waitForTimeout(800); assert.equal(weatherMock.requests.length, 1, "Fresh forecasts must survive reload without another request");
  await resetFocus(weather.page); await navigateKey(weather.page, "Control+Alt+t"); await navigateKey(weather.page, "Home");
  const today = await selectedDay(weather.page); await capture(weather.page, "calendar-weather-unselected");
  const clip = { x: 0, y: 0, width: 1440, height: Math.floor(today.top - 23) };
  assert.ok(clip.height > 70); const todayWeather = await weather.page.screenshot({ clip });
  await navigateKey(weather.page, "ArrowRight"); await paint(weather.page);
  await expectClipChange(weather.page, todayWeather, clip, "Selecting tomorrow must update weather above the date strip");
  assert.deepEqual(JSON.parse(await saved(weather.page)), weatherFixture, "Calendar selection must preserve all task records and the featured countdown");
  await navigateKey(weather.page, "Home"); const beforeZoom = await selectedDay(weather.page);
  await navigateKey(weather.page, "="); const zoomed = await selectedDay(weather.page);
  assert.ok(zoomed.width > beforeZoom.width + 5, "Keyboard zoom must enlarge calendar days");
  await navigateKey(weather.page, "-"); const restored = await selectedDay(weather.page);
  assert.ok(Math.abs(restored.width - beforeZoom.width) <= 2, "Zoom out must reveal more dates at the previous readable scale");
  await weather.page.mouse.move(restored.x, restored.y); await weather.page.mouse.down();
  await weather.page.mouse.move(restored.x + 80, restored.y, { steps: 8 }); await weather.page.mouse.up(); await paint(weather.page);
  const panned = await selectedDay(weather.page);
  assert.ok(panned.x - restored.x > 55, "Dragging the date strip must move the selected date across the continuous axis");
  assert.deepEqual(JSON.parse(await saved(weather.page)), weatherFixture, "Pan and zoom must not edit or feature tasks");
  await weather.page.touchscreen.tap(panned.x, panned.y); await paint(weather.page); // Refocus the band after dragging.
  await navigateKey(weather.page, "Home"); const touchToday = await selectedDay(weather.page);
  await weather.page.touchscreen.tap(touchToday.x + touchToday.width + 2, touchToday.y); await paint(weather.page);
  await expectClipChange(weather.page, todayWeather, clip, "Touching the adjacent day must update the same weather strip");
  assert.deepEqual(JSON.parse(await saved(weather.page)), weatherFixture); await capture(weather.page, "calendar-weather-selected");
  await weather.page.setViewportSize({ width: 390, height: 844 }); await weather.page.waitForTimeout(300);
  await selectedDay(weather.page); // Shrinking keeps the previously visible selection on screen.
  assert.equal(await weather.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true); await capture(weather.page, "calendar-mobile");
  await release(weather.context, "weather-calendar");

  const fahrenheit = { name: "Sample Ridge", latitude: 35.68, longitude: 139.76, unit: "fahrenheit" }, fahrenheitMock = { config: fahrenheit, requests: [] };
  const alternate = await newPage("weather-fahrenheit", { raw: JSON.stringify({ ...fixture, preferences: { ...fixture.preferences, weather: fahrenheit } }), weatherSettings: fahrenheit, weather: fahrenheitMock });
  await boot(alternate.page); checkWeather(await expectWeatherCache(alternate.page, fahrenheit), fahrenheit); assert.equal(fahrenheitMock.requests.length, 1);
  await release(alternate.context, "weather-fahrenheit");
  const staleCache = structuredClone(cached); staleCache.forecast.fetched_at = new Date(Date.now() - 24 * 3600 * 1000).toISOString();
  const outageMock = { config: celsius, requests: [], fail: true };
  const outage = await newPage("weather-stale-outage", { raw: JSON.stringify(weatherFixture), weatherSettings: celsius, weatherCache: staleCache, weather: outageMock });
  await boot(outage.page); await outage.page.waitForTimeout(3500);
  assert.equal(outageMock.requests.length, 1, "An outage must not create a repaint-driven request loop");
  assert.deepEqual(await outage.page.evaluate(key => JSON.parse(localStorage.getItem(key)), weatherCacheKey), staleCache);
  await release(outage.context, "weather-stale-outage");
  assert.deepEqual(issues, [], "Browser errors or unexpected remote requests");
  console.log("Browser smoke passed: 19 bridge-race scenarios, Tasks startup, calculator popup from Tasks/Sheet, arithmetic/fractions/conversions/unrounded ans/reload/invalid input, temporary samples, saved notes and linked tasks, JSON confirmation/invalid imports, corrupt/stale-tab protection, independent contexts, desktop/medium/mobile layout, continuous calendar keyboard/touch/pan/zoom, and mocked local weather/cache/offline recovery.");
} catch (error) {
  console.error(`Browser smoke failed during: ${phase}`, error);
  for (const { page, label } of pages.slice(-2)) if (!page.isClosed()) await page.screenshot({ path: resolve(results, `failure-${label}.png`), timeout: 3000 }).catch(() => {});
  if (issues.length) console.error("Browser diagnostics:", issues.join("\n"));
  throw error;
} finally {
  clearTimeout(suiteTimer); await bounded(browser.close(), 10000, "Closing Chromium").catch(error => { console.error(String(error)); process.exit(1); });
}

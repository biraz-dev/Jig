// Real Chromium checks for the built Rust/WASM preview. All datasets are synthetic.
// Serve dist at /Jig/ before running: node tests/browser_smoke.mjs
import assert from "node:assert/strict";
import { mkdir, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium } from "playwright";

const base = process.env.JIG_PREVIEW_URL ?? "http://127.0.0.1:4173/Jig/";
const origin = new URL(base).origin;
const key = "jig.personal.v1";
const weatherSettingsKey = "jig.weather.settings.v1";
const weatherCacheKey = "jig.weather.cache.v1";
const weatherOrigins = ["https://api.open-meteo.com", "https://geocoding-api.open-meteo.com"];
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

async function newPage(label, {
  raw, viewport = { width: 1440, height: 1100 }, weather, weatherSettings, weatherCache,
} = {}) {
  const context = await browser.newContext({ viewport, acceptDownloads: true });
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
          await route.fulfill({ status: 200, contentType: "application/json", headers: { "access-control-allow-origin": "*" }, body: JSON.stringify(mockForecast(weather.config)) });
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
  if (reload) await page.reload({ waitUntil: "load" });
  else await page.goto(address, { waitUntil: "load" });
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

async function expectTransferredInput(page, result, unit) {
  // Copy through the ordinary focused text field, not a test-only state hook.
  await page.evaluate(() => navigator.clipboard.writeText("Jig copy pending"));
  await page.keyboard.press("Control+Alt+u");
  await page.waitForTimeout(300);
  await page.keyboard.press("Control+a");
  await page.keyboard.press("Control+c");
  await page.waitForFunction(async expected => (await navigator.clipboard.readText()) === expected, `${result}${unit}`);
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

function mockForecast(config) {
  const unit = config.unit;
  const today = new Date();
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
  await page.waitForFunction(({ key, config }) => {
    try {
      const cache = JSON.parse(localStorage.getItem(key));
      return cache.config.name === config.name && cache.config.unit === config.unit
        && cache.forecast.days.length === 2 && cache.forecast.current !== null;
    } catch { return false; }
  }, { key: weatherCacheKey, config });
  return page.evaluate(key => JSON.parse(localStorage.getItem(key)), weatherCacheKey);
}

function checkWeather(cache, config) {
  const raw = mockForecast(config);
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
  await boot(demo.page, false, `${base}#weather=Sample%20Harbor`);
  assert.equal(await saved(demo.page), null, "Fresh preview should start with temporary sample data");
  await demo.page.screenshot({ path: resolve(results, "sample-desktop.png") });
  await calculate(demo.page, "1 / 3");
  await demo.page.waitForTimeout(1200);
  assert.equal(await saved(demo.page), null, "Editing sample data must not create personal browser state");

  const personal = await newPage("personal-desktop", { raw: fixtureRaw });
  await boot(personal.page);
  assert.equal(await saved(personal.page), fixtureRaw, "Opening personal state must not rewrite it");
  await calculate(personal.page, "1 / 3");
  await expectCalculation(personal.page, "1 / 3", 1 / 3, 2);
  await personal.context.grantPermissions(["clipboard-read", "clipboard-write"], { origin });
  await expectTransferredInput(personal.page, 1 / 3, "in");
  // A calculator result becomes a length in the current source unit without
  // rounding to its two-decimal display or resetting the user's chosen unit.
  await personal.page.keyboard.type("2cm", { delay: 15 });
  await expectTransferredInput(personal.page, 1 / 3, "cm");
  assert.equal(JSON.parse(await saved(personal.page)).history.length, 2, "Transferring a result must not create a calculation");
  assert.deepEqual(JSON.parse(await saved(personal.page)).entries, fixture.entries, "Hiding calendar entries must not delete saved notes");
  await boot(personal.page, true);
  assert.equal(JSON.parse(await saved(personal.page)).history.at(-1).result, 1 / 3);
  assert.deepEqual(JSON.parse(await saved(personal.page)).entries, fixture.entries, "Calendar notes must survive restart while hidden");
  await calculate(personal.page, "ans * 3");
  await expectCalculation(personal.page, "ans * 3", 1, 3);

  // The merged calculator/converter must work in both its expanded and closed
  // dashboard forms, with no extra history records caused by switching views.
  await personal.page.keyboard.press("F2");
  await calculate(personal.page, "3 / 8");
  await expectCalculation(personal.page, "3 / 8", 0.375, 4);
  await personal.page.screenshot({ path: resolve(results, "calculator-units-expanded.png") });
  await personal.page.keyboard.press("F2");
  await calculate(personal.page, "ans * 8");
  await expectCalculation(personal.page, "ans * 8", 3, 5);
  assert.deepEqual(JSON.parse(await saved(personal.page)).entries, fixture.entries, "Combined-tool interactions must preserve hidden calendar notes");
  await personal.page.screenshot({ path: resolve(results, "personal-desktop.png") });

  // Exercise the public file bridge: export is a local browser download.
  const exportRaw = await saved(personal.page);
  const downloadEvent = personal.page.waitForEvent("download");
  await personal.page.evaluate(text => window.jigExport(text), exportRaw);
  const download = await downloadEvent;
  assert.equal(download.suggestedFilename(), "jig.json");
  const downloadPath = await download.path();
  assert.ok(downloadPath);
  const exported = JSON.parse(await readFile(downloadPath, "utf8"));
  assert.deepEqual(exported, JSON.parse(exportRaw));
  assert.deepEqual(exported.entries, fixture.entries, "JSON export must retain hidden calendar notes");

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
  // Reload explicit synthetic personal state so narrow-screen keyboard input
  // is verified through a real saved result rather than only a screenshot.
  await compact.page.evaluate(text => window.jigSave(text), fixtureRaw);
  await boot(compact.page, true);
  await calculate(compact.page, "12 * 1.5");
  await expectCalculation(compact.page, "12 * 1.5", 18, 2);
  assert.deepEqual(JSON.parse(await saved(compact.page)).entries, fixture.entries);
  await compact.page.screenshot({ path: resolve(results, "calculator-units-mobile.png") });

  // Weather is opt-in. Existing contexts reject provider requests, even when
  // a URL hash prefills a city; all enabled cases below intercept the provider.
  const celsius = { name: "Sample Harbor", latitude: 51.5, longitude: -0.12, unit: "celsius" };
  const weatherFixture = { ...fixture, preferences: { ...fixture.preferences, weather: celsius } };
  const weatherMock = { config: celsius, requests: [] };
  const weather = await newPage("weather-celsius", {
    raw: JSON.stringify(weatherFixture), weatherSettings: celsius, weather: weatherMock,
  });
  await boot(weather.page);
  const cached = await expectWeatherCache(weather.page, celsius);
  checkWeather(cached, celsius);
  assert.equal(weatherMock.requests.length, 1, "A configured location needs one forecast request");
  assert.deepEqual(JSON.parse(await saved(weather.page)).tasks, fixture.tasks);
  assert.deepEqual(JSON.parse(await saved(weather.page)).history, fixture.history);
  await weather.page.screenshot({ path: resolve(results, "weather-desktop.png") });
  await boot(weather.page, true);
  await weather.page.waitForTimeout(1600);
  assert.equal(weatherMock.requests.length, 1, "A fresh cached forecast should survive restart without another provider request");
  checkWeather(await expectWeatherCache(weather.page, celsius), celsius);
  await weather.page.setViewportSize({ width: 390, height: 844 });
  await weather.page.waitForTimeout(500);
  assert.equal(await weather.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true);
  await weather.page.screenshot({ path: resolve(results, "weather-compact.png") });

  // A second city/unit configuration is independent and must request Fahrenheit.
  const fahrenheit = { name: "Sample Ridge", latitude: 35.68, longitude: 139.76, unit: "fahrenheit" };
  const fahrenheitMock = { config: fahrenheit, requests: [] };
  const alternate = await newPage("weather-fahrenheit", {
    raw: JSON.stringify({ ...fixture, preferences: { ...fixture.preferences, weather: fahrenheit } }),
    weatherSettings: fahrenheit, weather: fahrenheitMock,
  });
  await boot(alternate.page);
  checkWeather(await expectWeatherCache(alternate.page, fahrenheit), fahrenheit);
  assert.equal(fahrenheitMock.requests.length, 1);

  // Stale readings remain available during an outage, and failed requests must
  // not be retried at the one-second dashboard repaint rate.
  const staleCache = structuredClone(cached);
  staleCache.forecast.fetched_at = new Date(Date.now() - 24 * 3600 * 1000).toISOString();
  const outageMock = { config: celsius, requests: [], fail: true };
  const outage = await newPage("weather-stale-outage", {
    raw: JSON.stringify(weatherFixture), weatherSettings: celsius,
    weatherCache: staleCache, weather: outageMock,
  });
  await boot(outage.page);
  await outage.page.waitForFunction(key => localStorage.getItem(key) !== null, weatherCacheKey);
  await outage.page.waitForTimeout(3500);
  assert.equal(outageMock.requests.length, 1, "An outage must not create a request loop");
  assert.deepEqual(await outage.page.evaluate(key => JSON.parse(localStorage.getItem(key)), weatherCacheKey), staleCache, "An outage must preserve the last usable forecast");
  await outage.page.screenshot({ path: resolve(results, "weather-stale.png") });

  assert.deepEqual(issues, [], "Browser errors or unexpected remote requests");
  console.log("Browser smoke passed: WASM startup, sample isolation, real compact/expanded calculator input, unrounded calculator-to-converter transfer, precise restart persistence, hidden calendar-note preservation, local JSON export, invalid import preservation, corrupt state recovery, stale-tab rejection, independent contexts, compact layout, opt-in mocked weather, Fahrenheit/Celsius forecasts, cache reuse and stale offline recovery.");
} catch (error) {
  for (const { page, label } of pages) {
    if (!page.isClosed()) {
      const diagnostics = await page.evaluate(key => ({
        // These contexts contain only the explicit synthetic fixtures above.
        saved: localStorage.getItem(key),
        activeElement: document.activeElement ? {
          tag: document.activeElement.tagName,
          id: document.activeElement.id,
          value: document.activeElement.value ?? null,
        } : null,
        viewport: { width: innerWidth, height: innerHeight },
      }), key).catch(error => ({ unavailable: String(error) }));
      console.error(`Failure diagnostics (${label}):`, JSON.stringify(diagnostics));
      await page.screenshot({ path: resolve(results, `failure-${label}.png`) }).catch(() => {});
    }
  }
  if (issues.length) console.error("Browser diagnostics:", issues.join("\n"));
  throw error;
} finally {
  await browser.close();
}

/* Optional Open-Meteo access. No task, deadline or calculation data is sent. */
(() => {
  "use strict";
  window.jigWeatherErrorText = error => String(error?.message ?? error);
  const settingsKey = "jig.weather.settings.v1";
  const cacheKey = "jig.weather.cache.v1";
  let forecast = null, forecastError = null, places = null, placesError = null;
  let forecastToken = 0, searchToken = 0, forecastAbort = null, searchAbort = null;
  const equal = (a, b) => a && b && a.name === b.name && a.latitude === b.latitude && a.longitude === b.longitude && a.unit === b.unit;
  window.jigWeatherLoadConfig = () => localStorage.getItem(settingsKey);
  window.jigWeatherSaveConfig = raw => {
    if (raw === null) localStorage.removeItem(settingsKey);
    else localStorage.setItem(settingsKey, raw);
    forecastToken++; forecastAbort?.abort(); forecast = null; forecastError = null;
    if (raw === null) {
      searchToken++; searchAbort?.abort(); places = null; placesError = null;
      localStorage.removeItem(cacheKey);
    }
  };
  window.jigWeatherLoadCache = () => localStorage.getItem(cacheKey);
  window.jigWeatherSaveCache = raw => {
    const entry = JSON.parse(raw), selected = JSON.parse(localStorage.getItem(settingsKey));
    if (equal(entry.config, selected)) localStorage.setItem(cacheKey, raw);
  };
  window.jigWeatherClearCache = () => localStorage.removeItem(cacheKey);
  async function request(url, controller) {
    const timeout = setTimeout(() => controller.abort(), 12000);
    try {
      const response = await fetch(url, { signal: controller.signal, credentials: "omit", referrerPolicy: "no-referrer" });
      if (!response.ok) throw new Error(`Weather service returned ${response.status}`);
      return await response.json();
    } finally { clearTimeout(timeout); }
  }
  window.jigWeatherRequest = raw => {
    const config = JSON.parse(raw), token = ++forecastToken;
    forecastAbort?.abort(); forecastAbort = new AbortController();
    forecast = null; forecastError = null;
    const url = new URL("https://api.open-meteo.com/v1/forecast");
    url.search = new URLSearchParams({
      latitude: String(config.latitude), longitude: String(config.longitude),
      current: "temperature_2m,relative_humidity_2m,weather_code",
      hourly: "relative_humidity_2m",
      daily: "temperature_2m_max,temperature_2m_min,precipitation_probability_max,weather_code",
      forecast_days: "16", timezone: "auto", temperature_unit: config.unit,
    }).toString();
    request(url, forecastAbort).then(data => {
      if (token === forecastToken) forecast = JSON.stringify({ config, fetched_at: new Date().toISOString(), data });
    }).catch(error => {
      if (token === forecastToken) forecastError = `Forecast unavailable: ${error.name === "AbortError" ? "request timed out" : error.message}`;
    });
  };
  window.jigWeatherTakeForecast = () => {
    if (forecastError) { const message = forecastError; forecastError = null; throw new Error(message); }
    const result = forecast; forecast = null; return result;
  };
  window.jigWeatherSearch = query => {
    const token = ++searchToken;
    searchAbort?.abort(); searchAbort = new AbortController(); places = null; placesError = null;
    const url = new URL("https://geocoding-api.open-meteo.com/v1/search");
    url.search = new URLSearchParams({ name: query, count: "5", language: "en", format: "json" }).toString();
    request(url, searchAbort).then(data => {
      if (token === searchToken) places = JSON.stringify(data);
    }).catch(error => {
      if (token === searchToken) placesError = `Location search unavailable: ${error.name === "AbortError" ? "request timed out" : error.message}`;
    });
  };
  window.jigWeatherTakeLocations = () => {
    if (placesError) { const message = placesError; placesError = null; throw new Error(message); }
    const result = places; places = null; return result;
  };
  window.jigWeatherSetupQuery = () => {
    const params = new URLSearchParams(location.hash.slice(1)), query = params.get("weather");
    if (!query || query.length > 200) return null;
    params.delete("weather");
    history.replaceState(null, "", location.pathname + location.search + (params.size ? "#" + params : ""));
    return query;
  };
})();

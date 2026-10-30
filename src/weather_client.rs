//! Browser network/cache adapter. Core weather parsing remains offline and portable.
use chrono::{DateTime, Utc};
use jig::weather::{parse_forecast, Forecast, TemperatureUnit, WeatherConfig};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = jigWeatherErrorText)]
    fn error_text(value: JsValue) -> String;
    #[wasm_bindgen(catch, js_name = jigWeatherLoadConfig)]
    fn config_json() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = jigWeatherSaveConfig)]
    fn store_config(value: JsValue) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigWeatherLoadCache)]
    fn cache_json() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = jigWeatherSaveCache)]
    fn store_cache(text: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigWeatherClearCache)]
    fn remove_cache() -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigWeatherRequest)]
    fn fetch_forecast(text: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigWeatherTakeForecast)]
    fn forecast_json() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = jigWeatherSearch)]
    fn search(query: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigWeatherTakeLocations)]
    fn locations_json() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = jigWeatherSetupQuery)]
    fn setup_json() -> JsValue;
}
fn error(value: JsValue) -> String {
    error_text(value)
}

fn text(value: JsValue) -> Result<Option<String>, String> {
    if value.is_null() || value.is_undefined() {
        Ok(None)
    } else {
        value
            .as_string()
            .map(Some)
            .ok_or("Expected weather JSON".into())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CachedForecast {
    pub config: WeatherConfig,
    pub forecast: Forecast,
}
#[derive(Deserialize)]
struct ForecastReply {
    config: WeatherConfig,
    fetched_at: DateTime<Utc>,
    data: serde_json::Value,
}
#[derive(Deserialize)]
struct Locations {
    #[serde(default)]
    results: Vec<Place>,
}
#[derive(Deserialize)]
struct Place {
    name: String,
    latitude: f64,
    longitude: f64,
    admin1: Option<String>,
    country: Option<String>,
}

pub fn load_config() -> Result<Option<WeatherConfig>, String> {
    let Some(raw) = text(config_json().map_err(error)?)? else {
        return Ok(None);
    };
    let config: WeatherConfig = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    config.validate()?;
    Ok(Some(config))
}
pub fn save_config(config: Option<&WeatherConfig>) -> Result<(), String> {
    let value = if let Some(config) = config {
        config.validate()?;
        JsValue::from_str(&serde_json::to_string(config).map_err(|e| e.to_string())?)
    } else {
        JsValue::NULL
    };
    store_config(value).map_err(error)
}
pub fn load_cache() -> Result<Option<CachedForecast>, String> {
    let Some(raw) = text(cache_json().map_err(error)?)? else {
        return Ok(None);
    };
    let cached: CachedForecast = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    cached.config.validate()?;
    cached.forecast.validate()?;
    if cached.forecast.fetched_at > Utc::now() + chrono::Duration::minutes(5) {
        return Err("Cached weather timestamp is in the future".into());
    }
    Ok(Some(cached))
}
pub fn clear_cache() -> Result<(), String> {
    remove_cache().map_err(error)
}
pub fn request_forecast(config: &WeatherConfig) -> Result<(), String> {
    config.validate()?;
    fetch_forecast(&serde_json::to_string(config).map_err(|e| e.to_string())?).map_err(error)
}
pub fn take_forecast() -> Result<Option<CachedForecast>, String> {
    let Some(raw) = text(forecast_json().map_err(error)?)? else {
        return Ok(None);
    };
    let reply: ForecastReply = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    reply.config.validate()?;
    let forecast = parse_forecast(&reply.data.to_string(), reply.fetched_at)?;
    let cached = CachedForecast {
        config: reply.config,
        forecast,
    };
    // A full browser cache must not prevent an already fetched forecast being shown.
    if let Ok(json) = serde_json::to_string(&cached) {
        let _ = store_cache(&json);
    }
    Ok(Some(cached))
}
pub fn request_locations(query: &str) -> Result<(), String> {
    let query = query.trim();
    if query.chars().count() < 2 || query.len() > 200 {
        return Err("Enter a city or postcode (2–200 characters)".into());
    }
    search(query).map_err(error)
}
pub fn take_locations() -> Result<Option<Vec<WeatherConfig>>, String> {
    let Some(raw) = text(locations_json().map_err(error)?)? else {
        return Ok(None);
    };
    let places: Locations = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    for place in places.results.into_iter().take(5) {
        let mut parts = vec![place.name];
        for part in [place.admin1, place.country].into_iter().flatten() {
            if !parts.contains(&part) {
                parts.push(part);
            }
        }
        let config = WeatherConfig {
            name: parts.join(", "),
            latitude: place.latitude,
            longitude: place.longitude,
            unit: TemperatureUnit::Fahrenheit,
        };
        config.validate()?;
        results.push(config);
    }
    Ok(Some(results))
}
pub fn setup_query() -> Option<String> {
    setup_json().as_string()
}

//! Portable weather data and Open-Meteo decoding; no network or browser APIs.
use crate::Result;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TemperatureUnit {
    #[default]
    Fahrenheit,
    Celsius,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WeatherConfig {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub unit: TemperatureUnit,
}

impl WeatherConfig {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() || self.name.chars().any(char::is_control) {
            return Err("Weather location needs a name without control characters".into());
        }
        if !self.latitude.is_finite()
            || !(-90.0..=90.0).contains(&self.latitude)
            || !self.longitude.is_finite()
            || !(-180.0..=180.0).contains(&self.longitude)
        {
            return Err("Weather coordinates are outside their valid range".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CurrentWeather {
    pub temperature: Option<f64>,
    pub humidity: Option<f64>,
    pub code: Option<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DayWeather {
    pub date: NaiveDate,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub rain_probability: Option<f64>,
    /// Mean of available hourly relative humidity values for this local date.
    pub humidity_mean: Option<f64>,
    pub code: Option<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Forecast {
    pub timezone: String,
    pub fetched_at: DateTime<Utc>,
    pub current: Option<CurrentWeather>,
    pub days: Vec<DayWeather>,
}

impl Forecast {
    /// Check both freshly decoded responses and untrusted browser cache entries.
    pub fn validate(&self) -> Result<()> {
        if self.timezone.trim().is_empty() || self.timezone.chars().any(char::is_control) {
            return Err("Invalid weather timezone".into());
        }
        if let Some(current) = &self.current {
            finite(current.temperature, "current temperature")?;
            percentage(current.humidity, "current humidity")?;
            weather_code(current.code)?;
        }
        let mut previous = None;
        for day in &self.days {
            if previous.is_some_and(|date| date >= day.date) {
                return Err("Weather dates must be unique and increasing".into());
            }
            previous = Some(day.date);
            finite(day.high, "daily high temperature")?;
            finite(day.low, "daily low temperature")?;
            if let (Some(high), Some(low)) = (day.high, day.low) {
                if high < low {
                    return Err("Daily high temperature cannot be below the low".into());
                }
            }
            percentage(day.rain_probability, "rain probability")?;
            percentage(day.humidity_mean, "mean humidity")?;
            weather_code(day.code)?;
        }
        Ok(())
    }
}

fn finite(value: Option<f64>, label: &str) -> Result<()> {
    if value.is_some_and(|value| !value.is_finite()) {
        return Err(format!("Invalid {label}"));
    }
    Ok(())
}

fn percentage(value: Option<f64>, label: &str) -> Result<()> {
    finite(value, label)?;
    if value.is_some_and(|value| !(0.0..=100.0).contains(&value)) {
        return Err(format!("{label} must be between 0 and 100"));
    }
    Ok(())
}

fn weather_code(code: Option<u8>) -> Result<()> {
    if code.is_some_and(|code| {
        !matches!(
            code,
            0 | 1
                | 2
                | 3
                | 45
                | 48
                | 51
                | 53
                | 55
                | 56
                | 57
                | 61
                | 63
                | 65
                | 66
                | 67
                | 71
                | 73
                | 75
                | 77
                | 80
                | 81
                | 82
                | 85
                | 86
                | 95
                | 96
                | 99
        )
    }) {
        return Err("Invalid WMO weather code".into());
    }
    Ok(())
}

#[derive(Deserialize)]
struct ApiForecast {
    timezone: String,
    current: Option<ApiCurrent>,
    daily: ApiDaily,
    hourly: Option<ApiHourly>,
}

#[derive(Deserialize)]
struct ApiCurrent {
    temperature_2m: Option<f64>,
    relative_humidity_2m: Option<f64>,
    weather_code: Option<u8>,
}

#[derive(Deserialize)]
struct ApiDaily {
    time: Vec<String>,
    temperature_2m_max: Option<Vec<Option<f64>>>,
    temperature_2m_min: Option<Vec<Option<f64>>>,
    precipitation_probability_max: Option<Vec<Option<f64>>>,
    weather_code: Option<Vec<Option<u8>>>,
}

#[derive(Deserialize)]
struct ApiHourly {
    time: Vec<String>,
    relative_humidity_2m: Option<Vec<Option<f64>>>,
}

fn series<T>(values: Option<Vec<Option<T>>>, length: usize, field: &str) -> Result<Vec<Option<T>>> {
    match values {
        Some(values) if values.len() != length => Err(format!(
            "Weather {field} length does not match its time array"
        )),
        Some(values) => Ok(values),
        None => Ok(std::iter::repeat_with(|| None).take(length).collect()),
    }
}

fn hourly_date(text: &str) -> Result<NaiveDate> {
    NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M")
        .or_else(|_| NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S"))
        .map(|time| time.date())
        .or_else(|_| DateTime::parse_from_rfc3339(text).map(|time| time.date_naive()))
        .map_err(|_| "Invalid hourly weather time".into())
}

/// Decode a forecast without inventing values for absent/null provider fields.
/// Hourly humidity is grouped by each timestamp's local calendar date, not by
/// blocks of 24 values; short/long days and incomplete data remain correctly dated.
pub fn parse_forecast(text: &str, fetched_at: DateTime<Utc>) -> Result<Forecast> {
    let api: ApiForecast =
        serde_json::from_str(text).map_err(|error| format!("Invalid weather response: {error}"))?;
    let count = api.daily.time.len();
    let highs = series(api.daily.temperature_2m_max, count, "temperature_2m_max")?;
    let lows = series(api.daily.temperature_2m_min, count, "temperature_2m_min")?;
    let rain = series(
        api.daily.precipitation_probability_max,
        count,
        "precipitation_probability_max",
    )?;
    let codes = series(api.daily.weather_code, count, "weather_code")?;
    let mut humidities: BTreeMap<NaiveDate, (f64, usize)> = BTreeMap::new();
    if let Some(hourly) = api.hourly {
        let values = series(
            hourly.relative_humidity_2m,
            hourly.time.len(),
            "relative_humidity_2m",
        )?;
        for (time, humidity) in hourly.time.iter().zip(values) {
            let date = hourly_date(time)?;
            percentage(humidity, "hourly humidity")?;
            if let Some(humidity) = humidity {
                let aggregate = humidities.entry(date).or_default();
                aggregate.0 += humidity;
                aggregate.1 += 1;
            }
        }
    }
    let mut days = Vec::with_capacity(count);
    for (index, text) in api.daily.time.iter().enumerate() {
        let date = NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .map_err(|_| "Invalid daily weather date")?;
        days.push(DayWeather {
            date,
            high: highs[index],
            low: lows[index],
            rain_probability: rain[index],
            humidity_mean: humidities
                .get(&date)
                .map(|(sum, count)| sum / *count as f64),
            code: codes[index],
        });
    }
    let forecast = Forecast {
        timezone: api.timezone,
        fetched_at,
        current: api.current.map(|current| CurrentWeather {
            temperature: current.temperature_2m,
            humidity: current.relative_humidity_2m,
            code: current.weather_code,
        }),
        days,
    };
    forecast.validate()?;
    Ok(forecast)
}

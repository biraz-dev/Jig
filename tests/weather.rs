use chrono::{NaiveDate, TimeZone, Utc};
use jig::{
    model::State,
    weather::{parse_forecast, TemperatureUnit, WeatherConfig},
};
use serde_json::{json, Value};

fn fetched_at() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 28, 12, 0, 0).unwrap()
}

fn response() -> Value {
    json!({
        "timezone": "Europe/Berlin",
        "current": {
            "temperature_2m": 86.0,
            "relative_humidity_2m": 65,
            "weather_code": 2
        },
        "daily": {
            "time": ["2026-03-29", "2026-03-30"],
            "temperature_2m_max": [90.0, 80.0],
            "temperature_2m_min": [70.0, 65.0],
            "precipitation_probability_max": [40, 90],
            "weather_code": [61, 3]
        },
        "hourly": {
            "time": ["2026-03-29T00:00", "2026-03-29T12:00", "2026-03-30T00:00", "2026-03-30T12:00"],
            "relative_humidity_2m": [50, 70, 80, 100]
        }
    })
}

#[test]
fn decodes_current_and_daily_weather_with_labelled_humidity_means() {
    let forecast = parse_forecast(&response().to_string(), fetched_at()).unwrap();
    assert_eq!(forecast.fetched_at, fetched_at());
    assert_eq!(forecast.timezone, "Europe/Berlin");
    let current = forecast.current.as_ref().unwrap();
    assert_eq!(current.temperature, Some(86.0));
    assert_eq!(current.humidity, Some(65.0));
    assert_eq!(current.code, Some(2));
    assert_eq!(forecast.days.len(), 2);
    assert_eq!(
        forecast.days[0].date,
        NaiveDate::from_ymd_opt(2026, 3, 29).unwrap()
    );
    assert_eq!(forecast.days[0].high, Some(90.0));
    assert_eq!(forecast.days[0].low, Some(70.0));
    assert_eq!(forecast.days[0].rain_probability, Some(40.0));
    assert_eq!(forecast.days[0].humidity_mean, Some(60.0));
    assert_eq!(forecast.days[1].humidity_mean, Some(90.0));
    let cached: jig::weather::Forecast =
        serde_json::from_str(&serde_json::to_string(&forecast).unwrap()).unwrap();
    cached.validate().unwrap();
    assert_eq!(cached.days[0].humidity_mean, Some(60.0));
}

#[test]
fn missing_and_null_provider_values_stay_unknown() {
    let mut data = response();
    data["current"] = json!({"temperature_2m": null});
    data["daily"]["temperature_2m_max"] = json!([null, 80.0]);
    data["daily"]
        .as_object_mut()
        .unwrap()
        .remove("temperature_2m_min");
    data["daily"]["precipitation_probability_max"] = Value::Null;
    data["daily"]["weather_code"] = json!([null, null]);
    data["hourly"]["relative_humidity_2m"] = json!([null, 70, null, null]);
    let forecast = parse_forecast(&data.to_string(), fetched_at()).unwrap();
    assert_eq!(forecast.current.as_ref().unwrap().temperature, None);
    assert_eq!(forecast.current.as_ref().unwrap().humidity, None);
    assert_eq!(forecast.days[0].high, None);
    assert_eq!(forecast.days[0].low, None);
    assert_eq!(forecast.days[0].rain_probability, None);
    assert_eq!(forecast.days[0].code, None);
    assert_eq!(forecast.days[0].humidity_mean, Some(70.0));
    assert_eq!(forecast.days[1].humidity_mean, None);
    data.as_object_mut().unwrap().remove("hourly");
    let no_hourly = parse_forecast(&data.to_string(), fetched_at()).unwrap();
    assert!(no_hourly.days.iter().all(|day| day.humidity_mean.is_none()));
}

#[test]
fn humidity_follows_local_dates_across_short_and_long_days() {
    for (first, second, repeated_hour) in [
        ("2026-03-29", "2026-03-30", false),
        ("2026-10-25", "2026-10-26", true),
    ] {
        let mut times = Vec::new();
        let mut values = Vec::new();
        for hour in 0..24 {
            if hour == 2 && !repeated_hour {
                continue;
            }
            times.push(format!("{first}T{hour:02}:00"));
            values.push(20);
            if hour == 2 && repeated_hour {
                times.push(format!("{first}T02:00"));
                values.push(20);
            }
        }
        for hour in 0..24 {
            times.push(format!("{second}T{hour:02}:00"));
            values.push(80);
        }
        let data = json!({
            "timezone": "Europe/Berlin",
            "daily": {"time": [first, second]},
            "hourly": {"time": times, "relative_humidity_2m": values}
        });
        let forecast = parse_forecast(&data.to_string(), fetched_at()).unwrap();
        assert_eq!(forecast.days[0].humidity_mean, Some(20.0));
        assert_eq!(forecast.days[1].humidity_mean, Some(80.0));
    }
    // Explicit offsets retain their local date rather than converting to UTC.
    let data = json!({
        "timezone": "Etc/GMT-2",
        "daily": {"time": ["2026-03-29", "2026-03-30"]},
        "hourly": {
            "time": ["2026-03-29T00:30:00+02:00", "2026-03-30T00:30:00+02:00"],
            "relative_humidity_2m": [10, 90]
        }
    });
    let forecast = parse_forecast(&data.to_string(), fetched_at()).unwrap();
    assert_eq!(forecast.days[0].humidity_mean, Some(10.0));
    assert_eq!(forecast.days[1].humidity_mean, Some(90.0));
}

#[test]
fn rejects_parallel_array_mismatches_and_invalid_values() {
    for field in [
        "temperature_2m_max",
        "temperature_2m_min",
        "precipitation_probability_max",
        "weather_code",
    ] {
        let mut data = response();
        data["daily"][field] = json!([null]);
        assert!(
            parse_forecast(&data.to_string(), fetched_at()).is_err(),
            "{field}"
        );
    }
    let mut data = response();
    data["hourly"]["relative_humidity_2m"] = json!([50]);
    assert!(parse_forecast(&data.to_string(), fetched_at()).is_err());
    for (path, invalid) in [
        (vec!["current", "relative_humidity_2m"], json!(101)),
        (vec!["current", "weather_code"], json!(255)),
        (
            vec!["daily", "precipitation_probability_max"],
            json!([-1, 20]),
        ),
        (vec!["daily", "temperature_2m_min"], json!([99, 65])),
        (vec!["daily", "weather_code"], json!([4, 3])),
        (
            vec!["hourly", "relative_humidity_2m"],
            json!([0, 20, 30, 101]),
        ),
        (
            vec!["hourly", "time"],
            json!([
                "bad",
                "2026-03-29T12:00",
                "2026-03-30T00:00",
                "2026-03-30T12:00"
            ]),
        ),
        (vec!["daily", "time"], json!(["2026-02-30", "2026-03-30"])),
        (vec!["daily", "time"], json!(["2026-03-29", "2026-03-29"])),
    ] {
        let mut data = response();
        data[path[0]][path[1]] = invalid;
        assert!(
            parse_forecast(&data.to_string(), fetched_at()).is_err(),
            "{path:?}"
        );
    }
    assert!(parse_forecast("{}", fetched_at()).is_err());
    assert!(parse_forecast("not json", fetched_at()).is_err());
}

#[test]
fn cached_forecasts_are_validated_independently_of_provider_decoding() {
    let forecast = parse_forecast(&response().to_string(), fetched_at()).unwrap();
    let mut invalid = forecast.clone();
    invalid.current.as_mut().unwrap().temperature = Some(f64::NAN);
    assert!(invalid.validate().is_err());
    let mut invalid = forecast.clone();
    invalid.days[0].humidity_mean = Some(f64::INFINITY);
    assert!(invalid.validate().is_err());
    let mut invalid = forecast.clone();
    invalid.days.swap(0, 1);
    assert!(invalid.validate().is_err());
    let mut invalid = forecast;
    invalid.timezone = "\n".into();
    assert!(invalid.validate().is_err());
}

#[test]
fn location_preferences_validate_and_old_state_remains_compatible() {
    let config = WeatherConfig {
        name: "Sample area".into(),
        latitude: 0.0,
        longitude: 0.0,
        unit: TemperatureUnit::Fahrenheit,
    };
    config.validate().unwrap();
    assert_eq!(
        serde_json::to_string(&TemperatureUnit::Fahrenheit).unwrap(),
        "\"fahrenheit\""
    );
    assert_eq!(TemperatureUnit::default(), TemperatureUnit::Fahrenheit);
    for (latitude, longitude) in [
        (91.0, 0.0),
        (0.0, -181.0),
        (f64::NAN, 0.0),
        (0.0, f64::INFINITY),
    ] {
        let invalid = WeatherConfig {
            latitude,
            longitude,
            ..config.clone()
        };
        assert!(invalid.validate().is_err());
    }
    for name in ["", "  ", "Bad\nname"] {
        assert!(WeatherConfig {
            name: name.into(),
            ..config.clone()
        }
        .validate()
        .is_err());
    }
    let mut state = State::default();
    assert!(state.preferences.weather.is_none());
    let mut old = serde_json::to_value(&state).unwrap();
    old["preferences"]
        .as_object_mut()
        .unwrap()
        .remove("weather");
    let restored = State::from_json(&old.to_string()).unwrap();
    assert!(restored.preferences.weather.is_none());
    state.preferences.weather = Some(config.clone());
    state.validate().unwrap();
    let restored = State::from_json(&serde_json::to_string(&state).unwrap()).unwrap();
    assert_eq!(restored.preferences.weather, Some(config));
    state.preferences.weather.as_mut().unwrap().latitude = 91.0;
    assert!(state.validate().is_err());
    assert!(jig::sample::state(fetched_at())
        .preferences
        .weather
        .is_none());
}

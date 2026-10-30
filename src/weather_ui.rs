//! Optional live weather, isolated from sample and personal workbench records.
use crate::weather_client::{self, CachedForecast};
use chrono::{DateTime, Local, NaiveDate, Utc};
use eframe::egui::{self, Color32, RichText, TextEdit, Ui};
use jig::weather::{DayWeather, TemperatureUnit, WeatherConfig};

const MUTED: Color32 = Color32::from_rgb(144, 161, 202);
const ACCENT: Color32 = Color32::from_rgb(109, 185, 238);
const ERROR: Color32 = Color32::from_rgb(234, 133, 173);
const REFRESH_SECONDS: i64 = 3600;

pub struct WeatherPanel {
    config: Option<WeatherConfig>,
    cache: Option<CachedForecast>,
    query: String,
    locations: Vec<WeatherConfig>,
    editing: bool,
    searching: bool,
    pending: bool,
    last_attempt: Option<DateTime<Utc>>,
    error: Option<String>,
    search_error: Option<String>,
}

impl WeatherPanel {
    /// A personal file preference wins; otherwise use this browser's separate
    /// weather preference. A setup link only pre-fills the location picker.
    pub fn new(preferred: Option<WeatherConfig>) -> Self {
        let (config, error) = match preferred {
            Some(config) => (Some(config), None),
            None => match weather_client::load_config() {
                Ok(config) => (config, None),
                Err(error) => (None, Some(error)),
            },
        };
        let setup = weather_client::setup_query();
        let cache = weather_client::load_cache()
            .ok()
            .flatten()
            .filter(|cache| config.as_ref() == Some(&cache.config));
        let mut panel = Self {
            config,
            cache,
            query: setup.clone().unwrap_or_default(),
            locations: vec![],
            editing: setup.is_some(),
            searching: false,
            pending: false,
            last_attempt: None,
            error,
            search_error: None,
        };
        if panel.config.is_some() {
            if let Err(error) = weather_client::save_config(panel.config.as_ref()) {
                panel.error = Some(error);
            }
        }
        panel.refresh_if_due();
        panel
    }

    pub fn config(&self) -> Option<&WeatherConfig> {
        self.config.as_ref()
    }
    pub fn is_enabled(&self) -> bool {
        self.config.is_some()
    }

    /// Used after an explicit dataset import/open. Passing None explicitly
    /// disables weather; callers can preserve a local preference by skipping it.
    pub fn replace_config(&mut self, config: Option<WeatherConfig>) {
        self.config = config;
        self.pending = false;
        self.last_attempt = None;
        self.error = weather_client::save_config(self.config.as_ref()).err();
        self.cache = weather_client::load_cache()
            .ok()
            .flatten()
            .filter(|cache| self.config.as_ref() == Some(&cache.config));
        self.locations.clear();
        if self.config.is_none() {
            self.cache = None;
            self.query.clear();
            self.editing = false;
            self.searching = false;
            self.search_error = None;
            if let Err(error) = weather_client::clear_cache() {
                self.error = Some(error);
            }
        }
        self.refresh_if_due();
    }

    pub fn disable(&mut self) {
        self.replace_config(None);
    }

    pub fn update(&mut self) {
        match weather_client::take_locations() {
            Ok(Some(locations)) => {
                self.searching = false;
                self.locations = locations;
                self.search_error = if self.locations.is_empty() {
                    Some("No matching locations.".into())
                } else {
                    None
                };
            }
            Ok(None) => {}
            Err(error) => {
                self.searching = false;
                self.search_error = Some(error);
            }
        }
        match weather_client::take_forecast() {
            Ok(Some(cache)) if self.config.as_ref() == Some(&cache.config) => {
                self.pending = false;
                self.cache = Some(cache);
                self.error = None;
            }
            Ok(_) => {}
            Err(error) => {
                self.pending = false;
                self.error = Some(error);
            }
        }
        self.refresh_if_due();
    }

    fn fresh(&self) -> bool {
        self.cache.as_ref().is_some_and(|cache| {
            let age = (Utc::now() - cache.forecast.fetched_at).num_seconds();
            (0..REFRESH_SECONDS).contains(&age)
        })
    }

    fn refresh_if_due(&mut self) {
        if self.config.is_none() || self.pending || self.fresh() {
            return;
        }
        if self
            .last_attempt
            .is_some_and(|at| (Utc::now() - at).num_seconds() < REFRESH_SECONDS)
        {
            return;
        }
        self.refresh();
    }

    fn refresh(&mut self) {
        let Some(config) = self.config.as_ref() else {
            return;
        };
        self.last_attempt = Some(Utc::now());
        match weather_client::request_forecast(config) {
            Ok(()) => {
                self.pending = true;
            }
            Err(error) => {
                self.pending = false;
                self.error = Some(error);
            }
        }
    }

    /// Returns true only when the location, unit or enabled setting changes.
    pub fn controls(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            if let Some(config) = self.config.clone() {
                ui.label(RichText::new("FORECAST").color(ACCENT).small());
                ui.label(&config.name);
                for (unit, label) in [
                    (TemperatureUnit::Fahrenheit, "°F"),
                    (TemperatureUnit::Celsius, "°C"),
                ] {
                    if ui.selectable_label(config.unit == unit, label).clicked()
                        && config.unit != unit
                    {
                        let mut next = config.clone();
                        next.unit = unit;
                        self.replace_config(Some(next));
                        changed = true;
                    }
                }
                if ui.small_button("location").clicked() {
                    self.editing = !self.editing;
                }
                if ui
                    .add_enabled(!self.pending, egui::Button::new("refresh").small())
                    .clicked()
                {
                    self.refresh();
                }
                if ui.small_button("off").clicked() {
                    self.disable();
                    self.editing = false;
                    changed = true;
                }
            } else {
                ui.label(RichText::new("Weather off").color(MUTED).small());
                if ui.small_button("Set location").clicked() {
                    self.editing = !self.editing;
                }
            }
        });
        if self.editing {
            ui.label(RichText::new("Location searches and chosen coordinates go to Open-Meteo. No Jig records are sent.").small().color(MUTED));
            ui.horizontal(|ui| {
                let response = ui.add(
                    TextEdit::singleline(&mut self.query)
                        .hint_text("City or town")
                        .desired_width((ui.available_width() - 85.0).max(90.0)),
                );
                let enter =
                    response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                if (ui
                    .add_enabled(!self.searching, egui::Button::new("Search"))
                    .clicked()
                    || enter)
                    && !self.searching
                {
                    self.locations.clear();
                    match weather_client::request_locations(self.query.trim()) {
                        Ok(()) => {
                            self.searching = true;
                            self.search_error = None;
                        }
                        Err(error) => self.search_error = Some(error),
                    }
                }
            });
            if self.searching {
                ui.label(RichText::new("Searching…").small().color(MUTED));
            }
            if let Some(error) = &self.search_error {
                ui.label(RichText::new(error).small().color(ERROR));
            }
            for mut location in self.locations.clone() {
                if ui.link(&location.name).clicked() {
                    location.unit = self
                        .config
                        .as_ref()
                        .map_or(TemperatureUnit::Fahrenheit, |config| config.unit);
                    self.replace_config(Some(location));
                    self.editing = false;
                    changed = true;
                }
            }
        }
        if let Some(cache) = &self.cache {
            if let Some(current) = &cache.forecast.current {
                let unit = self.unit();
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} {} · humidity {}",
                            if self.fresh() && self.error.is_none() { "Now" } else { "Cached" },
                            temperature(current.temperature, unit),
                            percentage(current.humidity)
                        ))
                        .color(ACCENT),
                    );
                    let condition = condition(current.code);
                    if !condition.is_empty() {
                        ui.label(RichText::new(condition).small().color(MUTED));
                    }
                });
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{}updated {} · {}",
                        if self.fresh() && self.error.is_none() {
                            ""
                        } else {
                            "Stale/offline · "
                        },
                        cache
                            .forecast
                            .fetched_at
                            .with_timezone(&Local)
                            .format("%d %b %H:%M"),
                        cache.forecast.timezone
                    ))
                    .small()
                    .color(MUTED),
                );
            });
        } else if self.pending {
            ui.label(RichText::new("Loading weather…").small().color(MUTED));
        }
        if let Some(error) = &self.error {
            ui.label(RichText::new(error).small().color(ERROR));
        }
        if self.config.is_some() || self.editing {
            ui.horizontal_wrapped(|ui| {
                ui.hyperlink_to(
                    RichText::new("Weather data by Open-Meteo").small(),
                    "https://open-meteo.com/",
                );
                ui.hyperlink_to(
                    RichText::new("CC BY 4.0").small(),
                    "https://creativecommons.org/licenses/by/4.0/",
                );
                if self.editing {
                    ui.hyperlink_to(
                        RichText::new("Locations: GeoNames").small(),
                        "https://www.geonames.org/",
                    );
                }
            });
        }
        changed
    }

    fn unit(&self) -> &'static str {
        match self.config.as_ref().map(|config| config.unit) {
            Some(TemperatureUnit::Celsius) => "°C",
            _ => "°F",
        }
    }

    pub fn day(&self, date: NaiveDate) -> Option<&DayWeather> {
        if date < Local::now().date_naive() {
            return None;
        }
        self.cache
            .as_ref()?
            .forecast
            .days
            .iter()
            .find(|day| day.date == date)
    }

    pub fn cell_text(&self, date: NaiveDate) -> String {
        match self.day(date) {
            Some(day) if day.high.is_some() || day.rain_probability.is_some() => format!(
                "{}\n{}°\nR{}",
                date.format("%-d"),
                number(day.high),
                percentage(day.rain_probability)
            ),
            _ => date.format("%-d").to_string(),
        }
    }

    pub fn day_tooltip(&self, date: NaiveDate) -> Option<String> {
        let day = self.day(date)?;
        let name = &self.config.as_ref()?.name;
        Some(format!(
            "{name} · {date}\nHigh {} · Low {}\nRain {} · Mean humidity {}\n{}",
            temperature(day.high, self.unit()),
            temperature(day.low, self.unit()),
            percentage(day.rain_probability),
            percentage(day.humidity_mean),
            condition(day.code)
        ))
    }

    pub fn selected_day(&self, ui: &mut Ui, date: NaiveDate) {
        if let Some(day) = self.day(date) {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{} · {} / {} · Rain {} · Mean humidity {}",
                        date.format("%d %b"),
                        temperature(day.high, self.unit()),
                        temperature(day.low, self.unit()),
                        percentage(day.rain_probability),
                        percentage(day.humidity_mean)
                    ))
                    .small()
                    .color(ACCENT),
                );
            });
        }
    }
}

fn number(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), |value| format!("{value:.0}"))
}
fn percentage(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), |value| format!("{value:.0}%"))
}
fn temperature(value: Option<f64>, unit: &str) -> String {
    format!("{}{unit}", number(value))
}
fn condition(code: Option<u8>) -> &'static str {
    match code {
        Some(0) => "Clear",
        Some(1 | 2) => "Partly cloudy",
        Some(3) => "Cloudy",
        Some(45 | 48) => "Fog",
        Some(51 | 53 | 55 | 56 | 57) => "Drizzle",
        Some(61 | 63 | 65 | 66 | 67) => "Rain",
        Some(71 | 73 | 75 | 77) => "Snow",
        Some(80..=82) => "Showers",
        Some(85 | 86) => "Snow showers",
        Some(95 | 96 | 99) => "Thunderstorms",
        _ => "",
    }
}

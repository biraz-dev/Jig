//! Portable pan and zoom math for a continuous, day-based calendar.
use chrono::{Datelike, NaiveDate};

pub const MIN_DAY_WIDTH: f32 = 24.0;
pub const MAX_DAY_WIDTH: f32 = 96.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    left_day: f64,
    day_width: f32,
}

impl Viewport {
    pub fn new(date: NaiveDate) -> Self {
        let mut view = Self {
            left_day: f64::from(date.num_days_from_ce() - 3).max(1.0),
            day_width: 40.0,
        };
        view.clamp(0.0);
        view
    }

    pub fn day_width(self) -> f32 {
        self.day_width
    }

    pub fn x_for(self, date: NaiveDate) -> f32 {
        ((f64::from(date.num_days_from_ce()) - self.left_day) * f64::from(self.day_width)) as f32
    }

    /// Dates include partially visible cells at either edge.
    pub fn visible(self, width: f32) -> Vec<(NaiveDate, f32)> {
        let width = view_width(width);
        if width == 0.0 {
            return Vec::new();
        }
        let first = self.left_day.floor() as i32;
        let count = (width / f64::from(self.day_width)).ceil() as i32 + 1;
        (first..first + count)
            .filter_map(|day| {
                let date = NaiveDate::from_num_days_from_ce_opt(day)?;
                let x = self.x_for(date);
                ((1..=9999).contains(&date.year())
                    && f64::from(x) < width
                    && x + self.day_width > 0.0)
                    .then_some((date, x))
            })
            .collect()
    }

    pub fn date_at(self, x: f32, width: f32) -> Option<NaiveDate> {
        if !x.is_finite() || x < 0.0 || f64::from(x) >= view_width(width) {
            return None;
        }
        let day = (self.left_day + f64::from(x / self.day_width)).floor() as i32;
        NaiveDate::from_num_days_from_ce_opt(day).filter(|date| (1..=9999).contains(&date.year()))
    }

    /// A positive drag moves the dates right, revealing earlier days.
    pub fn pan(&mut self, pixels: f32, width: f32) {
        if pixels.is_finite() {
            self.left_day -= f64::from(pixels) / f64::from(self.day_width);
            self.clamp(width);
        }
    }

    /// Keep the date beneath the pointer fixed while changing the day width.
    pub fn zoom(&mut self, factor: f32, anchor_x: f32, width: f32) {
        if !factor.is_finite() || factor <= 0.0 || !anchor_x.is_finite() {
            return;
        }
        let anchor_x = f64::from(anchor_x).clamp(0.0, view_width(width));
        let anchor_day = self.left_day + anchor_x / f64::from(self.day_width);
        self.day_width = (f64::from(self.day_width) * f64::from(factor))
            .clamp(f64::from(MIN_DAY_WIDTH), f64::from(MAX_DAY_WIDTH))
            as f32;
        self.left_day = anchor_day - anchor_x / f64::from(self.day_width);
        self.clamp(width);
    }

    pub fn reveal(&mut self, date: NaiveDate, width: f32) {
        self.left_day = f64::from(date.num_days_from_ce()) + 0.5
            - view_width(width) / (2.0 * f64::from(self.day_width));
        self.clamp(width);
    }

    pub fn ensure_visible(&mut self, date: NaiveDate, width: f32) {
        let day = f64::from(date.num_days_from_ce());
        if self.x_for(date) < 0.0 {
            self.left_day = day;
        } else if f64::from(self.x_for(date) + self.day_width) > view_width(width) {
            self.left_day = day + 1.0 - view_width(width) / f64::from(self.day_width);
        }
        self.clamp(width);
    }

    fn clamp(&mut self, width: f32) {
        let last = f64::from(
            NaiveDate::from_ymd_opt(9999, 12, 31)
                .unwrap()
                .num_days_from_ce(),
        );
        let max_left =
            (last + 1.0 - view_width(width) / f64::from(self.day_width)).clamp(1.0, last);
        self.left_day = self.left_day.clamp(1.0, max_left);
    }
}

fn view_width(width: f32) -> f64 {
    if width.is_finite() {
        f64::from(width).clamp(0.0, 32_768.0)
    } else {
        0.0
    }
}

//! Browser colors; the shared RGB palette also drives the terminal and charts.
use eframe::egui::Color32;
use jig::palette;

pub const fn rgb([r, g, b]: [u8; 3]) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const BG: Color32 = rgb(palette::BACKGROUND);
pub const SURFACE: Color32 = rgb(palette::SURFACE);
pub const TEXT: Color32 = rgb(palette::TEXT);
pub const MUTED: Color32 = rgb(palette::MUTED);
pub const RULE: Color32 = rgb(palette::BORDER);
pub const GRID: Color32 = rgb(palette::GRID);
pub const ACCENT: Color32 = rgb(palette::ACCENT);
pub const SELECTED: Color32 = rgb(palette::SELECTED);
pub const HOVER: Color32 = rgb(palette::HOVER);
pub const ACTIVE: Color32 = rgb(palette::ACTIVE);
pub const ERROR: Color32 = rgb(palette::ERROR);
pub const SUCCESS: Color32 = rgb(palette::SUCCESS);
pub const WEEKEND: Color32 = rgb(palette::WEEKEND);

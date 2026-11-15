//! Browser colors; the shared RGB palette also drives the terminal and charts.
use eframe::egui::{self, Color32, FontId, Stroke};
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
pub const WEEKEND: Color32 = rgb(palette::WEEKEND);

/// Colors belong to the egui context, so independent app views do not share
/// mutable theme state. Explicit document and task colors are not rewritten.
#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub surface: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub rule: Color32,
    pub grid: Color32,
    pub accent: Color32,
    pub selected: Color32,
    pub hover: Color32,
    pub active: Color32,
    pub error: Color32,
    pub weekend: Color32,
    pub pinned: Color32,
}

impl Palette {
    pub const GRAPHITE: Self = Self {
        bg: BG,
        surface: SURFACE,
        text: TEXT,
        muted: MUTED,
        rule: RULE,
        grid: GRID,
        accent: ACCENT,
        selected: SELECTED,
        hover: HOVER,
        active: ACTIVE,
        error: ERROR,
        weekend: WEEKEND,
        pinned: rgb([48, 42, 36]),
    };

    pub const LIGHT: Self = Self {
        bg: rgb([246, 242, 236]),
        surface: rgb([235, 229, 221]),
        text: rgb([49, 43, 39]),
        muted: rgb([102, 92, 83]),
        rule: rgb([157, 144, 132]),
        grid: rgb([207, 197, 186]),
        accent: rgb([175, 64, 38]),
        selected: rgb([239, 210, 196]),
        hover: rgb([229, 219, 209]),
        active: rgb([226, 192, 173]),
        error: rgb([167, 47, 39]),
        weekend: rgb([240, 234, 226]),
        pinned: rgb([239, 226, 203]),
    };
}

pub fn colors(ctx: &egui::Context) -> Palette {
    if ctx.style().visuals.dark_mode {
        Palette::GRAPHITE
    } else {
        Palette::LIGHT
    }
}

/// Install both palettes once. egui then follows OS changes in System mode.
pub fn apply(ctx: &egui::Context, theme: jig::app_settings::Theme) {
    let installed = egui::Id::new("jig-theme-styles");
    if !ctx.data(|data| data.get_temp::<bool>(installed).unwrap_or(false)) {
        for (mode, colors) in [
            (egui::Theme::Dark, Palette::GRAPHITE),
            (egui::Theme::Light, Palette::LIGHT),
        ] {
            let mut style = (*ctx.style_of(mode)).clone();
            style.visuals = if mode == egui::Theme::Dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            };
            style.visuals.panel_fill = colors.bg;
            style.visuals.window_fill = colors.bg;
            style.visuals.override_text_color = Some(colors.text);
            style.visuals.extreme_bg_color = colors.bg;
            style.visuals.faint_bg_color = colors.surface;
            style.visuals.code_bg_color = colors.surface;
            style.visuals.hyperlink_color = colors.accent;
            style.visuals.error_fg_color = colors.error;
            style.visuals.selection.bg_fill = colors.selected;
            style.visuals.selection.stroke = Stroke::new(1.0_f32, colors.accent);
            for (widget, fill) in [
                (&mut style.visuals.widgets.noninteractive, colors.surface),
                (&mut style.visuals.widgets.inactive, colors.surface),
                (&mut style.visuals.widgets.hovered, colors.hover),
                (&mut style.visuals.widgets.active, colors.active),
                (&mut style.visuals.widgets.open, colors.active),
            ] {
                widget.bg_fill = fill;
                widget.weak_bg_fill = fill;
                widget.bg_stroke = Stroke::new(1.0_f32, colors.rule);
                widget.fg_stroke = Stroke::new(1.0_f32, colors.text);
            }
            style.spacing.item_spacing = egui::vec2(8.0, 7.0);
            style.interaction.selectable_labels = false;
            for (kind, size) in [
                (egui::TextStyle::Body, 14.0),
                (egui::TextStyle::Button, 13.0),
                (egui::TextStyle::Monospace, 14.0),
            ] {
                style.text_styles.insert(kind, FontId::monospace(size));
            }
            ctx.set_style_of(mode, style);
        }
        ctx.data_mut(|data| data.insert_temp(installed, true));
    }
    ctx.set_theme(match theme {
        jig::app_settings::Theme::Graphite => egui::ThemePreference::Dark,
        jig::app_settings::Theme::Light => egui::ThemePreference::Light,
        jig::app_settings::Theme::System => egui::ThemePreference::System,
    });
}

pub fn srgb(color: Color32) -> [u8; 3] {
    [color.r(), color.g(), color.b()]
}

/// Quiet, warm graphite shades shared by combined task-list views and tabs.
pub fn task_list_fill(index: usize) -> Color32 {
    const SHADES: [[u8; 3]; 5] = [
        [37, 33, 30],
        [44, 39, 35],
        [50, 44, 39],
        [40, 36, 33],
        [47, 42, 38],
    ];
    rgb(SHADES[index % SHADES.len()])
}

pub fn task_list_fill_for(ctx: &egui::Context, index: usize) -> Color32 {
    if ctx.style().visuals.dark_mode {
        task_list_fill(index)
    } else {
        const SHADES: [[u8; 3]; 5] = [
            [241, 236, 229],
            [234, 227, 218],
            [228, 219, 208],
            [237, 231, 223],
            [231, 223, 213],
        ];
        rgb(SHADES[index % SHADES.len()])
    }
}

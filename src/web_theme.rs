//! Browser colors; the shared RGB palette also drives the terminal and charts.
use eframe::egui::{self, Color32, FontId, Stroke};
use jig::{
    app_settings::{AppSettings, Theme},
    palette::{self, ThemePalette},
};

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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
    pub const EMBER: Self = Self {
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

    fn from_theme(colors: ThemePalette) -> Self {
        if colors == ThemePalette::EMBER {
            return Self::EMBER;
        }
        if colors == ThemePalette::PAPER {
            return Self::LIGHT;
        }
        Self {
            bg: rgb(colors.background),
            surface: rgb(colors.surface),
            text: rgb(colors.text),
            muted: rgb(colors.muted),
            rule: rgb(colors.border),
            grid: rgb(colors.grid),
            accent: rgb(colors.accent),
            selected: rgb(colors.selected),
            hover: rgb(colors.hover),
            active: rgb(colors.active),
            error: rgb(colors.error),
            weekend: rgb(colors.weekend),
            pinned: rgb(colors.pinned),
        }
    }
}

pub fn colors(ctx: &egui::Context) -> Palette {
    Palette::from_theme(resolved(ctx))
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct ThemeStyles {
    dark: ThemePalette,
    light: ThemePalette,
}

/// Read the palette selected by egui, including live OS changes in System mode.
pub fn resolved(ctx: &egui::Context) -> ThemePalette {
    let installed = egui::Id::new("jig-theme-styles");
    let dark = ctx.theme() == egui::Theme::Dark;
    ctx.data(|data| data.get_temp::<ThemeStyles>(installed))
        .map_or_else(
            || {
                if dark {
                    ThemePalette::EMBER
                } else {
                    ThemePalette::PAPER
                }
            },
            |styles| if dark { styles.dark } else { styles.light },
        )
}

pub fn system_accent_available(ctx: &egui::Context) -> bool {
    ctx.data(|data| {
        data.get_temp::<bool>(egui::Id::new("jig-system-accent"))
            .unwrap_or(false)
    })
}

/// Rebuild styles only when colors change. System mode installs both palettes
/// so an OS appearance change does not need an application preference write.
pub fn apply(ctx: &egui::Context, settings: &AppSettings, system_accent: Option<[u8; 3]>) {
    let installed = egui::Id::new("jig-theme-styles");
    let accent = system_accent.filter(|_| settings.system_accent);
    let styles = ThemeStyles {
        dark: ThemePalette::resolve(settings.theme, settings.custom_theme, true, accent),
        light: ThemePalette::resolve(settings.theme, settings.custom_theme, false, accent),
    };
    if ctx.data(|data| data.get_temp::<ThemeStyles>(installed)) != Some(styles) {
        for (mode, colors) in [
            (egui::Theme::Dark, Palette::from_theme(styles.dark)),
            (egui::Theme::Light, Palette::from_theme(styles.light)),
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
        ctx.data_mut(|data| data.insert_temp(installed, styles));
    }
    ctx.data_mut(|data| {
        data.insert_temp(egui::Id::new("jig-system-accent"), system_accent.is_some());
    });
    let preference = if settings.theme == Theme::System {
        egui::ThemePreference::System
    } else if styles.dark.is_dark {
        egui::ThemePreference::Dark
    } else {
        egui::ThemePreference::Light
    };
    if ctx.options(|options| options.theme_preference) != preference {
        ctx.set_theme(preference);
    }
}

pub fn srgb(color: Color32) -> [u8; 3] {
    [color.r(), color.g(), color.b()]
}

/// Quiet, warm Ember shades shared by combined task-list views and tabs.
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
    let colors = resolved(ctx);
    if colors.background == palette::BACKGROUND && colors.surface == palette::SURFACE {
        task_list_fill(index)
    } else if colors.background == ThemePalette::PAPER.background
        && colors.surface == ThemePalette::PAPER.surface
    {
        const SHADES: [[u8; 3]; 5] = [
            [241, 236, 229],
            [234, 227, 218],
            [228, 219, 208],
            [237, 231, 223],
            [231, 223, 213],
        ];
        rgb(SHADES[index % SHADES.len()])
    } else {
        const AMOUNTS: [f32; 5] = [0.04, 0.09, 0.14, 0.065, 0.115];
        let amount = AMOUNTS[index % AMOUNTS.len()];
        rgb(std::array::from_fn(|channel| {
            (f32::from(colors.background[channel]) * (1.0 - amount)
                + f32::from(colors.text[channel]) * amount)
                .round() as u8
        }))
    }
}

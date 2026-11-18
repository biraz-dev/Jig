//! Portable interface palettes. The original warm colors are retained as Ember.
//!
//! Task, category, and axis colors remain separate from these interface roles.

use crate::app_settings::{CustomTheme, Theme};

pub const BACKGROUND: [u8; 3] = [30, 27, 25];
pub const SURFACE: [u8; 3] = [41, 37, 34];
pub const TEXT: [u8; 3] = [230, 225, 219];
pub const MUTED: [u8; 3] = [172, 163, 155];
pub const BORDER: [u8; 3] = [113, 102, 93];
pub const GRID: [u8; 3] = [75, 67, 62];
pub const ACCENT: [u8; 3] = [244, 107, 74];
pub const SELECTED: [u8; 3] = [67, 42, 34];
pub const HOVER: [u8; 3] = [56, 49, 44];
pub const ACTIVE: [u8; 3] = [77, 49, 39];
pub const ERROR: [u8; 3] = [237, 142, 132];
pub const SUCCESS: [u8; 3] = [166, 180, 153];
pub const WEEKEND: [u8; 3] = [35, 31, 28];

/// Semantic interface colors. Document, task and series colors stay separate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThemePalette {
    pub background: [u8; 3],
    pub surface: [u8; 3],
    pub text: [u8; 3],
    pub muted: [u8; 3],
    pub border: [u8; 3],
    pub grid: [u8; 3],
    pub accent: [u8; 3],
    pub selected: [u8; 3],
    pub hover: [u8; 3],
    pub active: [u8; 3],
    pub error: [u8; 3],
    pub weekend: [u8; 3],
    pub pinned: [u8; 3],
    pub is_dark: bool,
}

impl ThemePalette {
    pub const EMBER: Self = Self {
        background: BACKGROUND,
        surface: SURFACE,
        text: TEXT,
        muted: MUTED,
        border: BORDER,
        grid: GRID,
        accent: ACCENT,
        selected: SELECTED,
        hover: HOVER,
        active: ACTIVE,
        error: ERROR,
        weekend: WEEKEND,
        pinned: [48, 42, 36],
        is_dark: true,
    };

    pub const PAPER: Self = Self {
        background: [246, 242, 236],
        surface: [235, 229, 221],
        text: [49, 43, 39],
        muted: [102, 92, 83],
        border: [157, 144, 132],
        grid: [207, 197, 186],
        accent: [175, 64, 38],
        selected: [239, 210, 196],
        hover: [229, 219, 209],
        active: [226, 192, 173],
        error: [167, 47, 39],
        weekend: [240, 234, 226],
        pinned: [239, 226, 203],
        is_dark: false,
    };

    pub const GRAPHITE: Self = Self {
        background: [32, 34, 37],
        surface: [43, 46, 51],
        text: [226, 228, 232],
        muted: [158, 164, 174],
        border: [91, 99, 111],
        grid: [61, 66, 73],
        accent: [128, 185, 176],
        selected: [42, 64, 63],
        hover: [47, 51, 57],
        active: [49, 77, 74],
        error: [237, 142, 132],
        weekend: [37, 40, 44],
        pinned: [45, 52, 52],
        is_dark: true,
    };

    pub const NORD: Self = Self {
        background: [46, 52, 64],
        surface: [59, 66, 82],
        text: [236, 239, 244],
        muted: [175, 185, 202],
        border: [101, 115, 136],
        grid: [75, 85, 104],
        accent: [136, 192, 208],
        selected: [61, 83, 102],
        hover: [67, 76, 94],
        active: [70, 102, 119],
        error: [230, 149, 155],
        weekend: [51, 58, 71],
        pinned: [59, 75, 82],
        is_dark: true,
    };

    pub const DRACULA: Self = Self {
        background: [40, 42, 54],
        surface: [52, 55, 70],
        text: [248, 248, 242],
        muted: [168, 177, 205],
        border: [100, 111, 148],
        grid: [68, 71, 90],
        accent: [189, 147, 249],
        selected: [72, 55, 100],
        hover: [61, 60, 79],
        active: [87, 62, 120],
        error: [255, 139, 147],
        weekend: [45, 46, 61],
        pinned: [61, 55, 75],
        is_dark: true,
    };

    /// Catppuccin's Mocha base, foreground and mauve accent.
    pub const CATPPUCCIN: Self = Self {
        background: [30, 30, 46],
        surface: [49, 50, 68],
        text: [205, 214, 244],
        muted: [166, 173, 200],
        border: [108, 112, 134],
        grid: [69, 71, 90],
        accent: [203, 166, 247],
        selected: [58, 48, 82],
        hover: [44, 43, 63],
        active: [75, 58, 102],
        error: [243, 139, 168],
        weekend: [35, 35, 52],
        pinned: [43, 40, 61],
        is_dark: true,
    };

    /// Gruvbox's dark background, warm foreground and bright yellow accent.
    pub const GRUVBOX: Self = Self {
        background: [40, 40, 40],
        surface: [60, 56, 54],
        text: [235, 219, 178],
        muted: [189, 174, 147],
        border: [146, 131, 116],
        grid: [80, 73, 69],
        accent: [250, 189, 47],
        selected: [80, 65, 36],
        hover: [55, 51, 44],
        active: [102, 81, 40],
        error: [251, 140, 122],
        weekend: [45, 43, 39],
        pinned: [60, 56, 45],
        is_dark: true,
    };

    /// Tokyo Night's standard Night background and blue accent.
    pub const TOKYO_NIGHT: Self = Self {
        background: [26, 27, 38],
        surface: [36, 40, 59],
        text: [192, 202, 245],
        muted: [169, 177, 214],
        border: [86, 95, 137],
        grid: [65, 72, 104],
        accent: [122, 162, 247],
        selected: [40, 52, 87],
        hover: [35, 39, 58],
        active: [51, 65, 111],
        error: [247, 118, 142],
        weekend: [30, 32, 47],
        pinned: [35, 43, 64],
        is_dark: true,
    };

    /// System chooses the existing warm dark/light pair. An optional native
    /// accent is supplied by the adapter only when the user enables it.
    pub fn resolve(
        theme: Theme,
        custom: Option<CustomTheme>,
        system_dark: bool,
        system_accent: Option<[u8; 3]>,
    ) -> Self {
        let mut colors = match theme {
            Theme::Ember => Self::EMBER,
            Theme::Paper => Self::PAPER,
            Theme::System if system_dark => Self::EMBER,
            Theme::System => Self::PAPER,
            Theme::Nord => Self::NORD,
            Theme::Dracula => Self::DRACULA,
            Theme::Graphite => Self::GRAPHITE,
            Theme::Catppuccin => Self::CATPPUCCIN,
            Theme::Gruvbox => Self::GRUVBOX,
            Theme::TokyoNight => Self::TOKYO_NIGHT,
            Theme::Custom => custom.map_or(Self::EMBER, Self::custom),
        };
        if let Some(accent) = system_accent {
            colors.accent = accent;
            colors.selected = mix(colors.background, accent, 0.18);
            colors.hover = mix(colors.background, accent, 0.10);
            colors.active = mix(colors.background, accent, 0.28);
            colors.pinned = mix(colors.background, accent, 0.08);
        }
        colors
    }

    fn custom(colors: CustomTheme) -> Self {
        let is_dark = colors.is_dark();
        Self {
            background: colors.background,
            surface: colors.surface,
            text: colors.text,
            muted: readable(mix(colors.background, colors.text, 0.70), colors.surface),
            border: mix(colors.background, colors.text, 0.43),
            grid: mix(colors.background, colors.text, 0.22),
            accent: colors.accent,
            selected: mix(colors.background, colors.accent, 0.18),
            hover: mix(colors.background, colors.text, 0.08),
            active: mix(colors.background, colors.accent, 0.28),
            error: readable(
                if is_dark {
                    [237, 142, 132]
                } else {
                    [167, 47, 39]
                },
                colors.surface,
            ),
            weekend: mix(colors.background, colors.text, 0.025),
            pinned: mix(colors.background, colors.accent, 0.08),
            is_dark,
        }
    }
}

fn mix(background: [u8; 3], foreground: [u8; 3], amount: f64) -> [u8; 3] {
    std::array::from_fn(|index| {
        (f64::from(background[index]) * (1.0 - amount) + f64::from(foreground[index]) * amount)
            .round() as u8
    })
}

/// Relative sRGB luminance for theme polarity and readable derived labels.
pub fn luminance(color: [u8; 3]) -> f64 {
    let linear = color.map(|channel| {
        let channel = f64::from(channel) / 255.0;
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    });
    0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2]
}

pub fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let a = luminance(a);
    let b = luminance(b);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn readable(color: [u8; 3], background: [u8; 3]) -> [u8; 3] {
    if contrast(color, background) >= 4.5 {
        return color;
    }
    let target = if contrast([255; 3], background) >= contrast([0; 3], background) {
        [255; 3]
    } else {
        [0; 3]
    };
    for step in 1..=20 {
        let adjusted = mix(color, target, f64::from(step) / 20.0);
        if contrast(adjusted, background) >= 4.5 {
            return adjusted;
        }
    }
    target
}

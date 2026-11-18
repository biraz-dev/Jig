use jig::{
    app_settings::{AppSettings, CustomTheme, Theme},
    palette::{self, contrast, ThemePalette},
};

#[test]
fn version_one_names_keep_their_existing_appearance() {
    let legacy = AppSettings::from_json(
        r#"{"version":1,"theme":"graphite","undo_levels":7,"auto_backup":true,"shortcuts":{"calculator":null}}"#,
    )
    .unwrap();
    assert_eq!(legacy.version, 2);
    assert_eq!(legacy.theme, Theme::Ember);
    assert_eq!(legacy.undo_levels, 7);
    assert!(legacy.auto_backup);
    assert_eq!(legacy.binding(jig::app_settings::Action::Calculator), None);
    let ember = ThemePalette::resolve(legacy.theme, None, false, None);
    assert_eq!(ember, ThemePalette::EMBER);
    assert_eq!(ember.background, palette::BACKGROUND);
    assert_eq!(ember.surface, palette::SURFACE);
    assert_eq!(ember.text, palette::TEXT);
    assert_eq!(ember.accent, palette::ACCENT);
    assert_eq!(ember.selected, palette::SELECTED);

    let light = AppSettings::from_json(r#"{"theme":"light"}"#).unwrap();
    assert_eq!(light.theme, Theme::Paper);
    let paper = ThemePalette::resolve(light.theme, None, true, None);
    assert_eq!(paper.background, [246, 242, 236]);
    assert_eq!(paper.text, [49, 43, 39]);
    assert_eq!(paper.accent, [175, 64, 38]);

    // The same word can now mean neutral Graphite in a version-two file.
    let neutral = AppSettings::from_json(r#"{"version":2,"theme":"graphite"}"#).unwrap();
    assert_eq!(neutral.theme, Theme::Graphite);
    assert_ne!(
        ThemePalette::resolve(neutral.theme, None, true, None),
        ember
    );

    // Adding Rosé Pine must retain existing Nord files and its palette anchors.
    let nord = AppSettings::from_json(r#"{"theme":"nord"}"#).unwrap();
    assert_eq!(nord.theme, Theme::Nord);
    assert_eq!(nord.theme.label(), "Nord");
    assert!(Theme::ALL.contains(&Theme::Nord));
    assert_eq!(serde_json::to_value(&nord).unwrap()["theme"], "nord");
    let colors = ThemePalette::resolve(nord.theme, None, true, None);
    assert_eq!(colors, ThemePalette::NORD);
    assert_eq!(colors.background, [46, 52, 64]);
    assert_eq!(colors.text, [236, 239, 244]);
    assert_eq!(colors.accent, [136, 192, 208]);
}

#[test]
fn presets_are_distinct_and_keep_small_text_readable() {
    let mut backgrounds = std::collections::BTreeSet::new();
    for theme in [
        Theme::Ember,
        Theme::Paper,
        Theme::Nord,
        Theme::Dracula,
        Theme::Graphite,
        Theme::Catppuccin,
        Theme::Gruvbox,
        Theme::TokyoNight,
        Theme::Dark,
        Theme::RosePine,
    ] {
        let colors = ThemePalette::resolve(theme, None, true, None);
        assert!(backgrounds.insert(colors.background), "{theme:?}");
        for background in [
            colors.background,
            colors.surface,
            colors.selected,
            colors.hover,
            colors.active,
            colors.pinned,
        ] {
            assert!(
                contrast(colors.text, background) >= 4.5,
                "{theme:?}: text over {background:?}"
            );
        }
        for background in [colors.background, colors.surface] {
            assert!(
                contrast(colors.muted, background) >= 4.5,
                "{theme:?}: muted text over {background:?}"
            );
        }
        assert!(contrast(colors.accent, colors.background) >= 3.0);
        assert_ne!(colors.selected, colors.background);
        assert_ne!(colors.active, colors.hover);
        assert_eq!(colors.is_dark, theme != Theme::Paper);
    }
}

#[test]
fn added_presets_keep_canonical_names_and_standard_anchor_colors() {
    for (theme, name, label, background, text, accent) in [
        (
            Theme::Catppuccin,
            "catppuccin",
            "Catppuccin",
            [30, 30, 46],
            [205, 214, 244],
            [203, 166, 247],
        ),
        (
            Theme::Gruvbox,
            "gruvbox",
            "Gruvbox",
            [40, 40, 40],
            [235, 219, 178],
            [250, 189, 47],
        ),
        (
            Theme::TokyoNight,
            "tokyo_night",
            "Tokyo Night",
            [26, 27, 38],
            [192, 202, 245],
            [122, 162, 247],
        ),
        (
            Theme::RosePine,
            "rose_pine",
            "Rosé Pine",
            [25, 23, 36],
            [224, 222, 244],
            [235, 188, 186],
        ),
    ] {
        let json = format!(r#"{{"version":2,"theme":"{name}"}}"#);
        let settings = AppSettings::from_json(&json).unwrap();
        assert_eq!(settings.theme, theme);
        assert_eq!(theme.label(), label);
        assert_eq!(
            Theme::ALL.iter().filter(|choice| **choice == theme).count(),
            1
        );
        assert_eq!(serde_json::to_value(&settings).unwrap()["theme"], name);
        let colors = ThemePalette::resolve(theme, None, true, None);
        assert_eq!(colors.background, background);
        assert_eq!(colors.text, text);
        assert_eq!(colors.accent, accent);
        for background in [colors.background, colors.surface] {
            assert!(contrast(colors.error, background) >= 4.5, "{theme:?}");
        }
        assert_eq!(
            ThemePalette::resolve(theme, None, false, None),
            colors,
            "Explicit dark presets must not follow system light mode"
        );
    }
}

#[test]
fn dark_is_neutral_and_retains_its_appearance_in_system_light_mode() {
    let settings = AppSettings::from_json(r#"{"version":2,"theme":"dark"}"#).unwrap();
    assert_eq!(settings.theme, Theme::Dark);
    assert_eq!(settings.theme.label(), "Dark");
    assert_eq!(serde_json::to_value(&settings).unwrap()["theme"], "dark");
    assert_eq!(
        Theme::ALL
            .iter()
            .filter(|choice| **choice == Theme::Dark)
            .count(),
        1
    );
    assert_eq!(AppSettings::default().theme, Theme::Ember);

    let dark = ThemePalette::resolve(Theme::Dark, None, true, None);
    assert_eq!(dark.background, [16; 3]);
    assert_eq!(dark.surface, [25; 3]);
    assert_eq!(dark.text, [216; 3]);
    assert_eq!(dark.accent, [192; 3]);
    assert!(
        palette::luminance(dark.background) < palette::luminance(ThemePalette::GRAPHITE.background)
    );
    for color in [
        dark.background,
        dark.surface,
        dark.text,
        dark.muted,
        dark.border,
        dark.grid,
        dark.accent,
        dark.selected,
        dark.hover,
        dark.active,
        dark.weekend,
        dark.pinned,
    ] {
        assert_eq!(color[0], color[1]);
        assert_eq!(color[1], color[2]);
    }
    for background in [dark.background, dark.surface] {
        assert!(contrast(dark.error, background) >= 4.5);
    }
    assert!(dark.error[0] > dark.error[1]);
    assert!(dark.is_dark);
    assert_eq!(ThemePalette::resolve(Theme::Dark, None, false, None), dark);
}

#[test]
fn system_mode_and_native_accent_use_predictable_fallbacks() {
    assert_eq!(
        ThemePalette::resolve(Theme::System, None, true, None),
        ThemePalette::EMBER
    );
    assert_eq!(
        ThemePalette::resolve(Theme::System, None, false, None),
        ThemePalette::PAPER
    );
    assert_eq!(
        ThemePalette::resolve(Theme::Custom, None, true, None),
        ThemePalette::EMBER
    );
    let original = ThemePalette::NORD;
    let accent = [230, 162, 93];
    let custom_accent = ThemePalette::resolve(Theme::Nord, None, true, Some(accent));
    assert_eq!(custom_accent.accent, accent);
    assert_eq!(custom_accent.background, original.background);
    assert_eq!(custom_accent.text, original.text);
    assert_eq!(custom_accent.grid, original.grid);
    assert_ne!(custom_accent.selected, original.selected);
    assert_ne!(custom_accent.active, original.active);
}

#[test]
fn custom_colors_preserve_anchors_and_derive_readable_roles() {
    for custom in [
        CustomTheme {
            background: [16, 23, 28],
            surface: [28, 36, 42],
            text: [232, 241, 237],
            accent: [125, 207, 168],
        },
        CustomTheme {
            background: [252, 249, 242],
            surface: [235, 232, 220],
            text: [42, 44, 37],
            accent: [131, 57, 23],
        },
        CustomTheme {
            background: [150; 3],
            surface: [145; 3],
            text: [10; 3],
            accent: [30; 3],
        },
    ] {
        let resolved = ThemePalette::resolve(Theme::Custom, Some(custom), false, None);
        assert_eq!(resolved.background, custom.background);
        assert_eq!(resolved.surface, custom.surface);
        assert_eq!(resolved.text, custom.text);
        assert_eq!(resolved.accent, custom.accent);
        assert_eq!(resolved.is_dark, custom.is_dark());
        assert!(contrast(resolved.muted, resolved.surface) >= 4.5);
        assert!(contrast(resolved.error, resolved.surface) >= 4.5);
        assert_ne!(resolved.selected, resolved.background);
        assert_ne!(resolved.border, resolved.grid);
    }
}

#[test]
fn theme_preferences_round_trip_and_reject_malformed_color_data() {
    for theme in Theme::ALL {
        let settings = AppSettings {
            theme,
            custom_theme: Some(CustomTheme {
                background: [19, 25, 31],
                surface: [31, 39, 45],
                text: [226, 233, 238],
                accent: [216, 159, 108],
            }),
            system_accent: true,
            undo_levels: 33,
            auto_backup: true,
            ..AppSettings::default()
        };
        assert_eq!(
            AppSettings::from_json(&settings.to_json().unwrap()).unwrap(),
            settings
        );
    }
    for text in [
        r#"[]"#,
        r#"[2]"#,
        r#"{"version":2,"custom_theme":{}}"#,
        r#"{"version":2,"custom_theme":{"background":[0,0],"surface":[0,0,0],"text":[255,255,255],"accent":[1,2,3]}}"#,
        r#"{"version":2,"custom_theme":{"background":[256,0,0],"surface":[0,0,0],"text":[255,255,255],"accent":[1,2,3]}}"#,
        r#"{"version":2,"custom_theme":{"background":[0,0,0],"surface":[0,0,0],"text":[255,255,255],"accent":[1,2,3],"extra":true}}"#,
        r#"{"version":2,"system_accent":"yes"}"#,
        r#"{"version":99,"theme":"nord"}"#,
        r#"{"version":null,"theme":"nord"}"#,
        r#"{"version":1,"version":2,"theme":"graphite"}"#,
        r#"{"version":2,"theme":"nord","theme":"paper"}"#,
    ] {
        assert!(AppSettings::from_json(text).is_err(), "{text}");
    }
}

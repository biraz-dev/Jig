use jig::app_settings::{Action, AppSettings, Chord, ShortcutKey, Theme, MAX_SETTINGS_BYTES};

#[test]
fn defaults_and_older_partial_settings_restore_expected_bindings() {
    let defaults = AppSettings::default();
    defaults.validate().unwrap();
    assert_eq!(AppSettings::from_json("{}").unwrap(), defaults);
    assert_eq!(defaults.theme, Theme::Graphite);
    assert_eq!(defaults.undo_levels, 20);
    assert!(!defaults.auto_backup);
    assert_eq!(
        defaults.binding(Action::Settings),
        Some(Chord::ctrl(ShortcutKey::Comma))
    );
    assert_eq!(
        defaults.binding(Action::Redo),
        Some(Chord::ctrl_shift(ShortcutKey::Z))
    );
    let partial =
        AppSettings::from_json(r#"{"theme":"light","shortcuts":{"calculator":null}}"#).unwrap();
    assert_eq!(partial.theme, Theme::Light);
    assert_eq!(partial.binding(Action::Calculator), None);
    assert_eq!(
        partial.binding(Action::Tasks),
        defaults.binding(Action::Tasks)
    );
}

#[test]
fn custom_settings_round_trip_and_reset_without_changing_other_preferences() {
    let mut settings = AppSettings {
        theme: Theme::System,
        undo_levels: 100,
        auto_backup: true,
        ..AppSettings::default()
    };
    let chord = Chord {
        shift: true,
        ..Chord::ctrl_alt(ShortcutKey::B)
    };
    settings
        .set_binding(Action::Calculator, Some(chord))
        .unwrap();
    settings.set_binding(Action::OpenLocal, None).unwrap();
    assert_eq!(
        AppSettings::from_json(&settings.to_json().unwrap()).unwrap(),
        settings
    );
    settings.reset_binding(Action::Calculator).unwrap();
    assert_eq!(
        settings.binding(Action::Calculator),
        Some(Action::Calculator.default_binding())
    );
    assert!(!settings.shortcuts.contains_key(&Action::Calculator));
    assert_eq!(settings.binding(Action::OpenLocal), None);
    settings.reset_shortcuts();
    assert_eq!(
        settings.binding(Action::OpenLocal),
        Some(Action::OpenLocal.default_binding())
    );
    assert_eq!(
        (settings.theme, settings.undo_levels, settings.auto_backup),
        (Theme::System, 100, true)
    );
}

#[test]
fn conflicts_include_default_bindings_and_rejected_changes_are_atomic() {
    let mut settings = AppSettings::default();
    let before = settings.clone();
    let error = settings
        .set_binding(Action::Calculator, settings.binding(Action::Tasks))
        .unwrap_err();
    assert!(error.contains("already assigned"));
    assert_eq!(settings, before);

    // A disabled action frees its shortcut, but resetting it must not silently
    // steal that shortcut back from the newly assigned action.
    settings.set_binding(Action::Data, None).unwrap();
    settings
        .set_binding(Action::Tasks, Some(Action::Data.default_binding()))
        .unwrap();
    let before = settings.clone();
    assert!(settings.reset_binding(Action::Data).is_err());
    assert_eq!(settings, before);
    let imported = settings.to_json().unwrap().replace(
        r#""data": null"#,
        r#""data": {"key":"D","ctrl":true,"alt":true}"#,
    );
    assert!(AppSettings::from_json(&imported)
        .unwrap_err()
        .contains("already assigned"));
    settings.reset_shortcuts();
    settings.validate().unwrap();
}

#[test]
fn standard_editor_and_browser_shortcuts_cannot_be_repurposed() {
    let mut settings = AppSettings::default();
    for key in [
        ShortcutKey::A,
        ShortcutKey::C,
        ShortcutKey::V,
        ShortcutKey::X,
        ShortcutKey::O,
        ShortcutKey::S,
        ShortcutKey::P,
        ShortcutKey::F,
        ShortcutKey::W,
        ShortcutKey::T,
        ShortcutKey::N,
        ShortcutKey::L,
        ShortcutKey::R,
        ShortcutKey::Minus,
        ShortcutKey::Equals,
    ] {
        for shift in [false, true] {
            for meta in [false, true] {
                let chord = Chord {
                    key,
                    ctrl: !meta,
                    meta,
                    shift,
                    alt: false,
                };
                assert!(
                    settings
                        .set_binding(Action::Calculator, Some(chord))
                        .is_err(),
                    "{}",
                    chord.label()
                );
            }
        }
    }
    settings.set_binding(Action::Undo, None).unwrap();
    assert!(settings
        .set_binding(Action::Tasks, Some(Chord::ctrl(ShortcutKey::Z)))
        .is_err());
    settings.set_binding(Action::Redo, None).unwrap();
    assert!(settings
        .set_binding(Action::Tasks, Some(Chord::ctrl_shift(ShortcutKey::Z)))
        .is_err());
    settings.reset_shortcuts();
    settings
        .set_binding(
            Action::Undo,
            Some(Chord {
                meta: true,
                ..Chord::new(ShortcutKey::Z)
            }),
        )
        .unwrap();
    settings
        .set_binding(
            Action::Redo,
            Some(Chord {
                meta: true,
                shift: true,
                ..Chord::new(ShortcutKey::Z)
            }),
        )
        .unwrap();
    // Ctrl-Alt defaults remain usable despite plain Ctrl browser reservations.
    settings
        .set_binding(Action::Calculator, Some(Chord::ctrl_alt(ShortcutKey::G)))
        .unwrap();
}

#[test]
fn ordinary_typing_and_fixed_context_keys_cannot_trigger_global_actions() {
    let mut settings = AppSettings::default();
    for chord in [
        Chord::new(ShortcutKey::A),
        Chord {
            shift: true,
            ..Chord::new(ShortcutKey::A)
        },
        Chord::new(ShortcutKey::Comma),
        Chord::new(ShortcutKey::F2),
        Chord::new(ShortcutKey::F5),
        Chord::new(ShortcutKey::F12),
        Chord {
            alt: true,
            ..Chord::new(ShortcutKey::F4)
        },
    ] {
        assert!(
            settings
                .set_binding(Action::Calculator, Some(chord))
                .is_err(),
            "{}",
            chord.label()
        );
    }
    settings
        .set_binding(Action::Calculator, Some(Chord::new(ShortcutKey::F8)))
        .unwrap();
    assert!(
        AppSettings::from_json(r#"{"shortcuts":{"settings":{"ctrl":true}}}"#).is_err(),
        "A modifier-only binding must have an explicit key"
    );
}

#[test]
fn malformed_or_out_of_range_imports_are_rejected() {
    for text in [
        r#"{"version":2}"#,
        r#"{"undo_levels":101}"#,
        r#"{"undo_levels":-1}"#,
        r#"{"undo_levels":1.5}"#,
        r#"{"theme":"unknown"}"#,
        r#"{"unexpected":true}"#,
        r#"{"shortcuts":{"unknown":null}}"#,
        r#"{"shortcuts":{"tasks":{"key":"Escape","ctrl":true}}}"#,
        r#"{"shortcuts":{"tasks":{"key":"A","control":true}}}"#,
    ] {
        assert!(AppSettings::from_json(text).is_err(), "{text}");
    }
    for levels in [0, 100] {
        AppSettings::from_json(&format!(r#"{{"undo_levels":{levels}}}"#)).unwrap();
    }
    let large = " ".repeat(MAX_SETTINGS_BYTES) + "{}";
    assert!(AppSettings::from_json(&large)
        .unwrap_err()
        .contains("32 KiB"));
}

#[test]
fn canonical_key_names_and_labels_are_portable() {
    let mut names = std::collections::BTreeSet::new();
    for key in ShortcutKey::ALL {
        assert!(names.insert(key.name()));
        assert_eq!(ShortcutKey::from_name(key.name()), Some(key));
        assert_eq!(
            serde_json::from_str::<String>(&serde_json::to_string(&key).unwrap()).unwrap(),
            key.name()
        );
    }
    assert_eq!(ShortcutKey::from_name("Shift"), None);
    assert_eq!(Chord::ctrl(ShortcutKey::Comma).label(), "Ctrl+,");
    assert_eq!(Chord::ctrl_shift(ShortcutKey::Z).label(), "Ctrl+Shift+Z");
    assert_eq!(
        Chord {
            ctrl: true,
            alt: true,
            shift: true,
            meta: true,
            key: ShortcutKey::F8
        }
        .label(),
        "Ctrl+Alt+Shift+Meta+F8"
    );
}

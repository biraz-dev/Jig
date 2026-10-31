use jig::{
    clay::{builtin_profiles, normalize_cone, Basis, Settings, STANDARD_104_SOURCE},
    model::State,
};

#[test]
fn standard_104_keeps_exact_published_cones_and_unspecified_starting_stage() {
    let profiles = builtin_profiles();
    assert_eq!(profiles.len(), 2);
    for profile in &profiles {
        profile.validate().unwrap();
        assert_eq!(profile.name, "Standard #104 Red with Grog");
        assert_eq!(profile.basis, Basis::Unspecified);
        assert_eq!(profile.source.as_deref(), Some(STANDARD_104_SOURCE));
    }
    assert_eq!(profiles[0].cone, "06");
    assert_eq!(profiles[0].linear_shrinkage, 6.0);
    assert_eq!(profiles[1].cone, "4");
    assert_eq!(profiles[1].linear_shrinkage, 11.0);
    assert!(!profiles.iter().any(|profile| profile.cone == "04"));
    assert!(!profiles.iter().any(|profile| profile.cone == "6"));
    assert_eq!(normalize_cone(" Cone 06 ").unwrap(), "06");
    assert_eq!(normalize_cone("cone 6").unwrap(), "6");
    assert_ne!(normalize_cone("06").unwrap(), normalize_cone("6").unwrap());
    for cone in ["022", "010", "01", "1", "14"] {
        assert_eq!(normalize_cone(cone).unwrap(), cone);
    }
    for bad in ["", "0", "00", "006", "023", "15", "6.0", "04 to 6"] {
        assert!(normalize_cone(bad).is_err(), "{bad}");
    }
}

#[test]
fn custom_test_profiles_are_portable_and_do_not_overwrite_published_data() {
    let mut settings = Settings::default();
    assert_eq!(settings.selected_profile().unwrap(), builtin_profiles()[0]);
    let wet = settings
        .add_custom("My #104 test bar", "cone 04", 7.25, Basis::WetToFired)
        .unwrap();
    let dry = settings
        .add_custom("My #104 dry test", "04", 2.5, Basis::DryToFired)
        .unwrap();
    assert_ne!(wet, dry);
    assert_eq!(settings.available_profiles().len(), 4);
    assert_eq!(builtin_profiles()[0].linear_shrinkage, 6.0);
    assert_eq!(settings.selected_profile().unwrap().id, dry);
    settings.validate().unwrap();

    let mut state = State::default();
    state.preferences.clay = Some(settings.clone());
    let file = serde_json::to_string_pretty(&state).unwrap();
    assert!(!file.contains("triangles"));
    let restored = State::from_json(&file).unwrap();
    assert_eq!(restored.preferences.clay, Some(settings.clone()));
    let chosen = restored
        .preferences
        .clay
        .unwrap()
        .selected_profile()
        .unwrap();
    assert_eq!(chosen.cone, "04");
    assert_eq!(chosen.basis, Basis::DryToFired);
    assert_eq!(chosen.linear_shrinkage, 2.5);
    assert_eq!(chosen.source, None);

    settings.remove_custom(&wet).unwrap();
    assert_eq!(settings.selected, dry);
    settings.remove_custom(&dry).unwrap();
    assert_eq!(settings.selected, Settings::default().selected);
    assert!(settings.remove_custom("standard-104-06").is_err());
    settings.validate().unwrap();
}

#[test]
fn old_state_files_need_no_clay_migration_and_invalid_profiles_are_rejected() {
    let state = State::default();
    let old = serde_json::to_string(&state).unwrap();
    assert!(!old.contains("\"clay\""));
    assert!(State::from_json(&old).unwrap().preferences.clay.is_none());
    let mut settings = Settings::default();
    let before = settings.clone();
    for percentage in [-1.0, 100.0, f64::NAN, f64::INFINITY] {
        assert!(settings
            .add_custom("Test", "04", percentage, Basis::WetToFired)
            .is_err());
        assert_eq!(settings, before);
    }
    assert!(settings
        .add_custom("\n", "04", 10.0, Basis::Unspecified)
        .is_err());
    assert!(settings
        .add_custom("Test", "not a cone", 10.0, Basis::Unspecified)
        .is_err());
    assert_eq!(settings, before);
    settings
        .add_custom("Test", "04", 10.0, Basis::Unspecified)
        .unwrap();
    let valid = settings.clone();
    settings.custom_profiles[0].id = "standard-104-06".into();
    assert!(settings.validate().is_err());
    settings = valid.clone();
    settings.selected = "missing".into();
    assert!(settings.validate().is_err());
    settings = valid.clone();
    settings.custom_profiles[0].source = Some("javascript:alert(1)".into());
    assert!(settings.validate().is_err());
    let mut invalid_basis = State::default();
    invalid_basis.preferences.clay = Some(valid.clone());
    let mut invalid_basis = serde_json::to_value(invalid_basis).unwrap();
    invalid_basis["preferences"]["clay"]["custom_profiles"][0]["basis"] =
        "bone_dry_to_bisque".into();
    assert!(State::from_json(&invalid_basis.to_string()).is_err());
    settings = valid;
    let mut invalid = State::default();
    settings.custom_profiles[0].linear_shrinkage = 100.0;
    invalid.preferences.clay = Some(settings);
    assert!(invalid.validate().is_err());
    assert!(State::from_json(&serde_json::to_string(&invalid).unwrap()).is_err());
}

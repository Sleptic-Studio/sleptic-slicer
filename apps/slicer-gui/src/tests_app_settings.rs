// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Compatibility checks for persisted application preferences.
#[allow(unused_imports)]
use crate::panel_place::Panel;
use super::*;

fn populated() -> AppSettings {
    let mut settings = AppSettings {
        orient_preset: -19,
        perspective_view: true,
        setup_complete: true,
        ..AppSettings::default()
    };
    settings.set_theme(ThemePreference::Dark);
    settings.set_language(LanguagePreference::from_combo_index(1));
    settings.set_menu_layout(MenuLayout::Compact);
    settings.set_currency_symbol(" EUR ".to_owned());
    settings.set_default_printer(Some(PrinterId::new("test-printer").unwrap()));
    settings.set_default_resin(Some(MaterialId::new("test-resin").unwrap()));
    settings.set_favourite_printers([PrinterId::new("test-printer").unwrap()].into());
    settings.set_favourite_resins([MaterialId::new("test-resin").unwrap()].into());
    settings.set_aa_level(AaLevel::X4);
    { let mut place = settings.panel(Panel::ObjectList); place.visible = false; settings.set_panel(Panel::ObjectList, place); }
    { let mut place = settings.panel(Panel::ObjectList); place.anchor_right = false; settings.set_panel(Panel::ObjectList, place); }
    { let mut place = settings.panel(Panel::ObjectList); place.anchor_bottom = true; settings.set_panel(Panel::ObjectList, place); }
    { let mut place = settings.panel(Panel::ObjectList); place.offset_x = Some(-7.25); settings.set_panel(Panel::ObjectList, place); }
    { let mut place = settings.panel(Panel::ObjectList); place.offset_y = Some(-7.25); settings.set_panel(Panel::ObjectList, place); }
    { let mut place = settings.panel(Panel::ObjectList); place.width = Some(-7.25); settings.set_panel(Panel::ObjectList, place); }
    { let mut place = settings.panel(Panel::ObjectList); place.height = Some(-7.25); settings.set_panel(Panel::ObjectList, place); }
    { let mut place = settings.panel(Panel::ObjectList); place.minimized = true; settings.set_panel(Panel::ObjectList, place); }
    { let mut place = settings.panel(Panel::SliceControls); place.anchor_right = true; settings.set_panel(Panel::SliceControls, place); }
    { let mut place = settings.panel(Panel::SliceControls); place.anchor_bottom = true; settings.set_panel(Panel::SliceControls, place); }
    { let mut place = settings.panel(Panel::SliceControls); place.offset_x = Some(-7.25); settings.set_panel(Panel::SliceControls, place); }
    { let mut place = settings.panel(Panel::SliceControls); place.offset_y = Some(-7.25); settings.set_panel(Panel::SliceControls, place); }
    { let mut place = settings.panel(Panel::SliceControls); place.width = Some(-7.25); settings.set_panel(Panel::SliceControls, place); }
    { let mut place = settings.panel(Panel::SliceControls); place.height = Some(-7.25); settings.set_panel(Panel::SliceControls, place); }
    { let mut place = settings.panel(Panel::SliceView); place.visible = false; settings.set_panel(Panel::SliceView, place); }
    { let mut place = settings.panel(Panel::SliceView); place.anchor_right = true; settings.set_panel(Panel::SliceView, place); }
    { let mut place = settings.panel(Panel::SliceView); place.anchor_bottom = false; settings.set_panel(Panel::SliceView, place); }
    { let mut place = settings.panel(Panel::SliceView); place.offset_x = Some(-7.25); settings.set_panel(Panel::SliceView, place); }
    { let mut place = settings.panel(Panel::SliceView); place.offset_y = Some(-7.25); settings.set_panel(Panel::SliceView, place); }
    { let mut place = settings.panel(Panel::SliceView); place.width = Some(-7.25); settings.set_panel(Panel::SliceView, place); }
    { let mut place = settings.panel(Panel::SliceView); place.height = Some(-7.25); settings.set_panel(Panel::SliceView, place); }
    { let mut place = settings.panel(Panel::SliceView); place.minimized = true; settings.set_panel(Panel::SliceView, place); }
    settings.set_last_open_dir(Some(PathBuf::from("models")));
    settings.set_recent_files(vec![
        PathBuf::from("b.stl"),
        PathBuf::from("a.stl"),
        PathBuf::from("b.stl"),
    ]);
    settings
}

fn digest(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

#[test]
fn serialized_defaults_and_all_fields_match_the_recorded_bytes() {
    for (settings, expected) in [
        (AppSettings::default(), 13629519830077301404),
        (populated(), 14807094621117730921),
    ] {
        let text = toml::to_string_pretty(&settings).unwrap();
        assert_eq!(digest(text.as_bytes()), expected);
        let restored: AppSettings = toml::from_str(&text).unwrap();
        assert_eq!(toml::to_string_pretty(&restored).unwrap(), text);
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("nested/prefs.toml");
        settings.save_to_path(&file).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), text);
        assert_eq!(
            toml::to_string_pretty(&AppSettings::load_from_path(&file)).unwrap(),
            text
        );
        settings.save_to_path(&file).unwrap();
        assert_eq!(fs::read_dir(file.parent().unwrap()).unwrap().count(), 1);
    }
}

#[test]
fn invalid_documents_reset_but_absent_files_are_not_created() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("prefs.toml");
    let defaults = toml::to_string_pretty(&AppSettings::default()).unwrap();
    assert_eq!(
        toml::to_string_pretty(&AppSettings::load_from_path(&file)).unwrap(),
        defaults
    );
    assert!(!file.exists());
    for input in [
        "[broken",
        "version = 0",
        "version = 999",
        "default_printer = 'INVALID ID'",
    ] {
        fs::write(&file, input).unwrap();
        assert_eq!(
            toml::to_string_pretty(&AppSettings::load_from_path(&file)).unwrap(),
            defaults
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), defaults);
    }
    assert_eq!(
        toml::to_string_pretty(&AppSettings::load_from_path(directory.path())).unwrap(),
        defaults
    );
    assert!(directory.path().is_dir());
}

#[test]
fn legacy_theme_adjustment_is_memory_only_and_depends_on_setup() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("prefs.toml");
    for complete in [false, true] {
        let input = format!("setup_complete = {complete}\n[appearance]\ntheme = 'system'\n");
        fs::write(&file, &input).unwrap();
        let settings = AppSettings::load_from_path(&file);
        assert_eq!(
            settings.theme(),
            if complete {
                ThemePreference::System
            } else {
                ThemePreference::Light
            }
        );
        assert_eq!(settings.version, 1);
        assert_eq!(fs::read_to_string(&file).unwrap(), input);
    }
}

#[test]
fn currency_normalization_occurs_only_in_the_setter() {
    let mut settings: AppSettings = toml::from_str("[appearance]\ncurrency_symbol = ''").unwrap();
    assert_eq!(settings.currency_symbol(), "");
    settings.set_currency_symbol(String::new());
    assert_eq!(settings.currency_symbol(), "$");
    settings.set_currency_symbol("  ".into());
    assert_eq!(settings.currency_symbol(), "  ");
}

#[test]
fn every_accessor_reports_the_value_assigned_without_clamping() {
    let settings = populated();
    assert_eq!(settings.theme(), ThemePreference::Dark);
    assert_eq!(settings.language(), settings.appearance.language);
    assert_eq!(settings.menu_layout(), MenuLayout::Compact);
    assert_eq!(
        settings.currency_symbol(),
        settings.appearance.currency_symbol
    );
    assert_eq!(settings.default_printer(), settings.default_printer);
    assert_eq!(settings.default_resin(), settings.default_resin);
    assert_eq!(settings.favourite_printers(), settings.favourite_printers);
    assert_eq!(settings.favourite_resins(), settings.favourite_resins);
    assert_eq!(settings.aa_level(), settings.aa_level);
    // Each panel's place survives being stored in a fresh settings value.
    for panel in [Panel::ObjectList, Panel::SliceControls, Panel::SliceView] {
        let mut copy = AppSettings::default();
        copy.set_panel(panel, settings.panel(panel));
        assert_eq!(copy.panel(panel), settings.panel(panel), "{panel:?}");
    }
    assert_eq!(settings.last_open_dir(), settings.last_open_dir);
    assert_eq!(settings.recent_files(), settings.recent_files);
}
#[test]
fn enum_encodings_and_partial_sections_keep_their_defaults() {
    for theme in [
        ThemePreference::System,
        ThemePreference::Light,
        ThemePreference::Dark,
    ] {
        for menu in [MenuLayout::Bar, MenuLayout::Compact] {
            for aa in [AaLevel::Off, AaLevel::X2, AaLevel::X4, AaLevel::X8] {
                let mut settings = AppSettings {
                    setup_complete: true,
                    ..AppSettings::default()
                };
                settings.set_theme(theme);
                settings.set_menu_layout(menu);
                settings.set_aa_level(aa);
                let encoded = toml::to_string_pretty(&settings).unwrap();
                let decoded: AppSettings = toml::from_str(&encoded).unwrap();
                assert_eq!(
                    (decoded.theme(), decoded.menu_layout(), decoded.aa_level()),
                    (theme, menu, aa)
                );
            }
        }
    }
    let sparse: AppSettings = toml::from_str(
        "[object_list]\nwidth=99\n[slice_view]\nwidth=101\n[slice_panel]\nwidth=103\n",
    )
    .unwrap();
    assert!(sparse.panel(Panel::ObjectList).visible && sparse.panel(Panel::ObjectList).anchor_right);
    assert!(!sparse.panel(Panel::ObjectList).anchor_bottom && !sparse.panel(Panel::ObjectList).minimized);
    assert!(sparse.panel(Panel::SliceView).visible && sparse.panel(Panel::SliceView).anchor_bottom);
    assert!(!sparse.panel(Panel::SliceView).anchor_right && !sparse.panel(Panel::SliceView).minimized);
    assert!(!sparse.panel(Panel::SliceControls).anchor_right && !sparse.panel(Panel::SliceControls).anchor_bottom);
    assert_eq!(
        (
            sparse.panel(Panel::ObjectList).width,
            sparse.panel(Panel::SliceView).width,
            sparse.panel(Panel::SliceControls).width
        ),
        (Some(99.), Some(101.), Some(103.))
    );
}
#[test]
fn replacement_failure_cleans_up_staging_and_preserves_target() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("occupied");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("keep"), b"preserved").unwrap();
    assert!(replace_file(&target, b"replacement").is_err());
    assert_eq!(fs::read(target.join("keep")).unwrap(), b"preserved");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    assert_eq!(
        staging_file(Path::new("/")).unwrap_err().to_string(),
        "settings path has no file name"
    );
}

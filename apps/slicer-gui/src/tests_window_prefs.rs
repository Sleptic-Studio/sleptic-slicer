// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! What each window takes from a preferences snapshot, on real windows.
use std::path::PathBuf;

use sc_print_primitives::AaLevel;
use slint::{Model, ModelRc, SharedString};

use crate::language::LanguagePreference;
use crate::tests_ui_audit::preferences;
use crate::window_prefs::TakesPreferences;
use crate::{AboutWindow, MainWindow, MenuLayout, PreferencesWindow, ShortcutsDialog, ThemePreference};

fn rows(model: &ModelRc<SharedString>) -> Vec<String> {
    model.iter().map(|s| s.to_string()).collect()
}

#[test]
fn every_window_takes_its_preferences() {
    i_slint_backend_testing::init_no_event_loop();
    let mut prefs = preferences();
    prefs.theme = ThemePreference::Dark;
    prefs.menu_layout = MenuLayout::Compact;
    prefs.recent_files = vec![PathBuf::from("a.stl"), PathBuf::from("b.3mf")];
    prefs.aa_level = AaLevel::X4;
    prefs.currency_symbol = "€".to_owned();
    prefs.language = LanguagePreference::System;

    let main = MainWindow::new().unwrap();
    main.take_preferences(&prefs);
    assert_eq!(main.get_theme(), ThemePreference::Dark);
    let layout = if cfg!(target_os = "macos") { MenuLayout::Bar } else { MenuLayout::Compact };
    assert_eq!(main.get_menu_style(), layout);
    assert_eq!(rows(&main.get_recent_files()), ["a.stl", "b.3mf"]);
    // The same list again keeps the model (a new one rebuilds the menu bar);
    // a different one replaces it.
    let shown = main.get_recent_files();
    main.take_preferences(&prefs);
    assert!(main.get_recent_files() == shown, "same files kept the model");
    prefs.recent_files.reverse();
    main.take_preferences(&prefs);
    assert_eq!(rows(&main.get_recent_files()), ["b.3mf", "a.stl"]);
    prefs.recent_files.clear();
    main.take_preferences(&prefs);
    assert!(rows(&main.get_recent_files()).is_empty());

    let window = PreferencesWindow::new().unwrap();
    prefs.theme = ThemePreference::Light;
    window.take_preferences(&prefs);
    assert_eq!(window.get_theme(), ThemePreference::Light);
    assert_eq!(window.get_aa_row(), 2);
    assert_eq!(window.get_currency(), "€");
    assert_eq!(window.get_language_row(), LanguagePreference::System.combo_index());
    assert_eq!(
        rows(&window.get_languages()),
        rows(&LanguagePreference::menu_rows())
    );

    let about = AboutWindow::new().unwrap();
    about.take_preferences(&prefs);
    assert_eq!(about.get_about_color_preference(), ThemePreference::Light);
    let shortcuts = ShortcutsDialog::new().unwrap();
    prefs.theme = ThemePreference::Dark;
    shortcuts.take_preferences(&prefs);
    assert_eq!(shortcuts.get_theme(), ThemePreference::Dark);
}

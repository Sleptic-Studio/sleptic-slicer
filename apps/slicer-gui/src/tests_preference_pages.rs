// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The Appearance and Slicing pages' own controls, through the Slint
//! testing backend: each choice reaches its callback as the value Rust
//! expects (the theme as its enum, the language and anti-aliasing as an
//! index), and the currency box works both ways.
use std::cell::RefCell;
use std::rc::Rc;

use i_slint_backend_testing::{AccessibleRole, ElementHandle};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::{PreferencesWindow, ThemePreference};

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(50));
}

fn page(section: i32) -> (PreferencesWindow, Rc<RefCell<Vec<String>>>) {
    i_slint_backend_testing::init_no_event_loop();
    let ui = PreferencesWindow::new().unwrap();
    ui.window().set_size(slint::LogicalSize::new(1100.0, 700.0));
    let languages: Vec<SharedString> = vec!["System".into(), "Deutsch".into(), "Nederlands".into()];
    ui.set_languages(ModelRc::new(VecModel::from(languages)));
    ui.set_section(section);
    let log = Rc::new(RefCell::new(Vec::new()));
    let keep = |log: &Rc<RefCell<Vec<String>>>| {
        let log = Rc::clone(log);
        move |entry: String| log.borrow_mut().push(entry)
    };
    let say = keep(&log);
    ui.on_theme_picked(move |theme| say(format!("theme {theme:?}")));
    let say = keep(&log);
    ui.on_language_picked(move |i| say(format!("language {i}")));
    let say = keep(&log);
    ui.on_currency_edited(move |text| say(format!("currency {text}")));
    let say = keep(&log);
    ui.on_aa_picked(move |i| say(format!("aa {i}")));
    ui.show().unwrap();
    settle();
    (ui, log)
}

/// The control labelled `label` with `role`, once.
fn control(ui: &PreferencesWindow, label: &str, role: AccessibleRole) -> ElementHandle {
    ElementHandle::find_by_accessible_label(ui, label)
        .find(|found| found.accessible_role() == Some(role))
        .unwrap_or_else(|| panic!("a {role:?} labelled {label:?}"))
}

/// Picks entry `steps` below the current one (above when negative) in the
/// combo box labelled `label`, as a keyboard user does: the box takes the
/// focus, its list is closed again, and the arrow keys step through it.
fn step(ui: &PreferencesWindow, label: &str, steps: i32) {
    use slint::platform::{Key, WindowEvent};
    let combo = control(ui, label, AccessibleRole::Combobox);
    combo.mock_single_click(slint::platform::PointerEventButton::Left);
    settle();
    let send = |key: Key| {
        ui.window().dispatch_event(WindowEvent::KeyPressed { text: key.into() });
        ui.window().dispatch_event(WindowEvent::KeyReleased { text: key.into() });
        settle();
    };
    send(Key::Escape);
    let key = if steps > 0 { Key::DownArrow } else { Key::UpArrow };
    for _ in 0..steps.abs() {
        send(key);
    }
}

#[test]
fn the_theme_box_sends_each_theme_as_itself() {
    let (ui, log) = page(0);
    step(&ui, "Theme", 2);
    assert_eq!(ui.get_theme(), ThemePreference::Dark);
    step(&ui, "Theme", -1);
    assert_eq!(ui.get_theme(), ThemePreference::Light);
    step(&ui, "Theme", -1);
    assert_eq!(ui.get_theme(), ThemePreference::System);
    let sent: Vec<String> = log.borrow().iter().filter(|e| e.starts_with("theme")).cloned().collect();
    assert_eq!(sent.last().map(String::as_str), Some("theme System"), "{sent:?}");
    assert!(sent.contains(&"theme Dark".to_string()) && sent.contains(&"theme Light".to_string()), "{sent:?}");
    // A saved theme from Rust shows in the box when the page opens (once
    // the box is used it keeps its own pick, as it always has).
    let fresh = PreferencesWindow::new().unwrap();
    fresh.set_theme(ThemePreference::Dark);
    fresh.show().unwrap();
    settle();
    assert_eq!(control(&fresh, "Theme", AccessibleRole::Combobox).accessible_value().unwrap(), "Dark");
}

#[test]
fn the_language_box_sends_its_position() {
    let (ui, log) = page(0);
    step(&ui, "Language", 2);
    assert_eq!(ui.get_language_row(), 2);
    step(&ui, "Language", -2);
    assert_eq!(ui.get_language_row(), 0);
    let sent: Vec<String> = log.borrow().iter().filter(|e| e.starts_with("language")).cloned().collect();
    assert!(sent.contains(&"language 2".to_string()), "{sent:?}");
    assert_eq!(sent.last().map(String::as_str), Some("language 0"), "{sent:?}");
}

#[test]
fn the_currency_box_shows_the_saved_symbol_and_reports_typing() {
    let (ui, log) = page(0);
    ui.set_currency("£".into());
    settle();
    let field = control(&ui, "Currency symbol", AccessibleRole::TextInput);
    assert_eq!(field.accessible_value().unwrap(), "£");
    field.set_accessible_value("€");
    assert_eq!(*log.borrow(), ["currency €"]);
    assert_eq!(ui.get_currency(), "€");
}

#[test]
fn the_anti_aliasing_strip_sends_the_level_clicked() {
    let (ui, log) = page(3);
    for (label, row) in [("8x", 3), ("2x", 1), ("Off", 0)] {
        control(&ui, label, AccessibleRole::Button).mock_single_click(slint::platform::PointerEventButton::Left);
        settle();
        assert_eq!(ui.get_aa_row(), row, "{label}");
    }
    let sent: Vec<String> = log.borrow().iter().filter(|e| e.starts_with("aa")).cloned().collect();
    assert_eq!(sent, ["aa 3", "aa 1", "aa 0"]);
    // Set from Rust (a saved preference), the strip shows it.
    ui.set_aa_row(2);
    settle();
    assert_eq!(control(&ui, "4x", AccessibleRole::Button).accessible_checked(), Some(true));
}

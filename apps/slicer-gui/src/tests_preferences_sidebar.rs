// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The Preferences side bar's tabs, through the Slint testing backend:
//! the arrow keys and Home / End move the keyboard highlight, Enter or
//! Space opens the highlighted section, a click opens one directly.
use i_slint_backend_testing::{AccessibleRole, ElementHandle};
use slint::ComponentHandle;
use slint::platform::{Key, PointerEventButton, WindowEvent};

use crate::PreferencesWindow;

fn window() -> PreferencesWindow {
    i_slint_backend_testing::init_no_event_loop();
    let ui = PreferencesWindow::new().unwrap();
    ui.window().set_size(slint::LogicalSize::new(1100.0, 700.0));
    ui.show().unwrap();
    // The window hands its focus to the side bar.
    settle();
    ui
}

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(50));
}

fn tap(ui: &PreferencesWindow, key: impl Into<slint::SharedString>) {
    let key = key.into();
    ui.window().dispatch_event(WindowEvent::KeyPressed { text: key.clone() });
    ui.window().dispatch_event(WindowEvent::KeyReleased { text: key });
    settle();
}

fn tabs(ui: &PreferencesWindow) -> Vec<ElementHandle> {
    let mut found: Vec<ElementHandle> = i_slint_backend_testing::ElementQuery::from_root(ui)
        .match_descendants()
        .match_accessible_role(AccessibleRole::Tab)
        .find_all();
    // An element can be listed once per component it is reached through:
    // keep each tab's first listing.
    let mut seen = std::collections::HashSet::new();
    found.retain(|tab| seen.insert(tab.accessible_label().unwrap_or_default()));
    found
}

/// Which tab says it is selected.
fn selected_tab(ui: &PreferencesWindow) -> String {
    let tabs = tabs(ui);
    let chosen: Vec<_> = tabs.iter().filter(|t| t.accessible_item_selected() == Some(true)).collect();
    assert_eq!(chosen.len(), 1, "one selected tab");
    chosen[0].accessible_label().unwrap().to_string()
}

#[test]
fn the_side_bar_lists_the_four_sections_with_the_first_open() {
    let ui = window();
    let labels: Vec<String> = tabs(&ui).iter().map(|t| t.accessible_label().unwrap().to_string()).collect();
    assert_eq!(labels, ["Appearance", "Printers", "Resins", "Slicing"]);
    assert_eq!(ui.get_section(), 0);
    assert_eq!(selected_tab(&ui), "Appearance");
}

#[test]
fn arrows_move_the_highlight_and_enter_opens_it() {
    let ui = window();
    tap(&ui, Key::DownArrow);
    tap(&ui, Key::DownArrow);
    assert_eq!(ui.get_section(), 0, "moving the highlight opens nothing");
    tap(&ui, Key::Return);
    assert_eq!(ui.get_section(), 2);
    assert_eq!(selected_tab(&ui), "Resins");
    // Up goes back one; it stops at the first and the last tab.
    tap(&ui, Key::UpArrow);
    tap(&ui, Key::Return);
    assert_eq!(ui.get_section(), 1);
    for _ in 0..3 {
        tap(&ui, Key::UpArrow);
    }
    tap(&ui, Key::Return);
    assert_eq!(ui.get_section(), 0);
    for _ in 0..6 {
        tap(&ui, Key::DownArrow);
    }
    tap(&ui, Key::Return);
    assert_eq!(ui.get_section(), 3);
}

#[test]
fn home_end_and_space() {
    let ui = window();
    tap(&ui, Key::End);
    tap(&ui, " ");
    assert_eq!(ui.get_section(), 3, "Space opens the last tab");
    tap(&ui, Key::Home);
    tap(&ui, " ");
    assert_eq!(ui.get_section(), 0, "and Home the first");
}

#[test]
fn a_click_opens_a_section_and_the_highlight_follows_it() {
    let ui = window();
    // The keyboard first, so the highlight has been moved by hand.
    tap(&ui, Key::DownArrow);
    tap(&ui, Key::Return);
    assert_eq!(ui.get_section(), 1);
    let slicing = tabs(&ui).into_iter().find(|t| t.accessible_label().as_deref() == Some("Slicing")).unwrap();
    slicing.mock_single_click(PointerEventButton::Left);
    settle();
    assert_eq!(ui.get_section(), 3);
    assert_eq!(selected_tab(&ui), "Slicing");
    // The keyboard carries on from the clicked tab, not from where it was.
    tap(&ui, Key::UpArrow);
    tap(&ui, Key::Return);
    assert_eq!(ui.get_section(), 2);
}

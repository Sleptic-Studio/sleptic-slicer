// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The main window's overlays, through the Slint testing backend: the load,
//! support and save progress panels with each phase's text, the drop hint,
//! and the slicing card with its Cancel and Escape.
use std::cell::Cell;
use std::rc::Rc;

use i_slint_backend_testing::{AccessibleRole, ElementHandle};
use slint::ComponentHandle;
use slint::platform::{Key, WindowEvent};

use crate::{OpenStep, MainWindow, WriteStep};

fn window() -> MainWindow {
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    ui.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
    ui.show().unwrap();
    ui
}

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(50));
}

fn shows(ui: &MainWindow, text: &str) -> bool {
    settle();
    ElementHandle::find_by_accessible_label(ui, text).next().is_some()
}

fn progress_bars(ui: &MainWindow) -> usize {
    settle();
    let mut found = i_slint_backend_testing::ElementQuery::from_root(ui)
        .match_descendants()
        .match_accessible_role(AccessibleRole::ProgressIndicator)
        .find_all();
    // An element can be listed once per component it is reached through.
    let mut seen = std::collections::HashSet::new();
    found.retain(|bar| seen.insert(format!("{:?}{:?}", bar.absolute_position(), bar.size())));
    found.len()
}

#[test]
fn the_load_panel_names_each_phase_while_a_file_loads() {
    let ui = window();
    let idle_bars = progress_bars(&ui);
    assert!(!shows(&ui, "Reading file…"));
    ui.set_opening(true);
    for (phase, text) in [
        (OpenStep::Reading, "Reading file…"),
        (OpenStep::Meshing, "Preparing geometry…"),
        (OpenStep::Uploading, "Uploading to GPU…"),
    ] {
        ui.set_open_step(phase);
        assert!(shows(&ui, text), "{text}");
    }
    assert_eq!(progress_bars(&ui), idle_bars + 1, "the panel's bar");
    ui.set_opening(false);
    assert!(!shows(&ui, "Uploading to GPU…"));
    assert_eq!(progress_bars(&ui), idle_bars);
}

#[test]
fn the_save_panel_names_each_phase_while_saving() {
    let ui = window();
    assert!(!shows(&ui, "Serializing model…"));
    ui.set_saving(true);
    for (phase, text) in [
        (WriteStep::Flattening, "Preparing geometry…"),
        (WriteStep::Serializing, "Serializing model…"),
        (WriteStep::Compressing, "Compressing…"),
        (WriteStep::Writing, "Writing file…"),
    ] {
        ui.set_save_step(phase);
        assert!(shows(&ui, text), "{text}");
    }
    ui.set_saving(false);
    assert!(!shows(&ui, "Writing file…"));
}

#[test]
fn the_support_panel_shows_the_generators_own_words_or_a_default() {
    let ui = window();
    ui.set_making_supports(true);
    assert!(shows(&ui, "Generating support structure…"));
    ui.set_supports_step("Placing tips".into());
    assert!(shows(&ui, "Placing tips"));
    assert!(!shows(&ui, "Generating support structure…"));
    ui.set_making_supports(false);
    assert!(!shows(&ui, "Placing tips"));
}

#[test]
fn the_drop_hint_shows_only_while_a_file_is_over_the_window() {
    let ui = window();
    assert!(!shows(&ui, "Drop to open"));
    ui.set_file_over_window(true);
    assert!(shows(&ui, "Drop to open"));
    ui.set_file_over_window(false);
    assert!(!shows(&ui, "Drop to open"));
}

#[test]
fn the_slicing_card_counts_layers_and_cancel_or_escape_stop_it() {
    let ui = window();
    let cancels = Rc::new(Cell::new(0));
    let counter = Rc::clone(&cancels);
    ui.on_cancel_slice(move || counter.set(counter.get() + 1));
    ui.set_slice_plate_label("Plate 1".into());
    ui.set_slicing(true);
    assert!(shows(&ui, "Plate 1"));
    assert!(shows(&ui, "Preparing..."), "no layer count yet");
    ui.set_layers_planned(10);
    ui.set_layers_done(3);
    assert!(shows(&ui, "Slicing layer 3 / 10"));
    let cancel = ElementHandle::find_by_accessible_label(&ui, "Cancel")
        .find(|b| b.accessible_role() == Some(AccessibleRole::Button))
        .expect("the card's Cancel");
    cancel.invoke_accessible_default_action();
    assert_eq!(cancels.get(), 1);
    // The card holds the keyboard: Escape cancels, other keys do nothing.
    for key in [Key::Escape, Key::Tab, Key::Delete] {
        ui.window().dispatch_event(WindowEvent::KeyPressed { text: key.into() });
    }
    assert_eq!(cancels.get(), 2);
}

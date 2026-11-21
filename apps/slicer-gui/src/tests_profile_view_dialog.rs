// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The resin profile dialog's own controls, driven through the Slint
//! testing backend: closing and saving, the edit fields writing back what
//! is typed, the calibration picker, and which cards each mode shows.
use std::cell::RefCell;
use std::rc::Rc;

use i_slint_backend_testing::{AccessibleRole, ElementHandle};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::ProfileViewDialog;

/// The dialog editing a custom profile: defaults editable, two
/// calibration targets, nothing picked yet. Callbacks go to the log.
fn editing(defaults: bool, motion_only: bool) -> (ProfileViewDialog, Rc<RefCell<Vec<String>>>) {
    i_slint_backend_testing::init_no_event_loop();
    let dialog = ProfileViewDialog::new().unwrap();
    dialog.window().set_size(slint::LogicalSize::new(480.0, 900.0));
    dialog.set_resin_heading("Maker Resin".into());
    dialog.set_allow_profile_edit(true);
    dialog.set_allow_defaults_edit(defaults);
    dialog.set_motion_only(motion_only);
    let targets: Vec<SharedString> = vec!["Generic".into(), "Printer A".into()];
    dialog.set_calibration_target_captions(ModelRc::new(VecModel::from(targets)));
    let log = Rc::new(RefCell::new(Vec::new()));
    let entry = |log: &Rc<RefCell<Vec<String>>>| {
        let log = Rc::clone(log);
        move |text: String| log.borrow_mut().push(text)
    };
    let say = entry(&log);
    dialog.on_viewer_close_requested(move || say("close".into()));
    let say = entry(&log);
    dialog.on_profile_save_requested(move || say("save".into()));
    let say = entry(&log);
    dialog.on_calibration_target_selected(move |i| say(format!("target {i}")));
    let say = entry(&log);
    dialog.on_motion_refresh(move || say("refresh".into()));
    dialog.show().unwrap();
    (dialog, log)
}

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(50));
}

fn buttons(dialog: &ProfileViewDialog, label: &str) -> Vec<ElementHandle> {
    ElementHandle::find_by_accessible_label(dialog, label)
        .filter(|found| found.accessible_role() == Some(AccessibleRole::Button))
        .collect()
}

/// Each edit field's text box, in the order the dialog shows them.
fn inputs(dialog: &ProfileViewDialog) -> Vec<ElementHandle> {
    ElementHandle::find_by_element_type_name(dialog, "ProfileInputRow")
        .map(|field| {
            let boxes = field.query_descendants().match_accessible_role(AccessibleRole::TextInput);
            boxes.find_first().expect("a text box in the field")
        })
        .collect()
}

fn take(log: &Rc<RefCell<Vec<String>>>) -> Vec<String> {
    std::mem::take(&mut *log.borrow_mut())
}

#[test]
fn close_and_save_buttons_and_escape() {
    let (dialog, log) = editing(true, false);
    take(&log);
    buttons(&dialog, "Save")[0].invoke_accessible_default_action();
    buttons(&dialog, "Close")[0].invoke_accessible_default_action();
    assert_eq!(take(&log), ["save", "close"]);
    // Escape anywhere in the dialog closes it too.
    let escape = slint::platform::WindowEvent::KeyPressed { text: slint::platform::Key::Escape.into() };
    dialog.window().dispatch_event(escape);
    assert_eq!(take(&log), ["close"]);
}

#[test]
fn a_read_only_profile_has_close_but_no_save() {
    let (dialog, _log) = editing(true, false);
    dialog.set_allow_profile_edit(false);
    settle();
    assert_eq!(buttons(&dialog, "Save").len(), 0);
    assert_eq!(buttons(&dialog, "Close").len(), 1);
    assert!(inputs(&dialog).is_empty(), "no edit cards");
}

#[test]
fn the_defaults_card_writes_back_what_is_typed() {
    let (dialog, _log) = editing(true, false);
    let fields = inputs(&dialog);
    // Six defaults, then the picker's (none until a target is picked).
    assert_eq!(fields.len(), 6, "the six defaults");
    let typed = ["101", "102", "103", "7", "25", "45"];
    for (field, text) in fields.iter().zip(typed) {
        field.set_accessible_value(text);
    }
    let got = [
        dialog.get_shrink_x_input(),
        dialog.get_shrink_y_input(),
        dialog.get_shrink_z_input(),
        dialog.get_default_bottom_layers_input(),
        dialog.get_defaults_price_text(),
        dialog.get_defaults_support_angle_text(),
    ];
    assert_eq!(got.map(|t| t.to_string()), typed.map(String::from));
    // A value set from Rust shows in its field again (after typing).
    dialog.set_shrink_x_input("99".into());
    settle();
    assert_eq!(inputs(&dialog)[0].accessible_value().unwrap(), "99");
}

#[test]
fn an_extension_has_no_defaults_card() {
    let (dialog, _log) = editing(false, false);
    assert!(inputs(&dialog).is_empty(), "nothing editable until a target is picked");
}

#[test]
fn picking_a_target_shows_its_six_values_and_lift_changes_refresh_the_motion() {
    let (dialog, log) = editing(false, false);
    dialog.set_selected_calibration_target(1);
    settle();
    let fields = inputs(&dialog);
    assert_eq!(fields.len(), 6, "the six calibration values");
    let typed = ["2.5", "25", "5", "6", "60", "120"];
    for (field, text) in fields.iter().zip(typed) {
        field.set_accessible_value(text);
    }
    let got = [
        dialog.get_normal_exposure_input(),
        dialog.get_bottom_exposure_input(),
        dialog.get_calibration_bottom_layers_input(),
        dialog.get_lift_distance_text(),
        dialog.get_lift_speed_text(),
        dialog.get_retract_speed_text(),
    ];
    assert_eq!(got.map(|t| t.to_string()), typed.map(String::from));
    settle();
    // Exposure and the three motion values each ask for a fresh timeline.
    let refreshes = take(&log).iter().filter(|e| *e == "refresh").count();
    assert_eq!(refreshes, 4);
}

#[test]
fn motion_only_hides_the_six_calibration_values() {
    let (dialog, _log) = editing(false, true);
    dialog.set_selected_calibration_target(0);
    settle();
    let labels = ["Normal exposure (s)", "Lift speed (mm/min)"];
    for label in labels {
        let found = ElementHandle::find_by_accessible_label(&dialog, label).count();
        assert_eq!(found, 0, "{label} is hidden");
    }
}

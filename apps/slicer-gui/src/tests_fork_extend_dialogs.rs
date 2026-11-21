// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The fork and extend dialogs' own controls, through the Slint testing
//! backend: the name fields both ways, the copy boxes, the calibration
//! target, and the buttons and Escape.
use std::cell::RefCell;
use std::rc::Rc;

use i_slint_backend_testing::{AccessibleRole, ElementHandle};
use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::{ExtendDialog, ForkDialog};

type Log = Rc<RefCell<Vec<String>>>;

fn recorder(log: &Log) -> impl Fn(String) + 'static {
    let log = Rc::clone(log);
    move |entry| log.borrow_mut().push(entry)
}

fn labels(texts: &[&str]) -> ModelRc<SharedString> {
    let texts: Vec<SharedString> = texts.iter().map(|t| SharedString::from(*t)).collect();
    ModelRc::new(VecModel::from(texts))
}

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(50));
}

fn with_role(root: &impl i_slint_backend_testing::ElementRoot, label: &str, role: AccessibleRole) -> Vec<ElementHandle> {
    ElementHandle::find_by_accessible_label(root, label).filter(|e| e.accessible_role() == Some(role)).collect()
}

fn escape(window: &slint::Window) {
    window.dispatch_event(WindowEvent::KeyPressed { text: Key::Escape.into() });
}

/// The fork dialog for a resin with two calibration blocks, prefilled.
fn fork() -> (ForkDialog, Log) {
    i_slint_backend_testing::init_no_event_loop();
    let dialog = ForkDialog::new().unwrap();
    dialog.set_fork_source_caption("Standard".into());
    dialog.set_new_profile_name("Standard copy".into());
    dialog.set_new_profile_brand("Maker".into());
    dialog.set_source_block_captions(labels(&["Generic", "Printer A"]));
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let say = recorder(&log);
    dialog.on_source_block_selection_changed(move |i, on| say(format!("block {i} {on}")));
    let say = recorder(&log);
    dialog.on_create_fork_requested(move || say("create".into()));
    let say = recorder(&log);
    dialog.on_dismiss_fork_requested(move || say("cancel".into()));
    dialog.show().unwrap();
    settle();
    (dialog, log)
}

/// The text box now showing `value`.
fn box_showing(dialog: &ForkDialog, value: &str) -> ElementHandle {
    i_slint_backend_testing::ElementQuery::from_root(dialog)
        .match_descendants()
        .match_accessible_role(AccessibleRole::TextInput)
        .find_all()
        .into_iter()
        .find(|found| found.accessible_value().as_deref() == Some(value))
        .unwrap_or_else(|| panic!("a box showing {value:?}"))
}

#[test]
fn the_fork_names_are_typed_back_and_a_new_prefill_shows() {
    let (dialog, _) = fork();
    box_showing(&dialog, "Maker").set_accessible_value("Other maker");
    box_showing(&dialog, "Standard copy").set_accessible_value("Mine");
    assert_eq!((dialog.get_new_profile_brand().as_str(), dialog.get_new_profile_name().as_str()), ("Other maker", "Mine"));
    // After typing, a value set by Rust still reaches the box.
    dialog.set_new_profile_name("Prefilled again".into());
    dialog.set_new_profile_brand("Brand again".into());
    settle();
    box_showing(&dialog, "Brand again");
    box_showing(&dialog, "Prefilled again");
}

#[test]
fn the_copy_boxes_start_ticked_and_report_their_changes() {
    let (dialog, log) = fork();
    let intent = with_role(&dialog, "Intent", AccessibleRole::Checkbox);
    let defaults = with_role(&dialog, "Defaults", AccessibleRole::Checkbox);
    assert!(intent[0].accessible_checked().unwrap() && defaults[0].accessible_checked().unwrap());
    intent[0].invoke_accessible_default_action();
    assert!(!dialog.get_include_source_intent());
    defaults[0].invoke_accessible_default_action();
    assert!(!dialog.get_include_source_defaults());
    // Each calibration box reports its own index; all start ticked.
    let second = with_role(&dialog, "Calibration: Printer A", AccessibleRole::Checkbox);
    assert!(second[0].accessible_checked().unwrap());
    second[0].invoke_accessible_default_action();
    let first = with_role(&dialog, "Calibration: Generic", AccessibleRole::Checkbox);
    first[0].invoke_accessible_default_action();
    assert_eq!(*log.borrow(), ["block 1 false", "block 0 false"]);
}

#[test]
fn the_fork_buttons_and_escape() {
    let (dialog, log) = fork();
    with_role(&dialog, "Create", AccessibleRole::Button)[0].invoke_accessible_default_action();
    with_role(&dialog, "Cancel", AccessibleRole::Button)[0].invoke_accessible_default_action();
    escape(dialog.window());
    assert_eq!(*log.borrow(), ["create", "cancel", "cancel"]);
}

/// The extend dialog with two possible targets, none picked.
fn extend() -> (ExtendDialog, Log) {
    i_slint_backend_testing::init_no_event_loop();
    let dialog = ExtendDialog::new().unwrap();
    dialog.set_extension_source_title("Standard".into());
    dialog.set_extension_target_choices(labels(&["Printer A", "Printer B"]));
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let say = recorder(&log);
    dialog.on_extension_save_requested(move || say("save".into()));
    let say = recorder(&log);
    dialog.on_extension_cancel_requested(move || say("cancel".into()));
    dialog.show().unwrap();
    settle();
    (dialog, log)
}

fn extend_fields(dialog: &ExtendDialog) -> Vec<ElementHandle> {
    ElementHandle::find_by_element_type_name(dialog, "ProfileInputRow")
        .map(|field| {
            let boxes = field.query_descendants().match_accessible_role(AccessibleRole::TextInput);
            boxes.find_first().expect("its text box")
        })
        .collect()
}

#[test]
fn the_values_appear_with_a_target_and_save_waits_for_one() {
    let (dialog, log) = extend();
    // The picker takes its first target by itself; with none picked there
    // are no values and Save is off.
    assert_eq!(dialog.get_extension_target_selection(), 0);
    dialog.set_extension_target_selection(-1);
    settle();
    assert!(extend_fields(&dialog).is_empty(), "no target, no values");
    let save = with_role(&dialog, "Save", AccessibleRole::Button);
    assert_eq!(save[0].accessible_enabled(), Some(false));
    save[0].invoke_accessible_default_action();
    assert!(log.borrow().is_empty(), "a disabled Save does nothing");

    dialog.set_extension_target_selection(1);
    settle();
    let fields = extend_fields(&dialog);
    assert_eq!(fields.len(), 6);
    let typed = ["2.5", "25", "5", "6", "60", "150"];
    for (field, text) in fields.iter().zip(typed) {
        field.set_accessible_value(text);
    }
    let got = [
        dialog.get_extension_normal_exposure_input(),
        dialog.get_extension_bottom_exposure_input(),
        dialog.get_extension_bottom_layer_input(),
        dialog.get_extension_lift_distance_input(),
        dialog.get_extension_lift_speed_input(),
        dialog.get_extension_retract_speed_input(),
    ];
    assert_eq!(got.map(|t| t.to_string()), typed.map(String::from));
    let save = with_role(&dialog, "Save", AccessibleRole::Button);
    assert_eq!(save[0].accessible_enabled(), Some(true));
    save[0].invoke_accessible_default_action();
    with_role(&dialog, "Cancel", AccessibleRole::Button)[0].invoke_accessible_default_action();
    escape(dialog.window());
    assert_eq!(*log.borrow(), ["save", "cancel", "cancel"]);
}

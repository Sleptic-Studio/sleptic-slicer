// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The resin page's own controls, clicked through the Slint testing
//! backend: which callback each button, the row and the star fire, with
//! which catalog index. Rows use sparse catalog indices (17, 42), so a
//! control that sent its visible row number instead would be caught.
use std::cell::RefCell;
use std::rc::Rc;

use i_slint_backend_testing::ElementHandle;
use slint::platform::PointerEventButton;
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::{CalibrationTagClass, PreferencesWindow, CalibrationTagData, ResinProfileEntry};

fn row(name: &str, id_index: i32, custom: bool, extension: bool) -> ResinProfileEntry {
    let tag = CalibrationTagData { calibration_caption: "Generic".into(), calibration_class: CalibrationTagClass::SharedCalibration };
    let line = ModelRc::new(VecModel::from(vec![tag]));
    ResinProfileEntry {
        resin_display_name: name.into(),
        resin_brand_name: "Maker".into(),
        resin_family_name: "Standard".into(),
        calibration_tag_lines: ModelRc::new(VecModel::from(vec![line])),
        has_custom_profile: custom,
        marked_as_favourite: false,
        represents_extension: extension,
        allows_extension: !custom,
        resin_catalog_position: id_index,
        ..Default::default()
    }
}

/// The preferences window on its resin page with two rows: a bundled
/// resin (catalog 17) and a custom extension row (catalog 42). Every
/// callback the page can fire is written to the returned log.
fn resin_page() -> (PreferencesWindow, Rc<RefCell<Vec<String>>>) {
    i_slint_backend_testing::init_no_event_loop();
    let ui = PreferencesWindow::new().unwrap();
    ui.window().set_size(slint::LogicalSize::new(1100.0, 700.0));
    ui.set_section(2);
    let rows = vec![row("Alpha", 17, false, false), row("Beta", 42, true, true)];
    ui.set_resin_list(ModelRc::new(VecModel::from(rows)));
    let log = Rc::new(RefCell::new(Vec::new()));
    let note = |log: &Rc<RefCell<Vec<String>>>| {
        let log = Rc::clone(log);
        move |entry: String| log.borrow_mut().push(entry)
    };
    let say = note(&log);
    ui.on_resin_picked(move |i| say(format!("select {i}")));
    let say = note(&log);
    ui.on_resin_star_toggled(move |i| say(format!("favourite {i}")));
    let say = note(&log);
    ui.on_view_resin(move |i| say(format!("view {i}")));
    let say = note(&log);
    ui.on_edit_resin(move |i, extension| say(format!("edit {i} {extension}")));
    let say = note(&log);
    ui.on_import_package(move || say("import".into()));
    let say = note(&log);
    ui.on_export_resins(move || say("export".into()));
    let say = note(&log);
    ui.on_tag_measured(move |i, extension, width| {
        say(format!("width {i} {extension} {width:.1}"))
    });
    ui.show().unwrap();
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(50));
    (ui, log)
}

/// The buttons labelled `label` (the hidden context menu has items with
/// the same labels).
fn labelled(ui: &PreferencesWindow, label: &str) -> Vec<ElementHandle> {
    ElementHandle::find_by_accessible_label(ui, label)
        .filter(|found| found.accessible_role() == Some(i_slint_backend_testing::AccessibleRole::Button))
        .collect()
}

/// Every element labelled `label`, whatever its role.
fn any_labelled(ui: &PreferencesWindow, label: &str) -> Vec<ElementHandle> {
    ElementHandle::find_by_accessible_label(ui, label).collect()
}

/// Runs `act`, then gives what it logged (pill widths left out).
fn after(log: &Rc<RefCell<Vec<String>>>, act: impl FnOnce()) -> Vec<String> {
    log.borrow_mut().clear();
    act();
    log.borrow().iter().filter(|entry| !entry.starts_with("width")).cloned().collect()
}

#[test]
fn each_rows_buttons_carry_its_catalog_index() {
    let (ui, log) = resin_page();
    let views = labelled(&ui, "View");
    assert_eq!(views.len(), 2, "one View per row");
    assert_eq!(after(&log, || views[1].invoke_accessible_default_action()), ["view 42"]);
    assert_eq!(after(&log, || views[0].invoke_accessible_default_action()), ["view 17"]);
    // Only the custom row can be edited, and it is an extension row.
    let edits = labelled(&ui, "Edit");
    assert_eq!(edits.len(), 1, "a bundled resin has no Edit");
    assert_eq!(after(&log, || edits[0].invoke_accessible_default_action()), ["edit 42 true"]);
}

#[test]
fn the_package_buttons_import_and_export() {
    let (ui, log) = resin_page();
    let import = labelled(&ui, "Import package...");
    assert_eq!(after(&log, || import[0].invoke_accessible_default_action()), ["import"]);
    let export = labelled(&ui, "Export materials...");
    assert_eq!(after(&log, || export[0].invoke_accessible_default_action()), ["export"]);
}

#[test]
fn clicking_a_row_selects_its_resin_and_the_star_only_toggles_the_favourite() {
    let (ui, log) = resin_page();
    let name = any_labelled(&ui, "Maker Beta");
    assert_eq!(name.len(), 1);
    let picked = after(&log, || name[0].mock_single_click(PointerEventButton::Left));
    assert_eq!(picked, ["select 42"]);
    assert_eq!(ui.get_chosen_resin(), 42);

    let stars: Vec<ElementHandle> =
        ElementHandle::find_by_element_type_name(&ui, "PreferenceFavoriteMarker").collect();
    assert_eq!(stars.len(), 2, "one star per row");
    let starred = after(&log, || stars[0].mock_single_click(PointerEventButton::Left));
    assert_eq!(starred, ["favourite 17"], "the star does not select its row");
    assert_eq!(ui.get_chosen_resin(), 42);
}

#[test]
fn every_row_reports_exactly_the_width_its_tags_may_use() {
    let (_ui, log) = resin_page();
    let widths: Vec<String> =
        log.borrow().iter().filter(|entry| entry.starts_with("width")).cloned().collect();
    // Measured on the page before its rewrite (R176), in a 1100 px window:
    // the extension row loses 24 px more indent and its Edit button.
    for expected in ["width 17 false 723.0", "width 42 true 655.0"] {
        assert!(widths.iter().any(|w| w == expected), "{expected} in {widths:?}");
    }
}

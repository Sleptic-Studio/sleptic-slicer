// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The menu commands both menus call (R182): each does its job only when
//! its condition holds. Menu items cannot be clicked in the testing
//! backend, so the commands are called directly, and a scan checks that
//! the menu bar and the compact menu both use them.
use std::cell::RefCell;
use std::rc::Rc;


use crate::{MainWindow, MenuLayout, WorkspaceSection};

type Log = Rc<RefCell<Vec<String>>>;

/// A window with every callback the commands reach written to the log.
fn window() -> (MainWindow, Log) {
    i_slint_backend_testing::init_no_event_loop();
    fresh()
}

/// Another such window, the testing backend already set up.
fn fresh() -> (MainWindow, Log) {
    let ui = MainWindow::new().unwrap();
    let log: Log = Rc::default();
    macro_rules! note {
        ($($set:ident => $name:literal),* $(,)?) => {$(
            let entries = Rc::clone(&log);
            ui.$set(move || entries.borrow_mut().push($name.to_string()));
        )*};
    }
    note!(
        on_calibrate_estimate_requested => "estimate",
        on_recipes_requested => "recipes",
        on_batch_prepare_requested => "batch",
        on_save_project => "save",
        on_save_project_as => "save as",
        on_export_model => "export as",
        on_start_slice => "slice",
        on_export_sliced_job_requested => "job",
        on_export_sliced_svg_requested => "svg",
        on_export_sliced_png_layers_requested => "png",
        on_undo => "undo",
        on_redo => "redo",
        on_duplicate_requested => "duplicate",
        on_delete_key => "delete",
    );
    let entries = Rc::clone(&log);
    ui.on_projection_toggled(move |on| entries.borrow_mut().push(format!("perspective {on}")));
    let entries = Rc::clone(&log);
    ui.on_parts_panel_toggled(move |on| entries.borrow_mut().push(format!("list {on}")));
    let entries = Rc::clone(&log);
    ui.on_layer_view_toggled(move |on| entries.borrow_mut().push(format!("layer view {on}")));
    let entries = Rc::clone(&log);
    ui.on_menu_style_picked(move |layout| entries.borrow_mut().push(format!("layout {layout:?}")));
    (ui, log)
}

fn take(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.borrow_mut())
}

#[test]
fn calibration_tools_open_unless_slicing() {
    let (ui, log) = window();
    ui.invoke_open_calibration_tool(0);
    ui.invoke_open_calibration_tool(1);
    assert!(ui.get_calibration_open() && ui.get_overhang_test_open());
    for (tool, machine) in [(2, 0), (3, 1), (4, 2)] {
        ui.invoke_open_calibration_tool(tool);
        assert_eq!(ui.get_machine_tool(), machine, "tool {tool}");
    }
    ui.invoke_open_calibration_tool(5);
    assert_eq!(take(&log), ["estimate"]);
    // While slicing, none of them opens.
    let (ui, log) = fresh();
    ui.set_slicing(true);
    let machine_before = ui.get_machine_tool();
    for tool in 0..=5 {
        ui.invoke_open_calibration_tool(tool);
    }
    assert!(!ui.get_calibration_open() && !ui.get_overhang_test_open());
    assert_eq!(ui.get_machine_tool(), machine_before);
    assert!(take(&log).is_empty());
}

#[test]
fn planners_and_slicing_wait_for_a_running_slice() {
    let (ui, log) = window();
    ui.invoke_open_planner(false);
    ui.invoke_open_planner(true);
    ui.invoke_run_slice();
    assert_eq!(take(&log), ["recipes", "batch", "slice"]);
    ui.set_slicing(true);
    ui.invoke_open_planner(false);
    ui.invoke_open_planner(true);
    ui.invoke_run_slice();
    assert!(take(&log).is_empty());
}

#[test]
fn saving_commands_wait_for_a_running_save() {
    let (ui, log) = window();
    for kind in 0..=2 {
        ui.invoke_run_save(kind);
    }
    assert_eq!(take(&log), ["save", "save as", "export as"]);
    ui.set_saving(true);
    for kind in 0..=2 {
        ui.invoke_run_save(kind);
    }
    assert!(take(&log).is_empty());
}

#[test]
fn slice_exports_need_a_slice_ready_to_export() {
    let (ui, log) = window();
    for kind in 0..=2 {
        ui.invoke_export_slice(kind);
    }
    assert!(take(&log).is_empty(), "nothing to export yet");
    ui.set_slice_export_ready(true);
    for kind in 0..=2 {
        ui.invoke_export_slice(kind);
    }
    assert_eq!(take(&log), ["job", "svg", "png"]);
}

#[test]
fn undo_and_redo_need_history_no_repair_and_no_slice_section() {
    let (ui, log) = window();
    ui.set_section(WorkspaceSection::Transform);
    ui.invoke_run_history(false);
    ui.invoke_run_history(true);
    assert!(take(&log).is_empty(), "no history");
    ui.set_undo_available(true);
    ui.set_redo_available(true);
    ui.invoke_run_history(false);
    ui.invoke_run_history(true);
    assert_eq!(take(&log), ["undo", "redo"]);
    ui.set_repairing(true);
    ui.invoke_run_history(false);
    ui.invoke_run_history(true);
    assert!(take(&log).is_empty(), "not during a repair");
    ui.set_repairing(false);
    ui.set_section(WorkspaceSection::Slice);
    ui.invoke_run_history(false);
    ui.invoke_run_history(true);
    assert!(take(&log).is_empty(), "not in Slice");
    ui.set_section(WorkspaceSection::Prepare);
    ui.invoke_run_history(true);
    assert_eq!(take(&log), ["redo"], "Prepare's repairs can be undone");
}

#[test]
fn duplicate_and_delete_need_a_selection_in_transform() {
    let (ui, log) = window();
    ui.set_section(WorkspaceSection::Transform);
    ui.invoke_run_on_selection(false);
    ui.invoke_run_on_selection(true);
    assert!(take(&log).is_empty(), "nothing selected");
    ui.set_chosen_part_row(0);
    ui.invoke_run_on_selection(false);
    ui.invoke_run_on_selection(true);
    assert_eq!(take(&log), ["duplicate", "delete"]);
    ui.set_section(WorkspaceSection::Prepare);
    ui.invoke_run_on_selection(true);
    assert!(take(&log).is_empty(), "only in Transform");
}

#[test]
fn view_toggles_flip_their_state_and_report_it() {
    let (ui, log) = window();
    let perspective = ui.get_perspective_view();
    ui.invoke_toggle_projection();
    assert_eq!(ui.get_perspective_view(), !perspective);
    let list = ui.get_parts_panel_shown();
    ui.invoke_toggle_object_list();
    assert_eq!(ui.get_parts_panel_shown(), !list);
    let layers = ui.get_layer_view_shown();
    ui.invoke_toggle_slice_view();
    assert_eq!(ui.get_layer_view_shown(), !layers);
    ui.set_menu_style(MenuLayout::Bar);
    ui.invoke_switch_menu_layout();
    ui.set_menu_style(MenuLayout::Compact);
    ui.invoke_switch_menu_layout();
    assert_eq!(
        take(&log),
        [
            format!("perspective {}", !perspective),
            format!("list {}", !list),
            format!("layer view {}", !layers),
            "layout Compact".to_string(),
            "layout Bar".to_string(),
        ]
    );
}

/// Both menus call the same commands: the bar (whose items may only act
/// when picked) and the compact menu.
#[test]
fn both_menus_use_the_commands() {
    let main = include_str!("../ui/main-window.slint");
    let bar = &main[main.find("    MenuBar {").unwrap()..main.find("compact-menu := ContextMenuArea").unwrap()];
    let compact = &main[main.find("compact-menu := ContextMenuArea").unwrap()..];
    let calls = [
        "root.open-calibration-tool(0)",
        "root.open-calibration-tool(1)",
        "root.open-calibration-tool(2)",
        "root.open-calibration-tool(3)",
        "root.open-calibration-tool(4)",
        "root.open-calibration-tool(5)",
        "root.open-planner(false)",
        "root.open-planner(true)",
        "root.run-save(0)",
        "root.run-save(1)",
        "root.run-save(2)",
        "root.run-slice()",
        "root.export-slice(0)",
        "root.export-slice(1)",
        "root.export-slice(2)",
        "root.run-history(false)",
        "root.run-history(true)",
        "root.run-on-selection(false)",
        "root.run-on-selection(true)",
        "root.toggle-projection()",
        "root.toggle-object-list()",
        "root.toggle-slice-view()",
        "root.switch-menu-layout()",
    ];
    for call in calls {
        assert_eq!(bar.matches(call).count(), 1, "menu bar: {call}");
        assert_eq!(compact.matches(call).count(), 1, "compact menu: {call}");
    }
}

/// No menu item is greyed out for good: Help's Documentation, Release
/// Notes, Report Issue, Check for Updates and View License never did
/// anything and were removed (R245). Both menus open the same menus in the
/// same order.
#[test]
fn menus_hold_no_dead_items_and_match() {
    let main = include_str!("../ui/main-window.slint");
    let bar = &main[main.find("    MenuBar {").unwrap()..main.find("    mv := MeshViewer {").unwrap()];
    let compact = &main[main.find("compact-menu := ContextMenuArea").unwrap()..];
    let compact = &compact[..compact.find("menu-button := Rectangle").unwrap()];
    for (name, menu) in [("bar", bar), ("compact", compact)] {
        assert!(!menu.contains("enabled: false"), "{name} menu has a dead item");
    }
    let titles = |menu: &str| -> Vec<String> {
        menu.lines()
            .map(str::trim)
            .filter(|l| l.starts_with("title: @tr(") && !l.contains("MenuItem"))
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(titles(bar), titles(compact));
    assert_eq!(titles(bar).len(), 6, "File, Open Recent, Edit, View, Calibration, Help");
}

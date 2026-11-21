// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Real MainWindow accessibility actions against a bound scene document.
#[allow(unused_imports)]
use crate::panel_place::Panel;
use super::*;
use crate::build_volume::FALLBACK_VOLUME;
use crate::language::LanguagePreference;
use crate::preferences_bus::{PreferenceHub, UserPreferences};
use i_slint_backend_testing::{ElementHandle, ElementQuery};
use sc_print_primitives::AaLevel;
use slint::ComponentHandle;
use std::collections::BTreeSet;
use std::rc::Rc;

pub(crate) fn preferences() -> UserPreferences {
    UserPreferences {
        theme: ThemePreference::System,
        menu_layout: MenuLayout::Bar,
        language: LanguagePreference::System,
        currency_symbol: "$".to_owned(),
        orient_preset: 0,
        perspective_view: false,
        default_printer: None,
        default_resin: None,
        favourite_printers: BTreeSet::new(),
        favourite_resins: BTreeSet::new(),
        aa_level: AaLevel::Off,
        build_volume: FALLBACK_VOLUME,
        object_list: Panel::ObjectList.start(),
        slice_panel: Panel::SliceControls.start(),
        slice_view: Panel::SliceView.start(),
        last_open_dir: None,
        recent_files: Vec::new(),
        setup_complete: true,
    }
}

fn find(ui: &MainWindow, label: &str) -> ElementHandle {
    ElementHandle::find_by_accessible_label(ui, label)
        .next()
        .unwrap_or_else(|| panic!("Missing control: {label}"))
}

/// A toolbar control, opening "More tools" first when the window is narrow
/// enough that the control lives in that flyout instead (Undo/Redo < 1600 px).
fn find_tool(ui: &MainWindow, label: &str) -> ElementHandle {
    if ElementHandle::find_by_accessible_label(ui, label)
        .next()
        .is_none()
    {
        find(ui, "More tools").invoke_accessible_default_action();
    }
    find(ui, label)
}

/// Toolbar icons the action loop deliberately does not click, each with the
/// reason. Every other IconTool must be in CONTROLS: the coverage guard below
/// fails when a new toolbar button is added without a check.
const NOT_CLICKED: &[(&str, &str)] = &[
    ("Add model", "opens the native file dialog"),
    ("Edit printer profile", "opens the printer editor window"),
    ("Edit resin profile", "opens the material editor window"),
    ("Preferences", "opens a separate window"),
    ("More tools", "exercised by find_tool and the Measure step"),
    (
        "Auto prepare",
        "starts the background support job; its steps are covered by the orient, arrange and support tests",
    ),
    (
        "Auto orient (turns the part; adds no supports)",
        "result depends on the search; covered by orient tests",
    ),
    ("Auto orient options", "opens the options popover"),
    (
        "Arrange (A)",
        "no-op for a centred single part; covered by layout tests",
    ),
    (
        "Place on face",
        "toggles a hover/click mode; covered by r52 place-on-face tests",
    ),
    ("Mirror", "opens the mirror axis popover"),
    ("Measure", "exercised after the loop via More tools"),
    ("Hide object panel (Ctrl+L)", "slides the panel away; covered by r109_object_panel_slides_out_and_back"),
    (
        "Delete all",
        "lives in More tools; covered by document tests",
    ),
    (
        "Center",
        "lives in More tools; covered by plate alignment tests",
    ),
    (
        "Drop to plate",
        "lives in More tools; covered by plate alignment tests",
    ),
    (
        "Cut",
        "opens the model tools panel (same panel as Cut / model tools)",
    ),
    ("Split", "opens the model tools panel"),
    ("Boolean", "opens the model tools panel"),
    ("Label (text)", "opens the model tools panel"),
    ("Simplify", "opens the model tools panel"),
    (
        "Islands & risks check",
        "starts a slice; covered by slice tests",
    ),
];

#[derive(Clone, Copy)]
enum Effect {
    Transform(i32),
    Panel(i32),
    Copy,
    Undo,
    Redo,
    Delete,
    /// The number of top-level objects grows.
    Grows,
    /// The number of top-level objects shrinks by one.
    Shrinks,
    /// The first object-list row's visibility flips.
    Visibility,
    /// The plate tabs viewer hides; the same button shows it again.
    PlateTabs,
}

struct Control {
    id: &'static str,
    label: &'static str,
    precondition: &'static str,
    effect: Effect,
    undo: &'static str,
}

// Each entry names an observable state effect. No callback counters stand in
// for the real document. Order intentionally exercises history across actions.
const CONTROLS: &[Control] = &[
    Control {
        id: "rotate",
        label: "Rotate (R)",
        precondition: "selected model on unlocked plate",
        effect: Effect::Transform(1),
        undo: "select Move",
    },
    Control {
        id: "scale",
        label: "Scale (S)",
        precondition: "selected model on unlocked plate",
        effect: Effect::Transform(2),
        undo: "select Move",
    },
    Control {
        id: "move",
        label: "Move (T)",
        precondition: "selected model on unlocked plate",
        effect: Effect::Transform(0),
        undo: "select Move",
    },
    Control {
        id: "supports",
        label: "Supports (generate and edit)",
        precondition: "loaded scene",
        effect: Effect::Panel(0),
        undo: "activate again",
    },
    Control {
        id: "hollow",
        label: "Hollow / Drain holes",
        precondition: "selected model on unlocked plate",
        effect: Effect::Panel(1),
        undo: "activate again",
    },
    Control {
        id: "model-tools",
        label: "Model tools (cut, split, boolean, label)",
        precondition: "selected model on unlocked plate",
        effect: Effect::Panel(2),
        undo: "activate again",
    },
    Control {
        id: "copy",
        label: "Add copy (Ctrl+D)",
        precondition: "selected model on unlocked plate",
        effect: Effect::Copy,
        undo: "Undo",
    },
    Control {
        id: "undo",
        label: "Undo (Ctrl+Z)",
        precondition: "copy edit in history",
        effect: Effect::Undo,
        undo: "Redo",
    },
    Control {
        id: "redo",
        label: "Redo (Ctrl+Y)",
        precondition: "undone copy edit",
        effect: Effect::Redo,
        undo: "Undo",
    },
    Control {
        id: "delete",
        label: "Delete selection (Del)",
        precondition: "selected copy on unlocked plate",
        effect: Effect::Delete,
        undo: "Undo",
    },
    Control {
        id: "fill",
        label: "Fill plate",
        precondition: "selected model on unlocked plate",
        effect: Effect::Grows,
        undo: "Undo",
    },
    Control {
        id: "remove-copy",
        label: "Remove copy (Delete)",
        precondition: "selected copy on unlocked plate",
        effect: Effect::Shrinks,
        undo: "Undo",
    },
    Control {
        id: "visibility",
        label: "Visibility",
        precondition: "object listed",
        effect: Effect::Visibility,
        undo: "activate again",
    },
    Control {
        id: "row-delete",
        label: "Delete (Del)",
        precondition: "selected copy in the object list",
        effect: Effect::Shrinks,
        undo: "Undo",
    },
    Control {
        // Sleptic: the top-right button toggles the plate tabs viewer (it used
        // to add a plate; the plate's own + tile does that).
        id: "plate-tabs",
        label: "Hide plate tabs",
        precondition: "loaded document",
        effect: Effect::PlateTabs,
        undo: "activate again",
    },
];

#[test]
fn r25_real_window_action_audit() {
    i_slint_backend_testing::init_no_event_loop();
    for theme in [ThemePreference::Light, ThemePreference::Dark] {
        for (width, height) in [(1280., 800.), (1920., 1080.)] {
            for plates in [1, 3] {
                let ui = MainWindow::new().unwrap();
                ui.window().set_size(slint::LogicalSize::new(width, height));
                ui.set_theme(theme);
                let doc = scene_document::SceneDocument::new();
                let ctrl = Rc::new(viewport::ViewController::new(
                    &ui,
                    doc.clone(),
                    Rc::new(pending_load::WaitingFile::new()),
                    mesh_render::DeviceLoss::default(),
                ));
                ctrl.listen_to_document();
                // The testing renderer has no GPU notification hook. All callback wiring
                // is installed before that optional hook is attempted.
                let _ =
                    wiring::connect_main_window_actions(&ui, ctrl.clone(), PreferenceHub::new(preferences()));
                // The application owns the clipboard; exercise its production
                // capture and placement primitives without an OS clipboard.
                {
                    let ctrl = ctrl.clone();
                    ui.on_duplicate_requested(move || {
                        if let Some(fragment) =
                            clipboard::copy_of_selection(&ctrl.scene_doc())
                        {
                            ctrl.duplicate_clipboard(&clipboard::Clipboard::new(fragment));
                        }
                    });
                }
                let graph = sc_scene_edit::SceneFragment::from_leaf_snapshot(
                    std::sync::Arc::new(sc_mesh_fixtures::unit_cube_mesh()),
                    sc_mesh_core::transform::Transform3D::Identity,
                )
                .into_scene();
                doc.take_mesh_scene(graph, "audit.stl".into());
                ctrl.switch_section(WorkspaceSection::Transform);
                for _ in 1..plates {
                    doc.add_plate();
                }
                doc.set_active_plate(1);
                ctrl.on_select_by_index(0);
                ui.show().unwrap();
                assert_eq!(
                    ElementHandle::find_by_accessible_label(&ui, "Select plate 1").count(),
                    usize::from(plates > 1)
                );
                for control in CONTROLS {
                    // Each control's precondition: a selected copy to delete or
                    // remove, the original selected before Fill plate.
                    match control.effect {
                        Effect::Delete => ctrl.on_select_by_index(1),
                        Effect::Shrinks => {
                            // A fresh copy to remove (Fill plate's are undone).
                            ctrl.on_select_by_index(0);
                            find_tool(&ui, "Add copy (Ctrl+D)").invoke_accessible_default_action();
                            ctrl.on_select_by_index(1);
                        }
                        Effect::Grows => ctrl.on_select_by_index(0),
                        _ => {}
                    }
                    // The first "Delete (Del)" found is row 0's; it acts on its
                    // own row, so that row must be the selected one.
                    if control.id == "row-delete" {
                        ctrl.on_select_by_index(0);
                    }
                    assert!(!control.precondition.is_empty() && !control.undo.is_empty());
                    let element = find_tool(&ui, control.label);
                    assert_ne!(
                        element.accessible_enabled(),
                        Some(false),
                        "{}: {}",
                        control.id,
                        control.precondition
                    );
                    let roots = doc.read_scene(|s| s.graph().roots().len()).unwrap();
                    let roots_before_action = roots;
                    let visible = {
                        use slint::Model;
                        ui.get_object_visibility().row_data(0)
                    };
                    element.invoke_accessible_default_action();
                    match control.effect {
                        Effect::Transform(mode) => {
                            assert_eq!(ui.get_active_tool(), mode, "{}", control.id)
                        }
                        Effect::Panel(flag) => {
                            assert!(
                                match flag {
                                    0 => ui.get_supports_tool_active(),
                                    1 => ui.get_hollow_tool_active(),
                                    _ => ui.get_model_tools_active(),
                                },
                                "{}",
                                control.id
                            );
                            find(&ui, control.label).invoke_accessible_default_action();
                        }
                        Effect::Copy | Effect::Redo => assert_eq!(
                            doc.read_scene(|s| s.graph().roots().len()),
                            Some(2),
                            "{}",
                            control.id
                        ),
                        Effect::Undo | Effect::Delete => assert_eq!(
                            doc.read_scene(|s| s.graph().roots().len()),
                            Some(1),
                            "{} ({theme:?}, {width} px, {plates} plates)",
                            control.id
                        ),
                        Effect::Grows => {
                            assert!(
                                doc.read_scene(|s| s.graph().roots().len()).unwrap() > roots,
                                "{}",
                                control.id
                            );
                            // A 1 mm cube fills the plate with ~1,400 copies; undo
                            // so later steps (and the run time) stay small.
                            doc.step_back().unwrap();
                            assert_eq!(
                                doc.read_scene(|s| s.graph().roots().len()),
                                Some(roots_before_action)
                            );
                        }
                        Effect::Shrinks => assert_eq!(
                            doc.read_scene(|s| s.graph().roots().len()),
                            Some(roots - 1),
                            "{}",
                            control.id
                        ),
                        Effect::Visibility => {
                            use slint::Model;
                            assert_ne!(
                                ui.get_object_visibility().row_data(0),
                                visible,
                                "{}",
                                control.id
                            );
                            find(&ui, control.label).invoke_accessible_default_action();
                        }
                        Effect::PlateTabs => {
                            assert!(!ui.get_plate_tabs_visible(), "{}", control.id);
                            find(&ui, "Show plate tabs").invoke_accessible_default_action();
                            assert!(ui.get_plate_tabs_visible(), "{}", control.id);
                            // Leave a selected model for the transform checks below.
                            doc.set_active_plate(1);
                            ctrl.on_select_by_index(0);
                        }
                    }
                }
                // Clicking the active tool puts it away (no tool), so only
                // pick Move when it is not already the tool.
                if ui.get_active_tool() != 0 {
                    find(&ui, "Move (T)").invoke_accessible_default_action();
                }
                assert_eq!(ui.get_active_tool(), 0);
                let revision = doc.revision();
                let position = find(&ui, "Position X (mm)");
                position.mock_single_click(slint::platform::PointerEventButton::Left);
                position.set_accessible_value("7");
                ui.window()
                    .dispatch_event(slint::platform::WindowEvent::KeyPressed {
                        text: slint::platform::Key::Return.into(),
                    });
                assert_ne!(
                    doc.revision(),
                    revision,
                    "Position field did not commit to the real document"
                );
                let icons = ElementQuery::from_root(&ui)
                    .match_descendants()
                    .match_inherits("IconTool")
                    .find_all();
                assert!(!icons.is_empty());
                let unlisted: BTreeSet<String> = icons
                    .iter()
                    .map(|icon| icon.accessible_label().unwrap_or_default().to_string())
                    .filter(|label| {
                        !CONTROLS.iter().any(|c| c.label == label)
                            && !NOT_CLICKED.iter().any(|(l, _)| l == label)
                    })
                    .collect();
                assert!(
                    unlisted.is_empty(),
                    "icon buttons neither audited nor listed in NOT_CLICKED: {unlisted:?}"
                );
                for icon in &icons {
                    assert!(icon.accessible_label().is_some_and(|s| !s.is_empty()));
                    assert!(
                        icon.size().width >= 24.,
                        "icon shrank: {:?}",
                        icon.accessible_label()
                    );
                }
                // Sleptic: Backspace deletes the selected model, like Delete.
                // A click gives the 3D view keyboard focus (and picks, so
                // select the copy again afterwards).
                ctrl.on_select_by_index(0);
                find_tool(&ui, "Add copy (Ctrl+D)").invoke_accessible_default_action();
                let roots = doc.read_scene(|s| s.graph().roots().len()).unwrap();
                ElementQuery::from_root(&ui)
                    .match_descendants()
                    .match_inherits("MeshViewer")
                    .find_first()
                    .expect("the 3D view")
                    .mock_single_click(slint::platform::PointerEventButton::Left);
                ctrl.on_select_by_index(1);
                ui.window()
                    .dispatch_event(slint::platform::WindowEvent::KeyPressed {
                        text: slint::platform::Key::Backspace.into(),
                    });
                assert_eq!(
                    doc.read_scene(|s| s.graph().roots().len()),
                    Some(roots - 1),
                    "Backspace did not delete the selected model"
                );
                // Sleptic: select, pick Rotate, click off, select again: the
                // last tool reopened. A selection starts with no tool.
                ctrl.on_select_by_index(0);
                find_tool(&ui, "Rotate (R)").invoke_accessible_default_action();
                assert_eq!(ui.get_active_tool(), 1);
                doc.choose(selection::Selection::None);
                ctrl.on_select_by_index(0);
                assert_eq!(ui.get_active_tool(), -1, "reselecting reopened the last tool");
                assert_eq!(ElementHandle::find_by_accessible_label(&ui, "Rotation X").count(), 0);
                find(&ui, "More tools").invoke_accessible_default_action();
                // The overflow menu's rows are held to the same rule as the
                // toolbar icons, and must not repeat a tool the bar shows.
                let rows = ElementQuery::from_root(&ui)
                    .match_descendants()
                    .match_inherits("MenuRow")
                    .find_all();
                assert!(rows.len() >= 9, "More tools menu has {} rows", rows.len());
                for row in &rows {
                    let label = row.accessible_label().unwrap_or_default().to_string();
                    assert!(
                        CONTROLS.iter().any(|c| c.label == label)
                            || NOT_CLICKED.iter().any(|(l, _)| *l == label),
                        "More tools row {label:?} is neither audited nor listed in NOT_CLICKED"
                    );
                    assert!(
                        icons.iter().all(|icon| icon.accessible_label().as_deref() != Some(label.as_str())),
                        "More tools repeats the toolbar's {label:?}"
                    );
                }
                find(&ui, "Measure").invoke_accessible_default_action();
                assert!(ui.get_measure_tool_active());
                ui.set_measure_tool_active(false);
                ctrl.switch_section(WorkspaceSection::Slice);
                ui.set_print_risks(
                    Rc::new(slint::VecModel::from(vec![RiskRow {
                        title: "Layers 2–4 · 1 × 2 mm".into(),
                        group: "Unsupported islands (1)".into(),
                        kind: 0,
                        layer: 2,
                        ..Default::default()
                    }]))
                    .into(),
                );
                assert_eq!(
                    ElementHandle::find_by_accessible_label(&ui, "Layers 2–4 · 1 × 2 mm").count(),
                    0
                );
                find(&ui, "Unsupported islands (1)").invoke_accessible_default_action();
                assert_eq!(ui.get_expanded_risk_kind(), 0);
                find(&ui, "Layers 2–4 · 1 × 2 mm").invoke_accessible_default_action();
                assert_eq!(ui.get_selected_risk(), 0);
                assert_eq!(ui.get_layer_shown(), 2.);
                assert_eq!(find(&ui, "Export").accessible_enabled(), Some(false));
                ui.hide().unwrap();
            }
        }
    }
}

/// Plate tiles are drawings, not touch areas: a drag that starts on one
/// stays a camera drag (a tile TouchArea used to grab the press and stop
/// a right-drag pan), and only a left click that does not move runs it.
#[test]
fn r26_plate_tiles_leave_drags_to_the_viewport() {
    use crate::viewport::ScreenPoint;
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    let doc = scene_document::SceneDocument::new();
    let ctrl = Rc::new(viewport::ViewController::new(
        &ui,
        doc.clone(),
        Rc::new(pending_load::WaitingFile::new()),
        mesh_render::DeviceLoss::default(),
    ));
    let _ = wiring::connect_main_window_actions(&ui, ctrl.clone(), PreferenceHub::new(preferences()));
    doc.take_mesh_scene(sc_scene::Scene::new(), "tiles.3mf".into());
    ctrl.switch_section(WorkspaceSection::Transform);
    // One "Add plate" tile (slot 6) covering 100..120 px.
    ui.set_plate_overlays(
        Rc::new(slint::VecModel::from(vec![PlateOverlay {
            id: 1,
            index: 1,
            slot: 6,
            x: 100.,
            y: 100.,
            w: 20.,
            h: 20.,
            ..Default::default()
        }]))
        .into(),
    );
    ctrl.set_plate_tile_hits_for_test(vec![
        [(100., 100.), (120., 100.), (120., 120.), (100., 120.)].map(|(x, y)| glam::Vec2::new(x, y)),
    ]);
    let plates = || doc.read_scene(|s| s.graph().plates().order.len()).unwrap();
    let at = |x, y| ScreenPoint::new(x, y);
    let none = PointerModifiers::default;

    ctrl.on_pointer_move(at(110., 110.), none());
    assert_eq!(ui.get_hovered_plate_tile(), 0, "hover over the tile");
    ctrl.on_pointer_move(at(200., 200.), none());
    assert_eq!(ui.get_hovered_plate_tile(), -1, "hover cleared off the tile");

    // A drag that starts on the tile is a camera drag, not a tile click.
    ctrl.on_pointer_down(at(110., 110.), none());
    ctrl.on_pointer_up(at(180., 160.), none());
    assert_eq!(plates(), 1);
    // A right press/release on the tile never reaches the tile.
    ctrl.right_down(110., 110.);
    assert_eq!(plates(), 1);
    // A left click on it runs it.
    ctrl.on_pointer_down(at(110., 110.), none());
    ctrl.on_pointer_up(at(111., 110.), none());
    assert_eq!(plates(), 2);

    let source = include_str!("../ui/main-window.slint");
    let tiles = source
        .split("for tile[ti] in root.plate-overlays")
        .nth(1)
        .unwrap()
        .split("if root.section != WorkspaceSection.Slice || root.preview-3d")
        .next()
        .unwrap();
    assert!(!tiles.contains("TouchArea {"), "a tile TouchArea would grab drags again");
}

/// The Project tab counts each plate's models from per-leaf plate
/// membership (a root-based count put every model on plate 1).
#[test]
fn r27_project_tab_counts_models_on_their_own_plates() {
    use slint::Model as _;
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    let doc = scene_document::SceneDocument::new();
    let ctrl = Rc::new(viewport::ViewController::new(
        &ui,
        doc.clone(),
        Rc::new(pending_load::WaitingFile::new()),
        mesh_render::DeviceLoss::default(),
    ));
    let _ = wiring::connect_main_window_actions(&ui, ctrl.clone(), PreferenceHub::new(preferences()));
    let graph = sc_scene_edit::SceneFragment::from_leaf_snapshot(
        std::sync::Arc::new(sc_mesh_fixtures::unit_cube_mesh()),
        sc_mesh_core::transform::Transform3D::Identity,
    )
    .into_scene();
    doc.take_mesh_scene(graph, "plates.3mf".into());
    doc.add_plate();
    let second = doc.active_plate();
    ctrl.on_select_by_index(0);
    doc.move_selection_to_plate(second);
    ui.set_hub_page(3);
    ui.invoke_hub_page_opened(3);
    let objects: Vec<i32> = ui.get_project_plates().iter().map(|r| r.objects).collect();
    assert_eq!(objects, vec![0, 1]);
    assert_eq!(ui.get_project_name().as_str(), "plates");
}

#[test]
fn r25_more_tools_capture_mode_is_supported() {
    let options = launch_options::LaunchOptions::parse(
        ["--verify-input", "popup-more"].map(std::ffi::OsString::from),
    )
    .unwrap();
    assert_eq!(options.verify_input.as_deref(), Some("popup-more"));
}

#[test]
fn r25_plate_name_survives_process_edit_and_undo() {
    let doc = scene_document::SceneDocument::new();
    doc.take_mesh_scene(sc_scene::Scene::new(), "plate.3mf".into());
    doc.rename_plate(" Production A ");
    assert_eq!(doc.plate_name().as_deref(), Some("Production A"));
    doc.process_changed();
    assert_eq!(doc.plate_name().as_deref(), Some("Production A"));
    doc.step_back().unwrap();
    assert_eq!(doc.plate_name(), None);
    doc.step_forward().unwrap();
    assert_eq!(doc.plate_name().as_deref(), Some("Production A"));
    doc.add_plate();
    assert_eq!(doc.plate_name(), None);
}

// R30 (M9 audit): Preview layers can be stepped by buttons, the mouse wheel
// over the slider and a typed "Go to" layer, all through one clamped helper.
#[test]
fn r30_preview_layer_stepping_controls() {
    let ui = include_str!("../ui/main-window.slint");
    assert!(ui.contains("function step-layer(delta: int)"));
    assert!(ui.contains("LayerStepButton { x: 6px; y: -58px; up: true; clicked => { root.step-layer(1); } }"));
    assert!(ui.contains("LayerStepButton { x: 6px; y: parent.height + 30px; up: false; clicked => { root.step-layer(-1); } }"));
    assert!(ui.contains("root.step-layer((e.delta-y > 0 ? 1 : -1) * (e.modifiers.shift ? 10 : 1));"));
    assert!(ui.contains("accessible-label: @tr(\"Go to layer\")"));
}

// R32 sweep: opening a mistyped path showed it under "Recently opened" as
// "File not found". Only files that exist are remembered.
#[test]
fn r32_missing_files_are_not_added_to_recent() {
    let bus = Rc::new(PreferenceHub::new(preferences()));
    let missing = std::env::temp_dir().join("sleptic-r32-does-not-exist.3mf");
    wiring::remember_recent_file(&bus, &missing);
    assert!(bus.current_values().recent_files.is_empty());
    let real = std::env::temp_dir().join("sleptic-r32-recent.stl");
    std::fs::write(&real, b"solid x\nendsolid x\n").unwrap();
    wiring::remember_recent_file(&bus, &real);
    assert_eq!(bus.current_values().recent_files, vec![real.clone()]);
    let _ = std::fs::remove_file(real);
}

// R32 sweep: Preview opened on the last layer, a near-empty sliver at the top
// of the model (lit area 0.00 mm²), so the 2D view looked blank.
#[test]
fn r32_preview_opens_on_the_top_layer_with_content() {
    use crate::slice_run::top_layer_with_content;
    assert_eq!(top_layer_with_content(&[0.2, 1.0, 0.4, 0.0], 4), 2);
    assert_eq!(top_layer_with_content(&[0.2, 1.0, 0.4], 3), 2);
    assert_eq!(top_layer_with_content(&[], 5), 4);
    assert_eq!(top_layer_with_content(&[0.0, 0.0], 2), 1);
}

// R34: the wider R28 sidebar pushed the add-plate "+" over the end of the
// toolbar at 1280 px; narrow windows keep the 340 px sidebar.
#[test]
fn r34_sidebar_narrows_on_small_windows() {
    let ui = include_str!("../ui/main-window.slint");
    assert!(ui.contains("in-out property <length> settings-width: root.width < 1500px ? 340px : 372px;"));
}

// R39 (Sleptic): users should not tune auto-orient weights; the popover keeps
// only the supports switch and the action.
#[test]
fn r39_orient_popover_has_no_weight_controls() {
    let ui = include_str!("../ui/main-window.slint");
    for gone in ["Advanced weights", "Zero-support head start", "\"Most per plate\""] {
        assert!(!ui.contains(gone), "{gone}");
    }
    assert!(ui.contains("Plan for automatic supports"));
}

// R40 (Sleptic): the white build-volume lines go (plate only, like Orca / Bambu),
// and the view cube shades its faces and highlights the one under the pointer.
#[test]
fn r40_no_build_volume_cage_and_a_hovering_view_cube() {
    let render = include_str!("mesh_render/mod.rs");
    assert!(!render.contains("pass.draw_indexed(0..24, 0, 0..1);"));
    let ui = include_str!("../ui/main-window.slint");
    // R44: hover draws face / edge / corner patches over the shaded faces.
    assert!(ui.contains("commands: root.cube-highlight; fill: #5b9befa0;"));
    assert!(ui.contains("root.cube-hovered(self.mouse-x / 1px, self.mouse-y / 1px);"));
}

// R42 (Sleptic: match Orca / Bambu): sidebar settings are compact one-line rows
// with the unit inside one value box, no stepper buttons; changed values show
// an accent label and a reset arrow.
#[test]
fn r42_sidebar_rows_are_compact_orca_style_value_boxes() {
    let ui = include_str!("../ui/main-window.slint");
    assert!(ui.contains("component ValueBox inherits Rectangle {"));
    assert!(!ui.contains("chevron-up.svg\"); width: 12px; height: 12px;"), "stepper buttons are gone");
    assert!(ui.contains("color: root.setting.modified ? ShellTheme.accent : ShellTheme.text;"));
    assert!(ui.contains("if e.text == Key.UpArrow { root.step(1); return accept; }"));
}

// R43 (Sleptic's reference image): Fusion-style view cube, axes along its edges,
// a home button (front-top-right view) and a view menu arrow; the separate
// axis tripod and the round view-menu / fit buttons are gone.
#[test]
fn r43_view_cube_has_home_and_menu_only() {
    let ui = include_str!("../ui/main-window.slint");
    assert!(ui.contains("accessible-label: @tr(\"Home view\");"));
    assert!(ui.contains("home-touch := TouchArea { mouse-cursor: pointer; clicked => { root.navigation-view(0); } }"));
    assert!(!ui.contains("Image { source: @image-url(\"icons/view-menu.svg\"); x: 6px; y: 6px;"));
    assert!(!ui.contains("Image { source: @image-url(\"icons/fit.svg\"); x: 6px; y: 6px;"));
    assert!(ui.contains("MenuRow { label: @tr(\"Zoom to fit (F)\");"));
}

// R45 / R46 (Sleptic): the cured layer is the UV light's violet in the 2D and
// 3D views, deeper the longer that layer is exposed (bottom layers darkest).
#[test]
fn r45_cured_layer_is_uv_pink_and_deepens_with_exposure() {
    use crate::slice_cap::cure_shade;
    assert_eq!(cure_shade(2.5, 2.5, 30.0), 0.0);
    assert_eq!(cure_shade(30.0, 2.5, 30.0), 1.0);
    assert!((cure_shade(16.25, 2.5, 30.0) - 0.5).abs() < 1e-6);
    assert_eq!(cure_shade(5.0, 2.5, 2.5), 0.0, "no bottom boost: light pink");
    let ui = include_str!("../ui/main-window.slint");
    assert!(ui.contains("property <color> cure: #4b0082.mix(#a259ff, root.layer-cure-shade);"));
    // R47: glow: LCD-black backdrop, halo strokes, lit edge; 3D halo blends.
    assert!(ui.contains("background: #07060c;"));
    assert!(ui.contains("stroke: #b07bff.with-alpha(halo.a);"));
    // Sleptic: no white outline through the part; the glow stays violet.
    assert!(!ui.contains("#e6d6ff"));
    assert!(!include_str!("mesh_render/shaders/cap.wgsl").contains("0.93, 0.85, 1.0"));
    assert!(include_str!("mesh_render/pipelines/cap.rs").contains("blend: Some(wgpu::BlendState::ALPHA_BLENDING),"));
    let shader = include_str!("mesh_render/shaders/cap.wgsl");
    assert!(shader.contains("mix(UV_BRIGHT, UV_DARK, clamp(scene.extra.w, 0.0, 1.0))"));
}

// R48 (Sleptic, Chitubox / Lychee): first launch walks through look, printer and
// resin; Help > Setup Wizard reopens it; Skip still marks setup as done.
#[test]
fn r48_setup_wizard_is_wired_and_reachable_from_help() {
    let ui = include_str!("../ui/main-window.slint");
    assert!(ui.contains("if root.setup-visible: SetupWizard {"));
    assert_eq!(ui.matches("title: @tr(\"MenuBar\" => \"Setup Wizard...\");").count(), 2);
    let wizard = include_str!("../ui/setup-wizard.slint");
    for step in ["Welcome to Sleptic Slicer", "Which printer do you use?", "Which resin are you printing with?", "You're all set"] {
        assert!(wizard.contains(step), "{step}");
    }
    let app = include_str!("app_state.rs");
    // Asked only on launches nobody automates.
    let launch = include_str!("lib.rs");
    let manual = &launch[launch.find("if automated {").expect("the automated branch")..];
    let manual = &manual[manual.find("} else {").expect("the manual branch")..];
    assert!(manual.contains("if app.needs_setup() {"), "setup is asked on manual launches");
    assert!(app.contains("snap.setup_complete = true;"));
}

// Sleptic's screenshot: on a model's top layer the glowing cut face z-fought the
// model's own top face (radial streaks). The cap pipeline is depth-biased
// toward the camera and passes on equal depth.
#[test]
fn r48_cap_wins_over_a_coincident_top_face() {
    let cap = include_str!("mesh_render/pipelines/cap.rs");
    assert!(cap.contains("depth_compare: Some(wgpu::CompareFunction::LessEqual),"));
    assert!(cap.contains("constant: -8,"));
}

/// Sleptic (R56): "do a whole long check to scan for these kind of things".
/// Findings fixed from the dialog scan, kept from coming back.
/// Sleptic: "when selecting things or clicking off or selecting features the
/// screen flashes" (his recorder saw it as a window switch). On Windows
/// Slint rebuilds the native menu bar whenever a property it reads
/// changes, hiding the window for the rebuild (WM_SETREDRAW). The menu
/// bar's bindings must not read state that changes in ordinary use.
#[test]
fn r59_menu_bar_reads_no_state_that_changes_while_editing() {
    let main = include_str!("../ui/main-window.slint");
    let start = main.find("    MenuBar {").expect("menu bar");
    let mut depth = 0;
    let mut end = start;
    for (i, c) in main[start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i;
                    break;
                }
            }
            _ => {}
        }
    }
    let volatile = [
        "chosen-part-row",
        "undo-available",
        "redo-available",
        "root.section",
        "root.slicing",
        "root.saving",
        "making-supports",
        "root.repairing",
        "slice-export-ready",
        "measure-tool-active",
        "unsaved-changes",
        // The menu commands' conditions (R182) change as one edits too.
        "may-undo",
        "may-redo",
        "may-edit-selection",
    ];
    // Handlers run only when an item is picked; the rebuild tracks the
    // bindings (title / enabled / checked / visible) alone.
    for line in main[start..end].lines() {
        let binding = line.split("activated").next().unwrap_or(line);
        for name in volatile {
            assert!(!binding.contains(name), "menu bar binding reads {name}: {line}");
        }
    }
    // Sleptic: deleting all the text in the Rotate / Scale boxes left them
    // disabled for good. A field must never gate itself on its own text.
    assert!(!main.contains("enabled: self.text"));
    assert!(main.contains("enabled: root.object-rotation[i] != \"\""));
    assert!(main.contains("enabled: root.object-scale[i] != \"\""));
}

#[test]
fn r56_dialog_scan_findings_stay_fixed() {
    for (name, source) in [
        ("error", include_str!("../ui/error-dialog.slint")),
        ("info", include_str!("../ui/info-dialog.slint")),
    ] {
        // The triangle glyphs rendered as a missing-character box. The
        // details disclosure is shared from dialog-parts.slint (R112),
        // through the message body both dialogs use (R179).
        assert!(!source.contains("\"▸ \"") && !source.contains("\"▾ \""), "{name}");
        assert!(source.contains("MessageBody"), "{name}");
        // Extra window space no longer spreads the rows apart.
        assert!(source.contains("max-height: body.preferred-height;"), "{name}");
    }
    let body = include_str!("../ui/message-body.slint");
    assert!(body.contains("DetailsBox") && !body.contains("\"▸ \""));
    assert!(include_str!("../ui/dialog-parts.slint").contains("icons/chevron-right.svg"));
    let about = include_str!("../ui/about-window.slint");
    assert!(!about.contains("Licensed under AGPL-3.0-only"), "license stated once");
    for (name, source) in [
        ("shortcuts", include_str!("../ui/shortcuts-dialog.slint")),
        ("profile", include_str!("../ui/profile-view-dialog.slint")),
    ] {
        assert!(source.contains("padding-right: 16px;"), "{name}: clear of the scrollbar");
    }
    let main = include_str!("../ui/main-window.slint");
    let plate_menu = &main[main.find("plate-menu := PopupWindow").unwrap()..];
    let plate_menu = &plate_menu[..plate_menu.find("plate-menu-layout").unwrap() + 2000];
    assert!(plate_menu.contains("MenuRow") && plate_menu.contains("MenuSeparator"));
    assert!(!plate_menu.contains("Button { text: @tr(\"Add plate\")"));
    let editor = include_str!("../ui/printer-editor.slint");
    for section in ["Display (LCD)", "Build volume", "Motion", "Start G-code"] {
        assert!(editor.contains(section), "printer editor section {section}");
    }
}

// R95: the supports panel names the next step. Supports stay a toolbar tool,
// not a tab of their own (the maintainer tried a Supports tab and did not
// want it).
#[test]
fn r95_supports_panel_names_the_next_step_and_is_no_tab() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    ui.window().set_size(slint::LogicalSize::new(1440., 900.));
    ui.show().unwrap();
    let tabs: Vec<String> = ElementQuery::from_root(&ui)
        .match_descendants()
        .match_inherits("WorkTab")
        .find_all()
        .iter()
        .filter_map(|t| t.accessible_label().map(|l| l.to_string()))
        .collect();
    assert!(tabs.iter().any(|t| t == "Prepare"), "{tabs:?}");
    assert!(!tabs.iter().any(|t| t == "Supports"), "{tabs:?}");
    assert!(ElementHandle::find_by_accessible_label(&ui, "Next: slice and preview").count() == 0);
    ui.set_supports_tool_active(true);
    assert!(ElementHandle::find_by_accessible_label(&ui, "Next: slice and preview").count() > 0);
}

// R95: Simple mode shows only the everyday tools; Fill plate, copies, model
// tools and Measure wait in More.
#[test]
fn r95_simple_mode_toolbar_moves_the_rest_into_more() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    ui.window().set_size(slint::LogicalSize::new(1920., 1080.));
    ui.show().unwrap();
    let hidden = ["Fill plate", "Add copy (Ctrl+D)", "Measure", "Model tools (cut, split, boolean, label)"];
    let count = |label: &str| ElementHandle::find_by_accessible_label(&ui, label).count();
    for label in hidden {
        assert_eq!(count(label), 1, "{label} on the Advanced toolbar");
    }
    ui.set_settings_level(0);
    for label in hidden {
        assert_eq!(count(label), 0, "{label} still on the Simple toolbar");
    }
    assert!(count("Auto prepare") > 0);
    find(&ui, "More tools").invoke_accessible_default_action();
    for label in ["Fill plate", "Add copy (Ctrl+D)", "Measure"] {
        assert!(count(label) > 0, "{label} missing from More in Simple mode");
    }
}

// R95: Preview opens on the 3D view of the sliced plate; the top layer alone
// often looked nearly empty, as if slicing had failed.
#[test]
fn r95_preview_opens_in_the_3d_view() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    assert!(ui.get_preview_3d());
}

// R97/R98: the settings sidebar is laid out like Orca's: printer and resin
// pickers, then Process with one tab per kind of setting; support settings
// have their own tab, with more fields at Advanced than at Simple.
#[test]
fn r98_settings_sidebar_has_orca_style_tabs() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    ui.window().set_size(slint::LogicalSize::new(1440., 1400.));
    ui.show().unwrap();
    let count = |label: &str| ElementHandle::find_by_accessible_label(&ui, label).count();
    let tabs = ElementQuery::from_root(&ui)
        .match_descendants()
        .match_accessible_role(i_slint_backend_testing::AccessibleRole::Tab)
        .find_all();
    // The testing backend reports a tab's inner items with its role too: keep
    // the first element of each label, in order.
    let mut labels: Vec<String> = Vec::new();
    let mut tabs_by_label = Vec::new();
    for tab in &tabs {
        let label = tab.accessible_label().map(|l| l.to_string()).unwrap_or_default();
        if !labels.contains(&label) {
            labels.push(label);
            tabs_by_label.push(tab.clone());
        }
    }
    let tabs = tabs_by_label;
    assert_eq!(labels, ["Quality", "Exposure", "Lift", "Supports", "Others"]);
    assert!(count("Printer") > 0 && count("Resin") > 0, "printer and resin pickers");
    // R99: the material block reads "Resin" and both pickers have a picture.
    assert_eq!(count("Material"), 0);
    assert_eq!(
        ElementQuery::from_root(&ui)
            .match_descendants()
            .match_inherits("PictureTile")
            .find_all()
            .len()
            .min(2),
        2
    );
    assert_eq!(ui.get_settings_tab(), 0);
    assert_eq!(count("Support preset"), 0, "Supports is not the open tab");
    tabs[3].invoke_accessible_default_action();
    assert_eq!(ui.get_settings_tab(), 3);
    assert!(count("Support preset") > 0);
    assert!(count("Support spacing") > 0);
    assert!(count("Tip length") > 0, "Advanced shows the tip fields");
    ui.set_settings_level(0);
    assert!(count("Support spacing") > 0);
    assert_eq!(count("Tip length"), 0, "Simple hides the tip fields");
    tabs[4].invoke_accessible_default_action();
    assert_eq!(count("Support spacing"), 0);
    assert!(count("Arrange gap") > 0, "Others holds the arrange gap, at every level");
}

// R101: each dropdown ends with "More printers..." / "More resins...", which
// opens a chooser over the whole catalog (with pictures and search); a click
// uses the printer and keeps it in the dropdown, the star only adds/removes.
#[test]
fn r101_more_rows_open_the_full_catalog_chooser() {
    use slint::Model;
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    ui.window().set_size(slint::LogicalSize::new(1440., 900.));
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/profiles");
    let catalog = Rc::new(std::cell::RefCell::new(Rc::new(
        profile_catalog::InstalledProfiles::scan_libraries(
            &sc_print_profiles::ProfileLibraries::new(None, vec![root]),
        ),
    )));
    let printers = catalog.borrow().printers().len();
    let bus = PreferenceHub::new(preferences());
    let doc = scene_document::SceneDocument::new();
    let _bridge =
        slice_panel::SliceProfileControls::attach_slice_profile_controls(&ui, catalog, bus.clone(), doc.clone());
    ui.show().unwrap();

    let options = ui.get_printer_choices();
    let more = options.row_count() - 1;
    assert_eq!(options.row_data(more).unwrap(), "More printers...");
    let before = ui.get_printer_choice();
    ui.invoke_printer_picked(more as i32);
    assert_eq!(ui.get_chooser_kind(), 1);
    let choices = |ui: &MainWindow| ui.get_chooser_rows().iter().filter(|row| row.heading.is_empty()).count();
    let first_choice = |ui: &MainWindow| ui.get_chooser_rows().iter().position(|row| row.heading.is_empty()).unwrap();
    let in_use = doc.chosen_printer();
    if in_use.is_some() {
        assert!(ui.get_chooser_brand() > 0, "it opens on the brand of the printer in use");
    }
    ui.invoke_chooser_brand_picked(0);
    assert_eq!(choices(&ui), printers, "every printer is listed under All");
    assert!(ui.get_chooser_rows().row_data(0).unwrap().heading.contains("Anycubic"), "grouped under brand headings");
    assert_eq!(ui.get_printer_choice(), before, "the More row is not a choice");

    // The brand list: "All", then each maker; picking one shows only its printers.
    let brands: Vec<String> = ui.get_chooser_brands().iter().map(|b| b.to_string()).collect();
    assert_eq!(brands[0], "All");
    let elegoo = brands.iter().position(|b| b.eq_ignore_ascii_case("Elegoo")).expect("Elegoo is a brand");
    ui.invoke_chooser_brand_picked(elegoo as i32);
    let headings: Vec<String> =
        ui.get_chooser_rows().iter().filter(|r| !r.heading.is_empty()).map(|r| r.heading.to_string()).collect();
    assert!(headings.contains(&"Mars".to_owned()) && headings.contains(&"Saturn".to_owned()), "{headings:?}");
    ui.invoke_chooser_picked(0);
    assert_eq!(ui.get_chooser_kind(), 1, "a heading is not a choice");
    ui.invoke_chooser_brand_picked(0);

    ui.set_chooser_search("saturn 4".into());
    ui.invoke_chooser_search_edited("saturn 4".into());
    let rows = ui.get_chooser_rows();
    assert!(choices(&ui) >= 2);
    for row in rows.iter().filter(|row| row.heading.is_empty()) {
        assert!(row.label.to_lowercase().contains("saturn 4"), "{} matched", row.label);
    }
    let at = first_choice(&ui);
    let first = rows.row_data(at).unwrap();
    assert!(first.picture.size().width > 0, "rows show the product picture");
    ui.invoke_chooser_picked(at as i32);
    assert_eq!(ui.get_chooser_kind(), 0, "picking closes the chooser");
    let picked = doc.chosen_printer().expect("a printer is selected");
    assert!(bus.current_values().favourite_printers.contains(&picked), "kept in the dropdown");
    let labels: Vec<String> = ui.get_printer_choices().iter().map(|s| s.to_string()).collect();
    assert!(labels.iter().any(|l| l.ends_with(first.label.as_str())), "{labels:?}");

    let more = ui.get_resin_choices().row_count() - 1;
    ui.invoke_resin_picked(more as i32);
    assert_eq!(ui.get_chooser_kind(), 2);
    let at = first_choice(&ui);
    assert!(at > 0, "resins are grouped too");
    assert!(!ui.get_chooser_rows().row_data(at).unwrap().in_list);
    ui.invoke_chooser_star(at as i32);
    assert!(ui.get_chooser_rows().row_data(at).unwrap().in_list, "the star adds it");
    assert_eq!(bus.current_values().favourite_resins.len(), 1);
    ui.invoke_chooser_star(at as i32);
    assert!(bus.current_values().favourite_resins.is_empty(), "and removes it again");
}

// R102: Home leads with the printer and resin the app is set up for (with
// "Change...") and the four steps to a print, not a sign-in that does not
// exist; Device leads with sending the print ("Slice now" when nothing is
// sliced) and folds the hardware specs and the advanced export options.
#[test]
fn r102_home_and_device_lead_with_what_a_newcomer_needs() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    ui.window().set_size(slint::LogicalSize::new(1440., 900.));
    ui.show().unwrap();
    let count = |label: &str| ElementHandle::find_by_accessible_label(&ui, label).count();

    ui.set_hub_page(1);
    assert_eq!(count("Sign in"), 0, "no sign-in for an account service that does not exist");
    assert!(count("Change Printer") > 0 && count("Change Resin") > 0);
    assert!(count("Getting started") > 0 && count("Auto prepare") > 0);
    let opened = Rc::new(std::cell::Cell::new(0));
    ui.on_chooser_open_requested({
        let opened = opened.clone();
        move |kind| opened.set(kind)
    });
    find(&ui, "Change Resin").invoke_accessible_default_action();
    assert_eq!(opened.get(), 2, "Change... opens the resin chooser");

    ui.set_hub_page(2);
    ui.set_scene_present(true);
    assert!(count("Slice now") > 0, "nothing sliced: the next step is offered");
    let sliced = Rc::new(std::cell::Cell::new(false));
    ui.on_start_slice({
        let sliced = sliced.clone();
        move || sliced.set(true)
    });
    find(&ui, "Slice now").invoke_accessible_default_action();
    assert!(sliced.get());
    assert_eq!(count("Build volume"), 0, "hardware details start folded");
    assert_eq!(count("After-export command"), 0, "advanced options start folded");
    find(&ui, "Hardware details").invoke_accessible_default_action();
    assert!(count("Build volume") > 0);
    find(&ui, "File format and automation").invoke_accessible_default_action();
    assert!(count("After-export command") > 0);
}

// R103: the Home action cards are at least as wide as their text (the
// "Open a file" subtitle ran past the card's right edge).
#[test]
fn r103_home_action_card_text_stays_inside_the_card() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    ui.window().set_size(slint::LogicalSize::new(1440., 900.));
    ui.show().unwrap();
    ui.set_hub_page(1);
    for (card, line) in [
        ("Open a file", "A model (STL, OBJ) or a project (3MF)"),
        ("New project", "Start with an empty plate"),
    ] {
        let card = ElementHandle::find_by_accessible_label(&ui, card)
            .find(|e| e.size().width > 200.)
            .expect("action card");
        let text = find(&ui, line);
        let card_right = card.absolute_position().x + card.size().width;
        let text_right = text.absolute_position().x + text.size().width;
        assert!(text_right <= card_right - 8., "{line}: text ends at {text_right}, card at {card_right}");
    }
}

// R105: the raft is one choice (none, solid, lattice, skate), not three
// checkboxes that could contradict each other; it stays in step with the
// settings both ways.
#[test]
fn r105_raft_type_is_one_choice() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    // `changed` handlers run on the next event-loop pass, as in the app.
    let state = |ui: &MainWindow| {
        slint::platform::update_timers_and_animations();
        (ui.get_support_raft(), ui.get_support_lattice_raft(), ui.get_support_skate_raft())
    };
    let kind = |ui: &MainWindow| {
        slint::platform::update_timers_and_animations();
        ui.get_support_raft_kind()
    };
    ui.set_support_raft_kind(2);
    assert_eq!(state(&ui), (true, true, false), "lattice");
    ui.set_support_raft_kind(3);
    assert_eq!(state(&ui), (true, false, true), "skate turns lattice off");
    ui.set_support_raft_kind(0);
    assert_eq!(state(&ui), (false, false, false), "no raft");
    ui.set_support_raft_kind(1);
    assert_eq!(state(&ui), (true, false, false), "solid");
    // A preset or saved settings change the flags: the choice follows.
    ui.set_support_lattice_raft(true);
    assert_eq!(kind(&ui), 2);
    ui.set_support_raft(false);
    assert_eq!(kind(&ui), 0);
    assert_eq!(
        ElementHandle::find_by_accessible_label(&ui, "Lattice raft").count(),
        0,
        "no separate lattice checkbox"
    );
}

// R106: Auto prepare's tuned support settings live inside the app: the
// user's settings are never written, and the tuned set is used once.
#[test]
fn r106_auto_prepare_tuning_never_touches_user_settings() {
    let source = include_str!("viewport/controller.rs");
    let start = source.find("fn pick_support_settings").unwrap();
    let body = &source[start..start + source[start..].find("\n    }\n").unwrap()];
    for setter in ["set_support_", "invoke_support_preset_selected"] {
        assert!(!body.contains(setter), "Auto prepare writes a user setting: {setter}");
    }
    // Compared without whitespace, so formatting that wraps the call
    // across lines does not fail it.
    let compact: String = body.split_whitespace().collect();
    assert!(compact.contains("auto_support_settings.replace"));
    let run = include_str!("auto_support_run.rs");
    assert!(run.contains("auto_support_settings.borrow_mut().take()"), "used once, then the user's again");
}

// R109: the right-hand panel slides away and comes back, and the 3D view
// takes its place.
#[test]
fn r109_object_panel_slides_out_and_back() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    ui.window().set_size(slint::LogicalSize::new(1440., 900.));
    ui.show().unwrap();
    let count = |label: &str| ElementHandle::find_by_accessible_label(&ui, label).count();
    assert!(count("Hide object panel (Ctrl+L)") > 0);
    assert_eq!(count("Show object panel (Ctrl+L)"), 0);
    find(&ui, "Hide object panel (Ctrl+L)").invoke_accessible_default_action();
    assert!(!ui.get_parts_panel_shown());
    assert!(count("Show object panel (Ctrl+L)") > 0, "a tab brings it back");
    find(&ui, "Show object panel (Ctrl+L)").invoke_accessible_default_action();
    assert!(ui.get_parts_panel_shown());
    // The view follows the panel's width rather than a fixed 284 px.
    let source = include_str!("../ui/main-window.slint");
    assert!(!source.contains("root.settings-width - 284px"));
}

// R110: the 2D layer view pans with the right (or middle) button, as the 3D
// view does; the left button no longer pans it.
#[test]
fn r110_layer_view_pans_like_the_3d_view() {
    let ui = include_str!("../ui/main-window.slint");
    let start = ui.find("preview-touch := TouchArea {").unwrap();
    let body = &ui[start..start + 1600];
    assert!(body.contains("e.button == PointerEventButton.right || e.button == PointerEventButton.middle"));
    assert!(!body.contains("if self.pressed"), "left-drag panning is gone");
    let viewer = include_str!("../ui/mesh-viewer.slint");
    assert!(viewer.contains("Right-drag pans"));
}

// R112 (the maintainer: the error pop-ups and the Preferences window looked
// unprofessional). Errors carry a red cross, information a blue "i"; the
// buttons sit in a footer band; technical details open in a box with a Copy
// button. Preferences has a headed, icon navigation and pages of cards whose
// rows explain each setting.
#[test]
fn r112_dialogs_and_preferences_look_finished() {
    let error = include_str!("../ui/error-dialog.slint");
    let info = include_str!("../ui/info-dialog.slint");
    assert!(error.contains("kind: 0;") && info.contains("kind: 2;"));
    // The badge sits in the message body both dialogs share (R179).
    let body = include_str!("../ui/message-body.slint");
    assert!(body.contains("DialogBadge") && body.contains("kind: root.badge-kind;"));
    for source in [error, info] {
        assert!(source.contains("DialogFooter") && source.contains("MessageBody"));
        assert!(source.contains("details-inline"));
        assert!(!source.contains("ShellTheme.warning"), "an error is not a warning");
    }
    let parts = include_str!("../ui/dialog-parts.slint");
    assert!(parts.contains("details-text.copy()") && parts.contains("read-only: true"));
    let window = include_str!("../ui/preferences/preferences-window.slint");
    assert!(window.contains("icons:") && window.contains("heading:"));
    let sidebar = include_str!("../ui/sidebar.slint");
    assert!(sidebar.contains("ShellTheme.accent") && !sidebar.contains("Palette.background.darker"));
    for page in [
        include_str!("../ui/preferences/appearance.slint"),
        include_str!("../ui/preferences/slicing.slint"),
    ] {
        assert!(page.contains("SettingsCard") && page.contains("description:"));
    }
    for page in [
        include_str!("../ui/preferences/printer.slint"),
        include_str!("../ui/preferences/resin.slint"),
        include_str!("../ui/preferences/appearance.slint"),
        include_str!("../ui/preferences/slicing.slint"),
    ] {
        assert!(page.contains("subtitle:"));
    }
}

/// R112: a reason in plain words is shown under the message; a dump of
/// technical output stays behind "Show technical details".
#[test]
fn r112_plain_details_show_and_technical_details_fold() {
    use crate::error_dialog::inline_details;
    assert!(inline_details("The selected folder is read-only."));
    assert!(inline_details("Benchy (2)
Bracket
Lid"));
    assert!(!inline_details(""));
    assert!(!inline_details("Io(Os { code: 32, kind: Uncategorized })"));
    assert!(!inline_details(r"could not open C:\Models\a.stl"));
    assert!(!inline_details("thread 'main' panicked at src/lib.rs:3"));
    assert!(!inline_details(&"word ".repeat(100)));
}

// R113 (the maintainer: the show-panel tab overlapped the plate tabs
// button; the delete tile should be dark red and obvious; selection turning a
// light model whiter was confusing; the plate should look professional).
#[test]
fn r113_plate_selection_and_corner_buttons() {
    let mesh = include_str!("mesh_render/shaders/mesh_render.wgsl");
    assert!(mesh.contains("SELECTION_TINT: vec3<f32> = vec3<f32>(0.26, 0.53, 0.95)"));
    assert!(mesh.contains("mix(own, mix(own, SELECTION_TINT, 0.42), f32(v.state.x))"));
    let outline = include_str!("mesh_render/shaders/outline.wgsl");
    assert!(outline.contains("const REACH: i32 = 2;"), "outline at least 4 px");
    let plate = include_str!("mesh_render/shaders/plate.wgsl");
    assert!(plate.contains("case TILE_DANGER: { return opaque(DELETE_RED); }") && plate.contains("BOLD_PITCH_MM: f32 = 50.0"));
    let main = include_str!("../ui/main-window.slint");
    // Both right-edge buttons share one column, one above the other.
    assert!(main.contains("x: root.width - 44px; y: 134px; width: 32px; height: 32px;"));
    assert!(main.contains("y: root.parts-panel-shown ? 134px : 174px;"));
    assert!(!main.contains("x: root.width - 30px; y: 76px"));
}

// R113: hiding the object panel flashed the 3D view (its size followed the
// panel's slide, so the GPU surface was resized every frame), and the plate
// list rail lay over the right-edge buttons.
#[test]
fn r113_panel_slide_does_not_resize_the_view_and_rail_clears_buttons() {
    let main = include_str!("../ui/main-window.slint");
    // Only the panel itself follows the animated width.
    assert_eq!(main.matches("root.right-panel-w").count(), 2);
    assert!(main.contains("root.right-panel-space + 140px); y: 134px;"));
}

// R115 (the maintainer: the grid lines should not change when zooming): the
// plate's grid lines have a fixed width in millimetres, with no screen-pixel
// widths and no zoom-dependent fade.
#[test]
fn r115_grid_lines_keep_their_size_on_the_plate() {
    let plate = include_str!("mesh_render/shaders/plate.wgsl");
    assert!(plate.contains("const FINE_WIDTH_MM: f32 = 0.3;"));
    assert!(plate.contains("const BOLD_WIDTH_MM: f32 = 0.6;"));
    assert!(!plate.contains("GRID_LINE_PX") && !plate.contains("let crowd"));
}

// R118 (the maintainer filmed the flash on hiding the object panel: the
// whole window black, the menu bar drawn at several heights). The View menu's
// toggle titles flipped with the state, and any change to a menu title
// rebuilds the native Windows menu. Only the menu-bar item may still flip
// (hiding the bar changes the window frame anyway).
#[test]
fn r118_menu_titles_do_not_follow_toggles() {
    let main = include_str!("../ui/main-window.slint");
    let start = main.find("    MenuBar {").unwrap();
    let menu = &main[start..start + main[start..].find("\n    }\n").unwrap()];
    let flipping: Vec<&str> = menu
        .lines()
        .filter(|l| l.contains("title: root.") && !l.contains("root.menu-style"))
        .collect();
    assert!(flipping.is_empty(), "{flipping:?}");
    assert!(!menu.contains("root.parts-panel-shown\n                    ?"));
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! What the controller tells the window when a scene opens or closes, and
//! the keyboard gizmo's section gate. Opening or closing must leave the
//! window with nothing selected, the arrow keys off the gizmo, and history
//! and title taken from the document (not from whatever was shown before).

use std::rc::Rc;
use std::sync::Arc;

use sc_mesh_core::transform::Transform3D;
use sc_scene::{LeafPayload, Scene};

use super::*;
use crate::pending_load::WaitingFile;

fn cube_scene() -> Scene {
    let mut scene = Scene::new();
    let root = scene.add_group(None, Transform3D::Identity);
    scene.add_root(root).expect("root group");
    let cube = LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh()));
    let leaf = scene.add_leaf(None, Transform3D::Identity, cube).expect("cube leaf");
    scene.add_child(root, leaf).expect("cube under the root");
    scene
}

/// A controller drawing into a real (headless) window and listening to
/// its document, as the app wires it.
fn windowed() -> (MainWindow, Rc<ViewController>) {
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().expect("window");
    let ctrl = Rc::new(ViewController::new(
        &ui,
        SceneDocument::new(),
        Rc::new(WaitingFile::new()),
        DeviceLoss::default(),
    ));
    ctrl.listen_to_document();
    (ui, ctrl)
}

/// Leftovers from an earlier scene that a fresh state must replace.
fn show_stale_state(ui: &MainWindow) {
    ui.set_chosen_part_row(3);
    ui.set_keys_drive_gizmo(true);
    ui.set_undo_available(true);
    ui.set_redo_available(true);
    ui.set_project_title("earlier.stl".into());
}

#[test]
fn opening_a_scene_starts_the_window_afresh() {
    let (ui, ctrl) = windowed();
    show_stale_state(&ui);
    ctrl.put_graph_for_test(cube_scene());
    assert!(ui.get_scene_present());
    assert_eq!(ui.get_chosen_part_row(), -1, "nothing selected yet");
    assert!(!ui.get_keys_drive_gizmo());
    assert!(!ui.get_undo_available() && !ui.get_redo_available(), "a new scene has no history");
    assert_ne!(ui.get_project_title().as_str(), "earlier.stl");
}

#[test]
fn closing_the_scene_clears_the_window() {
    let (ui, ctrl) = windowed();
    ctrl.put_graph_for_test(cube_scene());
    show_stale_state(&ui);
    ctrl.close_scene();
    assert!(!ui.get_scene_present());
    assert_eq!(ui.get_chosen_part_row(), -1);
    assert!(!ui.get_keys_drive_gizmo());
    assert!(!ui.get_undo_available() && !ui.get_redo_available());
    assert_eq!(ui.get_project_title().as_str(), "", "no file, no name");
    assert_eq!(ui.get_object_model_count(), 0);
}

#[test]
fn the_arrow_keys_take_the_gizmo_only_where_the_layout_can_change() {
    let ctrl = ViewController::for_test();
    ctrl.put_graph_for_test(cube_scene());
    ctrl.on_select_by_index(0);
    ctrl.switch_section(WorkspaceSection::Slice);
    ctrl.take_keyboard_focus(KeyboardTransformFocus::MoveFocus);
    assert_eq!(ctrl.pointer.viewport_keyboard_motion().keyboard_transform_focus(), KeyboardTransformFocus::NoTransformFocus);
    // Back in Transform the same request is honoured.
    ctrl.switch_section(WorkspaceSection::Transform);
    ctrl.take_keyboard_focus(KeyboardTransformFocus::MoveFocus);
    assert_eq!(ctrl.pointer.viewport_keyboard_motion().keyboard_transform_focus(), KeyboardTransformFocus::MoveFocus);
}

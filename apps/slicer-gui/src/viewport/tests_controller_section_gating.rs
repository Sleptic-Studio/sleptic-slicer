// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! What each workspace section allows: delete and the keyboard gizmo only
//! in Transform, undo and redo everywhere but Slice, selection everywhere.
//! The pointer-drag gate needs a camera and gizmo geometry, so it is
//! tested with that scaffolding in `viewport_click_tests`.
use std::sync::Arc;

use sc_mesh_core::IndexedMesh;
use sc_mesh_core::primitives::Vertex;
use sc_mesh_core::transform::Transform3D;
use sc_scene::{LeafPayload, Scene};

use super::controller::ViewController;
use super::arrow_keys::KeyboardTransformFocus;
use crate::WorkspaceSection::{self, Prepare, Slice, Transform};

/// A controller with one part, selected, under a group root as a loaded
/// file has it, in `section`.
fn one_part_in(section: WorkspaceSection) -> ViewController {
    let mut scene = Scene::new();
    let root = scene.add_group(None, Transform3D::Identity);
    scene.add_root(root).unwrap();
    let mesh = IndexedMesh {
        meta: None,
        vertices: vec![Vertex::new([0.0; 3])],
        faces: vec![],
    };
    let part = scene
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(mesh)),
        )
        .unwrap();
    scene.add_child(root, part).unwrap();
    let ctrl = ViewController::for_test();
    ctrl.put_graph_for_test(scene);
    ctrl.on_select_by_index(0);
    ctrl.switch_section(section);
    ctrl
}

#[test]
fn delete_works_only_in_transform() {
    for (section, parts_left) in [(Prepare, 1), (Slice, 1), (Transform, 0)] {
        let ctrl = one_part_in(section);
        ctrl.delete_selected();
        assert_eq!(
            ctrl.leaf_total_for_test(),
            parts_left,
            "{section:?}"
        );
    }
}

#[test]
fn the_keyboard_gizmo_arms_only_in_transform_and_disarms_on_leaving() {
    let ctrl = one_part_in(Slice);
    ctrl.keys_move();
    assert!(matches!(ctrl.key_focus_for_test(), KeyboardTransformFocus::NoTransformFocus));
    let ctrl = one_part_in(Transform);
    ctrl.keys_move();
    assert!(matches!(
        ctrl.key_focus_for_test(),
        KeyboardTransformFocus::MoveFocus
    ));
    ctrl.keys_rotate();
    assert!(matches!(ctrl.key_focus_for_test(), KeyboardTransformFocus::RotationFocus));
    ctrl.switch_section(Prepare);
    assert!(matches!(ctrl.key_focus_for_test(), KeyboardTransformFocus::NoTransformFocus));
}

#[test]
fn undo_and_redo_work_everywhere_but_slice() {
    for (section, history_works) in [(Transform, true), (Prepare, true), (Slice, false)] {
        let ctrl = one_part_in(Transform);
        ctrl.delete_selected();
        ctrl.switch_section(section);
        ctrl.undo();
        let after_undo = if history_works { 1 } else { 0 };
        assert_eq!(
            ctrl.leaf_total_for_test(),
            after_undo,
            "undo in {section:?}"
        );
        if history_works {
            ctrl.redo();
            assert_eq!(
                ctrl.leaf_total_for_test(),
                0,
                "redo in {section:?}"
            );
        }
    }
    // Redo too is held back in Slice.
    let ctrl = one_part_in(Transform);
    ctrl.delete_selected();
    ctrl.undo();
    ctrl.switch_section(Slice);
    ctrl.redo();
    assert_eq!(ctrl.leaf_total_for_test(), 1, "redo in Slice");
}

#[test]
fn selection_works_outside_transform() {
    let ctrl = one_part_in(Prepare);
    assert_eq!(ctrl.chosen(), Some(0));
    ctrl.select_nothing();
    assert_eq!(ctrl.chosen(), None);
}

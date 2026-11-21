// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
use sc_mesh_core::{IndexedMesh, primitives::Vertex, transform::Transform3D};
use sc_scene::{LeafPayload, Scene};
use std::sync::Arc;

fn controller_with_objects(count: usize) -> ViewController {
    let controller = ViewController::for_test();
    let mut scene = Scene::new();
    let root = scene.add_group(None, Transform3D::Identity);
    scene.add_root(root).unwrap();
    for _ in 0..count {
        let mesh = IndexedMesh {
            meta: None,
            vertices: vec![Vertex::new([0.0; 3])],
            faces: vec![],
        };
        let leaf = scene
            .add_leaf(
                None,
                Transform3D::Identity,
                LeafPayload::Mesh(Arc::new(mesh)),
            )
            .unwrap();
        scene.add_child(root, leaf).unwrap();
    }
    controller.put_graph_for_test(scene);
    controller
}

#[test]
fn selection_lifecycle_and_invalid_indices() {
    let empty = ViewController::for_test();
    assert_eq!(empty.chosen(), None);
    empty.on_select_by_index(0);
    assert_eq!(empty.chosen(), None);
    let c = controller_with_objects(3);
    for index in [1, 2] {
        c.on_select_by_index(index);
        assert_eq!(c.chosen(), Some(index as usize));
    }
    c.on_select_by_index(99);
    assert_eq!(c.chosen(), None);
    c.on_select_by_index(1);
    c.select_nothing();
    assert_eq!(c.chosen(), None);
    c.on_select_by_index(2);
    c.close_scene();
    assert_eq!(c.chosen(), None);
    c.on_select_by_index(2);
    assert_eq!(c.chosen(), None);
}

#[test]
fn selecting_current_item_does_not_request_another_frame() {
    let c = controller_with_objects(2);
    c.on_select_by_index(0);
    c.forget_frame_for_test();
    c.on_select_by_index(0);
    assert!(!c.redraw_due_for_test());
}

#[test]
fn incomplete_pointer_input_does_not_select() {
    for point in [ScreenPoint::new(50.0, 50.0), ScreenPoint::new(0.0, 0.0)] {
        let c = controller_with_objects(1);
        c.on_pointer_down(point, PointerModifiers::default());
        c.on_pointer_up(point, PointerModifiers::default());
        assert_eq!(c.chosen(), None);
    }
    let c = controller_with_objects(1);
    c.on_pointer_up(ScreenPoint::new(50.0, 50.0), PointerModifiers::default());
    assert_eq!(c.chosen(), None);
}

#[test]
fn cycling_obeys_direction_wrap_and_empty_cases() {
    let c = ViewController::for_test();
    c.cycle_selection(1);
    assert_eq!(c.chosen(), None);
    let c = controller_with_objects(0);
    c.cycle_selection(1);
    assert_eq!(c.chosen(), None);
    let c = controller_with_objects(1);
    for step in [1, 1, -1] {
        c.cycle_selection(step);
        assert_eq!(c.chosen(), Some(0));
    }
    let c = controller_with_objects(3);
    for initial_step in [1, -1] {
        c.select_nothing();
        c.cycle_selection(initial_step);
        assert_eq!(c.chosen(), Some(0));
    }
    for (step, expected) in [
        (1, 1),
        (1, 2),
        (1, 0),
        (-1, 2),
        (-1, 1),
        (-1, 0),
        (99, 1),
        (0, 1),
        (-99, 0),
    ] {
        c.cycle_selection(step);
        assert_eq!(c.chosen(), Some(expected));
    }
}

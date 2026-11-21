// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
use sc_scene::{LeafPayload, Scene};
use std::sync::Arc;

fn cube_at(offset: [f64; 3]) -> ViewController {
    let controller = ViewController::for_test();
    let mut scene = Scene::new();
    let root = scene.add_group(None, Transform3D::Identity);
    scene.add_root(root).unwrap();
    let leaf = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_translation(offset)),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_child(root, leaf).unwrap();
    controller.put_graph_for_test(scene);
    controller.on_select_by_index(0);
    controller
}

fn pose(controller: &ViewController) -> Transform3D {
    controller
        .state
        .document
        .read_scene(|scene| {
            scene
                .leaf(scene.leaf_id_at_index(0).unwrap())
                .unwrap()
                .local_transform()
        })
        .unwrap()
}

#[test]
fn alignment_moves_only_needed_axes_and_has_one_undo() {
    let c = cube_at([12.0, -8.0, 5.0]);
    assert!(!c.state.document.has_past());
    c.on_center_on_plate_requested();
    assert_eq!(translation_of(&pose(&c)), [0.0, 0.0, 5.0]);
    assert!(c.state.document.has_past());
    c.state.document.step_back().unwrap();
    assert_eq!(translation_of(&pose(&c)), [12.0, -8.0, 5.0]);
    assert!(!c.state.document.has_past());
    let c = cube_at([3.0, -4.0, 10.0]);
    c.on_drop_to_plate_requested();
    assert_eq!(translation_of(&pose(&c)), [3.0, -4.0, 0.5]);
}

#[test]
fn alignment_ignores_noop_and_non_transform_section() {
    let centered = cube_at([0.0, 0.0, 3.0]);
    centered.on_center_on_plate_requested();
    assert!(!centered.state.document.has_past());
    let c = cube_at([12.0, -8.0, 5.0]);
    c.state.place.set_section(WorkspaceSection::Prepare);
    c.on_center_on_plate_requested();
    assert_eq!(translation_of(&pose(&c)), [12.0, -8.0, 5.0]);
    c.on_drop_to_plate_requested();
    assert_eq!(translation_of(&pose(&c)), [12.0, -8.0, 5.0]);
}

#[test]
fn placing_a_face_preserves_location_refreshes_cache_and_undoes() {
    let c = cube_at([3.0, -4.0, 0.5]);
    c.refresh_place_faces();
    let index = {
        let cache = c.place_faces.borrow();
        let faces = &cache.as_ref().unwrap().faces;
        assert_eq!(faces.len(), 6);
        faces.iter().position(|f| f.normal.x > 0.999).unwrap()
    };
    c.place_face(index);
    let placed = pose(&c);
    let point = |v| {
        glam::DVec3::from_array(
            placed
                .apply_to_vertex(sc_mesh_core::Vertex::new(v))
                .coords()
                .map(f64::from),
        )
    };
    assert!((point([1.0, 0.0, 0.0]) - point([0.0, 0.0, 0.0])).z < -0.999);
    for (actual, expected) in translation_of(&placed).into_iter().zip([3.0, -4.0, 0.5]) {
        assert!((actual - expected).abs() < 1e-6);
    }
    c.refresh_place_faces();
    assert_eq!(c.place_faces.borrow().as_ref().unwrap().pose, placed);
    c.state.document.step_back().unwrap();
    for (actual, expected) in translation_of(&pose(&c)).into_iter().zip([3.0, -4.0, 0.5]) {
        assert!((actual - expected).abs() < 1e-9);
    }
}

#[test]
fn face_placement_requires_selection() {
    let c = cube_at([0.0, 0.0, 0.5]);
    c.state.document.choose(Selection::None);
    c.refresh_place_faces();
    assert!(c.place_faces.borrow().is_none());
    c.place_face(0);
    assert!(!c.state.document.has_past());
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

use super::*;
use sc_mesh_core::{IndexedMesh, primitives::Vertex};
use sc_scene::{LeafPayload, Scene};
use std::sync::Arc;

fn controller(points: &[[f32; 3]]) -> ViewController {
    let ctrl = ViewController::for_test();
    let mut scene = Scene::new();
    let root = scene.add_group(None, Transform3D::Identity);
    scene.add_root(root).unwrap();
    let mesh = IndexedMesh {
        meta: None,
        vertices: points.iter().copied().map(Vertex::new).collect(),
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
    ctrl.put_graph_for_test(scene);
    ctrl.on_select_by_index(0);
    ctrl
}

fn offset(ctrl: &ViewController) -> [f64; 3] {
    ctrl.state
        .document
        .read_scene(|scene| {
            let leaf = scene.leaf_id_at_index(0).unwrap();
            translation_of(&scene.leaf(leaf).unwrap().local_transform())
        })
        .unwrap()
}

#[test]
fn position_fields_apply_axes_and_ignore_invalid_or_gated_input() {
    let ctrl = controller(&[[0.0, 0.0, 0.0]]);
    for (axis, value, expected) in [
        (0, "12.5", [12.5, 0.0, 0.0]),
        (1, "-3", [12.5, -3.0, 0.0]),
        (2, "7", [12.5, -3.0, 0.0]),
        (0, "invalid", [12.5, -3.0, 0.0]),
    ] {
        ctrl.on_position_committed(axis, value.into());
        assert_eq!(offset(&ctrl), expected);
    }
    ctrl.state.place.set_section(WorkspaceSection::Prepare);
    ctrl.on_position_committed(0, "99".into());
    assert_eq!(offset(&ctrl), [12.5, -3.0, 0.0]);
}

#[test]
fn position_edit_retains_linear_transform_and_undoes_once() {
    let ctrl = controller(&[[0.0, 0.0, 0.0]]);
    let doc = &ctrl.state.document;
    let id = doc.main_chosen_node().unwrap();
    let seeded = AffineTransform3D::from_rotation_z(0.4)
        .compose(&AffineTransform3D::from_non_uniform_scale([2.0, 3.0, 4.0]));
    doc.commit_edit(SceneEdit::SetLocalTransform {
        node_id: id,
        before: Transform3D::Identity,
        after: Transform3D::Affine(seeded),
    })
    .unwrap();
    ctrl.on_position_committed(0, "50".into());
    assert_eq!(offset(&ctrl), [50.0, 0.0, 0.0]);
    doc.read_scene(|scene| match scene.leaf(id).unwrap().local_transform() {
        Transform3D::Affine(a) => assert_eq!(a.linear(), seeded.linear()),
        Transform3D::Identity => panic!("linear transform lost"),
    })
    .unwrap();
    doc.step_back().unwrap();
    assert_eq!(offset(&ctrl), [0.0, 0.0, 0.0]);
    doc.step_back().unwrap();
    assert!(!doc.has_past());

    let fresh = controller(&[[0.0, 0.0, 0.0]]);
    assert!(!fresh.state.document.has_past());
    fresh.on_position_committed(0, "10".into());
    assert!(fresh.state.document.has_past());
    assert_eq!(offset(&fresh), [10.0, 0.0, 0.0]);
    fresh.state.document.step_back().unwrap();
    assert_eq!(offset(&fresh), [0.0, 0.0, 0.0]);
    assert!(!fresh.state.document.has_past());
}

#[test]
fn displayed_position_tracks_bounds_instead_of_mesh_origin() {
    let ctrl = controller(&[[90.0, 1400.0, 20.0], [110.0, 1420.0, 30.0]]);
    let shown = || {
        ctrl.state
            .document
            .read_scene(|scene| displayed_position(scene, scene.leaf_id_at_index(0).unwrap()))
            .flatten()
            .unwrap()
    };
    assert_eq!(shown(), [100.0, 1410.0, 20.0]);
    for axis in 0..3 {
        ctrl.on_position_committed(axis, "0".into());
    }
    assert_eq!(shown(), [0.0, 0.0, 0.0]);
    assert_eq!(offset(&ctrl), [-100.0, -1410.0, -20.0]);
}

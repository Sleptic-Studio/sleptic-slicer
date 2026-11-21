// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
use sc_scene::{LeafPayload, Scene};
use std::sync::Arc;
fn move_by(v: [f64; 3]) -> Transform3D {
    Transform3D::Affine(AffineTransform3D::from_translation(v))
}
fn fixture(parent: Transform3D) -> (SceneSpatial, NodeId) {
    let mut scene = Scene::new();
    let group = scene.add_group(None, parent);
    scene.add_root(group).unwrap();
    let leaf = scene
        .add_leaf(
            None,
            move_by([0.0, 0.0, 0.5]),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_child(group, leaf).unwrap();
    (SceneSpatial::from_graph(scene), leaf)
}
fn xyz(t: Transform3D) -> [f64; 3] {
    match t {
        Transform3D::Identity => [0.0; 3],
        Transform3D::Affine(a) => *a.translation(),
    }
}
#[test]
fn movement_respects_walls_and_keeps_free_axes() {
    let (scene, id) = fixture(Transform3D::Identity);
    let before = move_by([0.0, 0.0, 0.5]);
    for (request, expected) in [
        ([1.0, 2.0, 3.0], [1.0, 2.0, 3.0]),
        ([60.0, 4.0, 0.5], [49.5, 4.0, 0.5]),
        ([0.0, 0.0, -2.0], [0.0, 0.0, 0.5]),
    ] {
        let result = clamp_move_edit(
            &scene,
            id,
            before,
            move_by(request),
            BuildVolume::new(100.0, 60.0, 80.0),
        );
        assert_eq!(xyz(result), expected);
    }
}
#[test]
fn scaled_parent_converts_clipped_world_motion() {
    let (scene, id) = fixture(Transform3D::Affine(AffineTransform3D::from_uniform_scale(
        2.0,
    )));
    let before = move_by([0.0, 0.0, 0.5]);
    let result = clamp_move_edit(
        &scene,
        id,
        before,
        move_by([100.0, 0.0, 0.5]),
        BuildVolume::new(100.0, 60.0, 80.0),
    );
    assert_eq!(xyz(result), [24.5, 0.0, 0.5]);
}
#[test]
fn settle_returns_bottom_to_plate() {
    let (scene, id) = fixture(Transform3D::Identity);
    let result = settle_on_plate(&scene, id, move_by([2.0, 3.0, 9.0]));
    assert_eq!(xyz(result), [2.0, 3.0, 0.5]);
}
#[test]
fn translated_parent_controls_world_limit() {
    let (scene, id) = fixture(move_by([69.0, 0.0, 0.0]));
    let before = move_by([0.0, 0.0, 0.5]);
    let volume = BuildVolume::new(100.0, 60.0, 80.0);
    assert_eq!(
        clamp_move_edit(&scene, id, before, move_by([5.0, 0.0, 0.5]), volume),
        before
    );
    let inward = move_by([-10.0, 0.0, 0.5]);
    assert_eq!(
        clamp_move_edit(&scene, id, before, inward, volume),
        inward
    );
}
#[test]
fn group_without_mesh_passes_through() {
    let (scene, _) = fixture(Transform3D::Identity);
    let proposed = move_by([1000.0, 0.0, 0.0]);
    assert_eq!(
        clamp_move_edit(
            &scene,
            scene.graph().roots()[0],
            Transform3D::Identity,
            proposed,
            BuildVolume::new(100.0, 60.0, 80.0)
        ),
        proposed
    );
    assert_eq!(
        settle_on_plate(&scene, scene.graph().roots()[0], proposed),
        proposed
    );
}
#[test]
fn owned_raft_limits_descent_and_preserves_model_height() {
    let mut graph = Scene::new();
    let model = graph
        .add_leaf(
            None,
            move_by([0.0, 0.0, 5.0]),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    graph.add_root(model).unwrap();
    let raft = graph
        .add_leaf(
            Some(sc_scene::SceneMetadata {
                name: None,
                comment: Some(format!(
                    "sc-generated-support-raft\nowner = {}\n",
                    model.get()
                )),
            }),
            move_by([0.0, 0.0, 0.5]),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    graph.add_root(raft).unwrap();
    let scene = SceneSpatial::from_graph(graph);
    let before = move_by([0.0, 0.0, 5.0]);
    assert_eq!(
        clamp_move_edit(
            &scene,
            model,
            before,
            move_by([0.0, 0.0, 3.0]),
            BuildVolume::new(100.0, 60.0, 80.0)
        ),
        before
    );
    let result = settle_on_plate(&scene, model, move_by([0.0, 0.0, 8.0]));
    assert_eq!(xyz(result), xyz(before));
}
#[test]
fn rotated_mesh_uses_vertex_bounds() {
    use sc_mesh_core::{IndexedMesh, primitives::Vertex};
    let mesh = IndexedMesh {
        meta: None,
        vertices: [
            [1., 0., 0.],
            [-1., 0., 0.],
            [0., 1., 0.],
            [0., -1., 0.],
            [0., 0., 1.],
            [0., 0., -1.],
        ]
        .into_iter()
        .map(Vertex::new)
        .collect(),
        faces: vec![],
    };
    let mut graph = Scene::new();
    let id = graph
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(mesh)),
        )
        .unwrap();
    graph.add_root(id).unwrap();
    let scene = SceneSpatial::from_graph(graph);
    let before = Transform3D::Affine(AffineTransform3D::from_rotation_z(
        std::f64::consts::FRAC_PI_4,
    ));
    let request = move_by([60., 0., 0.]).compose(&before);
    let actual = xyz(clamp_move_edit(
        &scene,
        id,
        before,
        request,
        BuildVolume::new(100., 60., 80.),
    ))[0];
    assert!((actual - (50. - f64::from(std::f32::consts::FRAC_1_SQRT_2))).abs() < 1e-4);
}
#[test]
fn settlement_keeps_xy_for_rotations_and_scale() {
    let (scene, id) = fixture(Transform3D::Identity);
    let pivot = move_by([0.0, 0.0, 0.5]);
    for shape in [
        AffineTransform3D::from_rotation_x(std::f64::consts::FRAC_PI_6),
        AffineTransform3D::from_uniform_scale(3.0),
    ] {
        let proposed = pivot.compose(&Transform3D::Affine(shape));
        let result = settle_on_plate(&scene, id, proposed);
        let leaf = scene.leaf(id).unwrap();
        let expected = leaf.transformed_aabb(proposed).unwrap();
        let actual = leaf.transformed_aabb(result).unwrap();
        assert!(actual.min().coords()[2].abs() < 1e-4);
        assert_eq!(actual.min().coords()[0], expected.min().coords()[0]);
        assert_eq!(actual.min().coords()[1], expected.min().coords()[1]);
    }
}
#[test]
fn empty_mesh_does_not_constrain_motion() {
    let mut graph = Scene::new();
    let id = graph
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(sc_mesh_core::IndexedMesh {
                meta: None,
                vertices: vec![],
                faces: vec![],
            })),
        )
        .unwrap();
    graph.add_root(id).unwrap();
    let scene = SceneSpatial::from_graph(graph);
    let request = move_by([10000., 0., 0.]);
    assert_eq!(
        clamp_move_edit(
            &scene,
            id,
            Transform3D::Identity,
            request,
            BuildVolume::new(100., 60., 80.)
        ),
        request
    );
    assert_eq!(settle_on_plate(&scene, id, request), request);
}
#[test]
fn outside_object_can_return_but_not_move_further_out() {
    let (scene, id) = fixture(Transform3D::Identity);
    let before = move_by([69., 0., 0.5]);
    let volume = BuildVolume::new(100., 60., 80.);
    let inward = move_by([59., 0., 0.5]);
    assert_eq!(
        clamp_move_edit(&scene, id, before, inward, volume),
        inward
    );
    assert_eq!(
        clamp_move_edit(&scene, id, before, move_by([74., 0., 0.5]), volume),
        before
    );
}
#[test]
fn wall_slide_and_blocked_identity() {
    let (scene, id) = fixture(Transform3D::Identity);
    let volume = BuildVolume::new(100., 60., 80.);
    let before = move_by([49.5, 0., 0.5]);
    assert_eq!(
        xyz(clamp_move_edit(
            &scene,
            id,
            before,
            move_by([54.5, 4., 0.5]),
            volume
        )),
        [49.5, 4., 0.5]
    );
    assert_eq!(
        clamp_move_edit(&scene, id, before, move_by([54.5, 0., 0.5]), volume),
        before
    );
    let (placed, leaf) = fixture(move_by([0., 0., 0.5]));
    assert_eq!(
        clamp_move_edit(
            &placed,
            leaf,
            Transform3D::Identity,
            move_by([0., 0., -1.]),
            volume
        ),
        Transform3D::Identity
    );
}
#[test]
fn rotated_cube_passes_small_move_and_clips_large_move() {
    let (scene, id) = fixture(Transform3D::Identity);
    let rotation = Transform3D::Affine(AffineTransform3D::from_rotation_z(
        std::f64::consts::FRAC_PI_4,
    ));
    let before = move_by([0., 0., 0.5]).compose(&rotation);
    let volume = BuildVolume::new(100., 60., 80.);
    let small = move_by([2., 1., 3.]).compose(&before);
    assert_eq!(
        clamp_move_edit(&scene, id, before, small, volume),
        small
    );
    let large = move_by([60., 0., 0.]).compose(&before);
    let actual = xyz(clamp_move_edit(&scene, id, before, large, volume))[0];
    assert!((actual - (50. - f64::from(std::f32::consts::FRAC_1_SQRT_2))).abs() < 1e-4);
}
#[test]
fn turning_supported_bar_preserves_its_bottom() {
    use sc_mesh_core::primitives::Vertex;
    let mut bar = sc_mesh_fixtures::unit_cube_mesh();
    for vertex in &mut bar.vertices {
        *vertex = Vertex::new([vertex[0] * 10., vertex[1] * 2., vertex[2]]);
    }
    let mut graph = Scene::new();
    let before = move_by([0., 0., 5.5]);
    let id = graph
        .add_leaf(None, before, LeafPayload::Mesh(Arc::new(bar)))
        .unwrap();
    graph.add_root(id).unwrap();
    let raft = graph
        .add_leaf(
            Some(sc_scene::SceneMetadata {
                name: None,
                comment: Some(format!("sc-generated-support-raft\nowner = {}\n", id.get())),
            }),
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    graph.add_root(raft).unwrap();
    let scene = SceneSpatial::from_graph(graph);
    let turn = before.compose(&Transform3D::Affine(AffineTransform3D::from_rotation_y(
        std::f64::consts::FRAC_PI_2,
    )));
    let leaf = scene.leaf(id).unwrap();
    let bottom = |t| leaf.transformed_aabb(t).unwrap().min().coords()[2];
    assert!(bottom(turn) < 1.);
    assert!((bottom(settle_on_plate(&scene, id, turn)) - bottom(before)).abs() < 1e-4);
}

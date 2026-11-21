// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use std::sync::Arc;

use sc_mesh_core::MeshMetadata;
use sc_mesh_core::primitives::{IndexedTriangle, Normal};
use sc_scene::{LeafPayload, SceneMetadata};

use super::*;

/// A mesh of `points`, with triangles walking round them three at a time.
fn mesh(points: &[[f32; 3]], triangles: usize) -> IndexedMesh {
    let at = |k: usize, j: usize| ((3 * k + j) % points.len()) as u32;
    IndexedMesh {
        meta: None,
        vertices: points.iter().copied().map(Vertex::new).collect(),
        faces: (0..triangles)
            .map(|k| {
                IndexedTriangle::from_raw_indices(
                    Normal::new([0.0, 0.0, 1.0]),
                    [at(k, 0), at(k, 1), at(k, 2)],
                )
            })
            .collect(),
    }
}

const UNIT: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

fn moved(x: f64, y: f64, z: f64) -> Transform3D {
    Transform3D::Affine(AffineTransform3D::from_translation([x, y, z]))
}

/// Parts under one group root, as a project opens; returns their ids.
fn scene(parts: Vec<(IndexedMesh, Transform3D)>) -> (Scene, Vec<NodeId>) {
    let mut scene = Scene::new();
    let root = scene.add_group(None, Transform3D::Identity);
    scene.add_root(root).unwrap();
    let ids = parts
        .into_iter()
        .map(|(mesh, transform)| {
            let id = scene
                .add_leaf(None, transform, LeafPayload::Mesh(Arc::new(mesh)))
                .unwrap();
            scene.add_child(root, id).unwrap();
            id
        })
        .collect();
    (scene, ids)
}

/// One unit triangle placed at (x, y, z).
fn triangle_at(x: f64, y: f64, z: f64) -> Scene {
    scene(vec![(mesh(&UNIT, 1), moved(x, y, z))]).0
}

/// 100 x 60 x 80 mm: x -50..50, y -30..30, z 0..80.
fn volume() -> BuildVolume {
    BuildVolume::new(100.0, 60.0, 80.0)
}

#[test]
fn each_part_keeps_its_own_vertices_and_indices() {
    let a = mesh(&[[1.0, -2.0, 3.0], [4.0, 5.0, -6.0], [0.0, 0.0, 0.0]], 1);
    let b = mesh(&[[-7.0, 8.0, 0.5], [0.0, 0.0, 9.0], [2.0, 2.0, 2.0]], 1);
    let (scene, ids) = scene(vec![
        (a, Transform3D::Identity),
        (b, moved(100.0, -50.0, 25.0)),
    ]);
    let mut seen = Vec::new();
    let data = build_gpu_scene_data(&scene, |triangles| seen.push(triangles)).unwrap();
    assert_eq!(seen, [1, 1]);
    // Placement is not baked in, and indices start at 0 for every part.
    assert_eq!(
        data.upload_scene_parts[1]
            .part_vertex_data
            .iter()
            .map(|v| v.vertex_coordinates)
            .collect::<Vec<_>>(),
        [[-7.0, 8.0, 0.5], [0.0, 0.0, 9.0], [2.0, 2.0, 2.0]]
    );
    assert_eq!(
        (
            data.upload_scene_parts[0].part_index_data.clone(),
            data.upload_scene_parts[1].part_index_data.clone()
        ),
        (vec![0, 1, 2], vec![0, 1, 2])
    );
    assert_eq!(
        data.upload_scene_parts.iter().map(|o| o.upload_part_node).collect::<Vec<_>>(),
        ids
    );
    // The bounds are in world space, over all parts.
    assert_eq!(
        (data.upload_bounds_min, data.upload_bounds_max),
        (Vec3::new(0.0, -50.0, -6.0), Vec3::new(102.0, 5.0, 34.0))
    );
}

#[test]
fn empty_parts_keep_their_place_and_an_empty_scene_is_refused() {
    let empty = IndexedMesh {
        meta: None,
        vertices: vec![],
        faces: vec![],
    };
    let (two, _) = scene(vec![
        (empty, Transform3D::Identity),
        (mesh(&UNIT, 1), Transform3D::Identity),
    ]);
    let data = build_gpu_scene_data(&two, |_| {}).unwrap();
    assert_eq!(
        (data.upload_scene_parts[0].part_index_data.len(), data.upload_scene_parts[1].part_index_data.len()),
        (0, 3)
    );
    assert!(build_gpu_scene_data(&scene(vec![]).0, |_| {}).is_err());
    // A flattening placement is accepted; the GPU draws it flat.
    let flat = AffineTransform3D::from_linear_and_translation(
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0; 3]],
        [0.0; 3],
    );
    assert!(
        build_gpu_scene_data(
            &scene(vec![(mesh(&UNIT, 1), Transform3D::Affine(flat))]).0,
            |_| {}
        )
        .is_ok()
    );
    // Shared mesh resources are followed.
    let mut shared = Scene::new();
    let resource = shared.add_mesh_resource(mesh(&UNIT, 1));
    let id = shared
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::MeshResource(resource),
        )
        .unwrap();
    shared.add_root(id).unwrap();
    assert_eq!(
        build_gpu_scene_data(&shared, |_| {}).unwrap().upload_scene_parts[0].part_index_data,
        [0, 1, 2]
    );
}

#[test]
fn bounds_follow_the_true_shape_not_its_turned_box() {
    let tilted = crate::tests_r21::tilted_octahedron();
    let data = build_gpu_scene_data(&tilted, |_| {}).unwrap();
    let exact = tilted.tight_aabb().unwrap().unwrap();
    assert_eq!(
        (data.upload_bounds_min, data.upload_bounds_max),
        (
            Vec3::from(exact.min().coords()),
            Vec3::from(exact.max().coords())
        )
    );
    assert!(tilted.aabb().unwrap().unwrap().max()[2] > data.upload_bounds_max.z + 0.5);
}

#[test]
fn placements_become_glam_matrices_that_move_points_alike() {
    assert_eq!(transform_to_mat4(&Transform3D::Identity), Mat4::IDENTITY);
    assert_eq!(
        transform_to_mat4(&moved(10.0, -5.0, 2.5)).col(3),
        Vec4::new(10.0, -5.0, 2.5, 1.0)
    );
    let turned = AffineTransform3D::from_translation([1.0, 2.0, 3.0]).compose(
        &AffineTransform3D::from_rotation_z(std::f64::consts::FRAC_PI_4)
            .compose(&AffineTransform3D::from_non_uniform_scale([2.0, 3.0, 4.0])),
    );
    for p in UNIT {
        let by_matrix = affine_to_gpu_matrix(&turned).transform_point3(Vec3::from(p));
        assert!(
            by_matrix.distance(Vec3::from(turned.apply_to_vertex(Vertex::new(p)).coords())) < 1e-5
        );
    }
}

#[test]
fn entries_carry_placement_selection_and_overhang_per_part() {
    let (two, ids) = scene(vec![
        (mesh(&UNIT, 1), moved(0.0, 0.0, 1.0)),
        (mesh(&UNIT, 1), moved(7.0, 0.0, 1.0)),
    ]);
    let entries = build_part_transform_entries(&two, Some(ids[1]), volume()).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(
        (entries[0].part_transform[3], entries[1].part_transform[3]),
        ([0.0, 0.0, 1.0, 1.0], [7.0, 0.0, 1.0, 1.0])
    );
    assert_eq!((entries[0].flags[0], entries[1].flags[0]), (0, 1));
    // The group root is not a drawn part: nothing is selected.
    let root = two.roots()[0];
    assert!(
        build_part_transform_entries(&two, Some(root), volume())
            .unwrap()
            .iter()
            .all(|e| e.flags[0] == 0)
    );
    // Sticking out: past a wall, below the floor, half over a wall; resting
    // on the floor is inside.
    for (at, out) in [
        ([0.0, 0.0, 1.0], 0),
        ([60.0, 0.0, 0.0], 1),
        ([0.0, 0.0, -0.5], 1),
        ([49.5, 0.0, 0.0], 1),
        ([0.0; 3], 0),
    ] {
        let entries =
            build_part_transform_entries(&triangle_at(at[0], at[1], at[2]), None, volume()).unwrap();
        assert_eq!(entries[0].flags[1], out, "{at:?}");
    }
}

#[test]
fn supports_and_painted_regions_reach_the_shader() {
    let with = |comment: &str| {
        let mut scene = Scene::new();
        let meta = SceneMetadata {
            name: None,
            comment: Some(comment.into()),
        };
        let id = scene
            .add_leaf(
                Some(meta),
                Transform3D::Identity,
                LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
            )
            .unwrap();
        scene.add_root(id).unwrap();
        build_part_transform_entries(
            &scene,
            None,
            crate::build_volume::FALLBACK_VOLUME,
        )
        .unwrap()
        .remove(0)
    };
    assert_eq!(with("sc-generated-support").flags[2], 1);
    assert_eq!(with("something else").flags[2], 0);
    let painted = with("sc-support-paint 1 2 3 4 1\nsc-support-paint 5 6 7 8 0\n");
    // A region to keep free of supports has a negative radius.
    assert_eq!(
        painted.paint[..3],
        [[1.0, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, -8.0], [0.0; 4]]
    );
}

#[test]
fn the_wall_mask_lights_each_wall_a_part_crosses() {
    let mask = |x, y, z| build_envelope_warning_mask(&triangle_at(x, y, z), volume()).unwrap();
    assert_eq!(mask(0.0, 0.0, 1.0), 0);
    assert_eq!(mask(60.0, 0.0, 0.0), OUTSIDE_FACE_RIGHT);
    assert_eq!(mask(-60.0, 0.0, 0.0), OUTSIDE_FACE_LEFT);
    assert_eq!(mask(0.0, -40.0, 0.0), OUTSIDE_FACE_FRONT);
    assert_eq!(mask(0.0, 40.0, 0.0), OUTSIDE_FACE_BACK);
    assert_eq!(mask(0.0, 0.0, 90.0), OUTSIDE_FACE_TOP);
    assert_eq!(mask(0.0, 0.0, -1.0), OUTSIDE_FACE_BOTTOM);
    // R51: a rounding error below the floor is not crossing it.
    assert_eq!(mask(0.0, 0.0, -0.000_2), 0);
    assert_eq!(mask(0.0, 0.0, -0.01), OUTSIDE_FACE_BOTTOM);
    // Several parts: the union.
    let (both, _) = scene(vec![
        (mesh(&UNIT, 1), moved(60.0, 0.0, 0.0)),
        (mesh(&UNIT, 1), moved(0.0, -40.0, 0.0)),
    ]);
    assert_eq!(
        build_envelope_warning_mask(&both, volume()).unwrap(),
        OUTSIDE_FACE_RIGHT | OUTSIDE_FACE_FRONT
    );
    // One plate: its mask is the scene's.
    assert_eq!(
        build_plate_warning_masks(&both, volume()).unwrap(),
        [OUTSIDE_FACE_RIGHT | OUTSIDE_FACE_FRONT]
    );
}

#[test]
fn every_drawn_part_is_counted_however_deep() {
    assert_eq!(count_renderable_parts(&Scene::new()).unwrap(), 0);
    let (mut nested, ids) = scene(vec![(mesh(&UNIT, 1), Transform3D::Identity)]);
    assert_eq!(count_renderable_parts(&scene(vec![]).0).unwrap(), 0);
    let group = nested.add_group(None, Transform3D::Identity);
    nested.add_child(nested.roots()[0], group).unwrap();
    for _ in 0..2 {
        let id = nested
            .add_leaf(
                None,
                Transform3D::Identity,
                LeafPayload::Mesh(Arc::new(mesh(&UNIT, 1))),
            )
            .unwrap();
        nested.add_child(group, id).unwrap();
    }
    assert_eq!(count_renderable_parts(&nested).unwrap(), 3);
    nested.remove_leaf(ids[0]).unwrap();
    assert_eq!(count_renderable_parts(&nested).unwrap(), 2);
    // A part that is itself a root, as a mesh file opens.
    let mut lone = Scene::new();
    let id = lone
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(mesh(&UNIT, 1))),
        )
        .unwrap();
    lone.add_root(id).unwrap();
    assert_eq!(count_renderable_parts(&lone).unwrap(), 1);
    lone.remove_leaf(id).unwrap();
    assert_eq!(count_renderable_parts(&lone).unwrap(), 0);
}

#[test]
fn offsets_round_up_to_the_alignment() {
    assert_eq!(
        [
            round_buffer_size_up(80, 256),
            round_buffer_size_up(257, 256),
            round_buffer_size_up(256, 256),
            round_buffer_size_up(512, 256)
        ],
        [256, 512, 256, 512]
    );
    assert_eq!([round_buffer_size_up(1, 1), round_buffer_size_up(80, 1)], [1, 80]);
}

/// Buffers over 1 KiB are refused in these tests.
const LIMIT: u64 = 1024;

/// Root parts: (vertex count, triangles, mesh name, part name).
fn sized(parts: &[(usize, usize, Option<&str>, Option<&str>)]) -> (Scene, Vec<NodeId>) {
    let mut scene = Scene::new();
    let mut ids = Vec::new();
    for &(points, triangles, mesh_name, part_name) in parts {
        let points: Vec<[f32; 3]> = (0..points).map(|i| [i as f32, 0.0, 0.0]).collect();
        let mut m = mesh(&points, triangles);
        m.meta = mesh_name.map(|name| MeshMetadata {
            name: Some(name.into()),
            header_comment: None,
        });
        let meta = part_name.map(|name| SceneMetadata {
            name: Some(name.into()),
            comment: None,
        });
        let id = scene
            .add_leaf(meta, Transform3D::Identity, LeafPayload::Mesh(Arc::new(m)))
            .unwrap();
        scene.add_root(id).unwrap();
        ids.push(id);
    }
    (scene, ids)
}

fn fits(scene: &Scene) -> std::result::Result<(), PartBufferOverLimit> {
    check_part_buffer_limits(&build_gpu_scene_data(scene, |_| {}).unwrap().upload_scene_parts, LIMIT)
}

#[test]
fn a_part_too_big_for_one_buffer_is_named_and_measured() {
    assert_eq!(check_part_buffer_limits(&[], LIMIT), Ok(()));
    // 85 triangles: 1020 index bytes, just inside.
    assert_eq!(fits(&sized(&[(3, 85, None, None)]).0), Ok(()));
    let (one_over, ids) = sized(&[(3, 86, None, None)]);
    let refusal = PartBufferOverLimit {
        upload_part_node: ids[0],
        oversized_part_name: None,
        oversized_buffer_kind: PartBufferCategory::IndexData,
        oversized_element_count: 86,
        upload_buffer_byte_count: 1032,
        device_buffer_limit: LIMIT,
    };
    assert_eq!(fits(&one_over), Err(refusal));
    // 45 drawn vertices of 24 bytes: 1080.
    let (vertices, _) = sized(&[(45, 15, None, Some("shell"))]);
    let err = fits(&vertices).unwrap_err();
    assert_eq!(
        (err.oversized_buffer_kind, err.oversized_element_count, err.upload_buffer_byte_count),
        (PartBufferCategory::VertexData, 45, 1080)
    );
    assert_eq!(
        err.to_string(),
        "Object 'shell' has more vertices (45) than this graphics device can hold in one buffer"
    );
    // Both over: the vertex buffer is named.
    assert_eq!(
        fits(&sized(&[(86, 86, None, None)]).0)
            .unwrap_err()
            .oversized_buffer_kind,
        PartBufferCategory::VertexData
    );
    // The first part over, in drawing order.
    let (three, ids) = sized(&[
        (3, 10, None, None),
        (3, 86, None, None),
        (3, 90, None, None),
    ]);
    assert_eq!(fits(&three).unwrap_err().upload_part_node, ids[1]);
}

#[test]
fn a_refused_part_goes_by_its_name_its_meshs_or_none() {
    let name = |mesh_name, part_name| fits(&sized(&[(3, 86, mesh_name, part_name)]).0).unwrap_err();
    let named = name(Some("mesh-name"), Some("hull"));
    assert_eq!(
        named.to_string(),
        "Object 'hull' has more triangles (86) than this graphics device can hold in one buffer"
    );
    assert_eq!(name(Some("part"), None).oversized_part_name.as_deref(), Some("part"));
    assert_eq!(
        name(None, None).to_string(),
        "An object has more triangles (86) than this graphics device can hold in one buffer"
    );
    assert_eq!(
        part_buffer("index buffer", Some("hull")),
        "index buffer of object 'hull'"
    );
    assert_eq!(
        part_buffer("vertex buffer", None),
        "vertex buffer of an object"
    );
}

#[test]
fn a_refused_buffer_says_what_it_was_for_and_its_size() {
    let refusal = SceneBufferRefused {
        refused_buffer_description: "index buffer of object 'hull'".into(),
        upload_buffer_byte_count: 4096,
        upload_error_source: wgpu::Error::Validation {
            source: Box::new(std::io::Error::other("too large")),
            description: "too large".into(),
        },
    };
    assert_eq!(
        refusal.to_string(),
        "The graphics device refused the index buffer of object 'hull' (4096 bytes)"
    );
}

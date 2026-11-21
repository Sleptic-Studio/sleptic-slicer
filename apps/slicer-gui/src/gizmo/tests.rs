// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Handle placement, size and the per-frame handle description.
use std::sync::Arc;

use float_cmp::assert_approx_eq;
use glam::Vec3;
use sc_mesh_core::IndexedMesh;
use sc_mesh_core::primitives::{IndexedTriangle, Normal, Vertex};
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_scene::{LeafPayload, Scene};

use super::axis::Axis;
use super::frame::handle_frame_for;
use super::*;
use crate::camera::ViewAxis;
use crate::mesh_render::{LiveDrag, HandleHover, HandleShowing, starting_fov_y};

fn triangle(corners: [[f32; 3]; 3]) -> IndexedMesh {
    IndexedMesh::new(
        None,
        corners.into_iter().map(Vertex::new).collect(),
        vec![IndexedTriangle::from_raw_indices(Normal::new([0.0, 0.0, 1.0]), [0, 1, 2])],
    )
}

/// Bounds (-1, -2, -3) to (3, 4, 5): centre (1, 1, 1).
fn skewed() -> IndexedMesh {
    triangle([[-1.0, -2.0, -3.0], [3.0, 0.0, 0.0], [-1.0, 4.0, 5.0]])
}

/// One part under a group, as files load. Returns the part's id too.
fn scene(mesh: IndexedMesh, transform: Transform3D) -> (SceneSpatial, NodeId) {
    let mut scene = Scene::new();
    let group = scene.add_group(None, Transform3D::Identity);
    scene.add_root(group).unwrap();
    let part = scene.add_leaf(None, transform, LeafPayload::Mesh(Arc::new(mesh))).unwrap();
    scene.add_child(group, part).unwrap();
    (SceneSpatial::from_graph(scene), part)
}

fn moved(by: [f64; 3]) -> Transform3D {
    Transform3D::Affine(AffineTransform3D::from_translation(by))
}

fn assert_vec(found: Vec3, expected: [f32; 3]) {
    for (f, e) in found.to_array().into_iter().zip(expected) {
        assert_approx_eq!(f32, f, e, epsilon = 1e-5);
    }
}

#[test]
fn the_pivot_is_the_centre_of_the_placed_bounds() {
    let (spatial, part) = scene(skewed(), Transform3D::Identity);
    assert_vec(part_pivot(&spatial, part).unwrap(), [1.0, 1.0, 1.0]);
    let (spatial, part) = scene(skewed(), moved([10.0, 20.0, 30.0]));
    assert_vec(part_pivot(&spatial, part).unwrap(), [11.0, 21.0, 31.0]);
    // Turned 45° about Z, the bounds of the turned shape (not the turned
    // box) give the centre.
    let turned = Transform3D::Affine(AffineTransform3D::from_rotation_z(std::f64::consts::FRAC_PI_4));
    let (spatial, part) = scene(triangle([[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 1.0]]), turned);
    assert_vec(part_pivot(&spatial, part).unwrap(), [0.0, std::f32::consts::FRAC_1_SQRT_2, 0.5]);
}

#[test]
fn no_pivot_without_usable_geometry_or_for_a_group() {
    let (spatial, part) = scene(IndexedMesh::new(None, vec![], vec![]), Transform3D::Identity);
    assert!(part_pivot(&spatial, part).is_none());
    let (spatial, _) = scene(skewed(), Transform3D::Identity);
    assert!(part_pivot(&spatial, spatial.graph().roots()[0]).is_none());
    // NaN on every vertex of one axis (one NaN alone is dropped by min/max),
    // infinities, and a finite mesh moved to infinity.
    for (mesh, transform) in [
        (triangle([[f32::NAN, 0.0, 0.0], [f32::NAN, 1.0, 0.0], [f32::NAN, 0.0, 1.0]]), Transform3D::Identity),
        (triangle([[f32::INFINITY, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]), Transform3D::Identity),
        (triangle([[0.0, 0.0, 0.0], [1.0, f32::NEG_INFINITY, 0.0], [0.0, 1.0, 0.0]]), Transform3D::Identity),
        (skewed(), moved([f64::INFINITY, 0.0, 0.0])),
    ] {
        let (spatial, part) = scene(mesh, transform);
        assert!(part_pivot(&spatial, part).is_none());
    }
}

fn camera_at(distance: f32) -> ViewCamera {
    let mut camera = ViewCamera::new(Vec3::ZERO, 1.0);
    camera.eye_distance = distance;
    camera
}

#[test]
fn handles_keep_their_screen_size() {
    let scale = |distance, height| gizmo_scale(&camera_at(distance), 1.0, height);
    // Ten times farther: ten times larger in the world.
    assert_approx_eq!(f32, scale(20.0, 480.0), scale(2.0, 480.0) * 10.0, ulps = 4);
    // A taller view has more pixels for the same world, so smaller handles.
    assert!(scale(5.0, 1080.0) < scale(5.0, 240.0));
    // The click radius is the same measure at 6 px instead of 120 px.
    let camera = camera_at(5.0);
    assert_approx_eq!(
        f32,
        hit_radius_world(&camera, 1.0, 480.0) * 20.0,
        gizmo_scale(&camera, 1.0, 480.0),
        ulps = 4
    );
    // A zero-height view counts as one pixel high instead of dividing by 0.
    assert_eq!(gizmo_scale(&camera, 1.0, 0.0), gizmo_scale(&camera, 1.0, 1.0));
}

fn frame(
    spatial: &SceneSpatial,
    selection: NodeId,
    camera: &ViewCamera,
    height: f32,
) -> Option<crate::mesh_render::HandleFrame> {
    handle_frame_for(spatial, camera, selection, height, HandleShowing::Resting, None, None)
}

#[test]
fn a_frame_carries_the_pivot_and_screen_constant_scale() {
    let camera = ViewCamera::new(Vec3::ZERO, 1.0);
    let (spatial, part) = scene(skewed(), moved([10.0, 20.0, 30.0]));
    let built = frame(&spatial, part, &camera, 720.0).unwrap();
    assert_vec(built.anchor, [11.0, 21.0, 31.0]);
    assert_eq!(built.arm_length, gizmo_scale(&camera, starting_fov_y(), 720.0));
    assert!(frame(&spatial, spatial.graph().roots()[0], &camera, 600.0).is_none());
    let (spatial, part) = scene(IndexedMesh::new(None, vec![], vec![]), Transform3D::Identity);
    assert!(frame(&spatial, part, &camera, 600.0).is_none());
}

#[test]
fn looking_along_x_locks_the_x_arrow_and_the_two_edge_on_rings() {
    let mut camera = ViewCamera::new(Vec3::ZERO, 1.0);
    camera.snap_to_axis(ViewAxis::Right);
    let (spatial, part) = scene(skewed(), Transform3D::Identity);
    let built = frame(&spatial, part, &camera, 600.0).unwrap();
    assert_eq!(built.fixed_axes, [true, false, false]);
    assert_eq!(built.fixed_rings, [false, true, true]);
}

#[test]
fn visibility_hover_and_drag_state_pass_through() {
    let (spatial, part) = scene(skewed(), Transform3D::Identity);
    let visibility = HandleShowing::Stretching { axis: Axis::Y };
    let hover = Some(HandleHover::Turn(Axis::Z));
    let drag = Some(LiveDrag::Stretch { axis: Axis::X, reach: 1.5 });
    let built = handle_frame_for(
        &spatial,
        &ViewCamera::new(Vec3::ZERO, 1.0),
        part,
        600.0,
        visibility,
        hover,
        drag,
    )
    .unwrap();
    assert_eq!((built.showing, built.under_pointer), (visibility, hover));
    let Some(LiveDrag::Stretch { axis: Axis::X, reach }) = built.live_drag else {
        panic!("{:?}", built.live_drag);
    };
    assert_eq!(reach, 1.5);
}

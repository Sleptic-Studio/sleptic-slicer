// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use std::sync::Arc;

use float_cmp::assert_approx_eq;
use sc_mesh_core::transform::AffineTransform3D;
use sc_scene::{LeafPayload, PLATE_GAP_MM, Scene};

use super::*;
use crate::build_volume::BuildVolume;

const FOV: f32 = 0.8;

fn down_at(x: f32, y: f32) -> PickRay {
    PickRay {
        start: Vec3::new(x, y, 100.0),
        heading: -Vec3::Z,
    }
}

fn affine(a: AffineTransform3D) -> Transform3D {
    Transform3D::Affine(a)
}

/// Parts are 1 mm cubes centred on x, y, from z = -0.5 to 0.5, under each
/// transform, in that order. A second list puts parts on a second plate.
fn scene_on_plates(first: &[Transform3D], second: &[Transform3D]) -> (SceneSpatial, Vec<NodeId>) {
    let mut scene = Scene::new();
    let mut plates = scene.plates().clone();
    let other = plates.add();
    let mut ids = Vec::new();
    for (on_second, transform) in first.iter().map(|t| (false, t)).chain(second.iter().map(|t| (true, t))) {
        let mesh = Arc::new(sc_mesh_fixtures::unit_cube_mesh());
        let id = scene.add_leaf(None, transform.clone(), LeafPayload::Mesh(mesh)).unwrap();
        scene.add_root(id).unwrap();
        if on_second {
            plates.assignments.insert(id, other);
        }
        ids.push(id);
    }
    assert!(scene.set_plates(plates));
    (SceneSpatial::from_graph(scene), ids)
}

fn scene(parts: &[Transform3D]) -> (SceneSpatial, Vec<NodeId>) {
    scene_on_plates(parts, &[])
}

fn at(x: f64, y: f64, z: f64) -> Transform3D {
    affine(AffineTransform3D::from_translation([x, y, z]))
}

#[test]
fn the_centre_pixel_looks_at_the_target_in_both_projections() {
    let mut camera = ViewCamera::new(Vec3::new(5.0, -3.0, 2.0), 20.0);
    for perspective in [true, false] {
        camera.is_perspective = perspective;
        let ray = ray_through_pixel(&camera, FOV, 800.0, 600.0, 400.0, 300.0);
        let to_target = (camera.look_at - ray.start).normalize();
        assert!(to_target.dot(ray.heading) > 0.99999, "{perspective}");
        assert_approx_eq!(f32, ray.heading.length(), 1.0, epsilon = 1e-6);
    }
    // Orthographic rays are parallel and start behind the scene.
    camera.is_perspective = false;
    let a = ray_through_pixel(&camera, FOV, 800.0, 600.0, 10.0, 20.0);
    let b = ray_through_pixel(&camera, FOV, 800.0, 600.0, 700.0, 500.0);
    assert_eq!(a.heading, b.heading);
    assert!((camera.look_at - a.start).dot(a.heading) > 9_000.0);
}

#[test]
fn projecting_a_point_and_casting_back_lands_on_it() {
    let mut camera = ViewCamera::new(Vec3::ZERO, 30.0);
    camera.turn_by_drag(80.0, -30.0, 600.0);
    let point = Vec3::new(7.0, -4.0, 3.0);
    for perspective in [true, false] {
        camera.is_perspective = perspective;
        let (x, y) = screen_point_of(&camera, FOV, 800.0, 600.0, point).unwrap();
        let ray = ray_through_pixel(&camera, FOV, 800.0, 600.0, x, y);
        let along = (point - ray.start).dot(ray.heading);
        assert!((ray.start + ray.heading * along).distance(point) < 1e-2, "{perspective}");
    }
    camera.is_perspective = true;
    assert!(screen_point_of(&camera, FOV, 800.0, 600.0, camera.eye_position() * 2.0).is_none());
}

#[test]
fn the_plate_is_hit_ahead_inside_its_edges() {
    assert_eq!(level_plane_crossing(&down_at(3.0, 4.0), 1.5), Some((3.0, 4.0)));
    let up = PickRay { start: Vec3::new(0.0, 0.0, 1.0), heading: Vec3::Z };
    assert_eq!(level_plane_crossing(&up, 0.0), None);
    let flat = PickRay { start: Vec3::ZERO, heading: Vec3::X };
    assert_eq!(level_plane_crossing(&flat, 0.0), None);
    let volume = BuildVolume::new(100.0, 60.0, 80.0);
    assert!(lands_on_plate(&down_at(50.0, -30.0), volume));
    assert!(!lands_on_plate(&down_at(50.1, 0.0), volume));
    assert!(!lands_on_plate(&down_at(0.0, 30.1), volume));
    assert_eq!(menu_for_click(true, true), ClickMenu::OnPart);
    assert_eq!(menu_for_click(false, true), ClickMenu::OnPlate);
    assert_eq!(menu_for_click(false, false), ClickMenu::Elsewhere);
}

#[test]
fn a_box_is_entered_at_its_near_face_or_at_zero_from_inside() {
    let (min, max) = (Vec3::splat(-1.0), Vec3::splat(1.0));
    assert_eq!(box_entry_distance(&down_at(0.5, 0.5), min, max), Some(99.0));
    assert_eq!(box_entry_distance(&down_at(1.5, 0.0), min, max), None);
    let inside = PickRay { start: Vec3::ZERO, heading: Vec3::X };
    assert_eq!(box_entry_distance(&inside, min, max), Some(0.0));
    let behind = PickRay { start: Vec3::new(0.0, 0.0, 5.0), heading: Vec3::Z };
    assert!(!crosses_box(&behind, min, max));
    // An axis-parallel ray (two zero components) still works both ways.
    let along = PickRay { start: Vec3::new(-5.0, 0.0, 0.0), heading: Vec3::X };
    assert_eq!(box_entry_distance(&along, min, max), Some(4.0));
    let beside = PickRay { start: Vec3::new(-5.0, 2.0, 0.0), heading: Vec3::X };
    assert_eq!(box_entry_distance(&beside, min, max), None);
}

#[test]
fn triangles_are_hit_from_the_front_only_and_slivers_never() {
    let (a, b, c) = (Vec3::ZERO, Vec3::X, Vec3::Y);
    // Counter-clockwise from above: the front faces up.
    assert_approx_eq!(f32, front_face_distance(&down_at(0.2, 0.2), a, b, c, 1.0).unwrap(), 100.0);
    assert!(front_face_distance(&down_at(0.2, 0.2), a, c, b, 1.0).is_none());
    // A mirrored part flips which side is the front.
    assert!(front_face_distance(&down_at(0.2, 0.2), a, c, b, -1.0).is_some());
    assert!(front_face_distance(&down_at(0.2, 0.2), a, b, c, -1.0).is_none());
    // Outside the edges, and a near-zero-area sliver.
    assert!(front_face_distance(&down_at(0.6, 0.6), a, b, c, 1.0).is_none());
    assert!(front_face_distance(&down_at(-0.1, 0.2), a, b, c, 1.0).is_none());
    let sliver = Vec3::new(1.0, 1e-9, 0.0);
    assert!(front_face_distance(&down_at(0.5, 0.0), a, b, sliver, 1.0).is_none());
    // A hit at the ray's own start does not count.
    let start = PickRay { start: Vec3::new(0.2, 0.2, 0.0), heading: -Vec3::Z };
    assert!(front_face_distance(&start, a, b, c, 1.0).is_none());
}

#[test]
fn the_nearer_part_is_picked_at_its_world_distance() {
    let (spatial, ids) = scene(&[at(0.0, 0.0, 0.0), at(0.0, 0.0, 5.0)]);
    assert_eq!(part_under_ray(&down_at(0.0, 0.0), &spatial), Some(ids[1]));
    let (_, t) = first_part_hit(&down_at(0.0, 0.0), &spatial, None).unwrap();
    assert_approx_eq!(f32, t, 100.0 - 5.5, epsilon = 1e-4);
    assert_eq!(part_under_ray(&down_at(3.0, 0.0), &spatial), None);
    // Scaled up 4 times: hit at the scaled top, distance in world units.
    let scaled = affine(AffineTransform3D::from_non_uniform_scale([4.0, 4.0, 4.0]));
    let (spatial, ids) = scene(&[scaled]);
    let (id, t) = first_part_hit(&down_at(1.5, 1.5), &spatial, None).unwrap();
    assert_eq!(id, ids[0]);
    assert_approx_eq!(f32, t, 98.0, epsilon = 1e-4);
}

#[test]
fn mirrored_parts_are_picked_and_flattened_ones_never() {
    let mirrored = affine(AffineTransform3D::from_non_uniform_scale([-1.0, 1.0, 1.0]));
    let (spatial, ids) = scene(&[mirrored]);
    // Which face is hit follows the renderer's culling; see STATUS (mirrored
    // parts in the viewer) before pinning a distance here.
    let (id, _) = first_part_hit(&down_at(0.1, 0.1), &spatial, None).unwrap();
    assert_eq!(id, ids[0]);
    let flat = affine(AffineTransform3D::from_non_uniform_scale([1.0, 1.0, 0.0]));
    let (spatial, _) = scene(&[flat]);
    assert_eq!(part_under_ray(&down_at(0.0, 0.0), &spatial), None);
}

#[test]
fn a_part_on_a_later_plate_is_picked_where_that_plate_is_drawn() {
    let (spatial, ids) = scene_on_plates(&[at(0.0, 0.0, 0.0)], &[at(0.0, 0.0, 0.0)]);
    let size = (100.0, 80.0);
    // Two plates sit side by side: the second is a plate width and a gap right.
    let second_x = 100.0 + PLATE_GAP_MM;
    assert_eq!(part_under_ray_on_plates(&down_at(0.0, 0.0), &spatial, Some(size)), Some(ids[0]));
    assert_eq!(part_under_ray_on_plates(&down_at(second_x, 0.0), &spatial, Some(size)), Some(ids[1]));
    // The surface point comes back in the plate's own coordinates.
    let (id, point) = part_surface_point(&down_at(second_x + 0.25, 0.1), &spatial, size).unwrap();
    assert_eq!(id, ids[1]);
    assert!(point.distance(Vec3::new(0.25, 0.1, 0.5)) < 1e-4, "{point}");
    assert!(part_surface_point(&down_at(second_x + 3.0, 0.0), &spatial, size).is_none());
}

#[test]
fn turned_parts_are_tested_by_their_turned_box() {
    // A cube turned 45° about Z reaches √½ along X; just inside that, a hit.
    let turned = affine(AffineTransform3D::from_rotation_z(std::f64::consts::FRAC_PI_4));
    let (spatial, ids) = scene(&[turned]);
    assert_eq!(part_under_ray(&down_at(0.69, 0.0), &spatial), Some(ids[0]));
    assert_eq!(part_under_ray(&down_at(0.6, 0.6), &spatial), None);
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use float_cmp::assert_approx_eq;

use super::*;

fn ray(origin: Vec3, towards: Vec3) -> PickRay {
    PickRay {
        start: origin,
        heading: (towards - origin).normalize(),
    }
}

/// A ray leaving the origin `degrees` away from +X, in the XZ plane.
fn tilted_from_x(degrees: f32) -> PickRay {
    let a = degrees.to_radians();
    PickRay {
        start: Vec3::ZERO,
        heading: Vec3::new(a.cos(), 0.0, a.sin()),
    }
}

#[test]
fn arrows_lock_within_eight_degrees_of_the_line_of_sight_either_way() {
    assert!(axis_pickable(&tilted_from_x(90.0), Axis::X));
    for degrees in [0.0, 5.0, 7.9, 180.0, 175.0] {
        assert!(!axis_pickable(&tilted_from_x(degrees), Axis::X), "{degrees}");
    }
    for degrees in [8.1, 30.0, 171.9] {
        assert!(axis_pickable(&tilted_from_x(degrees), Axis::X), "{degrees}");
    }
}

#[test]
fn rings_lock_within_eight_degrees_of_edge_on_either_way() {
    // Looking along X shows the X ring face-on.
    assert!(arc_pickable(&tilted_from_x(0.0), Axis::X));
    for degrees in [90.0, 85.0, 95.0, 82.1, 270.0] {
        assert!(!arc_pickable(&tilted_from_x(degrees), Axis::X), "{degrees}");
    }
    for degrees in [81.9, 98.1, 0.0, 180.0] {
        assert!(arc_pickable(&tilted_from_x(degrees), Axis::X), "{degrees}");
    }
}

#[test]
fn the_cylinder_is_entered_at_its_wall_inside_its_caps() {
    // Down onto the X axis at s = 5 from z = 10, radius 0.5: enters at z = 0.5.
    let down = ray(Vec3::new(5.0, 0.0, 10.0), Vec3::new(5.0, 0.0, 0.0));
    let entered = ray_enters_cylinder(&down, Vec3::ZERO, Vec3::X, 4.0, 6.0, 0.5).unwrap();
    assert_approx_eq!(f32, entered, 9.5, epsilon = 1e-5);
    // Outside the radius, outside the caps, facing away, starting inside.
    let beside = ray(Vec3::new(5.0, 1.0, 10.0), Vec3::new(5.0, 1.0, 0.0));
    assert!(ray_enters_cylinder(&beside, Vec3::ZERO, Vec3::X, 4.0, 6.0, 0.5).is_none());
    assert!(ray_enters_cylinder(&down, Vec3::ZERO, Vec3::X, 6.5, 8.0, 0.5).is_none());
    let away = ray(Vec3::new(5.0, 0.0, 10.0), Vec3::new(5.0, 0.0, 20.0));
    assert!(ray_enters_cylinder(&away, Vec3::ZERO, Vec3::X, 4.0, 6.0, 0.5).is_none());
    let inside = ray(Vec3::new(5.0, 0.0, 0.1), Vec3::new(5.0, 0.0, 10.0));
    assert!(ray_enters_cylinder(&inside, Vec3::ZERO, Vec3::X, 4.0, 6.0, 0.5).is_none());
    // Along the axis from beyond the cap: enters at the cap.
    let end_on = ray(Vec3::new(10.0, 0.1, 0.0), Vec3::new(0.0, 0.1, 0.0));
    assert_approx_eq!(
        f32,
        ray_enters_cylinder(&end_on, Vec3::ZERO, Vec3::X, 4.0, 6.0, 0.5).unwrap(),
        4.0,
        epsilon = 1e-5
    );
}

fn from_above(x: f32, y: f32) -> PickRay {
    ray(Vec3::new(x, y, 10.0), Vec3::new(x, y, 0.0))
}

#[test]
fn only_an_arrow_head_takes_the_click() {
    // Length 2, radius 0.2: the head spans 2 to 2.5 along each axis.
    let pivot = Vec3::new(1.0, 1.0, 0.0);
    let pick = |x: f32, y: f32| handle_under(&from_above(x, y), pivot, 2.0, 0.2).map(|(axis, _)| axis);
    assert_eq!(pick(1.0 + 2.2, 1.0), Some(Axis::X));
    assert_eq!(pick(1.0, 1.0 + 2.4), Some(Axis::Y));
    // On the shaft, beyond the head, in empty space.
    assert_eq!(pick(1.0 + 1.0, 1.0), None);
    assert_eq!(pick(1.0 + 2.7, 1.0), None);
    assert_eq!(pick(5.0, 5.0), None);
    // Z points at the camera from above: locked even through its head.
    assert_eq!(handle_under(&ray(Vec3::new(1.0, 1.0, 10.0), pivot), pivot, 2.0, 0.2), None);
}

#[test]
fn the_nearest_handle_wins_where_two_overlap_on_screen() {
    // Looking along -Y-ish from the side, the X and Z heads both sit on the
    // line of sight; the X head is nearer.
    let eye = Vec3::new(2.2, -10.0, 2.2);
    let through_x_head = ray(eye, Vec3::new(2.2, 0.0, 0.0));
    let (axis, t) = handle_under(&through_x_head, Vec3::ZERO, 2.0, 0.3).unwrap();
    assert_eq!(axis, Axis::X);
    assert!(t > 0.0);
    // Equal distances go to the earlier axis.
    assert_eq!(nearest(|_| true, |_| Some(1.0)), Some((Axis::X, 1.0)));
    assert_eq!(nearest(|a| a != Axis::X, |_| Some(1.0)), Some((Axis::Y, 1.0)));
    assert_eq!(nearest(|_| true, |a| (a == Axis::Z).then_some(3.0)), Some((Axis::Z, 3.0)));
}

#[test]
fn a_ring_is_hit_anywhere_round_its_circle() {
    let pivot = Vec3::new(1.0, 2.0, 0.0);
    // The Z ring from above: radius 2, band 0.1.
    for degrees in [0.0f32, 45.0, 135.0, 270.0] {
        let a = degrees.to_radians();
        let on = from_above(pivot.x + 2.0 * a.cos(), pivot.y + 2.0 * a.sin());
        let (axis, t) = ring_under(&on, pivot, 2.0, 0.1).unwrap();
        assert_eq!(axis, Axis::Z);
        assert_approx_eq!(f32, t, 10.0, epsilon = 1e-4);
    }
    // Inside, outside, nowhere.
    for (x, y) in [(pivot.x + 1.5, pivot.y), (pivot.x + 2.5, pivot.y), (9.0, 9.0)] {
        assert!(ray_hits_rotate_ring(&from_above(x, y), pivot, Axis::Z, 2.0, 0.1).is_none());
    }
    assert!(ray_hits_rotate_ring(&from_above(pivot.x + 2.0, pivot.y), pivot, Axis::Z, 0.0, 0.1).is_none());
    // The edge-on X and Y rings are never offered from straight above.
    assert_eq!(ring_under(&from_above(pivot.x, pivot.y + 2.0), pivot, 2.0, 0.1).map(|h| h.0), Some(Axis::Z));
}

#[test]
fn a_tilted_ring_has_a_wider_band_up_to_its_cap() {
    let pivot = Vec3::ZERO;
    // Seen 60° off face-on (facing 0.5): band 0.1 widens to 0.2.
    let a = 60f32.to_radians();
    let eye = Vec3::new(3.0 + 10.0 * a.sin(), 0.0, 10.0 * a.cos());
    let point = |r: f32| ray(eye, Vec3::new(r, 0.0, 0.0));
    assert!(ray_hits_rotate_ring(&point(2.15), pivot, Axis::Z, 2.0, 0.1).is_some());
    assert!(ray_hits_rotate_ring(&point(2.25), pivot, Axis::Z, 2.0, 0.1).is_none());
}

#[test]
fn scale_cubes_sit_at_the_shaft_ends_and_move_with_the_pivot() {
    let pivot = Vec3::new(10.0, -5.0, 0.0);
    let pick = |x: f32, y: f32| cube_under(&from_above(x, y), pivot, 2.0, 0.4).map(|(axis, _)| axis);
    assert_eq!(pick(pivot.x + 2.0, pivot.y), Some(Axis::X));
    assert_eq!(pick(pivot.x + 2.15, pivot.y + 0.15), Some(Axis::X));
    assert_eq!(pick(pivot.x, pivot.y + 2.0), Some(Axis::Y));
    // Not at the shaft's middle, not beside the cube.
    assert_eq!(pick(pivot.x + 1.0, pivot.y), None);
    assert_eq!(pick(pivot.x + 2.0, pivot.y + 0.3), None);
    // The Z cube is straight under the camera, which looks along Z: locked.
    assert_eq!(pick(pivot.x, pivot.y), None);
    // From the side, the nearer of two cubes in line wins.
    let eye = Vec3::new(pivot.x + 2.0, pivot.y - 20.0, 0.0);
    let (axis, _) = cube_under(&ray(eye, Vec3::new(pivot.x + 2.0, pivot.y + 2.0, 0.0)), pivot, 2.0, 0.4).unwrap();
    assert_eq!(axis, Axis::X);
}

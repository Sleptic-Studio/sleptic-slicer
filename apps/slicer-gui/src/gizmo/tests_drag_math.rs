// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use std::f32::consts::{FRAC_PI_2, PI};

use float_cmp::assert_approx_eq;

use super::*;
use crate::gizmo::axis::ALL_AXES;
use crate::gizmo::LIMITS;

fn ray(origin: [f32; 3], direction: Vec3) -> PickRay {
    PickRay {
        start: Vec3::from(origin),
        heading: direction.normalize(),
    }
}

fn near(found: Vec3, expected: [f32; 3]) {
    for (f, e) in found.to_array().into_iter().zip(expected) {
        assert_approx_eq!(f32, f, e, epsilon = 1e-5);
    }
}

#[test]
fn the_axis_point_is_where_the_ray_passes_nearest() {
    // Straight down through x = 2, and down a line beside the axis.
    near(closest_point_on_axis(&ray([2.0, 0.0, 5.0], -Vec3::Z), Vec3::ZERO, Axis::X).unwrap(), [2.0, 0.0, 0.0]);
    near(closest_point_on_axis(&ray([3.0, 4.0, -5.0], Vec3::Z), Vec3::ZERO, Axis::X).unwrap(), [3.0, 0.0, 0.0]);
    // Around a pivot elsewhere, along Y, from an oblique ray through (5, 7, 9).
    let pivot = Vec3::new(5.0, 1.0, 9.0);
    let oblique = ray([5.0, 7.0, 9.0], Vec3::new(1.0, 0.0, 1.0));
    near(closest_point_on_axis(&oblique, pivot, Axis::Y).unwrap(), [5.0, 7.0, 9.0]);
}

#[test]
fn moving_the_mouse_along_the_axis_moves_the_point_by_as_much() {
    let camera = Vec3::new(0.0, -10.0, 4.0);
    let point = |x: f32| {
        let target = Vec3::new(x, 0.0, 0.0);
        closest_point_on_axis(&ray(camera.to_array(), target - camera), Vec3::ZERO, Axis::X).unwrap()
    };
    assert_approx_eq!(f32, point(3.5).x - point(1.0).x, 2.5, epsilon = 1e-4);
}

#[test]
fn rays_within_eight_degrees_of_the_axis_give_nothing() {
    for direction in [Vec3::X, -Vec3::X] {
        assert!(closest_point_on_axis(&ray([0.0, 1.0, 0.0], direction), Vec3::ZERO, Axis::X).is_none());
    }
    let tilted = |degrees: f32| {
        let a = degrees.to_radians();
        ray([0.0, 0.0, 5.0], Vec3::new(a.cos(), 0.0, -a.sin()))
    };
    assert!(closest_point_on_axis(&tilted(7.0), Vec3::ZERO, Axis::X).is_none());
    assert!(closest_point_on_axis(&tilted(9.0), Vec3::ZERO, Axis::X).is_some());
}

#[test]
fn the_plane_is_hit_in_front_only() {
    let pivot = Vec3::new(1.0, 2.0, 3.0);
    near(ray_hits_axis_plane(&ray([4.0, 5.0, 10.0], -Vec3::Z), pivot, Axis::Z).unwrap(), [4.0, 5.0, 3.0]);
    assert!(ray_hits_axis_plane(&ray([4.0, 5.0, 10.0], Vec3::Z), pivot, Axis::Z).is_none());
    assert!(ray_hits_axis_plane(&ray([4.0, 5.0, 10.0], Vec3::X), pivot, Axis::Z).is_none());
}

#[test]
fn angles_turn_counter_clockwise_about_each_axis() {
    for axis in ALL_AXES {
        let (u, v) = axis_basis(axis);
        let pivot = Vec3::new(3.0, -2.0, 1.0);
        let n = axis.unit_vector();
        let angle = |towards: Vec3| {
            let origin = pivot + towards * 2.0 + n * 5.0;
            rotate_drag_angle(&ray(origin.to_array(), -n), pivot, axis).unwrap()
        };
        assert_approx_eq!(f32, angle(u), 0.0, epsilon = 1e-5);
        assert_approx_eq!(f32, angle(v), FRAC_PI_2, epsilon = 1e-5);
        assert_approx_eq!(f32, angle(-u).abs(), PI, epsilon = 1e-5);
        assert_approx_eq!(f32, angle(-v), -FRAC_PI_2, epsilon = 1e-5);
    }
    // Edge-on or facing away: no angle.
    let pivot = Vec3::ZERO;
    assert!(rotate_drag_angle(&ray([1.0, 0.0, 5.0], Vec3::X), pivot, Axis::Z).is_none());
    assert!(rotate_drag_angle(&ray([1.0, 0.0, 5.0], Vec3::Z), pivot, Axis::Z).is_none());
}

#[test]
fn axis_scale_is_the_ratio_along_the_axis_within_limits() {
    let pivot = Vec3::new(10.0, 10.0, 10.0);
    let at = |x: f32, y: f32| pivot + Vec3::new(x, y, 0.0);
    assert_eq!(axis_stretch(at(2.0, 0.0), at(3.0, 0.0), pivot, Axis::X), 1.5);
    // Only the axis component counts.
    assert_eq!(axis_stretch(at(2.0, 7.0), at(4.0, -3.0), pivot, Axis::X), 2.0);
    assert_eq!(axis_stretch(at(2.0, 0.0), at(0.0, 0.0), pivot, Axis::X), LIMITS.min_scale);
    assert_eq!(axis_stretch(at(2.0, 0.0), at(-2.0, 0.0), pivot, Axis::X), LIMITS.min_scale);
    assert_eq!(axis_stretch(at(0.001, 0.0), at(100.0, 0.0), pivot, Axis::X), 1.0 / LIMITS.min_scale);
    assert_eq!(axis_stretch(at(0.0, 5.0), at(9.0, 0.0), pivot, Axis::X), 1.0);
}

#[test]
fn uniform_scale_follows_the_press_line_and_never_grows_past_the_pivot() {
    // Pivot at (100, 100), pressed 10 px to the right.
    let factor = |x: f32, y: f32| even_stretch(100.0, 100.0, 110.0, 100.0, x, y);
    assert_eq!(factor(120.0, 100.0), 2.0);
    assert_eq!(factor(105.0, 100.0), 0.5);
    // Sideways movement does not count.
    assert_eq!(factor(120.0, 160.0), 2.0);
    // Through the pivot and beyond: held at the minimum, never growing back.
    for x in [100.0, 95.0, 50.0, -500.0] {
        assert_eq!(factor(x, 100.0), LIMITS.min_scale);
    }
    assert_eq!(factor(100_000.0, 100.0), 1.0 / LIMITS.min_scale);
    // Diagonal press: measured along the diagonal.
    assert_approx_eq!(f32, even_stretch(0.0, 0.0, 3.0, 4.0, 6.0, 8.0), 2.0, ulps = 2);
    assert_eq!(even_stretch(5.0, 5.0, 5.0, 5.0, 50.0, 50.0), 1.0);
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use float_cmp::assert_approx_eq;

use super::*;

const FOV: f32 = 0.8;

fn camera() -> ViewCamera {
    ViewCamera::new(Vec3::new(10.0, 20.0, 5.0), 50.0)
}

fn near(a: Vec3, b: Vec3, epsilon: f32) {
    assert!(a.distance(b) <= epsilon, "{a} vs {b}");
}

#[test]
fn home_looks_down_the_front_right_top_corner_from_three_radii() {
    let mut c = camera();
    c.turn_by_drag(120.0, -40.0, 600.0);
    c.shift_by_drag(30.0, 10.0, FOV, 600.0);
    c.home_view();
    assert_eq!(c.look_at, Vec3::new(10.0, 20.0, 5.0));
    assert_eq!(c.eye_distance, 150.0);
    // The eye sits along (1, -1, 1): right of the front (-Y), above.
    let toward_eye = (c.eye_position() - c.look_at).normalize();
    near(toward_eye, Vec3::new(1.0, -1.0, 1.0).normalize(), 1e-6);
    // The same view the cube's corner gives.
    let mut corner = camera();
    corner.look_from_direction(Vec3::new(1.0, -1.0, 1.0));
    assert_approx_eq!(f32, corner.azimuth, c.azimuth, epsilon = 1e-6);
    assert_approx_eq!(f32, corner.elevation, c.elevation, epsilon = 1e-6);
}

#[test]
fn axis_views_aim_at_the_scene_along_each_axis() {
    for (view, toward_eye) in [
        (ViewAxis::Front, -Vec3::Y),
        (ViewAxis::Back, Vec3::Y),
        (ViewAxis::Right, Vec3::X),
        (ViewAxis::Left, -Vec3::X),
        (ViewAxis::Top, Vec3::Z),
        (ViewAxis::Bottom, -Vec3::Z),
    ] {
        let mut c = camera();
        c.shift_by_drag(100.0, 0.0, FOV, 600.0);
        c.snap_to_axis(view);
        assert_eq!(c.look_at, Vec3::new(10.0, 20.0, 5.0));
        near((c.eye_position() - c.look_at).normalize(), toward_eye, 2e-3);
    }
    // From above, the plate's front (-Y) is at the bottom of the view.
    let mut top = camera();
    top.look_from_direction(Vec3::Z);
    let (right, up) = top.right_and_up();
    near(right, Vec3::X, 1e-3);
    near(up, Vec3::Y, 1e-3);
    // A zero direction changes nothing.
    let before = top.azimuth;
    top.look_from_direction(Vec3::ZERO);
    assert_eq!(top.azimuth, before);
}

#[test]
fn orbiting_stops_short_of_the_poles() {
    let mut c = camera();
    c.turn_by_drag(0.0, 1e6, 600.0);
    assert_eq!(c.elevation, FRAC_PI_2 - POLE_MARGIN);
    c.turn_by_drag(0.0, -1e6, 600.0);
    assert_eq!(c.elevation, -(FRAC_PI_2 - POLE_MARGIN));
    // The full view height turns half a circle.
    let yaw = c.azimuth;
    c.turn_by_drag(600.0, 0.0, 600.0);
    assert_approx_eq!(f32, yaw - c.azimuth, PI, epsilon = 1e-5);
}

#[test]
fn panning_moves_the_scene_with_the_mouse_even_over_the_poles() {
    let mut c = camera();
    let before = c.look_at;
    c.shift_by_drag(10.0, 0.0, FOV, 600.0);
    let (right, _) = c.right_and_up();
    near(c.look_at - before, -right * 10.0 * c.mm_per_pixel(FOV, 600.0), 1e-4);
    // At the clamp and just below it, a pan goes the same way.
    for pitch in [1.0, -1.0] {
        let mut at_pole = camera();
        at_pole.turn_by_drag(0.0, pitch * 1e6, 600.0);
        let mut below = at_pole.clone();
        below.elevation -= pitch * 0.01;
        for c in [&mut at_pole, &mut below] {
            c.look_at = Vec3::ZERO;
            c.shift_by_drag(10.0, 5.0, FOV, 600.0);
        }
        assert!(at_pole.look_at.normalize().dot(below.look_at.normalize()) > 0.99);
    }
}

#[test]
fn zoom_keeps_the_point_under_the_cursor_and_respects_its_limits() {
    let mut c = camera();
    let target = c.look_at;
    c.zoom_about(0.5, 400.0, 300.0, FOV, 800.0, 600.0);
    assert_eq!(c.eye_distance, 75.0);
    near(c.look_at, target, 1e-4);
    // Off centre the target moves towards the cursor's point.
    c.zoom_about(0.5, 700.0, 300.0, FOV, 800.0, 600.0);
    let (right, _) = c.right_and_up();
    assert!((c.look_at - target).dot(right) > 0.0);
    // Limits: 50 radii out; in, 5% of the detail radius or 3% of the view
    // radius, whichever is larger. Factors are clamped to 0.1..10 per call.
    for _ in 0..20 {
        c.zoom_about(100.0, 400.0, 300.0, FOV, 800.0, 600.0);
    }
    assert_eq!(c.eye_distance, 50.0 * 50.0);
    for _ in 0..40 {
        c.zoom_about(0.0, 400.0, 300.0, FOV, 800.0, 600.0);
    }
    assert_eq!(c.eye_distance, 50.0 * 0.05);
    let mut small = ViewCamera::framing_with_detail(Vec3::ZERO, 100.0, 1.0);
    assert_eq!(small.eye_distance, 300.0);
    for _ in 0..40 {
        small.zoom_about(0.0, 400.0, 300.0, FOV, 800.0, 600.0);
    }
    assert_approx_eq!(f32, small.eye_distance, 100.0 * 0.03, ulps = 2);
    let mut tiny = ViewCamera::framing_with_detail(Vec3::ZERO, 100.0, 70.0);
    for _ in 0..40 {
        tiny.zoom_about(0.0, 400.0, 300.0, FOV, 800.0, 600.0);
    }
    assert_approx_eq!(f32, tiny.eye_distance, 70.0 * 0.05, ulps = 2);
}

#[test]
fn framing_a_box_centres_it_but_not_closer_than_the_detail_allows() {
    let mut c = ViewCamera::framing_with_detail(Vec3::ZERO, 100.0, 5.0);
    let yaw = c.azimuth;
    c.zoom_to_box(Vec3::new(0.0, 0.0, 0.0), Vec3::new(30.0, 40.0, 0.0));
    assert_eq!(c.look_at, Vec3::new(15.0, 20.0, 0.0));
    assert_eq!(c.eye_distance, 25.0 * HOME_RANGE_FACTOR * 0.8);
    assert_eq!(c.azimuth, yaw);
    c.zoom_to_box(Vec3::ZERO, Vec3::splat(0.1));
    assert_eq!(c.eye_distance, 5.0 * HOME_RANGE_FACTOR * 0.8);
}

#[test]
fn orthographic_shows_the_target_at_perspective_size_and_never_clips() {
    let mut c = camera();
    let point = c.look_at + c.right_and_up().0 * 10.0;
    let on_screen = |c: &ViewCamera| {
        let clip = c.view_to_clip(FOV, 1.5, 1.0, 5000.0) * c.world_to_view() * point.extend(1.0);
        clip.x / clip.w
    };
    let perspective = on_screen(&c);
    c.is_perspective = false;
    assert_approx_eq!(f32, on_screen(&c), perspective, epsilon = 1e-4);
    // Depth covers from behind the eye to past the target.
    let depth = |p: Vec3| {
        let clip = c.view_to_clip(FOV, 1.5, 1.0, 5000.0) * c.world_to_view() * p.extend(1.0);
        clip.z / clip.w
    };
    let forward = (c.look_at - c.eye_position()).normalize();
    for p in [c.eye_position() - forward * 100.0, c.look_at, c.look_at + forward * 100.0] {
        assert!((0.0..=1.0).contains(&depth(p)), "{p}");
    }
}

#[test]
fn degenerate_radii_are_raised_to_a_micron() {
    let c = ViewCamera::framing_with_detail(Vec3::ZERO, 0.0, -1.0);
    assert_eq!(c.eye_distance, 1e-3 * HOME_RANGE_FACTOR);
    assert_eq!(c.mm_per_pixel(FOV, 0.0), c.mm_per_pixel(FOV, 1.0));
}

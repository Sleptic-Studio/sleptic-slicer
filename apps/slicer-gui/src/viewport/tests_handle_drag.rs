// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

use super::*;

fn cast_from_above(x: f32, y: f32) -> picking::PickRay {
    picking::PickRay {
        start: Vec3::new(x, y, 10.0),
        heading: -Vec3::Z,
    }
}

#[test]
fn reanchoring_ignores_non_scale_gestures() {
    let camera = ViewCamera::new(Vec3::ZERO, 100.0);
    let ctx = ReanchorView {
        lens: &camera,
        pivot_point: Vec3::ZERO,
        fov_y: 1.0,
        view_px: (800.0, 600.0),
        cursor_px: (400.0, 300.0),
    };
    let op = DragKind::Translate { axis: Axis::X, anchor: Vec3::ZERO };
    assert!(reanchor_axis_scale(op, Transform3D::Identity, &ctx, true).is_none());
}

fn displacement(t: Transform3D) -> [f64; 3] {
    match t {
        Transform3D::Identity => [0.0; 3],
        Transform3D::Affine(a) => *a.translation(),
    }
}

#[test]
fn plate_motion_rounds_only_when_snap_is_enabled() {
    for (snap, expected) in [(false, [1.75, -1.25, 0.0]), (true, [2.0, -1.0, 0.0])] {
        let op = DragKind::Plate {
            anchor: Vec3::new(2.0, 3.0, 0.0),
            snap,
            press: (0.0, 0.0),
            activated: true,
        };
        let (result, _) = drag_to_transform(
            op,
            &cast_from_above(3.75, 1.75),
            Vec3::ZERO,
            &Transform3D::Identity,
            &Transform3D::Identity,
            0.0,
            0.0,
        )
        .unwrap();
        assert_eq!(displacement(result), expected);
    }
}

#[test]
fn plate_motion_rejects_a_singular_parent() {
    let op = DragKind::Plate {
        anchor: Vec3::ZERO,
        snap: false,
        press: (0.0, 0.0),
        activated: true,
    };
    let parent = Transform3D::Affine(AffineTransform3D::from_uniform_scale(0.0));
    assert!(
        drag_to_transform(
            op,
            &cast_from_above(1.0, 2.0),
            Vec3::ZERO,
            &parent,
            &Transform3D::Identity,
            0.0,
            0.0
        )
        .is_none()
    );
}

#[test]
fn plate_motion_converts_world_delta_through_parent_scale() {
    let op = DragKind::Plate {
        anchor: Vec3::ZERO,
        snap: false,
        press: (0.0, 0.0),
        activated: true,
    };
    let parent = Transform3D::Affine(AffineTransform3D::from_non_uniform_scale([2.0, 4.0, 1.0]));
    let (result, _) = drag_to_transform(
        op,
        &cast_from_above(6.0, 8.0),
        Vec3::ZERO,
        &parent,
        &Transform3D::Identity,
        0.0,
        0.0,
    )
    .unwrap();
    assert_eq!(displacement(result), [3.0, 2.0, 0.0]);
}

#[test]
fn plane_move_tracks_the_intersection_from_its_press_anchor() {
    let op = DragKind::TranslatePlane {
        normal: Axis::Z,
        anchor: Vec3::new(1.0, 2.0, 0.0),
    };
    let (result, _) = drag_to_transform(
        op,
        &cast_from_above(4.0, 6.0),
        Vec3::ZERO,
        &Transform3D::Identity,
        &Transform3D::Identity,
        0.0,
        0.0,
    )
    .unwrap();
    assert_eq!(displacement(result), [3.0, 4.0, 0.0]);
}

#[test]
fn centre_scale_uses_signed_screen_displacement() {
    let op = DragKind::ScaleUniform {
        press_screen: (0.0, 0.0),
    };
    for (x, y, factor) in [(120.0, 0.0, 2.0), (0.0, 120.0, 0.5)] {
        let (result, _) = drag_to_transform(
            op,
            &cast_from_above(0.0, 0.0),
            Vec3::ZERO,
            &Transform3D::Identity,
            &Transform3D::Identity,
            x,
            y,
        )
        .unwrap();
        let Transform3D::Affine(result) = result else {
            panic!("expected scale")
        };
        assert_eq!(
            *result.linear(),
            [[factor, 0.0, 0.0], [0.0, factor, 0.0], [0.0, 0.0, factor]]
        );
    }
}

#[test]
fn angle_wrap_is_continuous_in_either_direction() {
    let pi = std::f32::consts::PI;
    for sign in [-1.0, 1.0] {
        let previous = sign * (pi - 0.1);
        let next = -previous;
        let actual = continue_angle(previous, previous, next);
        assert!((actual - sign * (pi + 0.1)).abs() < 1e-5);
    }
}

#[test]
fn angle_accumulates_past_one_turn() {
    let pi = std::f32::consts::PI;
    for direction in [-1.0, 1.0] {
        let mut previous = 0.0;
        let mut total = 0.0;
        for raw in [0.5 * pi, pi - 0.01, -pi + 0.01, -0.5 * pi, -0.01, 0.01] {
            let next = direction * raw;
            total = continue_angle(previous, total, next);
            previous = next;
        }
        assert!((total - direction * (2.0 * pi + 0.01)).abs() < 1e-5);
    }
    assert!((continue_angle(0.5, 0.5, 0.6) - 0.6).abs() < 1e-6);
}

#[test]
fn marked_angles_have_distinct_capture_windows() {
    for (input, expected) in [
        (13.0_f32, 15.0_f32),
        (47.5, 45.0),
        (22.0, 22.0),
        (52.0, 52.0),
        (85.5, 90.0),
        (184.0, 180.0),
        (1.0, 0.0),
        (81.0, 81.0),
        (-31.0, -30.0),
    ] {
        let actual = snap_rotation(input.to_radians()).to_degrees();
        assert!((actual - expected).abs() < 0.001, "{input}: {actual}");
    }
}

#[test]
fn axis_scale_changes_only_its_selected_dimension() {
    for (axis, index) in [(Axis::X, 0), (Axis::Y, 1), (Axis::Z, 2)] {
        let transform = stretch_along(axis, 2.0);
        let matrix = transform.linear();
        for row in 0..3 {
            for col in 0..3 {
                let expected = if row != col {
                    0.0
                } else if row == index {
                    2.0
                } else {
                    1.0
                };
                assert_eq!(matrix[row][col], expected);
            }
        }
    }
}

#[test]
fn pivot_is_expressed_in_parent_coordinates() {
    let parent = Transform3D::Affine(AffineTransform3D::from_translation([40.0, 0.0, 0.0]));
    let pivot = Vec3::new(40.0, 0.0, 0.0);
    for operation in [
        AffineTransform3D::from_uniform_scale(2.0),
        rotation_about(Axis::Z, std::f32::consts::FRAC_PI_2).unwrap(),
    ] {
        let local = about_pivot(&operation, pivot, &parent, &Transform3D::Identity);
        assert_eq!(displacement(local), [0.0; 3]);
    }
}

#[test]
fn axis_drag_uses_cursor_position_and_updates_scale_anchor() {
    let ray = picking::PickRay {
        start: Vec3::new(2.0, -3.0, 0.0),
        heading: Vec3::Y,
    };
    let anchor = Vec3::X;
    let translate = DragKind::Translate {
        axis: Axis::X,
        anchor,
    };
    let (moved, _) = drag_to_transform(
        translate,
        &ray,
        Vec3::ZERO,
        &Transform3D::Identity,
        &Transform3D::Identity,
        0.0,
        0.0,
    )
    .unwrap();
    assert_eq!(displacement(moved), [1.0, 0.0, 0.0]);
    let Transform3D::Affine(moved) = moved else {
        panic!("expected translation")
    };
    assert_eq!(
        *moved.linear(),
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
    );

    let scale = DragKind::ScaleAxis {
        axis: Axis::X,
        start_anchor: anchor,
        current_anchor: anchor,
        proportional: false,
        press_screen: (0.0, 0.0),
        pivot_screen: (0.0, 0.0),
    };
    let (scaled, updated) = drag_to_transform(
        scale,
        &ray,
        Vec3::ZERO,
        &Transform3D::Identity,
        &Transform3D::Identity,
        0.0,
        0.0,
    )
    .unwrap();
    let Transform3D::Affine(scaled) = scaled else {
        panic!("expected affine transform")
    };
    assert_eq!(scaled.linear()[0][0], 2.0);
    assert_eq!(scaled.linear()[1][1], 1.0);
    assert_eq!(scaled.linear()[2][2], 1.0);
    assert_eq!(*scaled.translation(), [0.0; 3]);
    let DragKind::ScaleAxis { current_anchor, .. } = updated else {
        panic!("expected axis scale")
    };
    assert_eq!(current_anchor, Vec3::new(2.0, 0.0, 0.0));
}

#[test]
fn proportional_axis_drag_uses_screen_distance_for_all_dimensions() {
    let op = DragKind::ScaleAxis {
        axis: Axis::X,
        start_anchor: Vec3::new(99.0, 0.0, 0.0),
        current_anchor: Vec3::new(99.0, 0.0, 0.0),
        proportional: true,
        press_screen: (150.0, 100.0),
        pivot_screen: (100.0, 100.0),
    };
    let ray = picking::PickRay {
        start: Vec3::new(0.0, -3.0, 0.0),
        heading: Vec3::Y,
    };
    let (result, _) = drag_to_transform(
        op,
        &ray,
        Vec3::ZERO,
        &Transform3D::Identity,
        &Transform3D::Identity,
        200.0,
        100.0,
    )
    .unwrap();
    let Transform3D::Affine(result) = result else {
        panic!("expected affine transform")
    };
    for axis in 0..3 {
        assert!((result.linear()[axis][axis] - 2.0).abs() < 1e-4);
    }
}

#[test]
fn rotation_drag_snaps_and_advances_its_angle() {
    for degrees in [88.0_f32, 90.0] {
        let angle = degrees.to_radians();
        let ray = picking::PickRay {
            start: Vec3::new(5.0, angle.cos(), angle.sin()),
            heading: -Vec3::X,
        };
        let op = DragKind::Rotate {
            axis: Axis::X,
            start_angle: 0.0,
            last_raw: 0.0,
            current_unwrapped: 0.0,
        };
        let (result, state) = drag_to_transform(
            op,
            &ray,
            Vec3::ZERO,
            &Transform3D::Identity,
            &Transform3D::Identity,
            0.0,
            0.0,
        )
        .unwrap();
        let Transform3D::Affine(result) = result else {
            panic!("expected affine transform")
        };
        assert!(result.linear()[0][1].abs() < 1e-5);
        assert!(result.linear()[1][1].abs() < 1e-5);
        assert!((result.linear()[2][1] - 1.0).abs() < 1e-5);
        let DragKind::Rotate {
            last_raw,
            current_unwrapped,
            ..
        } = state
        else {
            panic!("expected rotation")
        };
        assert!((last_raw - angle).abs() < 1e-5);
        assert!((current_unwrapped - angle).abs() < 1e-5);
    }
}

#[test]
fn gestures_keep_the_centre_under_a_translated_parent() {
    let parent = Transform3D::Affine(AffineTransform3D::from_translation([40.0, 0.0, 0.0]));
    let pivot = Vec3::new(40.0, 0.0, 0.0);
    let scale = DragKind::ScaleAxis {
        axis: Axis::X,
        start_anchor: pivot + Vec3::X,
        current_anchor: pivot + Vec3::X,
        proportional: true,
        press_screen: (150.0, 100.0),
        pivot_screen: (100.0, 100.0),
    };
    let rotate = DragKind::Rotate {
        axis: Axis::Z,
        start_angle: 0.0,
        last_raw: 0.0,
        current_unwrapped: 0.0,
    };
    let scale_ray = picking::PickRay {
        start: pivot - 3.0 * Vec3::Y,
        heading: Vec3::Y,
    };
    let rotate_ray = picking::PickRay {
        start: pivot + Vec3::Y + 5.0 * Vec3::Z,
        heading: -Vec3::Z,
    };
    for (op, ray) in [(scale, scale_ray), (rotate, rotate_ray)] {
        let (local, _) = drag_to_transform(
            op,
            &ray,
            pivot,
            &parent,
            &Transform3D::Identity,
            200.0,
            100.0,
        )
        .unwrap();
        assert_eq!(displacement(parent.compose(&local)), [40.0, 0.0, 0.0]);
        if matches!(op, DragKind::ScaleAxis { .. }) {
            let Transform3D::Affine(a) = local else {
                panic!("expected scale")
            };
            for i in 0..3 {
                assert!((a.linear()[i][i] - 2.0).abs() < 1e-4);
            }
        }
    }
}

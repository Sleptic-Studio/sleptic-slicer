// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use glam::Vec3;

use super::*;
use crate::gizmo::SIZES;

#[test]
fn only_a_cursor_near_the_gizmo_is_hit_tested() {
    let mut camera = ViewCamera::new(Vec3::ZERO, 1.0);
    camera.eye_distance = 10.0;
    let (fov_y, w, h) = (starting_fov_y(), 1024.0, 768.0);
    let (x, y) = picking::screen_point_of(&camera, fov_y, w, h, Vec3::ZERO).unwrap();
    // How far on screen a handle can be: the ring plus a handle's length.
    let ring =
        gizmo::gizmo_scale(&camera, fov_y, h) * SIZES.ring_radius;
    let reach = ring / camera.mm_per_pixel(fov_y, h) + SIZES.arm_px;
    let cases = [
        ("on the pivot", x, y, true),
        ("halfway out", x + reach * 0.5, y, true),
        ("a pixel past the reach", x + reach + 1.0, y, false),
        (
            "the viewport corner, hundreds of pixels away",
            0.0,
            0.0,
            false,
        ),
    ];
    for (place, cx, cy, near) in cases {
        let cursor = ScreenPoint::new(cx, cy);
        assert_eq!(
            cursor_near_gizmo(&camera, fov_y, w, h, Vec3::ZERO, cursor),
            near,
            "{place}"
        );
    }
    // A pivot behind the camera does not project: the ray tests decide.
    let behind = camera.eye_position() + (camera.eye_position() - Vec3::ZERO);
    assert!(cursor_near_gizmo(
        &camera,
        fov_y,
        w,
        h,
        behind,
        ScreenPoint::new(0.0, 0.0)
    ));
}

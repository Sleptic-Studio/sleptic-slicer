// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use float_cmp::assert_approx_eq;

use super::*;
use crate::gizmo::SIZES;

fn points(mesh: &Mesh) -> Vec<Vec3> {
    mesh.0.iter().map(|v| Vec3::from(v.position)).collect()
}

/// Every index in range, and every triangle faces away from `inside`.
fn closed_outwards(mesh: &Mesh, inside: impl Fn(Vec3) -> Vec3) {
    let p = points(mesh);
    assert!(mesh.1.iter().all(|i| (*i as usize) < p.len()));
    for t in mesh.1.chunks(3) {
        let [a, b, c] = [p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]];
        let centre = (a + b + c) / 3.0;
        assert!((b - a).cross(c - a).dot(centre - inside(centre)) > 0.0, "{t:?}");
    }
}

#[test]
fn the_shaft_runs_from_the_pivot_to_the_arrow_head() {
    let shaft = build_axis_shaft_line_geometry();
    assert_eq!(points(&shaft), [Vec3::ZERO, Vec3::new(CONE_START, 0.0, 0.0)]);
    assert_eq!(shaft.1, [0, 1]);
}

#[test]
fn the_arrow_head_is_a_cone_pointing_along_x() {
    let cone = build_axis_cone_tip_geometry();
    assert_eq!((cone.0.len(), cone.1.len()), (13, 36));
    assert_eq!(*points(&cone).last().unwrap(), Vec3::new(CONE_END, 0.0, 0.0));
    for p in &points(&cone)[..12] {
        assert_eq!(p.x, CONE_START);
        assert_approx_eq!(f32, glam::Vec2::new(p.y, p.z).length(), CONE_RADIUS, epsilon = 1e-6);
    }
    // Faces point away from the axis (or forward at the tip).
    closed_outwards(&cone, |c| Vec3::new(c.x - 1.0, 0.0, 0.0));
}

#[test]
fn the_move_handle_adds_a_two_sided_square_after_the_head() {
    let (vertices, indices) = build_move_handle_geometry();
    assert_eq!((vertices.len(), indices.len()), (17, 36 + MOVE_PLANE_INDEX_COUNT as usize));
    let square = &indices[36..];
    let p = |i: u32| Vec3::from(vertices[i as usize].position);
    let normal = |t: &[u32]| (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]));
    assert!(normal(&square[0..3]).x > 0.0 && normal(&square[3..6]).x > 0.0);
    assert!(normal(&square[6..9]).x < 0.0 && normal(&square[9..12]).x < 0.0);
    for &i in square {
        let q = p(i);
        assert_eq!(q.x, 0.0);
        assert!((0.22..=0.42).contains(&q.y) && (0.22..=0.42).contains(&q.z));
    }
}

#[test]
fn the_scale_cube_sits_on_the_shaft_end() {
    let cube = build_axis_scale_cube_geometry();
    assert_eq!((cube.0.len(), cube.1.len()), (8, 36));
    let p = points(&cube);
    let centre = p.iter().sum::<Vec3>() / 8.0;
    assert_approx_eq!(f32, centre.x, ARM_LENGTH * SIZES.cube_at, epsilon = 1e-6);
    assert_approx_eq!(f32, glam::Vec2::new(centre.y, centre.z).length(), 0.0, epsilon = 1e-6);
    let side = p.iter().map(|q| q.x).fold(f32::MIN, f32::max) - p.iter().map(|q| q.x).fold(f32::MAX, f32::min);
    assert_approx_eq!(f32, side, SIZES.cube_px / SIZES.arm_px, epsilon = 1e-6);
    closed_outwards(&cube, |_| centre);
}

#[test]
fn the_ring_is_a_closed_circle_in_the_yz_plane_and_its_tube_wraps_it() {
    let (line, loop_indices) = build_rotate_ring_line_geometry();
    assert_eq!((line.len(), loop_indices.len()), (96, 192));
    assert_eq!((loop_indices[0], *loop_indices.last().unwrap()), (0, 0));
    for v in &line {
        assert_eq!(v.position[0], 0.0);
        assert_approx_eq!(f32, Vec3::from(v.position).length(), SIZES.ring_radius, epsilon = 1e-6);
    }
    let tube = build_rotate_ring_geometry();
    assert_eq!((tube.0.len(), tube.1.len()), (96 * 6, 96 * 6 * 6));
    for p in points(&tube) {
        let core = Vec3::new(0.0, p.y, p.z).normalize() * SIZES.ring_radius;
        assert_approx_eq!(f32, p.distance(core), SIZES.ring_tube, epsilon = 1e-5);
    }
    // Faces point away from the tube's core circle.
    closed_outwards(&tube, |c| Vec3::new(0.0, c.y, c.z).normalize() * SIZES.ring_radius);
}

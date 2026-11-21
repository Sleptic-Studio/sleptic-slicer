// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Handle meshes, built once for the +X axis with the pivot at the origin
//! and one handle length = 1. The renderer draws each three times, turned
//! onto X, Y and Z, in each axis's colour.
//!
//! Lines: the shaft and the rotate ring's crisp edge. Triangles: the parts
//! that take clicks (arrow head, plane square, scale cube, ring tube).
//! Triangles wind counter-clockwise seen from outside (back faces culled).
use std::f32::consts::TAU;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use crate::gizmo::SIZES;


#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(crate) struct HandleVertex {
    pub position: [f32; 3],
}

type Mesh = (Vec<HandleVertex>, Vec<u32>);

fn vertex(position: [f32; 3]) -> HandleVertex {
    HandleVertex { position }
}

/// The shaft runs to 1; the arrow head from there to 1.18 (clicks reach
/// 1.25, see `gizmo::hit_test`).
const ARM_LENGTH: f32 = 1.0;
const CONE_START: f32 = ARM_LENGTH;
const CONE_END: f32 = 1.18;
const CONE_RADIUS: f32 = 0.07;
const CONE_FACETS: u32 = 12;

/// The scale cube's on-screen size in handle lengths.
const SCALE_CUBE_SIDE_LOCAL: f32 = SIZES.cube_px / SIZES.arm_px;

/// Ring smoothness: segments round the ring and round its tube.
const ROTATE_RING_SEGMENTS: u32 = 96;
const ROTATE_RING_TUBE_SIDES: u32 = 6;

/// Index count of the plane square that follows each arrow head in
/// `build_move_handle_geometry`. Parts rest on the plate, so the renderer
/// draws the X and Y heads and, from the Z copy, only the square (the flat
/// XY move handle).
pub(crate) const MOVE_PLANE_INDEX_COUNT: u32 = 12;

/// The shaft: one line from the pivot to the arrow head.
pub(crate) fn build_axis_shaft_line_geometry() -> Mesh {
    (vec![vertex([0.0; 3]), vertex([CONE_START, 0.0, 0.0])], vec![0, 1])
}

/// The arrow head: a 12-sided cone. Its base is left open; the shaft
/// hides it.
pub(crate) fn build_axis_cone_tip_geometry() -> Mesh {
    let mut vertices: Vec<HandleVertex> = (0..CONE_FACETS)
        .map(|i| {
            let (s, c) = ((i as f32 / CONE_FACETS as f32) * TAU).sin_cos();
            vertex([CONE_START, CONE_RADIUS * c, CONE_RADIUS * s])
        })
        .collect();
    vertices.push(vertex([CONE_END, 0.0, 0.0]));
    let indices = (0..CONE_FACETS)
        .flat_map(|i| [i, (i + 1) % CONE_FACETS, CONE_FACETS])
        .collect();
    (vertices, indices)
}

/// The arrow head followed by a small square in the plane square to the
/// axis, drawn from both sides, on the same positive corner the plane
/// hit test uses.
pub(crate) fn build_move_handle_geometry() -> Mesh {
    let (mut vertices, mut indices) = build_axis_cone_tip_geometry();
    let first = vertices.len() as u32;
    vertices.extend(
        [[0.22, 0.22], [0.42, 0.22], [0.42, 0.42], [0.22, 0.42]].map(|[y, z]| vertex([0.0, y, z])),
    );
    // Front (towards +X), then the same corners wound the other way.
    indices.extend([0, 1, 2, 0, 2, 3, 2, 1, 0, 3, 2, 0].map(|i| first + i));
    (vertices, indices)
}

/// The scale cube at the end of the shaft.
pub(crate) fn build_axis_scale_cube_geometry() -> Mesh {
    let centre = Vec3::new(ARM_LENGTH * SIZES.cube_at, 0.0, 0.0);
    let h = SCALE_CUBE_SIDE_LOCAL * 0.5;
    // Corners: bit 0 picks +x, bit 1 +y, on the -z face then the +z face,
    // walking each face round (0, 1, 3, 2 in bit order).
    let vertices = [(-h, -h), (h, -h), (h, h), (-h, h)]
        .into_iter()
        .map(|(dx, dy)| (dx, dy, -h))
        .chain([(-h, -h), (h, -h), (h, h), (-h, h)].into_iter().map(|(dx, dy)| (dx, dy, h)))
        .map(|(dx, dy, dz)| vertex([centre.x + dx, centre.y + dy, centre.z + dz]))
        .collect();
    let indices = vec![
        4, 5, 6, 4, 6, 7, // +Z
        0, 3, 2, 0, 2, 1, // -Z
        3, 7, 6, 3, 6, 2, // +Y
        0, 1, 5, 0, 5, 4, // -Y
        1, 2, 6, 1, 6, 5, // +X
        0, 4, 7, 0, 7, 3, // -X
    ];
    (vertices, indices)
}

/// Angle of step `i` of `n` round a full turn.
fn ring_angle(i: u32, n: u32) -> (f32, f32) {
    (i as f32 * TAU / n as f32).sin_cos()
}

/// The ring's edge: a closed loop round +X in the YZ plane. Rings lie flat
/// in their axis's plane; they used to turn to face the camera, which made
/// them swing about with perspective.
pub(crate) fn build_rotate_ring_line_geometry() -> Mesh {
    let r = SIZES.ring_radius;
    let vertices = (0..ROTATE_RING_SEGMENTS)
        .map(|i| {
            let (s, c) = ring_angle(i, ROTATE_RING_SEGMENTS);
            vertex([0.0, r * c, r * s])
        })
        .collect();
    let indices = (0..ROTATE_RING_SEGMENTS)
        .flat_map(|i| [i, (i + 1) % ROTATE_RING_SEGMENTS])
        .collect();
    (vertices, indices)
}

/// The ring itself: a thin tube round +X, grabbed anywhere along it.
pub(crate) fn build_rotate_ring_geometry() -> Mesh {
    let (n, sides) = (ROTATE_RING_SEGMENTS, ROTATE_RING_TUBE_SIDES);
    let mut vertices = Vec::with_capacity((n * sides) as usize);
    for i in 0..n {
        let (s, c) = ring_angle(i, n);
        let outward = Vec3::new(0.0, c, s);
        for j in 0..sides {
            let (ts, tc) = ring_angle(j, sides);
            let p = outward * (SIZES.ring_radius + SIZES.ring_tube * tc)
                + Vec3::X * (SIZES.ring_tube * ts);
            vertices.push(vertex(p.to_array()));
        }
    }
    let at = |i: u32, j: u32| (i % n) * sides + j % sides;
    let indices = (0..n)
        .flat_map(|i| (0..sides).map(move |j| (i, j)))
        .flat_map(|(i, j)| {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            [a, b, c, a, c, d]
        })
        .collect();
    (vertices, indices)
}

#[cfg(test)]
#[path = "tests_gizmo_geometry.rs"]
mod tests;


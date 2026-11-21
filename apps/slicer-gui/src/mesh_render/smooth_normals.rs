// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Per-vertex shading normals for the viewer.
//!
//! The mesh shader used to light every pixel by a normal taken from
//! screen-space derivatives of its position: flat facets, and on thin round
//! parts (support tips, branches, pillars) a normal that changed from one
//! 2x2 pixel block to the next, which read as pixelated stair steps.
//! Here each corner of a triangle gets the average of the normals of the
//! faces round its vertex that meet this face at under [`CREASE_DEG`], each
//! weighted by its angle at the vertex: round surfaces shade smoothly, real
//! edges (a cube's, a pillar's end caps) stay sharp. A vertex shared by
//! faces on both sides of an edge is split into one vertex per side.

use std::collections::HashMap;

/// Faces meeting at less than this (degrees) shade as one smooth surface.
/// Above the 45 degrees between an 8-sided support's faces, below a
/// cube's 90.
pub(crate) const CREASE_DEG: f32 = 50.0;

/// Vertices (position, normal) and triangle indices for `positions` and
/// `triangles`, split where faces meet at a crease.
pub(crate) fn smooth_vertices(
    positions: &[[f32; 3]],
    triangles: &[[u32; 3]],
) -> (Vec<([f32; 3], [f32; 3])>, Vec<u32>) {
    let p = |i: u32| glam::Vec3::from(positions[i as usize]);
    let face_normal: Vec<glam::Vec3> = triangles
        .iter()
        .map(|t| {
            (p(t[1]) - p(t[0]))
                .cross(p(t[2]) - p(t[0]))
                .normalize_or_zero()
        })
        .collect();
    // The angle of each triangle at each of its corners.
    let corner_angle: Vec<[f32; 3]> = triangles
        .iter()
        .map(|t| {
            std::array::from_fn(|k| {
                let a = p(t[k]);
                let (b, c) = (p(t[(k + 1) % 3]), p(t[(k + 2) % 3]));
                (b - a).angle_between(c - a)
            })
        })
        .map(|a: [f32; 3]| a.map(|x| if x.is_finite() { x } else { 0.0 }))
        .collect();
    // The faces round each vertex, as (face, corner).
    let mut start = vec![0u32; positions.len() + 1];
    for t in triangles {
        for &v in t {
            start[v as usize + 1] += 1;
        }
    }
    for i in 0..positions.len() {
        start[i + 1] += start[i];
    }
    let mut fill = start.clone();
    let mut around = vec![(0u32, 0u8); triangles.len() * 3];
    for (f, t) in triangles.iter().enumerate() {
        for (k, &v) in t.iter().enumerate() {
            around[fill[v as usize] as usize] = (f as u32, k as u8);
            fill[v as usize] += 1;
        }
    }
    let cos_crease = CREASE_DEG.to_radians().cos();
    let mut out: Vec<([f32; 3], [f32; 3])> = Vec::with_capacity(positions.len());
    let mut index = Vec::with_capacity(triangles.len() * 3);
    let mut seen: HashMap<(u32, [i16; 3]), u32> = HashMap::with_capacity(positions.len());
    for (f, t) in triangles.iter().enumerate() {
        let own = face_normal[f];
        for &v in t {
            let mut sum = glam::Vec3::ZERO;
            for &(g, k) in &around[start[v as usize] as usize..start[v as usize + 1] as usize] {
                let n = face_normal[g as usize];
                if n.dot(own) >= cos_crease {
                    sum += n * corner_angle[g as usize][k as usize];
                }
            }
            let normal = if sum.length_squared() > 1e-20 {
                sum.normalize()
            } else {
                own
            };
            let key = (v, normal.to_array().map(|c| (c * 4096.0).round() as i16));
            let id = *seen.entry(key).or_insert_with(|| {
                out.push((positions[v as usize], normal.to_array()));
                (out.len() - 1) as u32
            });
            index.push(id);
        }
    }
    (out, index)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ring of `sides` quads round the Z axis, radius 1, height 1.
    fn tube(sides: u32) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
        let mut positions = Vec::new();
        for z in [0.0, 1.0] {
            for i in 0..sides {
                let a = i as f32 * std::f32::consts::TAU / sides as f32;
                positions.push([a.cos(), a.sin(), z]);
            }
        }
        let mut triangles = Vec::new();
        for i in 0..sides {
            let j = (i + 1) % sides;
            triangles.push([i, j, sides + j]);
            triangles.push([i, sides + j, sides + i]);
        }
        (positions, triangles)
    }

    /// A round surface shades smoothly: each vertex of a 24-sided tube gets
    /// a normal pointing straight out from the axis, not a facet's.
    #[test]
    fn a_round_tube_gets_radial_normals() {
        let (positions, triangles) = tube(24);
        let (vertices, index) = smooth_vertices(&positions, &triangles);
        assert_eq!(vertices.len(), positions.len(), "no seam on a smooth tube");
        assert_eq!(index.len(), triangles.len() * 3);
        for (p, n) in &vertices {
            let radial = glam::Vec3::new(p[0], p[1], 0.0).normalize();
            assert!(glam::Vec3::from(*n).dot(radial) > 0.999, "{n:?} at {p:?}");
        }
    }

    /// An 8-sided support part (45 degrees between faces) is still smooth.
    #[test]
    fn an_eight_sided_part_is_still_smooth() {
        let (positions, triangles) = tube(8);
        let (vertices, _) = smooth_vertices(&positions, &triangles);
        assert_eq!(vertices.len(), positions.len());
    }

    /// A cube's edges stay sharp: each corner splits into three vertices,
    /// one per face, each with that face's normal.
    #[test]
    fn a_cube_keeps_its_edges() {
        let positions: Vec<[f32; 3]> = (0..8)
            .map(|i| [(i & 1) as f32, ((i >> 1) & 1) as f32, ((i >> 2) & 1) as f32])
            .collect();
        let triangles = vec![
            [0, 2, 1],
            [1, 2, 3],
            [4, 5, 6],
            [5, 7, 6],
            [0, 1, 4],
            [1, 5, 4],
            [2, 6, 3],
            [3, 6, 7],
            [0, 4, 2],
            [2, 4, 6],
            [1, 3, 5],
            [3, 7, 5],
        ];
        let (vertices, _) = smooth_vertices(&positions, &triangles);
        assert_eq!(vertices.len(), 24);
        for (_, n) in &vertices {
            let biggest = n.iter().map(|c| c.abs()).fold(0.0, f32::max);
            assert!((biggest - 1.0).abs() < 1e-5, "{n:?} is not a face normal");
        }
    }
}

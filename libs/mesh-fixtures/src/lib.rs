// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Small meshes with fixed coordinates for geometry and topology tests.

use sc_mesh_core::{IndexedMesh, IndexedTriangle, Normal, Vertex};

fn triangle(mesh: &mut IndexedMesh, indices: [u32; 3]) {
    let [a, b, c] = indices.map(|i| mesh.vertices[i as usize].to_f64());
    let cross = b.diff(&a).cross_product(&c.diff(&a));
    let length = cross.norm();
    let normal = Normal::new(cross.coords().map(|v| (v / length) as f32));
    mesh.faces
        .push(IndexedTriangle::from_raw_indices(normal, indices));
}

fn quad(mesh: &mut IndexedMesh, [a, b, c, d]: [u32; 4]) {
    triangle(mesh, [a, b, c]);
    triangle(mesh, [a, c, d]);
}

fn empty_with_vertices(points: impl IntoIterator<Item = [f32; 3]>) -> IndexedMesh {
    IndexedMesh {
        meta: None,
        vertices: points.into_iter().map(Vertex::new).collect(),
        faces: Vec::new(),
    }
}

fn box_corners(radius: f32, bottom: f32, top: f32) -> [[f32; 3]; 8] {
    [
        [-radius, -radius, bottom],
        [radius, -radius, bottom],
        [radius, radius, bottom],
        [-radius, radius, bottom],
        [-radius, -radius, top],
        [radius, -radius, top],
        [radius, radius, top],
        [-radius, radius, top],
    ]
}

/// A closed, outward-wound unit cube centred at the origin.
///
/// Vertices run counter-clockwise in XY, bottom four first, then top four.
pub fn unit_cube_mesh() -> IndexedMesh {
    let mut mesh = empty_with_vertices(box_corners(0.5, -0.5, 0.5));
    quad(&mut mesh, [3, 2, 1, 0]);
    quad(&mut mesh, [4, 5, 6, 7]);
    for i in 0..4 {
        let j = (i + 1) % 4;
        quad(&mut mesh, [i, j, j + 4, i + 4]);
    }
    mesh
}

/// A 2 by 2 by 2 box with a centred 0.8 by 0.8 through-hole along Z.
pub fn cuboid_with_z_tunnel() -> IndexedMesh {
    let mut mesh = empty_with_vertices(
        box_corners(1.0, 0.0, 2.0)
            .into_iter()
            .chain(box_corners(0.4, 0.0, 2.0)),
    );
    for i in 0..4 {
        let j = (i + 1) % 4;
        quad(&mut mesh, [i, j, j + 4, i + 4]);
        quad(&mut mesh, [j + 8, i + 8, i + 12, j + 12]);
        // Matching corner connections partition each cap into four trapezoids.
        quad(&mut mesh, [j, i, i + 8, j + 8]);
        quad(&mut mesh, [i + 4, j + 4, j + 12, i + 12]);
    }
    mesh
}

/// Two closed tetrahedra sharing only their apex, vertex 3.
///
/// Edges are manifold; the shared vertex intentionally has two face fans.
pub fn bowtie_tetrahedra() -> IndexedMesh {
    let mut mesh = empty_with_vertices([
        [1.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 1.0, 2.0],
        [-1.0, 1.0, 2.0],
        [0.0, -1.0, 2.0],
    ]);
    for face in [
        [0, 2, 1],
        [0, 1, 3],
        [1, 2, 3],
        [2, 0, 3],
        [4, 5, 6],
        [5, 4, 3],
        [6, 5, 3],
        [4, 6, 3],
    ] {
        triangle(&mut mesh, face);
    }
    mesh
}

#[cfg(test)]
mod tests_geometry;

/// The tunnel box with its square through-hole along Y.
/// Outer corners retain the ordinary box order; mouth corners run in XZ.
pub fn cuboid_with_y_tunnel() -> IndexedMesh {
    let mut mesh = empty_with_vertices(
        box_corners(1.0, 0.0, 2.0).into_iter().chain(
            [-1.0, 1.0]
                .into_iter()
                .flat_map(|y| [[-0.4, y, 0.6], [0.4, y, 0.6], [0.4, y, 1.4], [-0.4, y, 1.4]]),
        ),
    );
    for face in [[3, 2, 1, 0], [4, 5, 6, 7], [3, 0, 4, 7], [1, 2, 6, 5]] {
        quad(&mut mesh, face);
    }
    let front = [0, 1, 5, 4];
    let back = [3, 2, 6, 7];
    for i in 0..4 {
        let j = (i + 1) % 4;
        quad(&mut mesh, [front[i], front[j], j as u32 + 8, i as u32 + 8]);
        quad(&mut mesh, [back[j], back[i], i as u32 + 12, j as u32 + 12]);
        quad(
            &mut mesh,
            [i as u32 + 8, j as u32 + 8, j as u32 + 12, i as u32 + 12],
        );
    }
    mesh
}

fn plane_tetrahedron(points: [[f32; 3]; 4]) -> IndexedMesh {
    let mut mesh = empty_with_vertices(points);
    for face in [[0, 1, 2], [0, 3, 1], [0, 2, 3], [1, 3, 2]] {
        triangle(&mut mesh, face);
    }
    mesh
}

/// One vertex on Z = 0, two above it and one below it.
pub fn aob_saddle_tetrahedron() -> IndexedMesh {
    plane_tetrahedron([
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 1.0],
        [-1.0, 0.0, 1.0],
        [0.0, 1.0, -1.0],
    ])
}

/// A coplanar, zero-volume tetrahedron crossing Z = 0 along edge 0-1.
/// Face order is fixed for slicing tests; no solid outward direction exists.
pub fn on_edge_crossing_tetrahedron() -> IndexedMesh {
    plane_tetrahedron([
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.5, 1.0, 1.0],
        [0.5, -1.0, -1.0],
    ])
}

/// A tetrahedron touching Z = 0 along edge 0-1.
/// The specified face order is inward and intentionally retained for callers.
pub fn on_edge_skim_above_tetrahedron() -> IndexedMesh {
    plane_tetrahedron([
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.5, 1.0, 1.0],
        [0.5, -1.0, 1.0],
    ])
}

/// A tunnel whose two pyramidal voids meet at vertex 16.
/// The apex has two face fans, deliberately violating vertex manifoldness.
pub fn cuboid_with_pinched_z_tunnel() -> IndexedMesh {
    let mut mesh = empty_with_vertices(
        cuboid_with_z_tunnel()
            .vertices
            .into_iter()
            .map(|v| v.coords())
            .chain([[0.0, 0.0, 1.0]]),
    );
    for i in 0..4 {
        let j = (i + 1) % 4;
        quad(&mut mesh, [i, j, j + 4, i + 4]);
        triangle(&mut mesh, [j + 8, i + 8, 16]);
        triangle(&mut mesh, [i + 12, j + 12, 16]);
        quad(&mut mesh, [j, i, i + 8, j + 8]);
        quad(&mut mesh, [i + 4, j + 4, j + 12, i + 12]);
    }
    mesh
}

/// Cube faces with pairwise disjoint edge-neighbourhoods.
pub const UNIT_CUBE_FLIPPED_FACES: [usize; 2] = [0, 2];
/// Z tunnel faces with pairwise disjoint edge-neighbourhoods, including a void wall.
pub const Z_TUNNEL_FLIPPED_FACES: [usize; 3] = [0, 2, 11];
/// Y tunnel faces with pairwise disjoint edge-neighbourhoods, including a void wall.
pub const Y_TUNNEL_FLIPPED_FACES: [usize; 3] = [0, 2, 12];

fn flipped(mut mesh: IndexedMesh, faces: &[usize]) -> IndexedMesh {
    for &index in faces {
        let face = &mut mesh.faces[index];
        face.vertices.reverse();
        face.normal = Normal::new(face.normal.coords().map(|v| -v));
    }
    mesh
}

/// The cube with the documented isolated faces reversed.
pub fn unit_cube_with_flipped_faces() -> IndexedMesh {
    flipped(unit_cube_mesh(), &UNIT_CUBE_FLIPPED_FACES)
}

/// The Z tunnel with the documented isolated faces reversed.
pub fn cuboid_with_z_tunnel_and_flipped_faces() -> IndexedMesh {
    flipped(cuboid_with_z_tunnel(), &Z_TUNNEL_FLIPPED_FACES)
}

/// The Y tunnel with the documented isolated faces reversed.
pub fn cuboid_with_y_tunnel_and_flipped_faces() -> IndexedMesh {
    flipped(cuboid_with_y_tunnel(), &Y_TUNNEL_FLIPPED_FACES)
}

/// A closed block with a step: an L-shaped profile in XZ, `(0,0) (2,0)
/// (2,0.375) (1,0.375) (1,1) (0,1)` shifted to centre X on 0, extruded over
/// Y from -0.5 to 0.5. Its step face lies at z = 0.375, a value exact in
/// f32 and f64, so a slicing plane can meet it exactly.
pub fn stepped_block() -> IndexedMesh {
    stepped_block_with(0.375, 1.0)
}

/// [`stepped_block`] with its step face at `step` and its top at `top`.
pub fn stepped_block_with(step: f32, top: f32) -> IndexedMesh {
    let profile: [[f32; 2]; 6] = [[0.0, 0.0], [2.0, 0.0], [2.0, step], [1.0, step], [1.0, top], [0.0, top]];
    let side = |y: f32| profile.map(|[x, z]| [x - 1.0, y, z]);
    let mut mesh = empty_with_vertices(side(-0.5).into_iter().chain(side(0.5)));
    // The front (y = -0.5) is the profile counter-clockwise as seen from -Y;
    // a fan from the first corner covers it, since that corner sees them all.
    for i in 1..5 {
        triangle(&mut mesh, [0, i, i + 1]);
        triangle(&mut mesh, [6, i + 7, i + 6]);
    }
    for i in 0..6 {
        let j = (i + 1) % 6;
        quad(&mut mesh, [i, i + 6, j + 6, j]);
    }
    mesh
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_mesh_core::{
    Vertex,
    geometry::{GeometryError, compute_normal_from_triangle_vertices},
};

#[test]
fn triangle_normal_contract() {
    let vertex = |x, y, z| Vertex::new([x, y, z]);
    let normal = compute_normal_from_triangle_vertices(&[
        vertex(0., 0., 0.),
        vertex(2., 0., 0.),
        vertex(0., 3., 0.),
    ])
    .unwrap();
    assert_eq!(normal, sc_mesh_core::Normal::new([0., 0., 1.]));
    let reversed = compute_normal_from_triangle_vertices(&[
        vertex(0., 0., 0.),
        vertex(0., 3., 0.),
        vertex(2., 0., 0.),
    ])
    .unwrap();
    assert_eq!(reversed, sc_mesh_core::Normal::new([0., 0., -1.]));
    for third in [
        vertex(4., 0., 0.),
        vertex(1., 0.0000001, 0.),
        vertex(f32::NAN, 1., 0.),
    ] {
        assert_eq!(
            compute_normal_from_triangle_vertices(&[vertex(0., 0., 0.), vertex(2., 0., 0.), third]),
            Err(GeometryError::DegenerateTriangle)
        );
    }
}

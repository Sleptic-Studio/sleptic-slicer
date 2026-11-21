// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;

#[test]
fn indexed_clearance_matches_the_full_triangle_scan_at_edges_and_interiors() {
    let tris: Vec<_> = (0..1000)
        .map(|i| {
            let x = (i % 40) as f32 * 10.;
            let y = (i / 40) as f32 * 10.;
            [
                Vertex::new([x, y, i as f32]),
                Vertex::new([x + 4., y, i as f32 + 1.]),
                Vertex::new([x, y + 4., i as f32 + 2.]),
            ]
        })
        .collect();
    let tree = HeightTree::new(&tris);
    for i in 0..1000 {
        for (dx, dy) in [
            (0., 0.),
            (1., 1.),
            (2., 2.),
            (4., 0.),
            (-1e-9, 0.),
            (9., 9.),
        ] {
            let x = (i % 40) as f64 * 10. + dx;
            let y = (i / 40) as f64 * 10. + dy;
            let predicate = |z: f64| z > i as f64 + 0.1;
            assert_eq!(
                tree.any(&tris, x, y, &predicate),
                tris.iter()
                    .filter_map(|t| height_at(t, x, y))
                    .any(predicate)
            );
        }
    }
}

#[test]
fn shared_ray_index_matches_independent_axis_queries_after_rotation() {
    use glam::{DQuat, DVec3};
    let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
    for vertex in &mut mesh.vertices {
        let [x, y, z] = vertex.coords();
        *vertex = Vertex::new([x * 12.0, y * 8.0, z * 6.0]);
    }
    let tree = crate::hollow::Tree::from_mesh(&mesh);
    for rotation in [
        DQuat::IDENTITY,
        DQuat::from_rotation_x(0.31) * DQuat::from_rotation_y(0.57),
        DQuat::from_rotation_z(0.82),
    ] {
        let mut placed = mesh.clone();
        for vertex in &mut placed.vertices {
            let p = rotation * DVec3::from_array(vertex.coords().map(f64::from));
            *vertex = Vertex::new(p.to_array().map(|v| v as f32));
        }
        let tris = triangles(&placed).unwrap();
        for axis in 0..3 {
            let permute = |p: [f32; 3]| [p[(axis + 1) % 3], p[(axis + 2) % 3], p[axis]];
            let projected: Vec<_> = tris
                .iter()
                .map(|t| t.map(|v| Vertex::new(permute(v.coords()))))
                .collect();
            let heights = HeightTree::new(&projected);
            for i in -10..=10 {
                for j in -10..=10 {
                    let mut origin = DVec3::splat(0.0);
                    origin[(axis + 1) % 3] = f64::from(i) * 0.731 + 0.13;
                    origin[(axis + 2) % 3] = f64::from(j) * 0.647 + 0.17;
                    origin[axis] = 15.0;
                    let direction = -DVec3::AXES[axis];
                    let expected = heights.any(
                        &projected,
                        origin[(axis + 1) % 3],
                        origin[(axis + 2) % 3],
                        &|z| (origin[axis] - z) > 1e-4 && (origin[axis] - z) < 30.0,
                    );
                    assert_eq!(
                        tree.ray_any(
                            rotation.inverse() * origin,
                            rotation.inverse() * direction,
                            1e-4,
                            30.0
                        ),
                        expected
                    );
                }
            }
        }
    }
}

#[test]
fn generated_contact_access_uses_the_raster_surface_tolerance() {
    let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
    for vertex in &mut mesh.vertices {
        let [x, y, z] = vertex.coords();
        *vertex = Vertex::new([x, y, z + 5.0]);
    }
    let contacts = [SupportPoint {
        position: [0.0, 0.0, 4.53],
        normal: [0.0, 0.0, -1.0],
    }];
    assert_eq!(analyze_contact_access(&mesh, &contacts, 1e-4).unwrap().0, 1);
    assert_eq!(
        analyze_contact_access(&mesh, &contacts, 0.0501).unwrap().0,
        0
    );
    let shadowed = [SupportPoint {
        position: [0.0, 0.0, 6.0],
        normal: [0.0, 0.0, -1.0],
    }];
    assert_eq!(
        analyze_contact_access(&mesh, &shadowed, 0.0501).unwrap().0,
        1
    );
}

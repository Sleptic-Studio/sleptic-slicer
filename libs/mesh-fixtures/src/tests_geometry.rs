// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

use std::collections::BTreeMap;

use sc_mesh_core::Vector3D;

use super::*;

fn check_mesh(mesh: &IndexedMesh, vertices: usize, faces: usize, euler: isize, volume: f64) {
    assert!(mesh.meta.is_none());
    assert_eq!(mesh.vertices.len(), vertices);
    assert_eq!(mesh.faces.len(), faces);
    mesh.validate_vertices().unwrap();
    let validated = mesh.validated_faces().unwrap();
    validated.validate_normal_vectors().unwrap();
    validated.validate_edge_manifold().unwrap();

    let mut edges = BTreeMap::<_, Vec<_>>::new();
    let mut signed_volume = 0.0;
    for face in &mesh.faces {
        let ids = face.vertices.map(usize::from);
        for (a, b) in [(ids[0], ids[1]), (ids[1], ids[2]), (ids[2], ids[0])] {
            edges.entry((a.min(b), a.max(b))).or_default().push(a < b);
        }
        let [a, b, c] = ids.map(|i| mesh.vertices[i].to_f64());
        let cross = b.diff(&a).cross_product(&c.diff(&a));
        let normal = face.normal.to_f64();
        assert!((normal.norm() - 1.0).abs() < 1e-6);
        assert!((cross.dot_product(&normal) / cross.norm() - 1.0).abs() < 1e-6);
        signed_volume += a.dot_product(&b.cross_product(&c)) / 6.0;
    }
    for directions in edges.values() {
        assert_eq!(directions.len(), 2);
        assert_ne!(directions[0], directions[1]);
    }
    assert_eq!(
        vertices as isize - edges.len() as isize + faces as isize,
        euler
    );
    assert!((signed_volume - volume).abs() < 1e-6, "{signed_volume}");
}

fn centroid(mesh: &IndexedMesh, face: &IndexedTriangle) -> [f64; 3] {
    let points = face
        .vertices
        .map(|i| mesh.vertices[usize::from(i)].to_f64().coords());
    std::array::from_fn(|axis| points.iter().map(|p| p[axis]).sum::<f64>() / 3.0)
}

fn assert_away_from(mesh: &IndexedMesh, face: &IndexedTriangle, centre: [f64; 3]) {
    let outward = Vector3D::new(centroid(mesh, face)).diff(&Vector3D::new(centre));
    // A positive signed distance from the face's plane puts the step outside
    // the convex lobe, independently of how far the centre is from that face.
    assert!(outward.dot_product(&face.normal.to_f64()) > 0.0);
    let step = Vector3D::new(std::array::from_fn(|axis| {
        centroid(mesh, face)[axis] + 0.001 * f64::from(face.normal[axis])
    }));
    assert!(
        step.diff(&Vector3D::new(centroid(mesh, face)))
            .dot_product(&face.normal.to_f64())
            > 0.0
    );
}

fn fan_count(mesh: &IndexedMesh, vertex: usize) -> usize {
    let mut pending: Vec<_> = mesh
        .faces
        .iter()
        .enumerate()
        .filter(|(_, f)| f.vertices.iter().any(|i| usize::from(*i) == vertex))
        .map(|(i, _)| i)
        .collect();
    let mut fans = 0;
    while let Some(seed) = pending.pop() {
        fans += 1;
        let mut frontier = vec![seed];
        while let Some(current) = frontier.pop() {
            let mut index = 0;
            while index < pending.len() {
                let adjacent = mesh.faces[current].vertices.iter().any(|v| {
                    usize::from(*v) != vertex && mesh.faces[pending[index]].vertices.contains(v)
                });
                if adjacent {
                    frontier.push(pending.swap_remove(index));
                } else {
                    index += 1;
                }
            }
        }
    }
    fans
}

#[test]
fn cube_geometry_and_topology() {
    let mesh = unit_cube_mesh();
    check_mesh(&mesh, 8, 12, 2, 1.0);
    assert_eq!(
        mesh.vertices.iter().map(|v| v.coords()).collect::<Vec<_>>(),
        [
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.5, 0.5, -0.5],
            [-0.5, 0.5, -0.5],
            [-0.5, -0.5, 0.5],
            [0.5, -0.5, 0.5],
            [0.5, 0.5, 0.5],
            [-0.5, 0.5, 0.5],
        ]
    );
    for face in &mesh.faces {
        assert_away_from(&mesh, face, [0.0; 3]);
    }
    for vertex in 0..8 {
        assert_eq!(fan_count(&mesh, vertex), 1);
    }
}

#[test]
fn tunnel_geometry_and_topology() {
    let mesh = cuboid_with_z_tunnel();
    check_mesh(&mesh, 16, 32, 0, 6.72);
    let mut expected = Vec::new();
    for radius in [1.0, 0.4] {
        for z in [0.0, 2.0] {
            for [x, y] in [
                [-radius, -radius],
                [radius, -radius],
                [radius, radius],
                [-radius, radius],
            ] {
                expected.push([x, y, z]);
            }
        }
    }
    assert_eq!(
        mesh.vertices.iter().map(|v| v.coords()).collect::<Vec<_>>(),
        expected
    );
    let inside = |p: [f64; 3]| {
        p[0].abs() < 1.0
            && p[1].abs() < 1.0
            && p[2] > 0.0
            && p[2] < 2.0
            && (p[0].abs() > f64::from(0.4_f32) || p[1].abs() > f64::from(0.4_f32))
    };
    for face in &mesh.faces {
        let centre = centroid(&mesh, face);
        let offset = |sign: f64| {
            std::array::from_fn(|axis| centre[axis] + sign * 0.001 * f64::from(face.normal[axis]))
        };
        assert!(!inside(offset(1.0)));
        assert!(inside(offset(-1.0)));
    }
    for vertex in 0..16 {
        assert_eq!(fan_count(&mesh, vertex), 1);
    }
}

#[test]
fn bowtie_geometry_and_topology() {
    let mesh = bowtie_tetrahedra();
    check_mesh(&mesh, 7, 8, 3, 4.0 / 3.0);
    assert_eq!(
        mesh.vertices.iter().map(|v| v.coords()).collect::<Vec<_>>(),
        [
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 1.0, 2.0],
            [-1.0, 1.0, 2.0],
            [0.0, -1.0, 2.0],
        ]
    );
    for (index, face) in mesh.faces.iter().enumerate() {
        assert_away_from(
            &mesh,
            face,
            [0.0, 0.25, if index < 4 { 0.25 } else { 1.75 }],
        );
    }
    for vertex in 0..7 {
        assert_eq!(fan_count(&mesh, vertex), if vertex == 3 { 2 } else { 1 });
    }
}

#[test]
fn y_tunnel_geometry_and_topology() {
    let mesh = cuboid_with_y_tunnel();
    check_mesh(&mesh, 16, 32, 0, 6.72);
    assert_eq!(&mesh.vertices[..8], &cuboid_with_z_tunnel().vertices[..8]);
    let expected: Vec<_> = [-1.0, 1.0]
        .into_iter()
        .flat_map(|y| [[-0.4, y, 0.6], [0.4, y, 0.6], [0.4, y, 1.4], [-0.4, y, 1.4]])
        .collect();
    assert_eq!(
        mesh.vertices[8..]
            .iter()
            .map(|v| v.coords())
            .collect::<Vec<_>>(),
        expected
    );
    for vertex in 0..16 {
        assert_eq!(fan_count(&mesh, vertex), 1);
    }
    let inside = |p: [f64; 3]| {
        p[0].abs() < 1.0
            && p[1].abs() < 1.0
            && p[2] > 0.0
            && p[2] < 2.0
            && (p[0].abs() > 0.4 || p[2] < 0.6 || p[2] > 1.4)
    };
    for face in &mesh.faces {
        let centre = centroid(&mesh, face);
        for sign in [-1.0, 1.0] {
            let step = std::array::from_fn(|axis| {
                centre[axis] + sign * 0.001 * f64::from(face.normal[axis])
            });
            assert_eq!(inside(step), sign < 0.0);
        }
    }
}

#[test]
fn plane_cases_preserve_coordinates_and_face_positions() {
    for (mesh, points, volume) in [
        (
            aob_saddle_tetrahedron(),
            [[0., 0., 0.], [1., 0., 1.], [-1., 0., 1.], [0., 1., -1.]],
            1.0 / 3.0,
        ),
        (
            on_edge_crossing_tetrahedron(),
            [[0., 0., 0.], [1., 0., 0.], [0.5, 1., 1.], [0.5, -1., -1.]],
            0.0,
        ),
        (
            on_edge_skim_above_tetrahedron(),
            [[0., 0., 0.], [1., 0., 0.], [0.5, 1., 1.], [0.5, -1., 1.]],
            -1.0 / 3.0,
        ),
    ] {
        check_mesh(&mesh, 4, 4, 2, volume);
        assert_eq!(
            mesh.vertices.iter().map(|v| v.coords()).collect::<Vec<_>>(),
            points
        );
        assert_eq!(
            mesh.faces
                .iter()
                .map(|f| f.vertices.map(usize::from))
                .collect::<Vec<_>>(),
            [[0, 1, 2], [0, 3, 1], [0, 2, 3], [1, 3, 2]]
        );
        for vertex in 0..4 {
            assert_eq!(fan_count(&mesh, vertex), 1);
        }
        let centre = std::array::from_fn(|axis| {
            points.iter().map(|p| f64::from(p[axis])).sum::<f64>() / 4.0
        });
        for face in &mesh.faces {
            let distance = Vector3D::new(centroid(&mesh, face))
                .diff(&Vector3D::new(centre))
                .dot_product(&face.normal.to_f64());
            if volume > 0.0 {
                assert!(distance > 0.0);
            } else if volume < 0.0 {
                assert!(distance < 0.0);
            } else {
                assert!(distance.abs() < 1e-12);
            }
        }
    }
}

#[test]
fn pinched_tunnel_geometry_and_topology() {
    let mesh = cuboid_with_pinched_z_tunnel();
    check_mesh(&mesh, 17, 32, 1, 8.0 - 2.0 * 0.8 * 0.8 / 3.0);
    assert_eq!(&mesh.vertices[..16], &cuboid_with_z_tunnel().vertices);
    assert_eq!(mesh.vertices[16].coords(), [0.0, 0.0, 1.0]);
    assert_eq!(
        mesh.faces
            .iter()
            .filter(|f| f.vertices.iter().any(|v| usize::from(*v) == 16))
            .count(),
        8
    );
    for vertex in 0..17 {
        assert_eq!(fan_count(&mesh, vertex), if vertex == 16 { 2 } else { 1 });
    }
    let inside = |p: [f64; 3]| {
        p[0].abs() < 1.0
            && p[1].abs() < 1.0
            && p[2] > 0.0
            && p[2] < 2.0
            && p[0].abs().max(p[1].abs()) > 0.4 * (p[2] - 1.0).abs()
    };
    for face in &mesh.faces {
        let centre = centroid(&mesh, face);
        for sign in [-1.0, 1.0] {
            let step = std::array::from_fn(|axis| {
                centre[axis] + sign * 0.001 * f64::from(face.normal[axis])
            });
            assert_eq!(inside(step), sign < 0.0);
        }
    }
    let mut reached = vec![false; mesh.faces.len()];
    reached[0] = true;
    loop {
        let mut changed = false;
        for i in 0..mesh.faces.len() {
            if !reached[i]
                && (0..mesh.faces.len()).any(|j| {
                    reached[j]
                        && mesh.faces[i]
                            .vertices
                            .iter()
                            .filter(|v| mesh.faces[j].vertices.contains(v))
                            .count()
                            == 2
                })
            {
                reached[i] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    assert!(reached.into_iter().all(|v| v));
}

#[test]
fn isolated_flips_are_exact_and_default_repair_restores_donors() {
    for (donor, mut broken, chosen) in [
        (
            unit_cube_mesh(),
            unit_cube_with_flipped_faces(),
            UNIT_CUBE_FLIPPED_FACES.as_slice(),
        ),
        (
            cuboid_with_z_tunnel(),
            cuboid_with_z_tunnel_and_flipped_faces(),
            Z_TUNNEL_FLIPPED_FACES.as_slice(),
        ),
        (
            cuboid_with_y_tunnel(),
            cuboid_with_y_tunnel_and_flipped_faces(),
            Y_TUNNEL_FLIPPED_FACES.as_slice(),
        ),
    ] {
        assert!(chosen.len() * 2 < donor.faces.len());
        assert_eq!(broken.vertices, donor.vertices);
        assert!(broken.meta.is_none());
        let neighbourhood = |index: usize| {
            donor
                .faces
                .iter()
                .enumerate()
                .filter(|(_, f)| {
                    f.vertices
                        .iter()
                        .filter(|v| donor.faces[index].vertices.contains(v))
                        .count()
                        >= 2
                })
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
        };
        for (position, &index) in chosen.iter().enumerate() {
            for &other in &chosen[position + 1..] {
                assert!(neighbourhood(index)
                    .iter()
                    .all(|i| !neighbourhood(other).contains(i)));
            }
        }
        if donor.faces.len() == 32 {
            assert!(chosen
                .iter()
                .any(|&i| donor.faces[i].vertices.iter().all(|v| usize::from(*v) >= 8)));
        }
        for (i, face) in broken.faces.iter().enumerate() {
            let mut expected = donor.faces[i];
            if chosen.contains(&i) {
                expected.vertices.reverse();
                expected.normal = Normal::new(expected.normal.coords().map(|v| -v));
            }
            assert_eq!(*face, expected);
        }
        assert!(broken
            .validated_faces()
            .unwrap()
            .validate_edge_manifold()
            .is_err());
        broken.repair(sc_mesh_core::indexed_mesh::RepairPasses::default_repair());
        assert_eq!(broken.vertices, donor.vertices);
        assert_eq!(broken.faces, donor.faces);
    }
}

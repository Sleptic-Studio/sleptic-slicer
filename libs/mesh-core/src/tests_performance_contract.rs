// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Ordering and work-count regressions for the mesh performance paths.
use crate::{indexed_mesh::RepairPasses, *};
use std::collections::HashMap;

#[test]
fn soup_welding_keeps_first_occurrence_bits_and_face_order() {
    let mut seed = 91u32;
    let pool: Vec<_> = (0..4096)
        .map(|i| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            Vertex::new([
                f32::from_bits(seed),
                if i % 2 == 0 { -0.0 } else { 0.0 },
                (i % 31) as f32,
            ])
        })
        .collect();
    let soup: Vec<_> = (0..20000)
        .map(|_| {
            Triangle::new(
                Normal::new([0., 0., 1.]),
                std::array::from_fn(|_| {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    pool[seed as usize % pool.len()]
                }),
            )
        })
        .collect();
    let mut lookup = HashMap::new();
    let mut vertices = Vec::new();
    let expected: Vec<_> = soup
        .iter()
        .map(|t| {
            t.vertices().map(|v| {
                let key = v.coords().map(|x| if x == 0. { 0 } else { x.to_bits() });
                *lookup.entry(key).or_insert_with(|| {
                    let id = vertices.len() as u32;
                    vertices.push(v.coords().map(f32::to_bits));
                    id
                })
            })
        })
        .collect();
    let mesh = indexed_mesh_from_soup(None, soup, u32::MAX as usize).unwrap();
    assert_eq!(
        mesh.vertices
            .iter()
            .map(|v| v.coords().map(f32::to_bits))
            .collect::<Vec<_>>(),
        vertices
    );
    assert_eq!(
        mesh.faces
            .iter()
            .map(|f| f.vertices.map(u32::from))
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn clean_repair_builds_edge_records_once() {
    let mut mesh = IndexedMesh::new(
        None,
        vec![Vertex::new([0.; 3]); 4],
        [[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]]
            .map(|f| IndexedTriangle::from_raw_indices(Normal::new([0., 0., 1.]), f))
            .to_vec(),
    );
    indexed_mesh::validated::INCIDENCE_BUILDS.with(|n| n.set(0));
    let report = mesh.repair(RepairPasses::default_repair());
    assert!(report.edge_manifold_after);
    assert!(report.fixes.is_empty());
    indexed_mesh::validated::INCIDENCE_BUILDS.with(|n| assert_eq!(n.get(), 1));
}

#[test]
fn soup_table_growth_keeps_dense_first_appearance_indices() {
    let soup: Vec<_> = (0..512)
        .map(|i| {
            Triangle::new(
                Normal::new([0., 0., 1.]),
                std::array::from_fn(|k| Vertex::new([(i * 3 + k) as f32, 1., -0.])),
            )
        })
        .collect();
    let mesh = indexed_mesh_from_soup(None, soup, 1536).unwrap();
    assert_eq!(mesh.vertices.len(), 1536);
    for (i, f) in mesh.faces.iter().enumerate() {
        assert_eq!(
            f.vertices.map(u32::from),
            [(i * 3) as u32, (i * 3 + 1) as u32, (i * 3 + 2) as u32]
        );
    }
}

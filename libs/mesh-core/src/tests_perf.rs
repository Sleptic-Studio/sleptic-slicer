// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Opt-in phase timings; model files remain outside the repository.
use crate::{indexed_mesh::RepairPasses, *};
use std::time::Instant;

#[test]
#[ignore = "set SC_MESH_PROFILE_STL to a local binary STL; run release with --nocapture"]
fn local_mesh_phases() {
    let path = std::env::var("SC_MESH_PROFILE_STL").expect("SC_MESH_PROFILE_STL");
    let data = std::fs::read(path).unwrap();
    let n = u32::from_le_bytes(data[80..84].try_into().unwrap()) as usize;
    let triangles: Vec<_> = data[84..]
        .as_chunks::<50>()
        .0
        .iter()
        .take(n)
        .map(|r| {
            let xyz = |offset| {
                std::array::from_fn(|k| {
                    f32::from_le_bytes(r[offset + k * 4..offset + k * 4 + 4].try_into().unwrap())
                })
            };
            Triangle::new(
                Normal::new(xyz(0)),
                [
                    Vertex::new(xyz(12)),
                    Vertex::new(xyz(24)),
                    Vertex::new(xyz(36)),
                ],
            )
        })
        .collect();
    let start = Instant::now();
    let mesh = indexed_mesh_from_soup(None, triangles, u32::MAX as usize).unwrap();
    eprintln!("soup {} faces: {:?}", mesh.faces.len(), start.elapsed());
    let start = Instant::now();
    let _ = mesh.validate_vertices();
    let errors = mesh.validate_faces().err().unwrap_or_default();
    eprintln!("vertices/faces: {:?}", start.elapsed());
    let mut valid = mesh.clone();
    let _ = valid.fix_zero_length_edges_into_validated(&errors);
    let start = Instant::now();
    let witness = valid.validated_faces().unwrap();
    let _ = witness.validate_normal_vectors();
    eprintln!("normals: {:?}", start.elapsed());
    let start = Instant::now();
    let result = witness.validated_edge_manifold();
    eprintln!("edge manifold: {:?}", start.elapsed());
    if let Ok(witness) = result {
        let start = Instant::now();
        let _ = indexed_mesh::MeshTopology::from_edge_manifold(&witness);
        eprintln!("topology: {:?}", start.elapsed());
    }
    for (label, passes) in [
        (
            "orientation",
            RepairPasses {
                separate_shells: false,
                ..RepairPasses::default_repair()
            },
        ),
        ("full repair", RepairPasses::default_repair()),
    ] {
        let mut copy = mesh.clone();
        let start = Instant::now();
        let report = copy.repair(passes);
        eprintln!(
            "{label}: {:?}; {} fixes",
            start.elapsed(),
            report.fixes.len()
        );
    }
}

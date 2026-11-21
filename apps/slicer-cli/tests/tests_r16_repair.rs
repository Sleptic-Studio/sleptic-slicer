// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Isolated, opt-in release repair profile on the local source model.
#[test]
#[ignore = "local release profile; SC_R16_MODEL required"]
fn r16_repair_profile() {
    let path = std::path::PathBuf::from(std::env::var_os("SC_R16_MODEL").expect("SC_R16_MODEL"));
    let mut scene = sc_project_formats::load_scene(&path, None, |_| {})
        .unwrap()
        .scene;
    let start = std::time::Instant::now();
    let reports = scene.repair_meshes(sc_mesh_core::indexed_mesh::RepairPasses::default_repair());
    let elapsed = start.elapsed().as_secs_f64();
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    for id in scene
        .reachable_leaves(sc_scene::CsgPolicy::Refuse, sc_scene::ReferencePolicy::Skip)
        .unwrap()
    {
        let mesh = scene.leaf_mesh_arc(id).unwrap();
        mesh.vertices.len().hash(&mut hash);
        mesh.faces.len().hash(&mut hash);
        for vertex in &mesh.vertices {
            vertex.coords().map(f32::to_bits).hash(&mut hash);
        }
        for face in &mesh.faces {
            face.vertices.map(|v| v.get()).hash(&mut hash);
            face.normal.coords().map(f32::to_bits).hash(&mut hash);
        }
    }
    eprintln!("R16 repaired mesh fingerprint: {:016x}", hash.finish());
    eprintln!(
        "R16 repair total: {:.6}s; fixes={}; refusals={}",
        elapsed,
        reports.iter().map(|r| r.fixes.len()).sum::<usize>(),
        reports.iter().filter(|r| r.refusal.is_some()).count()
    );
}

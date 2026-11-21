// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Auto orient from many starting poses (Sleptic: the same Benchy gets
//! "drastically" different Auto prepare poses depending on its starting tilt
//! and rotation). Orients one model from a set of start rotations with the
//! context Auto prepare uses and prints the chosen pose in the model's own
//! terms: which model axis points up, height, contacts, zero-support.
//!
//!   SLEPTIC_ORIENT_MODEL=<path to .stl>[|<another>...]
//!   cargo test -p sc-slicer-cli --release --test tests_r79_orient_starts -- --ignored --nocapture
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};

fn starts() -> Vec<(String, Transform3D)> {
    let rot = |x: f64, y: f64, z: f64| {
        Transform3D::Affine(AffineTransform3D::from_rotation_z(z))
            .compose(&Transform3D::Affine(AffineTransform3D::from_rotation_y(y)))
            .compose(&Transform3D::Affine(AffineTransform3D::from_rotation_x(x)))
    };
    [
        (0.0, 0.0, 0.0),
        (0.0, 0.0, 0.6),
        (0.0, 0.0, 1.9),
        (0.3, 0.0, 0.0),
        (0.0, -0.45, 1.0),
        (1.2, 0.3, -0.7),
        (2.5, 0.0, 0.2),
        (-0.8, 1.4, 2.8),
    ]
    .into_iter()
    .map(|(x, y, z)| (format!("x{x} y{y} z{z}"), rot(x, y, z)))
    .collect()
}

#[test]
#[ignore = "diagnostic; needs SLEPTIC_ORIENT_MODEL"]
fn r79_orient_from_many_starts() {
    let path = std::env::var("SLEPTIC_ORIENT_MODEL").expect("SLEPTIC_ORIENT_MODEL");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let printer = sc_print_profiles::load_printer_profile_from_path(
        &root.join("assets/profiles/printers/sleptic-diy-msla.toml"),
    )
    .unwrap();
    let material = sc_print_profiles::load_material_profile_from_path(
        &root.join("assets/profiles/materials/standard-405.toml"),
    )
    .unwrap()
    .into();
    let mut context =
        sc_print_glue::orient::Context::for_profiles(&printer, &material, &Default::default())
            .unwrap();
    context.aa = sc_sl1cer::profile::AaLevel::Off;
    let weights = sc_print_glue::orient::Preset::Balanced.weights();
    let volume = sc_print_primitives::BuildVolume::new(192.0, 120.0, 200.0).unwrap();
    // SLEPTIC_ORIENT_STARTS=0,1 runs only those starts.
    let only: Option<Vec<usize>> = std::env::var("SLEPTIC_ORIENT_STARTS")
        .ok()
        .map(|v| v.split(',').filter_map(|s| s.parse().ok()).collect());
    for path in path.split('|') {
    println!("MODEL {path}");
    for (i, (label, start)) in starts().into_iter().enumerate() {
        if only.as_ref().is_some_and(|o| !o.contains(&i)) {
            continue;
        }
        eprintln!("START {label}");
        let mut scene = sc_project_formats::load_scene(std::path::Path::new(&path), None, |_| {})
            .unwrap()
            .scene;
        scene.repair_meshes(sc_mesh_core::indexed_mesh::RepairPasses::default_repair());
        let id = *scene
            .reachable_leaves(sc_scene::CsgPolicy::Descend, sc_scene::ReferencePolicy::Skip)
            .unwrap()
            .first()
            .unwrap();
        let before = scene.node(id).unwrap().local_transform;
        scene.set_local_transform(id, start.compose(&before)).unwrap();
        scene.drop_at([0.0, 0.0], 0.0).unwrap();
        let placed = scene.node(id).unwrap().local_transform;
        let p = sc_print_glue::orient::search_with_context(
            &scene,
            id,
            volume,
            context.support.overhang_angle_deg,
            weights,
            context,
        )
        .unwrap();
        // Where the model's own axes point after start + orient.
        let total = p.delta.compose(&placed);
        let axis = |v: [f32; 3]| {
            let o = total.apply_to_vertex(sc_mesh_core::Vertex::new([0.0; 3])).coords();
            let t = total.apply_to_vertex(sc_mesh_core::Vertex::new(v)).coords();
            let d = [t[0] - o[0], t[1] - o[1], t[2] - o[2]];
            let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            d.map(|c| c / n)
        };
        let up = [axis([1.0, 0.0, 0.0])[2], axis([0.0, 1.0, 0.0])[2], axis([0.0, 0.0, 1.0])[2]];
        let c = p.chosen;
        println!(
            "{label:22} up(model)=({:+.3},{:+.3},{:+.3}) h {:6.2} contacts {:5.1} zero {} anchored {} min {:5.0} copies {} retained {}",
            up[0], up[1], up[2], c.height, c.contacts, c.zero_support, c.anchored, c.print_minutes, c.copies, p.retained
        );
    }
    }
}

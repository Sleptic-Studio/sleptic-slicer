// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Opt-in local measurement; no external model is required by workspace tests.
//! Set SC_BENCHY_STL and run this test with --ignored --nocapture.

use sc_mesh_core::indexed_mesh::RepairPasses;
use sc_mesh_core::{IndexedMesh, transform::Transform3D};
use sc_print_glue::{SliceRun, SlicingParams};
use sc_print_primitives::{AaLevel, PositiveFiniteMm};
use sc_print_profiles::{load_material_profile_from_path, load_printer_profile_from_path};
use sc_scene::{LeafPayload, Scene};
use sc_sl1cer::CancelToken;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn triangle_cross(mesh: &IndexedMesh, face: &sc_mesh_core::IndexedTriangle) -> [f64; 3] {
    let [a, b, c] = face
        .vertices
        .map(|i| mesh.vertices[usize::from(i)].coords().map(f64::from));
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]
}

fn signed_volume(mesh: &IndexedMesh) -> f64 {
    mesh.faces
        .iter()
        .map(|face| {
            let a = mesh.vertices[usize::from(face.vertices[0])]
                .coords()
                .map(f64::from);
            let n = triangle_cross(mesh, face);
            (a[0] * n[0] + a[1] * n[1] + a[2] * n[2]) / 6.0
        })
        .sum()
}

fn scene_from_mesh(mesh: IndexedMesh) -> Scene {
    let mut scene = Scene::new();
    let leaf = scene
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(mesh)),
        )
        .unwrap();
    scene.add_root(leaf).unwrap();
    scene.drop_at([0., 0.], 0.).unwrap();
    scene
}

#[test]
#[ignore = "requires local SC_BENCHY_STL; measurement only"]
fn benchy_repair_measurement() {
    let path = PathBuf::from(std::env::var_os("SC_BENCHY_STL").expect("set SC_BENCHY_STL"));
    let mut raw = sc_mesh_formats::load_mesh_from_path(&path, None, None)
        .unwrap()
        .graph;
    let leaf = raw.first_leaf_id().unwrap();
    let before = raw.mesh_at_leaf(leaf).unwrap().clone();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let printer = load_printer_profile_from_path(
        &root.join("assets/profiles/printers/elegoo-mars-3-ctb.toml"),
    )
    .unwrap();
    let material =
        load_material_profile_from_path(&root.join("assets/profiles/materials/standard-405.toml"))
            .unwrap()
            .into();
    let params = SlicingParams {
        process_overrides: Default::default(),
        layer_height_request: Some(PositiveFiniteMm::new(0.05).unwrap()),
        first_layer_height: None,
        aa_level: AaLevel::X4,
    };
    raw.drop_at([0., 0.], 0.).unwrap();
    let refusal = sc_print_glue::PlateJob::new(&raw, &printer, &material, &params).slice(&CancelToken::new(), |_| {}, false)
    .unwrap_err();
    println!("Strict unrepaired baseline: {refusal}");
    let outcomes = raw.repair_meshes(RepairPasses::default_repair());
    for outcome in &outcomes {
        assert!(outcome.refusal.is_none(), "{outcome:?}");
    }
    let after = raw.mesh_at_leaf(leaf).unwrap();
    println!(
        "Faces before={} after={} removed={}; fixes={}",
        before.faces.len(),
        after.faces.len(),
        before.faces.len() - after.faces.len(),
        outcomes.iter().map(|o| o.fixes.len()).sum::<usize>()
    );
    println!(
        "Signed mesh volume mm3 before={:.12} after={:.12} delta={:.12}",
        signed_volume(&before),
        signed_volume(after),
        signed_volume(after) - signed_volume(&before)
    );

    // Independent tolerant reference: discard ONLY repeated-index triangles,
    // without invoking repair or changing any surviving face's winding.
    // This is not a claim that the strict production pipeline accepts the raw STL.
    let mut reference = before.clone();
    reference.faces.retain(|face| {
        let [a, b, c] = face.vertices;
        let collapsed = a == b || b == c || c == a;
        if collapsed {
            assert_eq!(triangle_cross(&before, face), [0.; 3]);
        }
        !collapsed
    });
    println!(
        "Collapsed-index zero-area reference faces removed={}",
        before.faces.len() - reference.faces.len()
    );
    let reference = scene_from_mesh(reference);
    let mut volumes = Vec::new();
    let mut stacks = Vec::new();
    for (label, scene) in [
        ("zero-area-tolerant reference", &reference),
        ("default repair", &raw),
    ] {
        let SliceRun::Completed(plate) = sc_print_glue::PlateJob::new(scene, &printer, &material, &params).slice(&CancelToken::new(), |_| {}, false)
        .unwrap() else {
            panic!("cancelled")
        };
        let stack = plate.merge().unwrap();
        // Same contour-area x slab-height estimator as GUI print_estimate.rs.
        let volume: f64 = stack
            .layers()
            .iter()
            .flat_map(|l| l.polygons())
            .map(|p| {
                p.outer().signed_area().abs()
                    - p.holes().iter().map(|h| h.signed_area().abs()).sum::<f64>()
            })
            .sum::<f64>()
            * 0.05;
        println!(
            "{label}: layers={} contour volume mm3={volume:.12}",
            stack.layers().len()
        );
        volumes.push(volume);
        stacks.push(stack);
    }
    println!(
        "Sliced reference-to-repair delta mm3={:.12} percent={:.12}",
        volumes[1] - volumes[0],
        100. * (volumes[1] - volumes[0]) / volumes[0]
    );
    assert_eq!(
        stacks[0], stacks[1],
        "repair must preserve this reference's contours"
    );
}

#[test]
#[ignore = "requires local SC_BENCHY_STL; R14b measurement and artifacts"]
fn benchy_columns_vs_tree() {
    use sc_print_profiles::support_settings::SupportSettings;
    let path = PathBuf::from(std::env::var_os("SC_BENCHY_STL").expect("set SC_BENCHY_STL"));
    let mut source = sc_mesh_formats::load_mesh_from_path(&path, None, None)
        .unwrap()
        .graph;
    source.repair_meshes(RepairPasses::default_repair());
    source.drop_at([0.0, 0.0], 0.0).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let printer = load_printer_profile_from_path(
        &root.join("assets/profiles/printers/elegoo-mars-3-ctb.toml"),
    )
    .unwrap();
    let material =
        load_material_profile_from_path(&root.join("assets/profiles/materials/standard-405.toml"))
            .unwrap()
            .into();
    let mut trunk_counts = Vec::new();
    for tree in [false, true] {
        let settings = SupportSettings {
            tree_supports: tree,
            ..Default::default()
        };
        let start = std::time::Instant::now();
        let (geometric, _) =
            sc_print_glue::supports::geometric_supported_scene(&source, &settings).unwrap();
        let profile = sc_print_glue::slice_profile_for_selection(
            &geometric,
            &printer,
            &material,
            Some(PositiveFiniteMm::new(0.05).unwrap()),
            None,
            AaLevel::X4,
            sc_sl1cer::profile::PackingMode::None,
        )
        .unwrap();
        let (scene, structure, analysis) =
            sc_print_glue::supports::auto_supported_scene_analyzed_with_fallback(
                &source, &settings, &profile, true,
            )
            .unwrap();
        let prepare_seconds = start.elapsed().as_secs_f64();
        let start = std::time::Instant::now();
        let outcome = sc_sl1cer::api::slice_scene_with_fallback(
            &scene,
            &profile,
            &CancelToken::new(),
            |_| {},
            true,
        )
        .unwrap();
        let slice_seconds = start.elapsed().as_secs_f64();
        assert!(matches!(outcome, sc_sl1cer::SliceOutcome::Completed(_)));
        let style = if tree { "tree" } else { "columns" };
        trunk_counts.push(structure.columns.len());
        assert!(
            analysis.islands.is_empty(),
            "{style}: {:?}",
            analysis.islands
        );
        if tree {
            for mesh in &structure.columns {
                if mesh.vertices.len() <= 40 {
                    continue;
                }
                for branch in mesh.vertices[30..].as_chunks::<40>().0 {
                    let centre = |ring: usize, axis: usize| {
                        branch[ring * 10..ring * 10 + 10]
                            .iter()
                            .map(|v| f64::from(v[axis]))
                            .sum::<f64>()
                            / 10.
                    };
                    let distance = (centre(2, 0) - centre(1, 0)).hypot(centre(2, 1) - centre(1, 1));
                    let rise = centre(2, 2) - centre(1, 2);
                    assert!(distance <= rise * settings.branch_angle_deg.to_radians().tan() + 1e-4);
                }
            }
        }

        println!(
            "R14B style={style} contacts={} trunks={} resin_ml={:.6} slice_seconds={slice_seconds:.6} prepare_seconds={prepare_seconds:.6} islands={}",
            structure.points.len(),
            structure.columns.len(),
            analysis.volume_ml,
            analysis.islands.len()
        );
        if let Some(output) = std::env::var_os("SC_R14B_OUTPUT") {
            let mut file =
                std::fs::File::create(PathBuf::from(output).join(format!("benchy-{style}.3mf")))
                    .unwrap();
            sc_project_formats::write_project(
                &scene,
                &sc_project_formats::ProjectPayload::default(),
                sc_project_formats::EmbedProfiles::No,
                None,
                None,
                sc_project_formats::ProjectFileType::ThreeMf,
                &mut file,
            )
            .unwrap();
        }
    }
    assert!(
        trunk_counts[1] * 100 <= trunk_counts[0] * 60,
        "{trunk_counts:?}"
    );
}

#[test]
#[ignore = "needs supported 3DBenchy 3MFs (not redistributable); set SLEPTIC_BENCHY_3MF_DIR"]
fn r15_saved_support_diagnosis() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for style in ["columns", "tree"] {
        let path =
            Path::new(&std::env::var("SLEPTIC_BENCHY_3MF_DIR").expect("SLEPTIC_BENCHY_3MF_DIR"))
                .join(format!("benchy-{style}.3mf"));
        let loaded = sc_project_formats::load_scene(&path, None, |_| {}).unwrap();
        for repair in [true] {
            let mut scene = loaded.scene.clone();
            if repair {
                scene.repair_meshes(RepairPasses::default_repair());
            }
            for (id, aa) in [
                ("elegoo-mars-3-ctb", AaLevel::X4),
                ("elegoo-mars-3-ctb", AaLevel::Off),
            ] {
                let printer = load_printer_profile_from_path(
                    &root.join(format!("assets/profiles/printers/{id}.toml")),
                )
                .unwrap();
                let material = load_material_profile_from_path(
                    &root.join("assets/profiles/materials/standard-405.toml"),
                )
                .unwrap()
                .into();
                let profile = sc_print_glue::slice_profile_for_selection(
                    &scene,
                    &printer,
                    &material,
                    Some(PositiveFiniteMm::new(0.05).unwrap()),
                    None,
                    aa,
                    sc_sl1cer::profile::PackingMode::None,
                )
                .unwrap();
                let sc_sl1cer::SliceOutcome::Completed(leaves) =
                    sc_sl1cer::api::slice_scene_with_fallback(
                        &scene,
                        &profile,
                        &CancelToken::new(),
                        |_| {},
                        true,
                    )
                    .unwrap()
                else {
                    panic!()
                };
                let stack = sc_sl1cer::contour::merge_stacks(
                    leaves.into_iter().map(|l| l.into_stack()).collect(),
                )
                .unwrap();
                let start = std::time::Instant::now();
                let analysis = sc_print_job::analyze_layers(&stack, &profile);
                println!(
                    "DIAG style={style} repair={repair} printer={id} aa={aa:?} seconds={} islands={} first={:?}",
                    start.elapsed().as_secs_f64(),
                    analysis.islands.len(),
                    analysis.islands.first()
                );
                assert!(
                    analysis.islands.is_empty(),
                    "saved {style} fixture: {:?}",
                    analysis.islands
                );
            }
        }
    }
}

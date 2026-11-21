// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! R17 local performance audit and portable output guard.
use sc_mesh_core::indexed_mesh::RepairPasses;
use sc_print_primitives::{AaLevel, PositiveFiniteMm};
use sc_sl1cer::{CancelToken, SliceOutcome};
use std::path::Path;

#[test]
#[ignore = "local library timing audit; set SLEPTIC_R17_MODELS"]
fn library_slice_timings() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = std::env::var("SLEPTIC_R17_MODELS").unwrap();
    let printer = sc_print_profiles::load_printer_profile_from_path(
        &root.join("assets/profiles/printers/sleptic-diy-msla.toml"),
    )
    .unwrap();
    let material = sc_print_profiles::load_material_profile_from_path(
        &root.join("assets/profiles/materials/standard-405.toml"),
    )
    .unwrap()
    .into();
    let names = std::env::var("SLEPTIC_R17_FILES").unwrap_or_else(|_| "d_full.stl|fixed jack in the box.stl|camera+mount.stl|THINGYAHAHAHHA.stl|low effort thing.stl|pillar 2.stl".into());
    for name in names.split('|') {
        let start = std::time::Instant::now();
        let mut scene = sc_project_formats::load_scene(&Path::new(&dir).join(name), None, |_| {})
            .unwrap()
            .scene;
        scene.repair_meshes(RepairPasses::default_repair());
        scene.drop_at([0., 0.], 0.).unwrap();
        let load = start.elapsed().as_secs_f64();
        let profile = sc_print_glue::slice_profile_for_selection(
            &scene,
            &printer,
            &material,
            Some(PositiveFiniteMm::new(0.05).unwrap()),
            None,
            AaLevel::X4,
            sc_sl1cer::profile::PackingMode::None,
        )
        .unwrap();
        let start = std::time::Instant::now();
        let result = sc_sl1cer::api::slice_scene_with_fallback(
            &scene,
            &profile,
            &CancelToken::new(),
            |_| {},
            true,
        )
        .unwrap();
        let seconds = start.elapsed().as_secs_f64();
        let SliceOutcome::Completed(leaves) = result else {
            panic!("cancelled")
        };
        let stack = sc_sl1cer::contour::merge_stacks(
            leaves.iter().cloned().map(|l| l.into_stack()).collect(),
        )
        .unwrap();
        let analysis_start = std::time::Instant::now();
        let analysis = sc_print_job::analyze_layers(&stack, &profile);
        println!(
            "R17 analysis {name}: seconds={:.6} risks={} volume_ml={:.6}",
            analysis_start.elapsed().as_secs_f64(),
            analysis.risks.len(),
            analysis.volume_ml
        );
        println!(
            "R17 {name}: load_repair={load:.6}s slice={seconds:.6}s layers={} tolerant_leaves={}",
            profile.layer_schedule().total_layers(),
            leaves.iter().filter(|l| l.tolerant()).count()
        );
    }
}

#[test]
#[ignore = "needs supported 3DBenchy 3MFs (not redistributable); set SLEPTIC_BENCHY_3MF_DIR"]
fn benchy_golden_output() {
    use sc_sl1cer::raster::{accuracy::rasterize_accurate, pixel_map::PixelMap};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let loaded = sc_project_formats::load_scene(
        &Path::new(&std::env::var("SLEPTIC_BENCHY_3MF_DIR").expect("SLEPTIC_BENCHY_3MF_DIR"))
            .join("benchy-columns.3mf"),
        None,
        |_| {},
    )
    .unwrap();
    let id = loaded
        .scene
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .unwrap()
        .into_iter()
        .find(|id| {
            !loaded
                .scene
                .node(*id)
                .and_then(|n| n.meta.as_ref())
                .and_then(|m| m.comment.as_ref())
                .is_some_and(|c| c.starts_with("sc-generated-support"))
        })
        .unwrap();
    let mesh = sc_scene::bake_leaf_to_world(&loaded.scene, id).unwrap();
    let mut scene = sc_scene::Scene::new();
    let id = scene
        .add_leaf(
            None,
            sc_mesh_core::transform::Transform3D::Identity,
            sc_scene::LeafPayload::Mesh(std::sync::Arc::new(mesh)),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    scene.repair_meshes(RepairPasses::default_repair());
    scene.drop_at([0., 0.], 0.).unwrap();
    let printer = sc_print_profiles::load_printer_profile_from_path(
        &root.join("assets/profiles/printers/sleptic-diy-msla.toml"),
    )
    .unwrap();
    let material = sc_print_profiles::load_material_profile_from_path(
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
        AaLevel::X4,
        sc_sl1cer::profile::PackingMode::None,
    )
    .unwrap();
    let SliceOutcome::Completed(leaves) = sc_sl1cer::api::slice_scene_with_fallback(
        &scene,
        &profile,
        &CancelToken::new(),
        |_| {},
        true,
    )
    .unwrap() else {
        panic!("cancelled")
    };
    let stack =
        sc_sl1cer::contour::merge_stacks(leaves.into_iter().map(|l| l.into_stack()).collect())
            .unwrap();
    let b = profile.build_volume();
    let map = PixelMap::new(
        PositiveFiniteMm::new(b.x_mm()).unwrap(),
        PositiveFiniteMm::new(b.y_mm()).unwrap(),
        *profile.pixel_resolution(),
        profile.mirror(),
    );
    let mut output = format!(
        "# SPDX-License-Identifier: AGPL-3.0-only\nlayers={}\n",
        stack.layers().len()
    );
    let mut volume = 0.0;
    let mut raster_volume = 0.0;
    for (i, layer) in stack.layers().iter().enumerate() {
        volume += layer
            .polygons()
            .iter()
            .map(|p| {
                p.outer().signed_area().abs()
                    - p.holes().iter().map(|h| h.signed_area().abs()).sum::<f64>()
            })
            .sum::<f64>()
            * 0.05;
        let raster = rasterize_accurate(layer, &map, AaLevel::X4, 0.0, profile.accuracy(), i);
        raster_volume += raster
            .runs()
            .iter()
            .map(|r| f64::from(r.value) * f64::from(r.length) / 255.0)
            .sum::<f64>()
            * b.x_mm()
            * b.y_mm()
            / (f64::from(profile.pixel_resolution().x())
                * f64::from(profile.pixel_resolution().y()))
            * 0.05;
        // Hash every maximal lit run's row-major offset, length and alpha.
        // Dark runs affect offsets; no full image allocation is necessary.
        let mut hash = 0xcbf29ce484222325u64;
        let mut offset = 0u64;
        let mut lit = 0u64;
        for r in raster.runs() {
            if r.value != 0 {
                lit += u64::from(r.length);
                for byte in offset
                    .to_le_bytes()
                    .into_iter()
                    .chain(r.length.to_le_bytes())
                    .chain([r.value])
                {
                    hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
                }
            }
            offset += u64::from(r.length);
        }
        output.push_str(&format!("{i}: {lit} {hash:016x}\n"));
    }
    output.push_str(&format!(
        "volume_mm3={volume:.6}\nraster_volume_mm3={raster_volume:.6}\n"
    ));
    let path = root.join("apps/slicer-cli/tests/fixtures/r17-benchy-golden.txt");
    if std::env::var_os("SLEPTIC_UPDATE_R17_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &output).unwrap();
    }
    assert_eq!(
        output,
        std::fs::read_to_string(path).expect("committed golden required"),
        "Slicer output changed; review geometry and images before deliberately updating the golden."
    );
}

#[test]
#[ignore = "local Logo diagnosis"]
fn logo_diagnosis() {
    let path = std::env::var("SLEPTIC_LOGO").unwrap();
    let mut scene = sc_project_formats::load_scene(Path::new(&path), None, |_| {})
        .unwrap()
        .scene;
    scene.repair_meshes(RepairPasses::default_repair());
    scene.drop_at([0., 0.], 0.).unwrap();
    let profile = sc_sl1cer::profile::SliceProfile::new(
        sc_sl1cer::profile::BuildVolume::new(192., 120., 200.).unwrap(),
        sc_sl1cer::profile::PixelResolution::new(1920, 1200).unwrap(),
        sc_sl1cer::profile::LayerSchedule::uniform(0.05, 0.05, 22).unwrap(),
        Default::default(),
        AaLevel::Off,
        sc_sl1cer::profile::PackingMode::None,
    );
    for id in scene
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .unwrap()
    {
        let result = sc_sl1cer::api::slice_leaf(&scene, id, &profile);
        println!(
            "leaf={id:?} faces={} result={:?}",
            scene.mesh_at_leaf(id).unwrap().faces.len(),
            result.as_ref().err()
        );
        assert!(
            result.is_ok(),
            "Strict stitching should recover the coplanar patch"
        );
    }
}

#[test]
#[ignore = "needs supported 3DBenchy 3MFs (not redistributable); set SLEPTIC_BENCHY_3MF_DIR"]
fn benchy_risk_audit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let printer = sc_print_profiles::load_printer_profile_from_path(&root.join(format!(
        "assets/profiles/printers/{}.toml",
        std::env::var("SLEPTIC_R17_PRINTER").unwrap_or_else(|_| "sleptic-diy-msla".into())
    )))
    .unwrap();
    let material = sc_print_profiles::load_material_profile_from_path(
        &root.join("assets/profiles/materials/standard-405.toml"),
    )
    .unwrap()
    .into();
    let mut counts = Vec::new();
    for style in ["none", "columns", "tree"] {
        let mut scene = sc_project_formats::load_scene(
            &Path::new(&std::env::var("SLEPTIC_BENCHY_3MF_DIR").expect("SLEPTIC_BENCHY_3MF_DIR"))
                .join(format!(
                    "benchy-{}.3mf",
                    if style == "none" { "columns" } else { style }
                )),
            None,
            |_| {},
        )
        .unwrap()
        .scene;
        if style == "none" {
            let id = scene
                .reachable_leaves(
                    sc_scene::CsgPolicy::Descend,
                    sc_scene::ReferencePolicy::Skip,
                )
                .unwrap()
                .into_iter()
                .find(|id| {
                    !scene
                        .node(*id)
                        .and_then(|n| n.meta.as_ref())
                        .and_then(|m| m.comment.as_ref())
                        .is_some_and(|c| c.starts_with("sc-generated-support"))
                })
                .unwrap();
            let mesh = sc_scene::bake_leaf_to_world(&scene, id).unwrap();
            scene = sc_scene::Scene::new();
            let id = scene
                .add_leaf(
                    None,
                    sc_mesh_core::transform::Transform3D::Identity,
                    sc_scene::LeafPayload::Mesh(std::sync::Arc::new(mesh)),
                )
                .unwrap();
            scene.add_root(id).unwrap();
            scene.drop_at([0., 0.], 0.).unwrap();
        }
        scene.repair_meshes(RepairPasses::default_repair());
        let profile = sc_print_glue::slice_profile_for_selection(
            &scene,
            &printer,
            &material,
            Some(PositiveFiniteMm::new(0.05).unwrap()),
            None,
            AaLevel::X4,
            sc_sl1cer::profile::PackingMode::None,
        )
        .unwrap();
        let SliceOutcome::Completed(leaves) = sc_sl1cer::api::slice_scene_with_fallback(
            &scene,
            &profile,
            &CancelToken::new(),
            |_| {},
            true,
        )
        .unwrap() else {
            panic!()
        };
        let stack =
            sc_sl1cer::contour::merge_stacks(leaves.into_iter().map(|l| l.into_stack()).collect())
                .unwrap();
        let analysis = sc_print_job::analyze_layers(&stack, &profile);
        let cups: Vec<_> = analysis
            .risks
            .iter()
            .filter(|r| r.kind == sc_print_job::RiskKind::SuctionCup)
            .collect();
        println!(
            "R17 {style}: cups={} trapped={} volume_ml={:.6}",
            cups.len(),
            analysis
                .risks
                .iter()
                .filter(|r| r.kind == sc_print_job::RiskKind::TrappedCavity)
                .count(),
            analysis.volume_ml
        );
        counts.push(cups.len());
        for cup in cups {
            println!(
                "R17 {style} cup: first={} last={} xy={:?} mm3={:.6}",
                cup.first_layer, cup.last_layer, cup.position_mm, cup.measured
            );
        }
    }
    assert_eq!(
        counts,
        [4, 4, 4],
        "Supports must not create reportable raft/pinhole cups"
    );
}

#[test]
#[ignore = "local wallet diagnosis"]
fn wallet_bounds() {
    let scene = sc_project_formats::load_scene(
        Path::new(&std::env::var("SLEPTIC_WALLET_3MF").expect("SLEPTIC_WALLET_3MF")),
        None,
        |_| {},
    )
    .unwrap()
    .scene;
    for id in scene
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .unwrap()
    {
        let mesh = sc_scene::bake_leaf_to_world(&scene, id).unwrap();
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for v in mesh.vertices {
            for a in 0..3 {
                lo[a] = lo[a].min(f64::from(v[a]));
                hi[a] = hi[a].max(f64::from(v[a]));
            }
        }
        println!(
            "wallet {id:?} lo={lo:?} hi={hi:?} size={:?}",
            [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]
        );
    }
}

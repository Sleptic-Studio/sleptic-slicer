// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Explicit local release audit. Wall times are evidence, never test assertions.
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

#[test]
#[ignore = "local models; release measurement only"]
fn r16_release_measurements() {
    let path = PathBuf::from(std::env::var_os("SC_R16_MODEL").expect("SC_R16_MODEL"));
    let output = PathBuf::from(std::env::var_os("SC_R16_METRICS").expect("SC_R16_METRICS"));
    let mut metrics =
        serde_json::json!({"model":path,"bytes":std::fs::metadata(&path).unwrap().len()});
    let started = Instant::now();
    let mut scene = sc_project_formats::load_scene(&path, None, |_| {})
        .unwrap()
        .scene;
    metrics["load_s"] = started.elapsed().as_secs_f64().into();
    metrics["triangles"] = scene
        .reachable_leaves(sc_scene::CsgPolicy::Refuse, sc_scene::ReferencePolicy::Skip)
        .unwrap()
        .iter()
        .map(|id| scene.leaf_mesh_arc(*id).unwrap().faces.len())
        .sum::<usize>()
        .into();
    eprintln!("R16 load: {}", metrics);
    let started = Instant::now();
    std::hint::black_box(crate::mesh_health::scene_mesh_health(&scene));
    metrics["health_s"] = started.elapsed().as_secs_f64().into();
    let started = Instant::now();
    let spatial = sc_mesh_spatial::SceneSpatial::primed_graph(scene.clone());
    metrics["spatial_s"] = started.elapsed().as_secs_f64().into();
    drop(spatial);
    let started = Instant::now();
    let reports = scene.repair_meshes(sc_mesh_core::indexed_mesh::RepairPasses::default_repair());
    metrics["repair_s"] = started.elapsed().as_secs_f64().into();
    metrics["repair_refusals"] = reports
        .iter()
        .filter(|r| r.refusal.is_some())
        .count()
        .into();
    scene.drop_at([0., 0.], 0.).unwrap();
    let library = sc_print_profiles::ProfileLibraries::platform_default();
    let printer = library
        .load_printer_by_id(&"elegoo-mars-3-ctb".parse().unwrap())
        .unwrap();
    let material = library
        .load_material_by_id(&"standard-405".parse().unwrap())
        .unwrap()
        .into();
    let params = sc_print_glue::SlicingParams {
        process_overrides: Default::default(),
        layer_height_request: Some(sc_print_primitives::PositiveFiniteMm::new(0.05).unwrap()),
        first_layer_height: None,
        aa_level: sc_print_primitives::AaLevel::Off,
    };
    let started = Instant::now();
    let slices = sc_print_glue::PlateJob::new(&scene, &printer, &material, &params).slice(&sc_sl1cer::CancelToken::new(), |_| {}, true)
    .unwrap();
    metrics["slice_s"] = started.elapsed().as_secs_f64().into();
    let sc_print_glue::SliceRun::Completed(slices) = slices else {
        panic!("cancelled");
    };
    let profile = slices.profile().clone();
    let stack = slices.merge().unwrap();
    metrics["layers"] = stack.layers().len().into();
    eprintln!("R16 slice: {}", metrics);
    let started = Instant::now();
    let analysis = sc_print_job::analyze_layers(&stack, &profile);
    metrics["risk_s"] = started.elapsed().as_secs_f64().into();
    metrics["islands"] = analysis.islands.len().into();
    let volume = crate::build_volume::BuildVolume::new(
        printer.hardware.build_volume().x_mm() as f32,
        printer.hardware.build_volume().y_mm() as f32,
        printer.hardware.build_volume().z_mm() as f32,
    );
    let samples: Vec<_> = (0..20)
        .map(|i| i * stack.layers().len().saturating_sub(1) / 19)
        .collect();
    let mut path_ms = Vec::new();
    let mut cap_ms = Vec::new();
    for i in samples {
        let layer = &stack.layers()[i];
        let started = Instant::now();
        let commands = crate::slice_cap::layer_path_commands(
            layer,
            f64::from(volume.width_mm()),
            f64::from(volume.depth_mm()),
        );
        std::hint::black_box(commands);
        path_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        let started = Instant::now();
        std::hint::black_box(crate::slice_cap::build_cap_raster(layer, &volume).unwrap());
        cap_ms.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    metrics["path_ms"] = serde_json::to_value(path_ms).unwrap();
    metrics["cap_ms"] = serde_json::to_value(cap_ms).unwrap();
    std::fs::write(&output, serde_json::to_vec_pretty(&metrics).unwrap()).unwrap();
    eprintln!("R16 metrics: {}", metrics);
}

#[test]
#[ignore = "release project I/O measurement only"]
fn r16_three_plate_project_measurement() {
    let mut scene = sc_project_formats::load_scene(
        &Path::new(&std::env::var("SLEPTIC_BENCHY_3MF_DIR").expect("SLEPTIC_BENCHY_3MF_DIR"))
            .join("benchy-tree.3mf"),
        None,
        |_| {},
    )
    .unwrap()
    .scene;
    let source = scene.clone();
    // Three complete supported plates, shared immutable geometry.
    let mut plates = scene.plates().clone();
    for _ in 0..2 {
        let plate = plates.add();
        let ids = source
            .reachable_leaves(sc_scene::CsgPolicy::Refuse, sc_scene::ReferencePolicy::Skip)
            .unwrap();
        let mapped: std::collections::BTreeMap<_, _> = ids
            .iter()
            .map(|&old| (old, scene.allocate_unbound_node_id()))
            .collect();
        for old in ids {
            let node = source.node(old).unwrap();
            let id = mapped[&old];
            let mut meta = node.meta.clone();
            if let Some(comment) = meta.as_mut().and_then(|m| m.comment.as_mut()) {
                *comment = comment
                    .lines()
                    .map(|line| {
                        if let Some(owner) = line
                            .strip_prefix("owner = ")
                            .and_then(|v| v.parse::<u32>().ok())
                            && let Some((_, new)) =
                                mapped.iter().find(|(old, _)| old.get() == owner)
                        {
                            return format!("owner = {}", new.get());
                        }
                        line.to_string()
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            scene
                .install_leaf_at(
                    id,
                    sc_scene::node::LeafAttachment::Root {
                        position: scene.roots().len(),
                    },
                    sc_scene::LeafPayload::Mesh(source.leaf_mesh_arc(old).unwrap().clone()),
                    node.local_transform,
                    meta,
                )
                .unwrap();
            plates.assignments.insert(id, plate);
        }
    }
    assert!(scene.set_plates(plates));
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("three-plates.3mf");
    let started = Instant::now();
    let mut writer = std::fs::File::create(&target).unwrap();
    sc_project_formats::write_project(
        &scene,
        &Default::default(),
        sc_project_formats::EmbedProfiles::No,
        None,
        None,
        sc_project_formats::ProjectFileType::ThreeMf,
        &mut writer,
    )
    .unwrap();
    let save = started.elapsed().as_secs_f64();
    drop(writer);
    let started = Instant::now();
    let loaded = sc_project_formats::load_scene(&target, None, |_| {}).unwrap();
    let load = started.elapsed().as_secs_f64();
    assert_eq!(loaded.scene.plates().order.len(), 3);
    let metrics = serde_json::json!({"save_s":save,"load_s":load,"bytes":std::fs::metadata(target).unwrap().len(),"plates":3,"leaves":scene.reachable_leaf_count()});
    let output = std::env::var_os("SC_R16_METRICS").expect("SC_R16_METRICS");
    std::fs::write(output, serde_json::to_vec_pretty(&metrics).unwrap()).unwrap();
    eprintln!("R16 project: {metrics}");
}

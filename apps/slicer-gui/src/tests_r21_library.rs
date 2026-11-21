// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Opt-in checks against Sleptic's local library. Output stays under target/.
use super::*;
use sc_sl1cer::{
    contour::ContourStack,
    profile::{AaLevel, PackingMode},
};

pub(super) fn profiles() -> (
    sc_print_profiles::PrinterProfile,
    sc_print_profiles::EffectiveMaterial,
) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    (
        sc_print_profiles::load_printer_profile_from_path(
            &root.join("assets/profiles/printers/sleptic-diy-msla.toml"),
        )
        .unwrap(),
        sc_print_profiles::load_material_profile_from_path(
            &root.join("assets/profiles/materials/standard-405.toml"),
        )
        .unwrap()
        .into(),
    )
}
pub(super) fn slice(
    scene: &Scene,
    printer: &sc_print_profiles::PrinterProfile,
    material: &sc_print_profiles::EffectiveMaterial,
) -> (ContourStack, sc_sl1cer::profile::SliceProfile) {
    let params = sc_print_glue::SlicingParams {
        process_overrides: Default::default(),
        layer_height_request: None,
        first_layer_height: None,
        aa_level: AaLevel::X4,
    };
    let sc_print_glue::SliceRun::Completed(result) =
        sc_print_glue::PlateJob::new(scene, printer, material, &params).slice(&sc_sl1cer::CancelToken::new(), |_| {}, true)
        .unwrap()
    else {
        panic!("cancelled")
    };
    let profile = result.profile().clone();
    (result.merge().unwrap(), profile)
}
pub(super) fn export_first(
    stack: &ContourStack,
    profile: &sc_sl1cer::profile::SliceProfile,
) -> Vec<u8> {
    let first = ContourStack::new(vec![stack.layers()[0].clone()]).unwrap();
    let dir = tempfile::tempdir().unwrap();
    sc_sla_formats::debug::png::write_per_layer(
        &first,
        dir.path(),
        &sc_sla_formats::WriterOptions::from_profile(profile, "R21"),
    )
    .unwrap();
    std::fs::read(dir.path().join("layer_0000.png")).unwrap()
}
#[test]
#[ignore = "requires SLEPTIC_R21_MODELS; full real-library slice/support/fill audit"]
fn library_consistency() {
    use std::io::Write;
    let dir = std::env::var_os("SLEPTIC_R21_MODELS").expect("local model library");
    let names = std::env::var("SLEPTIC_R21_FILES").unwrap_or_else(|_| "vice_grips_v25_full.stl|Lapras_Cuerpo (1).STL|Jaw.STL|fly no color.stl|butterfly done repaired.stl|camera mount.stl|3DBenchy.stl|Devil-FanShowdown.stl|bottom plate v3.stl|BELT TENSION NEW.stl|belt tension thing.stl|bottom plate v2 .stl".into());
    let (printer, material) = profiles();
    let envelope = printer.hardware.build_volume();
    let volume = crate::build_volume::BuildVolume::new(
        envelope.x_mm() as f32,
        envelope.y_mm() as f32,
        envelope.z_mm() as f32,
    );
    let config = sc_print_profiles::resolve(&printer, &material, &Default::default()).unwrap();
    let context =
        sc_print_glue::orient::Context::for_profiles(&printer, &material, &Default::default())
            .unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    let mut csv = std::fs::File::create(root.join("r21-consistency.csv")).unwrap();
    writeln!(csv,"model,pose,supports,bbox_x,bbox_y,bbox_z,position_x,position_y,position_z,scene_top,loose_top,layers,slice_top,predicted_minutes,actual_minutes,predicted_copies,filled_copies,columns,volume_ml,weight_g,first_png_bytes").unwrap();
    for name in names.split('|') {
        let mut original =
            sc_project_formats::load_scene(&std::path::Path::new(&dir).join(name), None, |_| {})
                .unwrap()
                .scene;
        original.repair_meshes(sc_mesh_core::indexed_mesh::RepairPasses::default_repair());
        original.drop_at([0.0, 0.0], 0.0).unwrap();
        let b = original.tight_aabb().unwrap().unwrap();
        let lo = b.min().coords();
        let hi = b.max().coords();
        let scale = (190.0 / f64::from(hi[0] - lo[0]))
            .min(118.0 / f64::from(hi[1] - lo[1]))
            .min(198.0 / f64::from(hi[2] - lo[2]))
            .min(1.0);
        for root in original.roots().to_vec() {
            let before = original.node(root).unwrap().local_transform;
            original
                .set_local_transform(
                    root,
                    Transform3D::Affine(AffineTransform3D::from_uniform_scale(scale))
                        .compose(&before),
                )
                .unwrap();
        }
        original.drop_at([0.0, 0.0], 0.0).unwrap();
        let id = original.first_leaf_id().unwrap();
        let proposal = sc_print_glue::orient::search_with_context(
            &original,
            id,
            envelope,
            45.0,
            sc_print_glue::orient::Preset::Balanced.weights(),
            context,
        )
        .unwrap();
        let mut oriented = original.clone();
        crate::layout_tools::world_edit(&oriented, id, proposal.delta)
            .unwrap()
            .apply_to_scene(&mut oriented)
            .unwrap();
        for (pose, source) in [("original", original), ("oriented", oriented)] {
            for supported in [false, true] {
                let mut ctx = context;
                ctx.supports = supported;
                let predicted =
                    sc_print_glue::orient::measure_with_context(&source, id, envelope, 45.0, ctx)
                        .unwrap();
                let (scene, generated_count) = if supported {
                    let (geometric, _) = sc_print_glue::supports::geometric_supported_scene(
                        &source,
                        &context.support,
                    )
                    .unwrap();
                    let profile = sc_print_glue::slice_profile_for_selection(
                        &geometric,
                        &printer,
                        &material,
                        None,
                        None,
                        AaLevel::X4,
                        PackingMode::None,
                    )
                    .unwrap();
                    let (scene, structure, _) =
                        sc_print_glue::supports::auto_supported_scene_analyzed_with_fallback(
                            &source,
                            &context.support,
                            &profile,
                            true,
                        )
                        .unwrap();
                    (scene, structure.points.len())
                } else {
                    (source.clone(), 0)
                };
                let spatial = sc_mesh_spatial::SceneSpatial::from_graph(scene.clone());
                let q = spatial.leaf(id).unwrap();
                let bbox = q.transformed_aabb(q.world_transform()).unwrap();
                let a = bbox.min().coords();
                let b = bbox.max().coords();
                let size = [0, 1, 2].map(|i| b[i] - a[i]);
                let baked = sc_scene::bake_leaf_to_world(&scene, id)
                    .unwrap()
                    .local_aabb()
                    .unwrap();
                for axis in 0..3 {
                    assert!((bbox.min()[axis] - baked.min()[axis]).abs() < 0.0001);
                    assert!((bbox.max()[axis] - baked.max()[axis]).abs() < 0.0001);
                }
                let position = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, a[2]];
                let pivot = crate::gizmo::part_pivot(&spatial, id).unwrap();
                assert!(
                    (pivot.x - position[0]).abs() < 0.0001
                        && (pivot.y - position[1]).abs() < 0.0001
                );
                assert!((pivot.z - position[2] - size[2] / 2.0).abs() < 0.0001);
                let tight = scene.tight_aabb().unwrap().unwrap();
                let cached = sc_mesh_spatial::scene_tight_aabb(&spatial).unwrap();
                assert!((cached.max()[2] - tight.max()[2]).abs() < 0.0001);
                let (stack, profile) = slice(&scene, &printer, &material);
                let n = stack.layers().len();
                let sc_sl1cer::profile::LayerSchedule::Uniform(grid) = profile.layer_schedule();
                let slice_top = grid.first_layer_height().value()
                    + (n as f64 - 1.0) * grid.layer_height().value();
                let top = f64::from(tight.max()[2]);
                assert!(
                    slice_top + 0.00001 >= top && slice_top - top < 0.05001,
                    "{name} {pose}: {top} -> {slice_top}"
                );
                let seconds = sc_print_job::ExposureSchedule::new(
                    &config.material_resolved,
                    profile.layer_schedule(),
                )
                .total_time_s();
                assert!(
                    (seconds / 60.0 - predicted.print_minutes).abs() < 0.19,
                    "{name} {pose} prediction {} vs {}",
                    predicted.print_minutes,
                    seconds / 60.0
                );
                let mut filled = scene.clone();
                let actual_copies =
                    crate::layout_tools::fill_plate(&mut filled, id, volume, context.gap_mm)
                        .map_or(0, |(edit, count)| {
                            edit.apply_to_scene(&mut filled).unwrap();
                            count + 1
                        });
                assert_eq!(
                    actual_copies, predicted.copies,
                    "{name} {pose} supports={supported} actual Fill plate"
                );
                let columns = crate::auto_support_run::support_ids(&scene)
                    .iter()
                    .filter(|&&id| {
                        !scene
                            .node(id)
                            .unwrap()
                            .meta
                            .as_ref()
                            .unwrap()
                            .comment
                            .as_ref()
                            .unwrap()
                            .starts_with("sc-generated-support-raft")
                    })
                    .count();
                assert_eq!(
                    columns, generated_count,
                    "{name} generated contacts vs scene/UI columns"
                );
                let volume_ml = sc_print_job::analyze_layers(&stack, &profile).volume_ml;
                let export_volume = sc_print_job::raster_volume_ml(&stack, &profile);
                assert!(
                    (volume_ml - export_volume).abs() < 1e-7,
                    "{name} Prepare/Preview vs export volume"
                );
                let estimate = sc_print_job::PrintEstimate::new(
                    &sc_print_job::ExposureSchedule::new(
                        &config.material_resolved,
                        profile.layer_schedule(),
                    ),
                    volume_ml,
                    material.defaults().density_g_ml,
                    None,
                );
                let png = export_first(&stack, &profile);
                let image = image::load_from_memory(&png).unwrap().to_luma8();
                // A contour tip can be thinner than a pixel and legitimately dark.
                if let Some((x, y, _)) = image.enumerate_pixels().find(|(_, _, p)| p[0] == 255) {
                    let point = sc_sl1cer::geom2d::Point2D::new(
                        -envelope.x_mm() / 2.0
                            + (f64::from(x) + 0.5) * envelope.x_mm() / f64::from(image.width()),
                        envelope.y_mm() / 2.0
                            - (f64::from(y) + 0.5) * envelope.y_mm() / f64::from(image.height()),
                    );
                    let contains = |ring: &sc_sl1cer::contour::ClosedRing| {
                        let vertices = ring.vertices();
                        let mut inside = false;
                        for i in 0..vertices.len() {
                            let a = vertices[i];
                            let b = vertices[(i + 1) % vertices.len()];
                            if (a.y > point.y) != (b.y > point.y)
                                && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x
                            {
                                inside = !inside;
                            }
                        }
                        inside
                    };
                    assert!(
                        stack.layers()[0]
                            .polygons()
                            .iter()
                            .any(|p| contains(p.outer()) && !p.holes().iter().any(contains)),
                        "{name} exported pixel ({x},{y}) disagrees with plate point {point:?}"
                    );
                }
                writeln!(csv,"{name},{pose},{supported},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{top:.4},{:.4},{n},{slice_top:.4},{:.4},{:.4},{},{actual_copies},{columns},{volume_ml:.6},{:.6},{}",size[0],size[1],size[2],position[0],position[1],position[2],scene.aabb().unwrap().unwrap().max()[2],predicted.print_minutes,seconds/60.0,predicted.copies,estimate.weight_g,png.len()).unwrap();
                csv.flush().unwrap();
                println!(
                    "R21 {name} {pose} supports={supported}: {n} layers, copies {actual_copies}, columns {columns}"
                );
            }
        }
    }
}

#[test]
#[ignore = "requires SLEPTIC_R21_MODELS; measures gizmo displacement on the 12 real models"]
fn library_gizmo_displacement() {
    use std::io::Write;
    let dir = std::env::var_os("SLEPTIC_R21_MODELS").unwrap();
    let names=std::env::var("SLEPTIC_R21_FILES").unwrap_or_else(|_| "vice_grips_v25_full.stl|Lapras_Cuerpo (1).STL|Jaw.STL|fly no color.stl|butterfly done repaired.stl|camera mount.stl|3DBenchy.stl|Devil-FanShowdown.stl|bottom plate v3.stl|BELT TENSION NEW.stl|belt tension thing.stl|bottom plate v2 .stl".into());
    let (printer, material) = profiles();
    let envelope = printer.hardware.build_volume();
    let context =
        sc_print_glue::orient::Context::for_profiles(&printer, &material, &Default::default())
            .unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    let mut csv = std::fs::File::create(root.join("r21-gizmo.csv")).unwrap();
    writeln!(csv, "model,pose,old_xy_offset_mm,current_xy_offset_mm").unwrap();
    for name in names.split('|') {
        let mut source =
            sc_project_formats::load_scene(&std::path::Path::new(&dir).join(name), None, |_| {})
                .unwrap()
                .scene;
        source.repair_meshes(sc_mesh_core::indexed_mesh::RepairPasses::default_repair());
        source.drop_at([0.0, 0.0], 0.0).unwrap();
        let bounds = source.tight_aabb().unwrap().unwrap();
        let a = bounds.min().coords();
        let b = bounds.max().coords();
        let scale = (190.0 / f64::from(b[0] - a[0]))
            .min(118.0 / f64::from(b[1] - a[1]))
            .min(198.0 / f64::from(b[2] - a[2]))
            .min(1.0);
        for root in source.roots().to_vec() {
            let before = source.node(root).unwrap().local_transform;
            source
                .set_local_transform(
                    root,
                    Transform3D::Affine(AffineTransform3D::from_uniform_scale(scale))
                        .compose(&before),
                )
                .unwrap();
        }
        source.drop_at([0.0, 0.0], 0.0).unwrap();
        let id = source.first_leaf_id().unwrap();
        let proposal = sc_print_glue::orient::search_with_context(
            &source,
            id,
            envelope,
            45.0,
            sc_print_glue::orient::Preset::Balanced.weights(),
            context,
        )
        .unwrap();
        let mut oriented = source.clone();
        crate::layout_tools::world_edit(&oriented, id, proposal.delta)
            .unwrap()
            .apply_to_scene(&mut oriented)
            .unwrap();
        for (pose, scene) in [("original", source), ("oriented", oriented)] {
            if name == "Lapras_Cuerpo (1).STL" && pose == "oriented" {
                let path = root.join("r21-worst-gizmo.3mf");
                sc_project_formats::threemf::write_threemf_project(
                    &scene,
                    &Default::default(),
                    sc_project_formats::EmbedProfiles::No,
                    None,
                    None,
                    &mut std::fs::File::create(path).unwrap(),
                )
                .unwrap();
            }
            let spatial = sc_mesh_spatial::SceneSpatial::from_graph(scene.clone());
            let query = spatial.leaf(id).unwrap();
            let local = query.local_aabb().unwrap();
            let a = local.min().coords();
            let b = local.max().coords();
            let center = Vertex::new([0, 1, 2].map(|i| (a[i] + b[i]) / 2.0));
            let before = match query.world_transform() {
                Transform3D::Identity => center,
                Transform3D::Affine(a) => a.apply_to_vertex(center),
            };
            let bounds = query.transformed_aabb(query.world_transform()).unwrap();
            let a = bounds.min().coords();
            let b = bounds.max().coords();
            let expected = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            let old_offset = (before[0] - expected[0]).hypot(before[1] - expected[1]);
            let pivot = crate::gizmo::part_pivot(&spatial, id).unwrap();
            let offset = (pivot.x - expected[0]).hypot(pivot.y - expected[1]);
            assert!(
                offset < 0.0001,
                "{name} {pose}: Position/gizmo differ by {offset} mm"
            );
            writeln!(csv, "{name},{pose},{old_offset:.6},{offset:.6}").unwrap();
            println!("R21GIZMO {name} {pose}: old {old_offset:.6} mm; current {offset:.6} mm");
        }
    }
}

#[test]
#[ignore = "R23 repaired library completion calibration"]
fn r23_library_support_completion() {
    let root = std::path::PathBuf::from(std::env::var_os("SLEPTIC_R21_MODELS").unwrap());
    let names = std::env::var("SLEPTIC_R23_FILES").unwrap_or_else(|_| {
        "camera mount.stl|fly no color.stl|butterfly done repaired.stl|Lapras_Cuerpo (1).STL".into()
    });
    let (printer, material) = profiles();
    let envelope = printer.hardware.build_volume();
    let context =
        sc_print_glue::orient::Context::for_profiles(&printer, &material, &Default::default())
            .unwrap();
    for name in names.split('|') {
        let mut source = sc_project_formats::load_scene(&root.join(name), None, |_| {})
            .unwrap()
            .scene;
        source.repair_meshes(sc_mesh_core::indexed_mesh::RepairPasses::default_repair());
        source.drop_at([0.0, 0.0], 0.0).unwrap();
        let id = source.first_leaf_id().unwrap();
        let proposal = sc_print_glue::orient::search_with_context(
            &source,
            id,
            envelope,
            45.0,
            sc_print_glue::orient::Preset::Balanced.weights(),
            context,
        )
        .unwrap();
        crate::layout_tools::world_edit(&source, id, proposal.delta)
            .unwrap()
            .apply_to_scene(&mut source)
            .unwrap();
        let (geometric, _) =
            sc_print_glue::supports::geometric_supported_scene(&source, &context.support).unwrap();
        let profile = sc_print_glue::slice_profile_for_selection(
            &geometric,
            &printer,
            &material,
            None,
            None,
            AaLevel::X4,
            PackingMode::None,
        )
        .unwrap();
        let (_, structure, analysis) =
            sc_print_glue::supports::auto_supported_scene_analyzed_with_fallback(
                &source,
                &context.support,
                &profile,
                true,
            )
            .unwrap();
        println!(
            "R23SUPPORT,{name},{:.2},{},{},{}",
            proposal.chosen.contacts,
            structure.points.len(),
            structure.columns.len(),
            analysis.islands.len()
        );
        assert!(
            analysis.islands.is_empty(),
            "{name}: {} islands",
            analysis.islands.len()
        );
        assert!(
            structure.points.len() <= ((proposal.chosen.contacts * 2.0).ceil() as usize).max(24),
            "{name}: {} generated vs {} estimated",
            structure.points.len(),
            proposal.chosen.contacts
        );
    }
}

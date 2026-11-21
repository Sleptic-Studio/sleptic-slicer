// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Opt-in audit using Sleptic's local model library; no files are downloaded.
use glam::DVec3;
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_scene::{Scene, node::NodeId};
use sc_scene_edit::SceneEdit;
#[path = "r18_legacy_orient/mod.rs"]
mod legacy;
#[allow(dead_code, unreachable_pub)]
#[path = "r18_legacy_orient/r18.rs"]
mod r18;
fn points(scene: &Scene, id: NodeId) -> Option<Vec<DVec3>> {
    Some(
        sc_scene::bake_leaf_to_world(scene, id)
            .ok()?
            .vertices
            .iter()
            .map(|v| DVec3::from_array(v.coords().map(f64::from)))
            .collect(),
    )
}
fn bounds(p: &[DVec3]) -> Option<(DVec3, DVec3)> {
    let v = *p.first()?;
    Some(p.iter().fold((v, v), |(a, b), p| (a.min(*p), b.max(*p))))
}
fn world_edit(scene: &Scene, id: NodeId, delta: Transform3D) -> Option<SceneEdit> {
    let before = scene.node(id)?.local_transform;
    let world = scene.world_transform_of(id).ok()?;
    let parent = world.compose(&before.inverse().ok()?);
    Some(SceneEdit::SetLocalTransform {
        node_id: id,
        before,
        after: parent.inverse().ok()?.compose(&delta).compose(&world),
    })
}
fn volume(mesh: &sc_mesh_core::IndexedMesh) -> f64 {
    mesh.faces
        .iter()
        .map(|f| {
            let [a, b, c] = f.vertices.map(|i| {
                DVec3::from_array(mesh.vertices[i.get() as usize].coords().map(f64::from))
            });
            a.dot(b.cross(c)) / 6.0
        })
        .sum::<f64>()
        .abs()
}
#[test]
#[ignore = "local R18 original/legacy/new orientation and real geometric-support audit"]
fn library_orientation_audit() {
    let dir = std::env::var("SLEPTIC_R18_MODELS").unwrap();
    let names=std::env::var("SLEPTIC_R18_FILES").unwrap_or_else(|_|"vice_grips_v25_full.stl|Lapras_Cuerpo (1).STL|Jaw.STL|fly no color.stl|butterfly done repaired.stl|camera mount.stl|3DBenchy.stl|Devil-FanShowdown.stl|bottom plate v3.stl|BELT TENSION NEW.stl|belt tension thing.stl|bottom plate v2 .stl".into());
    let envelope = sc_print_primitives::BuildVolume::new(192.0, 120.0, 200.0).unwrap();
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
    let context =
        sc_print_glue::orient::Context::for_profiles(&printer, &material, &Default::default())
            .unwrap();
    println!("R20CONTEXT seconds_per_mm={}", context.seconds_per_mm);
    println!(
        "R18CSV,model,triangles,scale,variant,fits,support_count,support_volume_mm3,layers,stair_mm2,footprint_mm2,seconds,support_area_mm2,minima,estimated_contacts,print_h_mm,copies,bed_contact_mm2,blocked,recessed"
    );
    let mut access_totals = std::collections::BTreeMap::<&str, (usize, usize)>::new();
    for name in names.split('|') {
        let path = std::path::Path::new(&dir).join(name);
        let mut scene = sc_project_formats::load_scene(&path, None, |_| {})
            .unwrap()
            .scene;
        scene.repair_meshes(sc_mesh_core::indexed_mesh::RepairPasses::default_repair());
        scene.drop_at([0.0, 0.0], 0.0).unwrap();
        let id = scene
            .reachable_leaves(
                sc_scene::CsgPolicy::Descend,
                sc_scene::ReferencePolicy::Skip,
            )
            .unwrap()[0];
        let (lo, hi) = bounds(&points(&scene, id).unwrap()).unwrap();
        let size = hi - lo;
        let scale = if std::env::var_os("SLEPTIC_R20_FULL_SIZE").is_some() {
            1.0
        } else {
            (190.0 / size.x)
                .min(118.0 / size.y)
                .min(198.0 / size.z)
                .min(1.0)
        };
        if scale < 1.0 {
            world_edit(
                &scene,
                id,
                Transform3D::Affine(AffineTransform3D::from_uniform_scale(scale)),
            )
            .unwrap()
            .apply_to_scene(&mut scene)
            .unwrap();
            scene.drop_at([0.0, 0.0], 0.0).unwrap();
        }
        let triangle_count = scene.mesh_at_leaf(id).unwrap().faces.len();
        let original_fit = sc_print_glue::orient::measure(&scene, id, envelope, 45.0)
            .unwrap()
            .fits;
        let mut r18_fit_minutes = None;
        for variant in ["original", "r18", "r20"] {
            if std::env::var_os("SLEPTIC_R20_PLACEMENT_ONLY").is_some() && variant != "r20" {
                continue;
            }
            let mut current = scene.clone();
            let start = std::time::Instant::now();
            if variant == "r18" || variant == "r20" {
                let result = if variant == "r18" {
                    r18::search(&current, id, envelope, 45.0, [0.45, 0.25, 0.20, 0.10])
                        .unwrap()
                        .delta
                } else {
                    let proposal = sc_print_glue::orient::search_with_context(
                        &current,
                        id,
                        envelope,
                        45.0,
                        sc_print_glue::orient::Preset::Balanced.weights(),
                        context,
                    )
                    .unwrap();
                    if let Some(minutes) = r18_fit_minutes {
                        assert!(
                            proposal.shortest_minutes <= minutes + 1.0,
                            "{name}: omitted a known shorter R18 fitting pose"
                        );
                    }
                    println!(
                        "R20WORK,{name},shortest_contacts={:.3},chosen_contacts={:.3},shortest_hands_minutes={:.3},chosen_hands_minutes={:.3},saved_hands_minutes={:.3}",
                        proposal.shortest.contacts,
                        proposal.chosen.contacts,
                        sc_print_glue::orient::hands_on_minutes(proposal.shortest),
                        sc_print_glue::orient::hands_on_minutes(proposal.chosen),
                        sc_print_glue::orient::hands_on_minutes(proposal.shortest)
                            - sc_print_glue::orient::hands_on_minutes(proposal.chosen)
                    );
                    if name == "Devil-FanShowdown.stl" {
                        assert!(
                            proposal.chosen.print_minutes < 240.0,
                            "Devil exceeded four hours"
                        );
                    }
                    println!(
                        "R20DECISION,{name},shortest_minutes={:.3},chosen_minutes={:.3},extra_minutes={:.3}",
                        proposal.shortest_minutes,
                        proposal.chosen.print_minutes,
                        proposal.chosen.print_minutes - proposal.shortest_minutes
                    );
                    proposal.delta
                };
                world_edit(&current, id, result)
                    .unwrap()
                    .apply_to_scene(&mut current)
                    .unwrap();
            }
            let seconds = start.elapsed().as_secs_f64();
            let mut m =
                sc_print_glue::orient::measure_with_context(&current, id, envelope, 45.0, context)
                    .unwrap();
            if variant == "r18" && m.fits {
                r18_fit_minutes = Some(m.print_minutes);
            }
            if variant == "r20" {
                assert!(!original_fit || m.fits, "{name} lost fit");
            }
            let mut mesh = sc_scene::bake_leaf_to_world(&current, id).unwrap();
            let min_z = mesh
                .vertices
                .iter()
                .map(|v| v[2])
                .fold(f32::INFINITY, f32::min);
            for v in &mut mesh.vertices {
                let [x, y, z] = v.coords();
                *v = sc_mesh_core::Vertex::new([x, y, z - min_z + 5.0]);
            }
            let settings = sc_print_profiles::support_settings::SupportSettings::default();
            let contacts = sc_print_glue::supports::sample_contacts(&mesh, &settings).unwrap();
            let supports = if std::env::var_os("SLEPTIC_R18_FULL_SUPPORTS").is_some() {
                use sc_sl1cer::profile::{
                    AaLevel, LayerSchedule, Mirror, PackingMode, PixelResolution, SliceProfile,
                };
                let profile = SliceProfile::new(
                    envelope,
                    PixelResolution::new(3840, 2400).unwrap(),
                    LayerSchedule::uniform(0.05, 0.05, ((m.height + 5.0) / 0.05).ceil() as u32)
                        .unwrap(),
                    Mirror::default(),
                    AaLevel::X4,
                    PackingMode::None,
                );
                let (_, structure, analysis) =
                    sc_print_glue::supports::auto_supported_scene_analyzed_with_fallback(
                        &current, &settings, &profile, true,
                    )
                    .unwrap();
                println!(
                    "R18VERIFY,{name},{variant},remaining_islands={}",
                    analysis.islands.len()
                );
                structure
            } else {
                sc_print_glue::supports::build_supports(&mesh, &contacts, &settings).unwrap()
            };
            let forecast_access = (m.blocked_contacts, m.recessed_contacts);
            let mut lifted = sc_scene::bake_leaf_to_world(&current, id).unwrap();
            for v in &mut lifted.vertices {
                let [x, y, z] = v.coords();
                *v = sc_mesh_core::Vertex::new([x, y, z + settings.lift_mm as f32]);
            }
            let actual_access =
                sc_print_glue::supports::analyze_contact_access(&lifted, &supports.points, 0.0501)
                    .unwrap();
            m.blocked_contacts = actual_access.0;
            m.recessed_contacts = actual_access.1;
            let access = access_totals.entry(variant).or_default();
            access.0 += actual_access.0;
            access.1 += actual_access.1;
            println!(
                "R20ACCESS,{name},{variant},candidate_blocked={},candidate_recessed={},generated_blocked={},generated_recessed={}",
                forecast_access.0, forecast_access.1, actual_access.0, actual_access.1
            );
            let estimated_copies = m.copies;
            let estimated_bed = m.bed_contact_area;
            // Audit the actual generated raft, including raster completion,
            // independently of the orientation search's geometric forecast.
            if let Some(raft) = &supports.raft {
                let floor = raft
                    .vertices
                    .iter()
                    .map(|v| f64::from(v[2]))
                    .fold(f64::INFINITY, f64::min);
                m.bed_contact_area = raft
                    .faces
                    .iter()
                    .filter_map(|f| {
                        let t = f.vertices.map(|i| {
                            DVec3::from_array(raft.vertices[usize::from(i)].coords().map(f64::from))
                        });
                        t.iter()
                            .all(|p| (p.z - floor).abs() < 1e-5)
                            .then(|| (t[1] - t[0]).cross(t[2] - t[0]).z.abs() * 0.5)
                    })
                    .sum();
            }
            let all_support_points = supports
                .columns
                .iter()
                .chain(supports.raft.iter())
                .flat_map(|mesh| {
                    mesh.vertices
                        .iter()
                        .map(|v| DVec3::from_array(v.coords().map(f64::from)))
                })
                .chain(
                    points(&current, id)
                        .unwrap()
                        .into_iter()
                        .map(|p| p + DVec3::Z * settings.lift_mm),
                );
            if let Some((lo, hi)) = bounds(&all_support_points.collect::<Vec<_>>()) {
                m.copies = sc_print_glue::orient::copies_per_plate(
                    hi.x - lo.x,
                    hi.y - lo.y,
                    envelope,
                    context.gap_mm,
                );
                let placed_fit = lo.x >= -envelope.x_mm() / 2.0 - 1e-4
                    && hi.x <= envelope.x_mm() / 2.0 + 1e-4
                    && lo.y >= -envelope.y_mm() / 2.0 - 1e-4
                    && hi.y <= envelope.y_mm() / 2.0 + 1e-4
                    && lo.z >= -1e-4
                    && hi.z <= envelope.z_mm() + 1e-4;
                println!("R20PLACEMENT,{name},{variant},fits={placed_fit},min={lo:?},max={hi:?}");
                if variant == "r20" {
                    assert!(
                        placed_fit,
                        "{name} placed supports outside plate: {lo:?} {hi:?}"
                    );
                }
            }
            println!(
                "R20FOOTPRINT,{name},{variant},estimated_copies={estimated_copies},generated_copies={},estimated_bed={estimated_bed:.3},generated_bed={:.3}",
                m.copies, m.bed_contact_area
            );
            let support_volume = supports.columns.iter().map(volume).sum::<f64>()
                + supports.raft.as_ref().map_or(0.0, volume);
            println!(
                "R18CSV,{name},{triangle_count},{scale:.6},{variant},{},{},{support_volume:.3},{},{:.3},{:.3},{seconds:.6},{:.3},{},{:.3},{},{},{:.3},{},{}",
                m.fits,
                supports.points.len(),
                ((m.height + settings.lift_mm) / 0.05).ceil() as u32,
                m.stair_area,
                m.footprint,
                m.support_area,
                m.minima,
                m.contacts,
                sc_print_job::format_duration(m.print_minutes * 60.0),
                m.copies,
                m.bed_contact_area,
                m.blocked_contacts,
                m.recessed_contacts
            );
        }
    }
    if std::env::var_os("SLEPTIC_R18_FILES").is_none()
        && std::env::var_os("SLEPTIC_R20_PLACEMENT_ONLY").is_none()
    {
        assert!(
            access_totals["r20"].0 < access_totals["r18"].0,
            "blocked contacts did not improve"
        );
        assert!(
            access_totals["r20"].1 < access_totals["r18"].1,
            "recessed contacts did not improve"
        );
    }
}

#[test]
fn calibration_geometry_schedule_and_archive_work_together() {
    use sc_scene::node::LeafPayload;
    use sc_sl1cer::profile::{
        AaLevel, LayerSchedule, Mirror, PackingMode, PixelResolution, SliceProfile,
    };
    use std::sync::Arc;
    let mut scene = Scene::new();
    for (i, source) in sc_print_glue::calibration::scenes(&[1.5, 2.5])
        .unwrap()
        .iter()
        .enumerate()
    {
        let id = source.roots()[0];
        let node = scene
            .add_leaf(
                source.node(id).unwrap().meta.clone(),
                Transform3D::Affine(AffineTransform3D::from_translation([
                    -35.0 + i as f64 * 38.0,
                    -12.0,
                    0.0,
                ])),
                LeafPayload::Mesh(Arc::clone(source.leaf_mesh_arc(id).unwrap())),
            )
            .unwrap();
        scene.add_root(node).unwrap();
    }
    let regions = sc_print_glue::calibration::regions(&scene).unwrap();
    assert_eq!(regions.len(), 2);
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
    let mut config = sc_print_profiles::resolve(&printer, &material, &Default::default()).unwrap();
    config.material_resolved.bottom_layer_count = 0;
    let profile = SliceProfile::new(
        printer.hardware.build_volume(),
        PixelResolution::new(192, 120).unwrap(),
        LayerSchedule::uniform(0.1, 0.1, 1).unwrap(),
        Mirror::default(),
        AaLevel::X4,
        PackingMode::None,
    );
    let sc_sl1cer::SliceOutcome::Completed(leaves) = sc_sl1cer::api::slice_scene_with_fallback(
        &scene,
        &profile,
        &sc_sl1cer::CancelToken::new(),
        |_| {},
        true,
    )
    .unwrap() else {
        panic!("cancelled");
    };
    let stack = sc_sl1cer::contour::merge_stacks(
        leaves
            .into_iter()
            .map(sc_sl1cer::LeafSlices::into_stack)
            .collect(),
    )
    .unwrap();
    let (schedule, exposures) = sc_print_glue::calibration::schedule(
        &config.material_resolved,
        profile.layer_schedule(),
        &regions,
    );
    assert_eq!(exposures, vec![vec![1.5], vec![2.5]]);
    assert_eq!(schedule.layers[0].exposure_s, 4.0);
    let estimate = sc_print_job::PrintEstimate::new(&schedule, 0.0, None, None);
    assert!((estimate.total_time_s - 12.4).abs() < 1e-8);
    let script = sc_print_job::render_multi_job(&config, &schedule, &estimate, &exposures).unwrap();
    let job = sc_sla_formats::PreparedJob {
        regions: regions.iter().map(|r| (r.min, r.max)).collect(),
        layer_z_mm: vec![0.1],
        exposure_s: vec![4.0],
        script,
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("calibration.zip");
    sc_sla_formats::write_gcode_zip(
        &stack,
        &path,
        &sc_sla_formats::WriterOptions::from_profile(&profile, "test"),
        &job,
    )
    .unwrap();
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    for name in ["1-tile1.png", "1-tile2.png", "run.gcode"] {
        assert!(archive.by_name(name).is_ok());
    }
    if let Some(path) = std::env::var_os("SLEPTIC_R18_CALIBRATION_PROJECT") {
        let mut file = std::fs::File::create(path).unwrap();
        sc_project_formats::write_project(
            &scene,
            &Default::default(),
            sc_project_formats::EmbedProfiles::No,
            None,
            None,
            sc_project_formats::ProjectFileType::ThreeMf,
            &mut file,
        )
        .unwrap();
    }
}

#[test]
#[ignore = "local R18 release timing comparison without support generation"]
fn orientation_search_timings() {
    let dir = std::env::var("SLEPTIC_R18_MODELS").unwrap();
    let volume = sc_print_primitives::BuildVolume::new(192.0, 120.0, 200.0).unwrap();
    for name in [
        "3rd body piston project .stl",
        "Skibidi (The Rock) Toilet.stl",
    ] {
        let mut scene =
            sc_project_formats::load_scene(&std::path::Path::new(&dir).join(name), None, |_| {})
                .unwrap()
                .scene;
        scene.repair_meshes(sc_mesh_core::indexed_mesh::RepairPasses::default_repair());
        scene.drop_at([0.0, 0.0], 0.0).unwrap();
        let id = scene.roots()[0];
        let (a, b) = bounds(&points(&scene, id).unwrap()).unwrap();
        let size = b - a;
        let scale = if std::env::var_os("SLEPTIC_R20_FULL_SIZE").is_some() {
            1.0
        } else {
            (190.0 / size.x)
                .min(118.0 / size.y)
                .min(198.0 / size.z)
                .min(1.0)
        };
        world_edit(
            &scene,
            id,
            Transform3D::Affine(AffineTransform3D::from_uniform_scale(scale)),
        )
        .unwrap()
        .apply_to_scene(&mut scene)
        .unwrap();
        let start = std::time::Instant::now();
        let _ = legacy::auto_orient(&scene, id, 45.0).unwrap();
        let old = start.elapsed().as_secs_f64();
        let mut new = Vec::new();
        for _ in 0..3 {
            let start = std::time::Instant::now();
            let result = sc_print_glue::orient::search(
                &scene,
                id,
                volume,
                45.0,
                sc_print_glue::orient::Preset::Balanced.weights(),
            )
            .unwrap();
            assert!(result.chosen.fits);
            new.push(start.elapsed().as_secs_f64());
        }
        new.sort_by(f64::total_cmp);
        println!(
            "R18TIME,{name},triangles={},old={old:.6},new_median={:.6},new_runs={new:?}",
            scene.mesh_at_leaf(id).unwrap().faces.len(),
            new[1]
        );
    }
}

#[test]
#[ignore = "local R18 oversized-model no-fit search timing"]
fn oversized_no_fit_search_timing() {
    let dir = std::env::var("SLEPTIC_R18_MODELS").unwrap();
    let mut scene = sc_project_formats::load_scene(
        &std::path::Path::new(&dir).join("Skibidi (The Rock) Toilet.stl"),
        None,
        |_| {},
    )
    .unwrap()
    .scene;
    scene.repair_meshes(sc_mesh_core::indexed_mesh::RepairPasses::default_repair());
    let id = scene.roots()[0];
    let start = std::time::Instant::now();
    let result = sc_print_glue::orient::search(
        &scene,
        id,
        sc_print_primitives::BuildVolume::new(10.0, 10.0, 10.0).unwrap(),
        45.0,
        sc_print_glue::orient::Preset::Balanced.weights(),
    )
    .unwrap();
    assert!(result.retained && !result.chosen.fits);
    println!(
        "R18NOFIT,triangles={},seconds={:.6}",
        scene.mesh_at_leaf(id).unwrap().faces.len(),
        start.elapsed().as_secs_f64()
    );
    assert!(
        start.elapsed().as_secs_f64() < 2.0,
        "oversized no-fit search exceeded the 2 s target"
    );
}

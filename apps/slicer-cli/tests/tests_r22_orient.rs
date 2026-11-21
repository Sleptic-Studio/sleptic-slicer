// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Local real-generator audit. Run explicitly with SLEPTIC_R18_MODELS.
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
#[test]
#[ignore = "local model library and completed support generation"]
fn r22_library_audit() {
    let dir = std::env::var("SLEPTIC_R18_MODELS").unwrap();
    let names = std::env::var("SLEPTIC_R22_FILES").unwrap_or_else(|_| "Jaw.STL|BELT TENSION NEW.stl|belt tension thing.stl|camera mount.stl|3DBenchy.stl|fly no color.stl|butterfly done repaired.stl|Lapras_Cuerpo (1).STL|Devil-FanShowdown.stl|bottom plate v2 .stl".into());
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
    for name in names.split('|') {
        let mut scene =
            sc_project_formats::load_scene(&std::path::Path::new(&dir).join(name), None, |_| {})
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
        let mesh = sc_scene::bake_leaf_to_world(&scene, id).unwrap();
        let size = mesh.local_aabb().unwrap();
        let scale = (190.0 / f64::from(size.max()[0] - size.min()[0]))
            .min(118.0 / f64::from(size.max()[1] - size.min()[1]))
            .min(195.0 / f64::from(size.max()[2] - size.min()[2]))
            .min(1.0);
        let scale = if std::env::var_os("SLEPTIC_R22_FULL_SIZE").is_some() {
            1.0
        } else {
            scale
        };
        if scale < 1.0 {
            let before = scene.node(id).unwrap().local_transform;
            scene
                .set_local_transform(
                    id,
                    Transform3D::Affine(AffineTransform3D::from_uniform_scale(scale))
                        .compose(&before),
                )
                .unwrap();
            scene.drop_at([0.0, 0.0], 0.0).unwrap();
        }
        for (label, weights) in [
            (
                "balanced",
                sc_print_glue::orient::Preset::Balanced.weights(),
            ),
            ("fill", sc_print_profiles::orient::FILL_PLATE_WEIGHTS),
        ] {
            let start = std::time::Instant::now();
            let proposal = sc_print_glue::orient::search_with_context(
                &scene, id, envelope, 45.0, weights, context,
            )
            .unwrap();
            let mut current = scene.clone();
            let before = current.node(id).unwrap().local_transform;
            current
                .set_local_transform(id, proposal.delta.compose(&before))
                .unwrap();
            let (geometric, _) =
                sc_print_glue::supports::geometric_supported_scene(&current, &context.support)
                    .unwrap();
            let profile = sc_print_glue::slice_profile_for_selection(
                &geometric,
                &printer,
                &material,
                None,
                None,
                sc_sl1cer::profile::AaLevel::X4,
                sc_sl1cer::profile::PackingMode::None,
            )
            .unwrap();
            let (generated, structure, analysis) =
                sc_print_glue::supports::auto_supported_scene_analyzed_with_fallback(
                    &current,
                    &context.support,
                    &profile,
                    true,
                )
                .unwrap();
            let mesh = generated.merge_meshes().unwrap().unwrap();
            let bb = mesh.local_aabb().unwrap();
            let future_copies = sc_print_glue::orient::copies_per_plate(
                f64::from(bb.max()[0] - bb.min()[0]),
                f64::from(bb.max()[1] - bb.min()[1]),
                envelope,
                context.gap_mm,
            );
            let bare = current.merge_meshes().unwrap().unwrap();
            let bb = bare.local_aabb().unwrap();
            // Independent shelf placement loop, the same dimensions Fill plate copies.
            let (w, d) = (
                f64::from(bb.max()[0] - bb.min()[0]),
                f64::from(bb.max()[1] - bb.min()[1]),
            );
            let mut actual_copies = 0;
            let mut y = 0.0;
            while y + d <= 120.0 + 1e-5 {
                let mut x = 0.0;
                while x + w <= 192.0 + 1e-5 {
                    actual_copies += 1;
                    x += w + context.gap_mm;
                }
                y += d + context.gap_mm;
            }
            let m = proposal.chosen;
            println!(
                "R22,{name},{label},scale={scale:.6},est={:.1},real={},copies={},forecast={},future_supported_copies={future_copies},minutes={:.2},per_part={:.3},zero={},available={},original_copies={},retained={},islands={},elapsed={:.3}",
                m.contacts,
                structure.points.len(),
                actual_copies,
                m.copies,
                m.print_minutes,
                m.print_minutes / actual_copies.max(1) as f64,
                m.zero_support,
                proposal.zero_support_available,
                proposal.original.copies,
                proposal.retained,
                analysis.islands.len(),
                start.elapsed().as_secs_f64()
            );
            if m.zero_support {
                assert!(structure.points.is_empty());
                assert!(analysis.islands.is_empty());
            }
        }
    }
}

#[test]
fn r22_mixed_plate_exports_direct_footprint_and_neighbours_raft_on_layer_one() {
    use sc_scene::{LeafPayload, Scene};
    use sc_sl1cer::profile::*;
    use std::sync::Arc;
    let mut source = Scene::new();
    for (x, z, name) in [
        (-15.0, 0.5, "Direct slab"),
        (15.0, 2.5, "Supported neighbour"),
    ] {
        let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
        for v in &mut mesh.vertices {
            *v = sc_mesh_core::Vertex::new([v[0] * 10.0 + x, v[1] * 10.0, v[2] + z]);
        }
        let id = source
            .add_leaf(
                Some(sc_scene::SceneMetadata {
                    name: Some(name.into()),
                    comment: None,
                }),
                Transform3D::Identity,
                LeafPayload::Mesh(Arc::new(mesh)),
            )
            .unwrap();
        source.add_root(id).unwrap();
    }
    let profile = SliceProfile::new(
        BuildVolume::new(60.0, 40.0, 20.0).unwrap(),
        PixelResolution::new(600, 400).unwrap(),
        LayerSchedule::uniform(0.1, 0.1, 90).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    let (scene, _, analysis) = sc_print_glue::supports::auto_supported_scene_analyzed(
        &source,
        &Default::default(),
        &profile,
    )
    .unwrap();
    assert!(analysis.islands.is_empty());
    let slices = sc_sl1cer::api::slice_scene(&scene, &profile).unwrap();
    let stack =
        sc_sl1cer::contour::merge_stacks(slices.into_iter().map(|l| l.into_stack()).collect())
            .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("mixed.zip");
    let job = sc_sla_formats::PreparedJob {
        regions: Vec::new(),
        layer_z_mm: stack.layers().iter().map(|l| l.z_mm()).collect(),
        exposure_s: vec![2.0; stack.layers().len()],
        script: "G28\n".into(),
    };
    sc_sla_formats::write_gcode_zip(
        &stack,
        &output,
        &sc_sla_formats::WriterOptions::from_profile(&profile, "r22-test"),
        &job,
    )
    .unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
    let mut first = zip.by_name("1.png").unwrap();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut first, &mut bytes).unwrap();
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; decoder.output_buffer_size().unwrap()];
    let info = decoder.next_frame(&mut pixels).unwrap();
    assert_eq!((info.width, info.height), (600, 400));
    assert_eq!(info.color_type, png::ColorType::Grayscale);
    let lit = |x: usize, y: usize| pixels[y * 600 + x] > 0;
    // Direct slab's exact 10 mm footprint occupies x=100..200, y=150..250.
    assert!(lit(150, 200));
    assert!(!lit(95, 200) && !lit(205, 200));
    // The supported neighbour has no model this low; these are its raft pixels.
    assert!(lit(450, 200));
    assert!(!lit(300, 200));
    if let Some(folder) = std::env::var_os("SLEPTIC_R22_CAPTURE_SOURCES") {
        let folder = std::path::PathBuf::from(folder);
        std::fs::create_dir_all(&folder).unwrap();
        for (name, graph) in [("mixed", &scene), ("mixed-source", &source)] {
            let mut file = std::fs::File::create(folder.join(format!("{name}.3mf"))).unwrap();
            sc_project_formats::write_project(
                graph,
                &Default::default(),
                sc_project_formats::EmbedProfiles::No,
                None,
                None,
                sc_project_formats::ProjectFileType::ThreeMf,
                &mut file,
            )
            .unwrap();
        }
        let mut slab = Scene::new();
        let id = slab
            .add_leaf(
                Some(sc_scene::SceneMetadata {
                    name: Some("Direct slab".into()),
                    comment: None,
                }),
                Transform3D::Affine(AffineTransform3D::from_translation([0.0, 0.0, 2.5])).compose(
                    &Transform3D::Affine(AffineTransform3D::from_non_uniform_scale([
                        40.0, 30.0, 5.0,
                    ])),
                ),
                LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
            )
            .unwrap();
        slab.add_root(id).unwrap();
        let mut file = std::fs::File::create(folder.join("slab.3mf")).unwrap();
        sc_project_formats::write_project(
            &slab,
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

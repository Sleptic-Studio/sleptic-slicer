// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;

fn options() -> Options {
    Options {
        recipe: None,
        orient: false,
        orient_weights: sc_print_glue::orient::Preset::Balanced.weights(),
        orient_convenience_rate: 1.0,
        arrange_gap_mm: 3.0,
        hollow: false,
        supports: false,
        wall_mm: 2.0,
        support: Default::default(),
        slicing: sc_print_glue::SlicingParams {
            process_overrides: Default::default(),
            layer_height_request: None,
            first_layer_height: None,
            aa_level: sc_print_primitives::AaLevel::Off,
        },
    }
}

#[test]
fn batch_recipe_scales_each_good_file_and_reports_bad_files() {
    let valid = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/batch-cube.obj");
    let temp = tempfile::tempdir().unwrap();
    let bad = temp.path().join("broken.stl");
    std::fs::write(&bad, b"invalid mesh").unwrap();
    let profiles = (
        sc_print_profiles::ProfileLibraries::platform_default()
            .load_printer_by_id(&"elegoo-mars-3-ctb".parse().unwrap())
            .unwrap(),
        sc_print_profiles::ProfileLibraries::platform_default()
            .effective_material_by_id(&"standard-405".parse().unwrap())
            .unwrap()
            .0,
    );
    let mut options = options();
    options.recipe = Some(sc_print_profiles::recipes::Recipe {
        version: 1,
        name: "Batch scale".into(),
        steps: vec![sc_print_profiles::recipes::Step::Scale { percent: 200.0 }],
    });
    let source = sc_project_formats::load_scene(&valid, None, |_| {})
        .unwrap()
        .scene;
    let before = source.aabb().unwrap().unwrap();
    let prepared = prepare_paths(
        Scene::new(),
        &[valid.clone(), bad, valid],
        &options,
        BuildVolume::new(192., 120., 200.),
        &CancelToken::new(),
        Some(&profiles),
        |_, _| {},
    )
    .unwrap();
    assert_eq!(prepared.imported, 2);
    assert_eq!(prepared.failures.len(), 1);
    let mut scene = Scene::new();
    while scene.nodes_len() < prepared.node_slots {
        scene.allocate_unbound_node_id();
    }
    prepared.edit.apply_to_scene(&mut scene).unwrap();
    for id in scene
        .reachable_leaves(CsgPolicy::Refuse, ReferencePolicy::Skip)
        .unwrap()
    {
        let mesh = sc_scene::bake_leaf_to_world(&scene, id).unwrap();
        let after = mesh.local_aabb().unwrap();
        assert!(
            (f64::from(after.max()[2] - after.min()[2])
                - 2. * f64::from(before.max()[2] - before.min()[2]))
            .abs()
                < 1e-5
        );
    }
}

#[test]
fn refused_repairs_are_reported_as_file_failures() {
    let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
    mesh.vertices[0] = sc_mesh_core::Vertex::new([f32::NAN, 0.0, 0.0]);
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            sc_mesh_core::transform::Transform3D::Identity,
            sc_scene::LeafPayload::Mesh(Arc::new(mesh)),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    assert!(
        repair_source(&mut scene)
            .unwrap_err()
            .starts_with("Repair failed:")
    );
}

#[test]
fn batch_reports_progress_and_bad_files_without_losing_successful_imports() {
    let valid = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/batch-cube.obj");
    let temporary = tempfile::tempdir().unwrap();
    let bad = temporary.path().join("broken.stl");
    std::fs::write(&bad, b"invalid mesh").unwrap();
    let mut progress = Vec::new();
    let prepared = prepare_paths(
        Scene::new(),
        &[valid, bad],
        &options(),
        BuildVolume::new(192.0, 120.0, 200.0),
        &CancelToken::new(),
        None,
        |done, total| progress.push((done, total)),
    )
    .unwrap();
    assert_eq!(prepared.imported, 1);
    assert_eq!(prepared.failures.len(), 1);
    assert!(prepared.failures[0].contains("broken.stl"));
    assert_eq!(progress, vec![(1, 2), (2, 2)]);
    let document = SceneDocument::new();
    document.choose_printer(Some("sleptic-diy-msla".parse().unwrap()));
    document.choose_resin(Some("standard-405".parse().unwrap()));
    document
        .apply_batch(prepared.edit, prepared.node_slots)
        .unwrap();
    assert_eq!(document.read_scene(|s| s.graph().roots().len()), Some(1));
    assert_eq!(
        document.chosen_printer().unwrap().as_str(),
        "sleptic-diy-msla"
    );
    document.step_back().unwrap();
    assert_eq!(document.read_scene(|s| s.graph().roots().len()), Some(0));
}

#[test]
fn cancelled_batch_does_not_commit_a_prepared_prefix() {
    let valid = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/batch-cube.obj");
    let token = CancelToken::new();
    let result = prepare_paths(
        Scene::new(),
        &[valid.clone(), valid],
        &options(),
        BuildVolume::new(192.0, 120.0, 200.0),
        &token,
        None,
        |_, _| token.cancel(),
    );
    assert!(result.is_err());
}

#[test]
fn oversized_file_is_reported_without_arranging_existing_parts() {
    let valid = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/batch-cube.obj");
    let prepared = prepare_paths(
        Scene::new(),
        &[valid],
        &options(),
        BuildVolume::new(2.0, 2.0, 2.0),
        &CancelToken::new(),
        None,
        |_, _| {},
    )
    .unwrap();
    assert_eq!(prepared.imported, 0);
    assert_eq!(prepared.failures.len(), 1);
    assert!(prepared.edit.is_noop());
}

#[test]
fn support_completion_uses_current_layer_height_first_layer_and_aa() {
    let printer = sc_print_profiles::load_printer_profile_from_path(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/profiles/printers/sleptic-diy-msla.toml"),
    )
    .unwrap();
    let material = sc_print_profiles::ProfileLibraries::platform_default()
        .effective_material_by_id(&"standard-405".parse().unwrap())
        .unwrap()
        .0;
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            sc_mesh_core::transform::Transform3D::Identity,
            sc_scene::LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let mut options = options();
    options.slicing.process_overrides.layer_height_mm =
        Some(sc_print_primitives::PositiveFiniteMm::new(0.1).unwrap());
    options.slicing.first_layer_height =
        Some(sc_print_primitives::PositiveFiniteMm::new(0.2).unwrap());
    options.slicing.aa_level = sc_print_primitives::AaLevel::X2;
    let profile = support_profile(&scene, &options, &printer, &material).unwrap();
    assert_eq!(profile.aa_level(), sc_print_primitives::AaLevel::X2);
    let tops: Vec<_> = profile.layer_schedule().iter_slab_tops().collect();
    assert!((tops[0] - 0.2).abs() < 1e-12);
    assert!((tops[1] - tops[0] - 0.1).abs() < 1e-12);
}

#[test]
fn batch_orientation_uses_the_selected_objective_weights() {
    use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
    use sc_scene::node::LeafPayload;
    let mut source = Scene::new();
    let id = source
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_non_uniform_scale([40.0, 20.0, 2.0])),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    source.add_root(id).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("plate.3mf");
    let mut file = std::fs::File::create(&path).unwrap();
    sc_project_formats::write_project(
        &source,
        &Default::default(),
        sc_project_formats::EmbedProfiles::No,
        None,
        None,
        sc_project_formats::ProjectFileType::ThreeMf,
        &mut file,
    )
    .unwrap();
    drop(file);
    let libraries = sc_print_profiles::ProfileLibraries::platform_default();
    let profiles = (
        libraries
            .load_printer_by_id(&"sleptic-diy-msla".parse().unwrap())
            .unwrap(),
        libraries
            .effective_material_by_id(&"standard-405".parse().unwrap())
            .unwrap()
            .0,
    );
    let run = |preset: sc_print_glue::orient::Preset| {
        let mut options = options();
        options.orient = true;
        options.orient_weights = preset.weights();
        let scene = prepare_file(&path, &options, &CancelToken::new(), Some(&profiles)).unwrap();
        let bounds = scene.aabb().unwrap().unwrap();
        let min = bounds.min();
        let max = bounds.max();
        (max[2] - min[2], (max[0] - min[0]) * (max[1] - min[1]))
    };
    let fast = run(sc_print_glue::orient::Preset::Fastest);
    let small = run(sc_print_glue::orient::Preset::SmallestFootprint);
    assert!((fast.0 - 2.0).abs() < 1e-4);
    assert!((small.0 - fast.0).abs() < 1e-4);
    assert!((small.1 - fast.1).abs() < 1e-4);
}

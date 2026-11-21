// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{fingerprint::*, prepare::*, projection::*, slice_plate::*};
use sc_mesh_core::{
    IndexedMesh, Vertex,
    transform::{AffineTransform3D, Transform3D},
};
use sc_print_profiles::*;
use sc_scene::{LeafPayload, NodeId, Scene, SceneMetadata};
use sc_sl1cer::{
    CancelToken,
    profile::{LayerSchedule, PackingMode},
};
use std::{
    num::NonZeroU32,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

fn mm(v: f64) -> PositiveFiniteMm {
    PositiveFiniteMm::new(v).unwrap()
}
fn selection() -> (PrinterProfile, EffectiveMaterial) {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/profiles");
    (
        load_printer_profile_from_library(&assets, &generic_printer_id()).unwrap(),
        load_material_profile_from_library(&assets, &MaterialId::new("abs-like-405").unwrap())
            .unwrap()
            .into(),
    )
}
fn scene_from(mesh: IndexedMesh) -> (Scene, NodeId) {
    let mut scene = Scene::new();
    let node = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_translation([0., 0., 0.5])),
            LeafPayload::Mesh(Arc::new(mesh)),
        )
        .unwrap();
    scene.add_root(node).unwrap();
    (scene, node)
}
fn cube() -> Scene {
    scene_from(sc_mesh_fixtures::unit_cube_mesh()).0
}
fn params() -> SlicingParams {
    SlicingParams {
        process_overrides: SceneOverrides::default(),
        layer_height_request: None,
        first_layer_height: None,
        aa_level: AaLevel::Off,
    }
}

#[test]
fn count_boundaries_match_the_observed_contract() {
    for (z, h, first, count) in [
        (0., 0.1, 0.1, 1),
        (-1., 0.1, 0.1, 1),
        (1., 0.1, 0.1, 10),
        (0.3, 0.1, 0.1, 3),
        (0.30000000000000004, 0.1, 0.1, 4),
        (0., 0., 0.1, 1),
        (1., -0.1, 0.1, 1),
        (1., f64::INFINITY, 0.1, 1),
        (4294967295., 1., 1., u32::MAX),
    ] {
        assert_eq!(total_layers_for_height(z, h, first).unwrap().get(), count);
    }
    for z in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            total_layers_for_height(z, 0.1, 0.1),
            Err(UnschedulableHeight::NonFiniteHeight { .. })
        ));
    }
    for (z, h, first) in [(1., 0., 0.1), (1., 0.1, f64::NAN), (4294967296., 1., 1.)] {
        assert!(matches!(
            total_layers_for_height(z, h, first),
            Err(UnschedulableHeight::LayerCountOverflow { .. })
        ));
    }
}

#[test]
fn hardware_projection_keeps_machine_metadata_and_grid() {
    let (mut printer, material) = selection();
    let calibration = MachineCalibration::new(LcdRotation::Deg90, [1.01, 0.99]).unwrap();
    printer.hardware = printer.hardware.with_calibration(calibration);
    let raw = hardware_profile(&printer.hardware, LayerPlan { layer_height: mm(0.05), first_layer_height: mm(0.2), layers: NonZeroU32::new(7).unwrap() }, AaLevel::X4, PackingMode::None);
    assert_eq!(*raw.build_volume(), printer.hardware.build_volume());
    assert_eq!(raw.resolution(), printer.hardware.pixel_resolution());
    assert_eq!(raw.mirror(), printer.hardware.mirror());
    assert_eq!(raw.calibration(), calibration);
    assert_eq!(raw.aa(), AaLevel::X4);
    assert_eq!(raw.accuracy().bottom_layers, 0);
    let mut resolved = resolve(&printer, &material, &SceneOverrides::default()).unwrap();
    resolved.material_resolved.bottom_layer_count = 9;
    resolved.material_resolved.transition_layers = 3;
    resolved.material_resolved.aa_levels = 16;
    resolved.material_resolved.bottom_compensation_mm = 0.2;
    resolved.material_resolved.xy_compensation_mm = -0.1;
    let profile = print_profile(&resolved, mm(0.2), NonZeroU32::new(7).unwrap(), AaLevel::X2, PackingMode::None);
    assert_eq!(profile.accuracy().bottom_layers, 9);
    assert_eq!(profile.accuracy().transition_layers, 3);
    assert_eq!(profile.accuracy().aa_levels, 16);
    assert_eq!(profile.accuracy().bottom_compensation_mm, 0.2);
    assert_eq!(profile.accuracy().xy_compensation_mm, -0.1);
    assert_eq!(profile.schedule().total_layers(), 7);
}

#[test]
fn geometry_without_exposure_retains_accuracy_precedence_and_angle() {
    let (printer, mut material) = selection();
    material.per_printer.clear();
    material.defaults = MaterialDefaults::default();
    material.defaults.support_overhang_angle_deg = Some(65.0);
    let original = material.clone();
    let profile = slice_profile_for_selection(
        &cube(),
        &printer,
        &material,
        None,
        None,
        AaLevel::Off,
        PackingMode::None,
    )
    .unwrap();
    assert_eq!(profile.schedule().total_layers(), 20);
    assert_eq!(profile.accuracy().bottom_layers, 0);
    assert_eq!(profile.support_overhang_angle(), 65.0);
    let o = SceneOverrides {
        bottom_layer_count: Some(7),
        transition_layers: Some(4),
        bottom_compensation_mm: Some(0.2),
        xy_compensation_mm: Some(-0.1),
        aa_levels: Some(8),
        ..Default::default()
    };
    let adjusted = with_accuracy_for_selection(profile, &printer, &material, &o);
    assert_eq!(
        (
            adjusted.accuracy().bottom_layers,
            adjusted.accuracy().transition_layers
        ),
        (7, 4)
    );
    assert_eq!(
        (
            adjusted.accuracy().bottom_compensation_mm,
            adjusted.accuracy().xy_compensation_mm,
            adjusted.accuracy().aa_levels
        ),
        (0.2, -0.1, 8)
    );
    assert_eq!(material, original);
}

#[test]
fn calibration_precedes_defaults_and_job_precedes_calibration() {
    let (printer, mut material) = selection();
    material.defaults.bottom_layer_count = Some(2);
    material.defaults.transition_layers = Some(1);
    let calibrated = material
        .per_printer
        .get_mut(&CalibrationTarget::Generic)
        .unwrap();
    calibrated.bottom_layer_count = Some(4);
    calibrated.transition_layers = Some(3);
    let profile = slice_profile_for_selection(
        &cube(),
        &printer,
        &material,
        None,
        None,
        AaLevel::Off,
        PackingMode::None,
    )
    .unwrap();
    assert_eq!(
        (
            profile.accuracy().bottom_layers,
            profile.accuracy().transition_layers
        ),
        (4, 3)
    );
    let profile = with_accuracy_for_selection(
        profile,
        &printer,
        &material,
        &SceneOverrides {
            bottom_layer_count: Some(8),
            transition_layers: Some(6),
            ..Default::default()
        },
    );
    assert_eq!(
        (
            profile.accuracy().bottom_layers,
            profile.accuracy().transition_layers
        ),
        (8, 6)
    );
}

#[test]
fn first_height_snaps_to_step_without_normal_height_range_clamping() {
    let (mut printer, material) = selection();
    printer.constraints.z_step_mm = Some(mm(0.01));
    printer.constraints.layer_height_range_mm = Some(LengthRange::new(mm(0.04), mm(0.1)).unwrap());
    for (requested, first, count) in [(0.004, 0.01, 21), (0.2, 0.2, 17)] {
        let profile = slice_profile_for_selection(
            &cube(),
            &printer,
            &material,
            None,
            Some(mm(requested)),
            AaLevel::Off,
            PackingMode::None,
        )
        .unwrap();
        let LayerSchedule::Uniform(grid) = profile.schedule();
        assert_eq!(grid.first_layer_height().value(), first);
        assert_eq!(grid.total_layers().get(), count);
    }
    let mut missing_height = material.clone();
    missing_height.intent.target_layer_height_mm = None;
    assert!(matches!(
        slice_profile_for_selection(
            &Scene::new(),
            &printer,
            &missing_height,
            None,
            None,
            AaLevel::Off,
            PackingMode::None
        ),
        Err(UnsliceableSelection::LayerHeight(_))
    ));
}

#[test]
fn dedicated_height_and_preparation_before_cancel_are_preserved() {
    let (printer, material) = selection();
    let mut p = params();
    p.process_overrides.layer_height_mm = Some(mm(0.1));
    assert_eq!(
        PlateJob::new(&cube(), &printer, &material, &p).schedule()
            .unwrap()
            .total_layers(),
        20
    );
    p.layer_height_request = Some(mm(0.1));
    assert_eq!(
        PlateJob::new(&cube(), &printer, &material, &p).schedule()
            .unwrap()
            .total_layers(),
        10
    );
    let cancel = CancelToken::new();
    cancel.cancel();
    assert!(matches!(
        PlateJob::new(&Scene::new(), &printer, &material, &p).slice(&cancel, |_| {}, false),
        Err(SlicePlateError::Prepare(UnsliceableSelection::Scene(
            UnsliceableScene::NoGeometry
        )))
    ));
    assert!(matches!(
        PlateJob::new(&cube(), &printer, &material, &p).slice(&cancel, |_| {}, false),
        Ok(SliceRun::Cancelled)
    ));
}

#[test]
fn actual_contours_progress_and_nonunion_merge_are_retained() {
    let (printer, material) = selection();
    let mut scene = cube();
    let node = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_translation([0., 0., 0.5])),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(node).unwrap();
    let calls = AtomicUsize::new(0);
    let complete = AtomicBool::new(false);
    let SliceRun::Completed(plate) = PlateJob::new(&scene, &printer, &material, &params()).slice(&CancelToken::new(), |p| {
            assert!(p.completed <= p.total);
            calls.fetch_add(1, Ordering::Relaxed);
            if p.fraction() == 1.0 {
                complete.store(true, Ordering::Relaxed);
            }
        }, false)
    .unwrap() else {
        panic!("completed")
    };
    assert_eq!(plate.leaves().len(), 2);
    assert_eq!(plate.schedule().total_layers(), 20);
    assert!(calls.load(Ordering::Relaxed) > 0 && complete.load(Ordering::Relaxed));
    let merged = plate.merge().unwrap();
    assert_eq!(merged.len(), 20);
    assert!(
        merged
            .layers()
            .iter()
            .all(|layer| layer.polygons().len() == 2)
    );
}

#[test]
fn merge_preserves_leaf_order_and_reports_mismatched_grids() {
    let (printer, material) = selection();
    let slice = |scene: &Scene, p: &SlicingParams| {
        let SliceRun::Completed(plate) =
            PlateJob::new(scene, &printer, &material, p).slice(&CancelToken::new(), |_| {}, false).unwrap()
        else {
            panic!("completed")
        };
        plate
    };
    let first = slice(&cube(), &params());
    let mut shifted = Scene::new();
    let id = shifted
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_translation([3., 0., 0.5])),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    shifted.add_root(id).unwrap();
    let second = slice(&shifted, &params());
    let expected_first = first.leaves[0].stack().layers()[0].polygons().to_vec();
    let expected_second = second.leaves[0].stack().layers()[0].polygons().to_vec();
    assert_ne!(expected_first, expected_second);
    let merged = SlicedPlate {
        leaves: vec![first.leaves[0].clone(), second.leaves[0].clone()],
        profile: first.profile.clone(),
    }
    .merge()
    .unwrap();
    assert_eq!(
        merged.layers()[0].polygons(),
        [expected_first, expected_second].concat()
    );
    let mut coarse = params();
    coarse.layer_height_request = Some(mm(0.1));
    let other = slice(&cube(), &coarse);
    let error = SlicedPlate {
        leaves: vec![first.leaves[0].clone(), other.leaves[0].clone()],
        profile: first.profile.clone(),
    }
    .merge()
    .unwrap_err();
    assert!(matches!(
        error,
        sc_sl1cer::contour::LayerAlignmentError::LayerCountMismatch { stack_idx: 1, .. }
    ));
    assert!(
        SlicedPlate {
            leaves: vec![],
            profile: first.profile
        }
        .merge()
        .unwrap()
        .is_empty()
    );
}

#[test]
fn tolerant_flag_is_passed_to_the_engine() {
    let (printer, material) = selection();
    let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
    mesh.faces.pop();
    let scene = scene_from(mesh).0;
    assert!(matches!(
        PlateJob::new(&scene, &printer, &material, &params()).slice(&CancelToken::new(), |_| {}, false),
        Err(SlicePlateError::Engine(_))
    ));
    let SliceRun::Completed(plate) = PlateJob::new(&scene, &printer, &material, &params()).slice(&CancelToken::new(), |_| {}, true)
    .unwrap() else {
        panic!("recovery")
    };
    assert!(plate.leaves()[0].tolerant());
}

#[test]
fn fingerprints_track_geometry_placement_and_schedule_not_names() {
    let (mut scene, node) = scene_from(sc_mesh_fixtures::unit_cube_mesh());
    let grid = LayerSchedule::uniform(0.05, 0.05, 20).unwrap();
    let original = slice_input_fingerprint(&scene, &grid).unwrap();
    assert_eq!(original.as_bytes().len(), 48);
    assert_eq!(original.to_token().len(), 96);
    assert!(
        original
            .to_token()
            .bytes()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
    );
    scene.set_meta(Some(SceneMetadata {
        name: Some("Renamed scene".into()),
        comment: None,
    }));
    scene
        .set_node_meta(
            node,
            Some(SceneMetadata {
                name: Some("Renamed part".into()),
                comment: None,
            }),
        )
        .unwrap();
    assert_eq!(slice_input_fingerprint(&scene, &grid).unwrap(), original);
    let mut different_normals = sc_mesh_fixtures::unit_cube_mesh();
    for face in &mut different_normals.faces {
        face.normal = sc_mesh_core::Normal::new([0., 0., 1.]);
    }
    assert_eq!(
        slice_input_fingerprint(&scene_from(different_normals).0, &grid).unwrap(),
        original
    );
    assert_ne!(
        slice_input_fingerprint(&scene, &LayerSchedule::uniform(0.1, 0.05, 20).unwrap()).unwrap(),
        original
    );
    scene
        .set_local_transform(
            node,
            Transform3D::Affine(AffineTransform3D::from_translation([1., 0., 0.5])),
        )
        .unwrap();
    assert_ne!(slice_input_fingerprint(&scene, &grid).unwrap(), original);
    let mut changed = sc_mesh_fixtures::unit_cube_mesh();
    changed.vertices[0] = Vertex::new([-0.6, -0.5, -0.5]);
    assert_ne!(
        slice_input_fingerprint(&scene_from(changed).0, &grid).unwrap(),
        original
    );
}

#[test]
fn cache_version_change_keeps_sources_and_rejects_legacy_or_unverifiable_tokens() {
    let (scene, node) = scene_from(sc_mesh_fixtures::unit_cube_mesh());
    let grid = LayerSchedule::uniform(0.05, 0.05, 20).unwrap();
    let before_node = scene.node(node).unwrap().clone();
    let before_mesh = scene.mesh_at_leaf(node).unwrap().clone();
    let fresh = slice_input_fingerprint(&scene, &grid).unwrap().to_token();
    let legacy = "a1183b9ec54676728e89f4554866c27a56f2cd9527a323df76bb25c383d515327a80de345aaef2b1e89d632f1aa2cdba";
    assert_eq!(
        validate_persisted_slices(&fresh, &scene, &grid),
        PersistedSliceValidity::InSync
    );
    assert_eq!(
        validate_persisted_slices(legacy, &scene, &grid),
        PersistedSliceValidity::Stale
    );
    assert_eq!(
        validate_persisted_slices("invalid", &scene, &grid),
        PersistedSliceValidity::Stale
    );
    assert_eq!(
        validate_persisted_slices(&fresh, &Scene::new(), &grid),
        PersistedSliceValidity::Stale
    );
    assert_eq!(*scene.node(node).unwrap(), before_node);
    assert_eq!(*scene.mesh_at_leaf(node).unwrap(), before_mesh);
}

#[test]
fn schedule_rejects_nonfinite_and_overflow_at_exact_boundaries() {
    for z in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(total_layers_for_height(z, 0., 0.), Err(UnschedulableHeight::NonFiniteHeight { .. })));
    }
    assert_eq!(total_layers_for_height(2., 1., 1.).unwrap().get(), 2);
    assert_eq!(total_layers_for_height(4294967295., 1., 1.).unwrap().get(), u32::MAX);
    assert_eq!(total_layers_for_height(1., 0., 1.).unwrap().get(), 1);
    for (z, h, first) in [(2., 0., 1.), (2., f64::NAN, 1.), (4294967296., 1., 1.)] {
        assert!(matches!(total_layers_for_height(z, h, first), Err(UnschedulableHeight::LayerCountOverflow { .. })));
    }
}

#[test]
fn unschedulable_height_messages_are_stable() {
    let infinite = UnschedulableHeight::NonFiniteHeight { z_max_mm: f64::INFINITY };
    assert_eq!(infinite.to_string(), "Scene top must be finite, got inf mm");
    let tall = UnschedulableHeight::LayerCountOverflow { z_max_mm: 1e12 };
    assert_eq!(tall.to_string(), "Scene top 1000000000000 mm cannot fit the layer-count range");
}

#[test]
fn unsliceable_messages_are_stable() {
    use std::error::Error as _;
    let graph = sc_scene::SceneError::InvalidPlates;
    let bounds = UnsliceableScene::Aabb(graph.clone());
    assert_eq!(bounds.to_string(), format!("Cannot measure scene bounds: {graph}"));
    assert!(bounds.source().is_some());
    assert_eq!(UnsliceableScene::NoGeometry.to_string(), "The scene has no geometry to slice");
    assert!(UnsliceableScene::NoGeometry.source().is_none());
    assert_eq!(
        UnsliceableScene::TopAtOrBelowPlate { z_max_mm: -0.5 }.to_string(),
        "Scene top -0.5 mm is at or below the build plate"
    );
    let grid = UnschedulableHeight::NonFiniteHeight { z_max_mm: f64::NAN };
    let unschedulable = UnsliceableScene::Unschedulable(grid.clone());
    assert_eq!(unschedulable.to_string(), grid.to_string());
    assert!(unschedulable.source().is_some());
    let refused = ResolveError::InvalidMotion("lift");
    let height = UnsliceableSelection::LayerHeight(refused.clone());
    assert_eq!(height.to_string(), format!("Cannot select layer height: {refused}"));
    assert!(height.source().is_some());
    let scene = UnsliceableSelection::Scene(UnsliceableScene::NoGeometry);
    assert_eq!(scene.to_string(), "The scene has no geometry to slice");
    assert!(scene.source().is_some());
}

#[test]
fn plate_errors_say_and_name_their_cause() {
    use std::error::Error as _;
    let prepare = SlicePlateError::Prepare(UnsliceableSelection::Scene(UnsliceableScene::NoGeometry));
    assert_eq!(prepare.to_string(), "The scene has no geometry to slice");
    assert_eq!(prepare.source().unwrap().to_string(), prepare.to_string());
    let engine = SlicePlateError::Engine(sc_sl1cer::error::SliceSceneError::EmptyScene);
    assert_eq!(engine.to_string(), "scene has no mesh leaves to slice");
    assert!(engine.source().is_some());
}

#[test]
fn fingerprint_bytes_are_pinned() {
    // Saved projects store these tokens: a change of encoding must be a new
    // version string, never a silent change of the bytes.
    let (mut scene, node) = scene_from(sc_mesh_fixtures::unit_cube_mesh());
    let grid = LayerSchedule::uniform(0.05, 0.05, 20).unwrap();
    let placed = slice_input_fingerprint(&scene, &grid).unwrap().to_token();
    scene
        .set_local_transform(node, Transform3D::Affine(AffineTransform3D::from_translation([1., 0., 0.5])))
        .unwrap();
    let moved = slice_input_fingerprint(&scene, &grid).unwrap().to_token();
    assert_eq!((placed.as_str(), moved.as_str()), ("67366ffe34b60b72ce1867eadca3d494c1dd7c9734f97fbe2c72bade4a771b79964b9866ed9ff128eea67b79304e72e9", "6a9251068f27717d012245dce1641bfd40102d8eff0bd2bd0119e3d85249afc6adb951aa473e61590009ae6c72ec1608"));
}

#[test]
fn fingerprint_error_messages_are_stable() {
    use std::error::Error as _;
    let graph = sc_scene::SceneError::InvalidPlates;
    let scene = SliceInputFingerprintError::Scene(graph.clone());
    assert_eq!(scene.to_string(), format!("Cannot resolve fingerprint inputs: {graph}"));
    assert!(scene.source().is_some());
    let empty = sc_sl1cer::error::SliceSceneError::EmptyScene;
    let listed = SliceInputFingerprintError::Enumeration(empty);
    assert_eq!(listed.to_string(), "Cannot enumerate fingerprint inputs: scene has no mesh leaves to slice");
    assert!(listed.source().is_some());
}

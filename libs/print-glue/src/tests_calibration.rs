// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
#[test]
fn sequences_validate_before_geometry() {
    assert_eq!(exposures(3, 1.0, 0.5, "").unwrap(), vec![1.0, 1.5, 2.0]);
    assert_eq!(exposures(2, 0.0, 0.0, "1.5, 2.5").unwrap(), vec![1.5, 2.5]);
    for list in ["1", "1,1", "1,NaN", "0,2", "1,2,3,4,5,6,7,8,9"] {
        assert!(exposures(2, 1.0, 1.0, list).is_err());
    }
}
#[test]
fn generated_tile_is_closed_and_contains_requested_features() {
    let mesh = tile(2.5).unwrap();
    mesh.validate_vertices().unwrap();
    mesh.validated_faces()
        .unwrap()
        .validate_edge_manifold()
        .unwrap();
    assert!(mesh.faces.len() > 500);
    assert!(mesh.vertices.iter().any(|v| (v[2] - 4.4).abs() < 1e-5));
    assert!(
        mesh.vertices
            .iter()
            .any(|v| (v[0] - 3.15).abs() < 1e-4 && (v[1] - 11.0).abs() < 1e-4)
    );
}
#[test]
fn tags_extract_exposure_and_reject_mixed_or_overlapping_jobs() {
    let mut scenes = scenes(&[1.5, 2.0]).unwrap();
    let first = scenes.remove(0);
    let regions = regions(&first).unwrap();
    assert_eq!(regions[0].exposure_s, 1.5);
    let mut mixed = first.clone();
    let id = mixed
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(box_mesh([0.0; 3], [1.0; 3]))),
        )
        .unwrap();
    mixed.add_root(id).unwrap();
    assert!(super::regions(&mixed).is_err());
}

#[test]
fn exposure_limits_precede_allocation_and_match_controller_milliseconds() {
    assert!(matches!(
        exposures(usize::MAX, 1.0, 0.1, ""),
        Err(CalibrationError::ExposureSet)
    ));
    for list in [
        "0.0001, 0.0002",
        "1.0001, 1.0002",
        "1.00000000000001, 1.00000000000002",
    ] {
        assert!(matches!(
            exposures(2, 1.0, 0.1, list),
            Err(CalibrationError::ExposureSet)
        ));
    }
    assert_eq!(exposures(2, 1.001, 0.001, "").unwrap(), vec![1.001, 1.002]);
    assert_eq!(exposure_text(1.001), "1.001");
    assert_eq!(exposure_text(1.5), "1.50");
    assert!(matches!(scenes(&[1.0]), Err(CalibrationError::ExposureSet)));
    assert!(matches!(tile(0.0001), Err(CalibrationError::Exposure)));
}
#[test]
fn overlapping_tile_rectangles_have_a_typed_error() {
    let mut scene = scenes(&[1.5, 2.0]).unwrap().remove(0);
    let source = scene.roots()[0];
    let id = scene
        .add_leaf(
            scene.node(source).unwrap().meta.clone(),
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::clone(scene.leaf_mesh_arc(source).unwrap())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    assert!(matches!(regions(&scene), Err(CalibrationError::Overlap)));
}

#[test]
fn machine_patterns_are_closed_and_orientation_has_one_layer() {
    use sc_sl1cer::{
        CancelToken, SliceOutcome,
        profile::{LayerSchedule, PackingMode, SliceProfile},
    };
    for tool in [
        MachineTool::Orientation,
        MachineTool::XyRuler,
        MachineTool::ZTower,
    ] {
        let scene = machine_scene(tool, [192.0, 120.0, 200.0], 0.05).unwrap();
        let id = scene.roots()[0];
        let mesh = sc_scene::bake_leaf_to_world(&scene, id).unwrap();
        mesh.validate_vertices().unwrap();
        mesh.validated_faces()
            .unwrap()
            .validate_edge_manifold()
            .unwrap();
        assert!(regions(&scene).unwrap().is_empty());
        if tool == MachineTool::Orientation {
            let profile = SliceProfile::new(
                sc_print_primitives::BuildVolume::new(192.0, 120.0, 200.0).unwrap(),
                sc_print_primitives::PixelResolution::new(1920, 1200).unwrap(),
                LayerSchedule::uniform(0.05, 0.05, 2).unwrap(),
                Default::default(),
                sc_print_primitives::AaLevel::Off,
                PackingMode::None,
            );
            let SliceOutcome::Completed(leaves) = sc_sl1cer::api::slice_scene_with_fallback(
                &scene,
                &profile,
                &CancelToken::new(),
                |_| {},
                false,
            )
            .unwrap() else {
                panic!("cancelled")
            };
            assert!(!leaves[0].stack().layers()[0].polygons().is_empty());
            assert!(leaves[0].stack().layers()[1].polygons().is_empty());
        }
        if tool == MachineTool::ZTower {
            for step in 0..5 {
                let x = -16.0 + step as f32 * 8.0;
                let expected = (step + 1) as f32 * 5.0;
                let top = mesh
                    .vertices
                    .iter()
                    .filter(|v| (v[0] - (x - 4.0)).abs() < 1e-5 || (v[0] - (x + 4.0)).abs() < 1e-5)
                    .filter(|v| v[1].abs() <= 6.01)
                    .map(|v| v[2])
                    .fold(0.0f32, f32::max);
                // At shared vertical boundaries the taller adjacent step is present.
                assert!(top >= expected);
                assert!(
                    mesh.vertices
                        .iter()
                        .any(|v| (v[0] - (x - 4.0)).abs() < 1e-5 && (v[2] - expected).abs() < 1e-5)
                );
            }
        }
    }
}
#[test]
fn invalid_machine_envelopes_fail_before_generating_geometry() {
    assert!(machine_scene(MachineTool::XyRuler, [100.0, 120.0, 200.0], 0.05).is_err());
    assert!(machine_scene(MachineTool::ZTower, [192.0, 120.0, 20.0], 0.05).is_err());
    assert!(machine_scene(MachineTool::Orientation, [192.0, 120.0, 200.0], f64::NAN).is_err());
}

#[test]
fn z_tower_cross_sections_end_at_each_known_step_height() {
    use sc_sl1cer::{
        CancelToken, SliceOutcome,
        profile::{LayerSchedule, PackingMode, SliceProfile},
    };
    let scene = machine_scene(MachineTool::ZTower, [192.0, 120.0, 200.0], 0.05).unwrap();
    let profile = SliceProfile::new(
        sc_print_primitives::BuildVolume::new(192.0, 120.0, 200.0).unwrap(),
        sc_print_primitives::PixelResolution::new(192, 120).unwrap(),
        LayerSchedule::uniform(5.0, 5.0, 6).unwrap(),
        Default::default(),
        sc_print_primitives::AaLevel::Off,
        PackingMode::None,
    );
    let SliceOutcome::Completed(leaves) = sc_sl1cer::api::slice_scene_with_fallback(
        &scene,
        &profile,
        &CancelToken::new(),
        |_| {},
        false,
    )
    .unwrap() else {
        panic!("cancelled")
    };
    for (layer_index, layer) in leaves[0].stack().layers().iter().enumerate() {
        let raster = sc_sl1cer::raster::scanline::rasterize_layer(
            layer,
            &profile.pixel_map(),
            sc_print_primitives::AaLevel::Off,
            0.0,
        );
        let pixels: Vec<_> = raster
            .runs()
            .iter()
            .flat_map(|r| std::iter::repeat_n(r.value, r.length as usize))
            .collect();
        for step in 0..5 {
            let x = 80 + step * 8;
            assert_eq!(
                pixels[60 * 192 + x] > 0,
                step >= layer_index,
                "step {step}, plane {}",
                layer.z_mm()
            );
        }
    }
}

/// R80: the overhang test is a closed solid flat on the plate, and its
/// ledges' undersides face down at exactly the labelled angles.
#[test]
fn the_overhang_test_has_one_ledge_per_angle() {
    let mesh = overhang_test().unwrap();
    mesh.validate_vertices().unwrap();
    mesh.validated_faces()
        .unwrap()
        .validate_edge_manifold()
        .unwrap();
    let z: Vec<f32> = mesh.vertices.iter().map(|v| v[2]).collect();
    assert_eq!(z.iter().copied().fold(f32::INFINITY, f32::min), 0.0);
    assert!(z.iter().copied().fold(0.0, f32::max) <= 15.0);
    let mut downward = std::collections::BTreeSet::new();
    for f in &mesh.faces {
        let n = f.normal.coords();
        if n[2] < -0.05 {
            // The underside's angle from horizontal is its normal's from
            // straight down.
            downward.insert((f64::from(-n[2]).acos().to_degrees()).round() as i64);
        }
    }
    for angle in OVERHANG_TEST_ANGLES {
        assert!(downward.contains(&(angle as i64)), "{angle}: {downward:?}");
    }
    let scene = overhang_test_scene().unwrap();
    assert_eq!(scene.roots().len(), 1);
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

use super::*;
use sc_print_profiles::{
    EffectiveMaterial, MaterialProfile, PrinterProfile, SceneOverrides, resolve,
};

fn config() -> ResolvedPrintConfig {
    let printer: PrinterProfile = toml_edit::de::from_str(include_str!(
        "../../../assets/profiles/printers/sleptic-diy-msla.toml"
    ))
    .unwrap();
    let material: MaterialProfile = toml_edit::de::from_str(include_str!(
        "../../../assets/profiles/materials/standard-405.toml"
    ))
    .unwrap();
    resolve(
        &printer,
        &EffectiveMaterial::from(material),
        &SceneOverrides::default(),
    )
    .unwrap()
}

#[test]
fn exclusive_ramp_and_bottom_clamping() {
    let mut m = config().material_resolved;
    m.bottom_layer_count = 2;
    m.transition_layers = 3;
    let schedule = ExposureSchedule::uniform(&m, 7, 0.05, 0.05);
    let expected = [30.0, 30.0, 23.125, 16.25, 9.375, 2.5, 2.5];
    assert_eq!(
        schedule
            .layers
            .iter()
            .map(|l| l.exposure_s)
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        schedule.layers.iter().filter(|l| l.is_transition).count(),
        3
    );
    assert!(ExposureSchedule::uniform(&m, 1, 0.05, 0.05).layers[0].is_bottom);
    m.transition_layers = 0;
    assert_eq!(
        ExposureSchedule::uniform(&m, 3, 0.05, 0.05).layers[2].exposure_s,
        2.5
    );
}

#[test]
fn four_thousand_layers_have_exact_decimal_z() {
    let m = config().material_resolved;
    let schedule = ExposureSchedule::uniform(&m, 4000, 0.075, 0.035);
    let mut values = BTreeMap::new();
    for (i, l) in schedule.layers.iter().enumerate() {
        layer_values(&mut values, l);
        let microns = 75 + i * 35;
        assert_eq!(
            values["z"],
            format!("{}.{:03}", microns / 1000, microns % 1000)
        );
    }
    assert_eq!(values["z"], "140.040");
}

#[test]
fn templates_render_custom_scalars_and_name_unknown_keys_with_line() {
    let c = config();
    let schedule = ExposureSchedule::uniform(&c.material_resolved, 2, 0.05, 0.05);
    let estimate = PrintEstimate::new(&schedule, 1.0, None, Some(40.0));
    let script = render_job(&c, &schedule, &estimate).unwrap();
    assert!(script.contains("M810 ; enable UV"));
    assert!(script.contains("G4 P30000"));
    assert_eq!(script.matches(";LAYER_START:").count(), 2);
    let err = render_template("layer", "ok\nG4 P{typo}", &BTreeMap::new()).unwrap_err();
    assert_eq!(
        (err.template.as_str(), err.line, err.placeholder.as_str()),
        ("layer", 2, "typo")
    );
    assert!(render_template("end", "{broken", &BTreeMap::new()).is_err());
    assert!(render_template("end", "stray}", &BTreeMap::new()).is_err());
    let values = BTreeMap::from([("custom.command".into(), "{literal}".into())]);
    assert_eq!(
        render_template("layer", "{custom.command}", &values).unwrap(),
        "{literal}"
    );
}

#[test]
fn time_weight_cost_and_density_diagnostic() {
    let mut m = config().material_resolved;
    m.bottom_layer_count = 1;
    m.transition_layers = 1;
    m.light_off_delay_s = 2.0;
    let schedule = ExposureSchedule::uniform(&m, 3, 0.05, 0.05);
    // 30 + 16.25 + 2.5 exposures, 6 seconds wait, 3 * (6 + 2.4) travel.
    let e = PrintEstimate::new(&schedule, 10.0, None, Some(40.0));
    assert!((e.total_time_s - 79.95).abs() < 1e-8);
    assert_eq!(e.weight_g, 11.0);
    assert_eq!(e.cost, Some(0.4));
    assert_eq!(e.diagnostics.len(), 1);
    assert!(e.summary(3).contains("print time 0:01:20"));
    assert!(
        PrintEstimate::new(&schedule, 10.0, Some(1.2), None)
            .diagnostics
            .is_empty()
    );
}

#[test]
fn pixel_volume_of_ten_mm_cube_is_one_ml() {
    use sc_sl1cer::contour::{ClosedRing, ContourLayer, Polygon};
    use sc_sl1cer::geom2d::Point2D;
    use sc_sl1cer::profile::{AaLevel, BuildVolume, Mirror, PackingMode, PixelResolution};
    let polygon = Polygon::new(
        ClosedRing::new(vec![
            Point2D::new(-5.0, -5.0),
            Point2D::new(5.0, -5.0),
            Point2D::new(5.0, 5.0),
            Point2D::new(-5.0, 5.0),
        ]),
        vec![],
    )
    .unwrap();
    let stack = ContourStack::new_unchecked(
        (0..100)
            .map(|i| ContourLayer::new((i as f64 + 0.5) * 0.1, vec![polygon.clone()]))
            .collect(),
    );
    let profile = SliceProfile::new(
        BuildVolume::new(20.0, 20.0, 20.0).unwrap(),
        PixelResolution::new(200, 200).unwrap(),
        LayerSchedule::uniform(0.1, 0.1, 100).unwrap(),
        Mirror::default(),
        AaLevel::X4,
        PackingMode::None,
    );
    assert!((raster_volume_ml(&stack, &profile) - 1.0).abs() < 0.01);
    // The GUI's single-pass analysis must report the same volume, and a
    // straight prism has nothing unsupported.
    let analysis = analyze_layers(&stack, &profile);
    assert!((analysis.volume_ml - raster_volume_ml(&stack, &profile)).abs() < 1e-9);
    assert!(analysis.islands.is_empty());
}

#[test]
fn material_precedence_validation_and_provenance() {
    let mut material: MaterialProfile = toml_edit::de::from_str(include_str!(
        "../../../assets/profiles/materials/standard-405.toml"
    ))
    .unwrap();
    material.defaults.transition_layers = Some(4);
    material.defaults.light_off_delay_s = Some(2.0);
    let printer = config().printer;
    let resolved = resolve(
        &printer,
        &EffectiveMaterial::from(material.clone()),
        &SceneOverrides::default(),
    )
    .unwrap();
    assert_eq!(resolved.material_resolved.transition_layers, 4);
    assert_eq!(resolved.material_resolved.light_off_delay_s, 2.0);
    let block = material.per_printer.values_mut().next().unwrap();
    block.transition_layers = Some(0);
    block.light_off_delay_s = Some(0.0);
    let resolved = resolve(
        &printer,
        &EffectiveMaterial::from(material),
        &SceneOverrides::default(),
    )
    .unwrap();
    assert_eq!(resolved.material_resolved.transition_layers, 0);
    assert_eq!(resolved.material_resolved.light_off_delay_s, 0.0);
    assert!(resolved.overrides_applied.iter().any(|r| r.field
        == sc_print_profiles::resolved_field::ResolvedField::TransitionLayers
        && r.source == sc_print_profiles::ValueSource::Calibrated));
    for text in [
        "transition_layers = -1",
        "transition_layers = 1001",
        "light_off_delay_s = -0.1",
        "light_off_delay_s = nan",
        "density_g_ml = 0.0",
    ] {
        assert!(
            toml_edit::de::from_str::<sc_print_profiles::MaterialDefaults>(text).is_err(),
            "{text}"
        );
        if !text.starts_with("density") {
            assert!(
                toml_edit::de::from_str::<sc_print_profiles::MaterialPerPrinter>(text).is_err(),
                "{text}"
            );
        }
    }
}

#[test]
fn job_schema_rejects_unknown_fields_and_non_scalar_custom_values() {
    use sc_print_profiles::job_profile::JobProfile;
    let base = "kind = 'gcode-zip'\n[templates]\nstart = ''\nlayer = ''\nend = ''\n";
    assert!(toml_edit::de::from_str::<JobProfile>(base).is_ok());
    assert!(toml_edit::de::from_str::<JobProfile>(&format!("typo = 1\n{base}")).is_err());
    assert!(toml_edit::de::from_str::<JobProfile>(&format!("{base}typo = ''")).is_err());
    assert!(
        toml_edit::de::from_str::<JobProfile>(&format!("{base}[custom]\nfoo = [1,2]")).is_err()
    );
    assert!(
        toml_edit::de::from_str::<JobProfile>(&format!(
            "{base}[custom]\na = true\nb = 1\nc = 2.5\nd = 'M810'"
        ))
        .is_ok()
    );
}

#[test]
fn builtin_templates_cover_machine_metadata_and_exposure_switching() {
    let mut c = config();
    c.printer.job = None;
    let schedule = ExposureSchedule::uniform(&c.material_resolved, 2, 0.1, 0.1);
    let estimate = PrintEstimate::new(&schedule, 1.0, Some(1.2), Some(40.0));
    let script = render_job(&c, &schedule, &estimate).unwrap();
    assert!(script.contains(";layerHeight:0.100\n"));
    assert!(script.contains(";resolutionX:3840\n;resolutionY:2400\n"));
    assert!(script.contains("M6054 \"1.png\"\nM106 S255\nG4 P30000\nM106 S0"));
    assert!(
        script.contains("G0 Z0.200 F150.000\nG91\nG0 Z6.000 F60.000\nG0 Z-6.000 F150.000\nG90")
    );
}

#[test]
fn long_schedule_time_does_not_round_below_half_second() {
    let c = config();
    let schedule = ExposureSchedule::uniform(&c.material_resolved, 960, 0.05, 0.05);
    let estimate = PrintEstimate::new(&schedule, 1.0, None, None);
    assert_eq!(estimate.total_time_s, 10601.5);
    assert!(estimate.summary(960).contains("2:56:42"));
}

#[test]
fn end_lift_clears_vat_and_clamps_to_machine_height_before_motor_release() {
    for use_default in [false, true] {
        let mut c = config();
        if use_default {
            c.printer.job = None;
        }
        for (layers, expected) in [(2, "60.100"), (4000, "200.000")] {
            let schedule = ExposureSchedule::uniform(&c.material_resolved, layers, 0.05, 0.05);
            let estimate = PrintEstimate::new(&schedule, 1.0, None, None);
            let script = render_job(&c, &schedule, &estimate).unwrap();
            let end = script
                .rfind(&format!("G1 Z{expected} F150.000"))
                .expect("final lift");
            assert!(end < script.rfind("M18").unwrap());
        }
    }
}

#[test]
fn scene_process_overrides_reach_exposure_schedule() {
    let base = config();
    let material: MaterialProfile = toml_edit::de::from_str(include_str!(
        "../../../assets/profiles/materials/standard-405.toml"
    ))
    .unwrap();
    let resolved = resolve(
        &base.printer,
        &EffectiveMaterial::from(material),
        &SceneOverrides {
            transition_layers: Some(7),
            bottom_compensation_mm: None,
            xy_compensation_mm: None,
            aa_levels: None,
            light_off_delay_s: Some(1.5),
            ..Default::default()
        },
    )
    .unwrap();
    let schedule = ExposureSchedule::uniform(&resolved.material_resolved, 30, 0.05, 0.05);
    assert_eq!(
        schedule.layers.iter().filter(|l| l.is_transition).count(),
        7
    );
    assert!(schedule.layers.iter().all(|l| l.light_off_delay_s == 1.5));
}

#[test]
fn accuracy_volume_and_islands_use_corrected_pixels() {
    use sc_sl1cer::{
        contour::{ClosedRing, ContourLayer, ContourStack, Polygon},
        geom2d::Point2D,
        profile::{
            AaLevel, BuildVolume, LayerSchedule, Mirror, PackingMode, PixelResolution, SliceProfile,
        },
        raster::accuracy::Accuracy,
    };
    let square = |side: f64| {
        Polygon::new(
            ClosedRing::new(vec![
                Point2D::new(-side / 2.0, -side / 2.0),
                Point2D::new(side / 2.0, -side / 2.0),
                Point2D::new(side / 2.0, side / 2.0),
                Point2D::new(-side / 2.0, side / 2.0),
            ]),
            vec![],
        )
        .unwrap()
    };
    let stack = ContourStack::new_unchecked(vec![
        ContourLayer::new(0.05, vec![square(10.0)]),
        ContourLayer::new(0.15, vec![square(10.0)]),
    ]);
    let profile = SliceProfile::new(
        BuildVolume::new(20.0, 20.0, 20.0).unwrap(),
        PixelResolution::new(200, 200).unwrap(),
        LayerSchedule::uniform(0.1, 0.1, 2).unwrap(),
        Mirror::default(),
        AaLevel::X4,
        PackingMode::None,
    )
    .with_accuracy(Accuracy {
        bottom_compensation_mm: 0.5,
        bottom_layers: 1,
        ..Default::default()
    });
    let expected = (81.0 + 100.0) * 0.1 / 1000.0;
    assert!((raster_volume_ml(&stack, &profile) - expected).abs() < 1e-10);
    let analysis = analyze_layers(&stack, &profile);
    assert!((analysis.volume_ml - expected).abs() < 1e-10);
    assert!(analysis.islands.is_empty());
    // Remove the entire bottom footprint: the unchanged next layer becomes an island.
    let profile = profile.with_accuracy(Accuracy {
        bottom_compensation_mm: 5.0,
        bottom_layers: 1,
        ..Default::default()
    });
    assert_eq!(analyze_layers(&stack, &profile).islands.len(), 1);
}

#[test]
fn two_stage_motion_ramps_and_drives_real_sleptic_template() {
    use sc_print_profiles::motion::Motion;
    let mut c = config();
    let m = &mut c.material_resolved;
    m.bottom_layer_count = 1;
    m.transition_layers = 1;
    m.motion.normal = Motion {
        lift_slow_distance_mm: Some(2.0),
        lift_slow_speed_mm_min: Some(60.0),
        lift_fast_distance_mm: Some(4.0),
        lift_fast_speed_mm_min: Some(240.0),
        retract_slow_distance_mm: Some(2.0),
        retract_slow_speed_mm_min: Some(120.0),
        retract_fast_distance_mm: Some(4.0),
        retract_fast_speed_mm_min: Some(240.0),
        wait_before_cure_s: Some(0.3),
        wait_after_lift_s: Some(0.4),
        wait_after_retract_s: Some(0.5),
        light_off_delay_s: Some(0.6),
    };
    m.motion.bottom.wait_after_lift_s = Some(2.4);
    let schedule = ExposureSchedule::uniform(m, 3, 0.05, 0.05);
    assert_eq!(
        schedule
            .layers
            .iter()
            .map(|l| l.motion.wait_after_lift_s.unwrap())
            .collect::<Vec<_>>(),
        vec![2.4, 1.4, 0.4]
    );
    let estimate = PrintEstimate::new(&schedule, 0.0, None, None);
    assert!((estimate.total_time_s - (30.0 + 16.25 + 2.5 + 3.0 * 6.8 + 3.0)).abs() < 1e-8);
    let script = render_job(&c, &schedule, &estimate).unwrap();
    assert!(script.contains("G1 Z2.000 F60.000\nG1 Z4.000 F240.000\nG4 P2400\nG1 Z-4.000 F240.000\nG1 Z-2.000 F120.000\nG90\nG4 P500\nG4 P300\n"));
    assert!(script.contains("G4 P600 ; settling delay"));
    assert!(!script.contains("G1 Z6.000"));
}
#[test]
fn legacy_motion_retains_script_bytes_and_seconds() {
    let c = config();
    let schedule = ExposureSchedule::uniform(&c.material_resolved, 2, 0.05, 0.05);
    let estimate = PrintEstimate::new(&schedule, 0.0, None, None);
    assert_eq!(estimate.total_time_s, 76.8);
    let first = render_job(&c, &schedule, &estimate).unwrap();
    let legacy = r#";fileName:run
;machineType:sleptic-diy-msla
;estimatedPrintTime:76.800
;volume:0.000
;resin:standard-405
;layerHeight:0.050
;resolutionX:3840
;resolutionY:2400
G21 ; millimetres
G28 Z ; home the Z axis
G90 ; absolute positioning
;LAYER_START:1
;Z:0.050
G1 Z0.050 F150.000 ; position at the layer height
G91 ; relative peel moves
G1 Z6.000 F60.000 ; peel lift
G1 Z-6.000 F150.000 ; retract by the same distance
G90 ; restore absolute coordinates
G4 P0 ; settling delay
M6054 "1.png" ; display layer image (controller integration required)
M810 ; enable UV
G4 P30000 ; exposure dwell in milliseconds
M811 ; disable UV
;LAYER_END
;LAYER_START:2
;Z:0.100
G1 Z0.100 F150.000 ; position at the layer height
G91 ; relative peel moves
G1 Z6.000 F60.000 ; peel lift
G1 Z-6.000 F150.000 ; retract by the same distance
G90 ; restore absolute coordinates
G4 P0 ; settling delay
M6054 "2.png" ; display layer image (controller integration required)
M810 ; enable UV
G4 P30000 ; exposure dwell in milliseconds
M811 ; disable UV
;LAYER_END
M811 ; ensure UV is disabled
G90 ; absolute final lift
G1 Z60.100 F150.000 ; raise plate clear of the vat
M18 ; release motors
"#;
    assert_eq!(first.as_bytes(), legacy.as_bytes());
    let mut second_schedule = schedule.clone();
    for layer in &mut second_schedule.layers {
        layer.motion = Default::default();
    }
    assert_eq!(
        first.as_bytes(),
        render_job(&c, &second_schedule, &estimate)
            .unwrap()
            .as_bytes()
    );
    assert!(first.contains("G91 ; relative peel moves\nG1 Z6.000 F60.000 ; peel lift\nG1 Z-6.000 F150.000 ; retract by the same distance\nG90 ; restore absolute coordinates"));
}
#[test]
fn multi_exposure_has_one_peel_and_two_masked_dwells_per_layer() {
    let c = config();
    let mut schedule = ExposureSchedule::uniform(&c.material_resolved, 2, 0.05, 0.05);
    let exposures = vec![vec![1.5, 1.5], vec![2.5, 2.5]];
    for layer in &mut schedule.layers {
        layer.exposure_s = 4.0;
    }
    let estimate = PrintEstimate::new(&schedule, 0.0, None, None);
    assert!((estimate.total_time_s - 24.8).abs() < 1e-8);
    let script = render_multi_job(&c, &schedule, &estimate, &exposures).unwrap();
    assert_eq!(script.matches("; relative peel moves").count(), 2);
    assert_eq!(script.matches("M6054").count(), 4);
    for name in ["1-tile1.png", "1-tile2.png", "2-tile1.png", "2-tile2.png"] {
        assert!(script.contains(name));
    }
    assert_eq!(script.matches("G4 P1500").count(), 2);
    assert_eq!(script.matches("G4 P2500").count(), 2);
}

#[test]
fn legacy_nonzero_delay_preserves_the_original_floating_point_order() {
    let mut material = config().material_resolved;
    material.bottom_layer_count = 0;
    material.transition_layers = 0;
    material.normal_exposure_sec = sc_print_primitives::PositiveFiniteSec::new(2.3).unwrap();
    material.light_off_delay_s = 1.7;
    let expected = material.normal_exposure_sec.value()
        + material.light_off_delay_s
        + 60.0
            * material.lift_distance_mm.value()
            * (1.0 / material.lift_speed_mm_min.value()
                + 1.0 / material.retract_speed_mm_min.value());
    let schedule = ExposureSchedule::uniform(&material, 1, 0.05, 0.05);
    assert_eq!(schedule.total_time_s().to_bits(), expected.to_bits());
}

#[test]
fn motion_cycles_preserve_template_move_family_and_expose_all_new_values() {
    let mut c = config();
    c.material_resolved.motion.normal.wait_before_cure_s = Some(0.1);
    let schedule = ExposureSchedule::uniform(&c.material_resolved, 1, 0.05, 0.05);
    let estimate = PrintEstimate::new(&schedule, 0.0, None, None);
    let sleptic = render_job(&c, &schedule, &estimate).unwrap();
    assert!(sleptic.contains("G1 Z6.000 F60.000"));
    c.printer.job = None;
    let default = render_job(&c, &schedule, &estimate).unwrap();
    assert!(default.contains("G0 Z6.000 F60.000"));
    assert_eq!(motion_command("{motion_cycle}"), "G1");
    let mut values = BTreeMap::new();
    layer_values(&mut values, &schedule.layers[0]);
    for key in [
        "lift_slow_distance_mm",
        "lift_slow_speed_mm_min",
        "lift_fast_distance_mm",
        "lift_fast_speed_mm_min",
        "retract_slow_distance_mm",
        "retract_slow_speed_mm_min",
        "retract_fast_distance_mm",
        "retract_fast_speed_mm_min",
        "wait_before_cure_s",
        "wait_after_lift_s",
        "wait_after_retract_s",
        "light_off_delay_s",
    ] {
        assert!(values.contains_key(key));
    }
}

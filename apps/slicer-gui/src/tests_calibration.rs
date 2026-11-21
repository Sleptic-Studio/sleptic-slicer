// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
#[test]
fn chosen_exposure_forks_material_and_preserves_other_calibration() {
    let mut source: sc_print_profiles::MaterialProfile = toml::from_str(include_str!(
        "../../../assets/profiles/materials/standard-405.toml"
    ))
    .unwrap();
    source.defaults.motion = Some(sc_print_profiles::motion::MotionProfile {
        normal: sc_print_profiles::motion::Motion {
            wait_before_cure_s: Some(0.25),
            ..Default::default()
        },
        ..Default::default()
    });
    source
        .per_printer
        .get_mut(&sc_print_profiles::CalibrationTarget::Generic)
        .unwrap()
        .motion = Some(sc_print_profiles::motion::MotionProfile {
        bottom: sc_print_profiles::motion::Motion {
            wait_after_lift_s: Some(1.5),
            ..Default::default()
        },
        ..Default::default()
    });
    let original = source.clone();
    let printer = "sleptic-diy-msla".parse().unwrap();
    let fork = result_profile(
        &source,
        &printer,
        1.75,
        "Standard 405 - exposure 1.75s",
        &BTreeSet::new(),
    )
    .unwrap();
    assert_ne!(fork.identity.id, source.identity.id);
    assert_eq!(source, original);
    assert!(!fork.extension);
    let block = fork
        .per_printer
        .get(&sc_print_profiles::CalibrationTarget::from_printer(
            &printer,
        ))
        .unwrap();
    assert_eq!(block.normal_exposure_sec.unwrap().value(), 1.75);
    assert_eq!(
        block.bottom_exposure_sec,
        source
            .per_printer
            .get(&sc_print_profiles::CalibrationTarget::Generic)
            .unwrap()
            .bottom_exposure_sec
    );
    assert_eq!(fork.defaults, source.defaults);
    assert_eq!(
        block.motion,
        source
            .per_printer
            .get(&sc_print_profiles::CalibrationTarget::Generic)
            .unwrap()
            .motion
    );
}

#[test]
fn machine_result_forks_round_trips_and_preserves_source_and_motion() {
    use sc_print_primitives::{LcdRotation, MachineCalibration, Mirror};
    let source: sc_print_profiles::PrinterProfile = toml::from_str(include_str!(
        "../../../assets/profiles/printers/sleptic-diy-msla.toml"
    ))
    .unwrap();
    let original = source.clone();
    let calibration = MachineCalibration::new(LcdRotation::Deg270, [1.01, 0.99]).unwrap();
    let fork = crate::printer_editor::duplicate_profile(
        source.clone(),
        &sc_print_profiles::ProfileLibraries::platform_default(),
    );
    let fork = machine_result_profile(fork, calibration, Mirror { x: true, y: false }).unwrap();
    assert_ne!(fork.identity.id, source.identity.id);
    assert_eq!(fork.hardware.calibration(), calibration);
    assert_eq!(fork.motion, source.motion);
    assert_eq!(fork.job, source.job);
    assert_eq!(source, original);
    let dir = tempfile::tempdir().unwrap();
    sc_print_profiles::save_printer_profile_to_library(dir.path(), &fork).unwrap();
    let loaded = sc_print_profiles::load_printer_profile_from_path(
        &sc_print_profiles::printer_profile_path_in_library(dir.path(), &fork.identity.id),
    )
    .unwrap();
    assert_eq!(loaded, fork);
    let corrected = calibration.measured([100.0; 2], [99.0, 101.0]).unwrap();
    assert!((corrected.xy_scale()[0] - 1.01 * 100.0 / 99.0).abs() < 1e-12);
    assert!((corrected.xy_scale()[1] - 0.99 * 100.0 / 101.0).abs() < 1e-12);
    assert_eq!(corrected.rotation, LcdRotation::Deg270);
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, 1.0] {
        assert!(calibration.measured([100.0; 2], [bad, 100.0]).is_err());
    }
    assert!(LcdRotation::try_from(45).is_err());
}

#[test]
fn machine_orientation_setting_uses_a_compact_row() {
    let source = include_str!("../ui/main-window.slint");
    assert!(
        source
            .contains("HorizontalLayout {height:28px;Text {text:@tr(\"Orientation (clockwise)\");")
    );
}

/// R80: "Save to resin" copies the resin with all its calibration and
/// the measured overhang angle.
#[test]
fn an_overhang_result_is_a_copy_of_the_resin_with_its_angle() {
    let source = sc_print_profiles::load_material_profile_from_path(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/profiles/materials/resione-f69.toml"),
    )
    .unwrap();
    let fork = overhang_result_profile(
        &source,
        60.0,
        "F69 - overhang 60",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(fork.defaults.support_overhang_angle_deg, Some(60.0));
    assert_eq!(source.defaults.support_overhang_angle_deg, Some(70.0));
    assert_eq!(fork.per_printer, source.per_printer);
    assert_ne!(fork.identity.id, source.identity.id);
    assert_eq!(fork.identity.brand, source.identity.brand);
}

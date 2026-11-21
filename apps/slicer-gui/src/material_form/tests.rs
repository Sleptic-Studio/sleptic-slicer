// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_print_profiles::{
    load_material_extension_from_library, load_material_profile_from_library,
    load_material_profile_from_path,
};

use super::*;

fn calibration(
    fill: impl FnOnce(&mut FieldTexts),
) -> Result<MaterialPerPrinter, FieldRejected> {
    let mut input = FieldTexts::default();
    fill(&mut input);
    calibration_from_texts(&input)
}

fn defaults(
    fill: impl FnOnce(&mut FieldTexts),
) -> Result<MaterialDefaults, FieldRejected> {
    let mut input = FieldTexts::default();
    fill(&mut input);
    defaults_from_texts(&input)
}

#[test]
fn empty_fields_are_unset_values() {
    assert_eq!(calibration(|_| {}).unwrap(), MaterialPerPrinter::default());
    assert_eq!(defaults(|_| {}).unwrap(), MaterialDefaults::default());
    // Spaces alone count as empty.
    assert_eq!(
        calibration(|f| f.set(ResinField::LiftRate, "  ")).unwrap(),
        MaterialPerPrinter::default()
    );
}

#[test]
fn filled_calibration_fields_become_values() {
    let block = calibration(|f| {
        for (field, text) in [
            (ResinField::Exposure, " 2.5 "),
            (ResinField::BottomLayersExposure, "30"),
            (ResinField::BottomLayers, "6"),
            (ResinField::LiftHeight, "6"),
            (ResinField::LiftRate, "60"),
            (ResinField::RetractRate, "150"),
        ] {
            f.set(field, text);
        }
    })
    .unwrap();
    assert_eq!(block.normal_exposure_sec.unwrap().value(), 2.5);
    assert_eq!(block.bottom_exposure_sec.unwrap().value(), 30.0);
    assert_eq!(block.bottom_layer_count, Some(6));
    assert_eq!(block.lift_distance_mm.unwrap().value(), 6.0);
    assert_eq!(block.lift_speed_mm_min.unwrap().value(), 60.0);
    assert_eq!(block.retract_speed_mm_min.unwrap().value(), 150.0);
}

#[test]
fn a_bad_field_is_named_in_english_for_logs_and_translated_for_dialogs() {
    let err = calibration(|f| f.set(ResinField::LiftRate, "fast")).unwrap_err();
    assert_eq!(
        err,
        FieldRejected::Unreadable {
            field: ResinField::LiftRate
        }
    );
    assert_eq!(err.to_string(), "Lift speed must be a number");
    assert_eq!(err.rejection_line(), "Lift speed must be a number");
    for zero in ["0", "-1"] {
        let err = calibration(|f| f.set(ResinField::Exposure, zero)).unwrap_err();
        assert_eq!(
            err,
            FieldRejected::NotAllowed {
                field: ResinField::Exposure
            }
        );
        assert_eq!(err.to_string(), "Normal exposure is out of range");
    }
    // A count must be a whole number of zero or more.
    for not_whole in ["-1", "2.5"] {
        let err = calibration(|f| f.set(ResinField::BottomLayers, not_whole)).unwrap_err();
        assert_eq!(
            err,
            FieldRejected::Unreadable {
                field: ResinField::BottomLayers
            }
        );
    }
}

#[test]
fn defaults_check_price_angle_and_all_or_no_scale_axes() {
    let priced = defaults(|f| f.set(ResinField::Price, "34.99")).unwrap();
    assert_eq!(priced.price_per_liter.unwrap().value(), 34.99);
    let err = defaults(|f| f.set(ResinField::Price, "0")).unwrap_err();
    assert_eq!(
        err,
        FieldRejected::NotAllowed {
            field: ResinField::Price
        }
    );
    assert_eq!(
        defaults(|f| f.set(ResinField::OverhangAngle, "30"))
            .unwrap()
            .support_overhang_angle_deg,
        Some(30.0)
    );
    for outside in ["0", "90", "120", "NaN"] {
        let err = defaults(|f| f.set(ResinField::OverhangAngle, outside)).unwrap_err();
        assert_eq!(
            err,
            FieldRejected::NotAllowed {
                field: ResinField::OverhangAngle
            },
            "{outside}"
        );
    }
    let scale = defaults(|f| {
        { f.set(ResinField::ScaleX, "0.1"); f.set(ResinField::ScaleY, "-0.2"); f.set(ResinField::ScaleZ, "0"); }
    })
    .unwrap()
    .scale_compensation_pct
    .unwrap();
    assert_eq!(
        (scale.x.value(), scale.y.value(), scale.z.value()),
        (0.1, -0.2, 0.0)
    );
    let partial = defaults(|f| f.set(ResinField::ScaleY, "1")).unwrap_err();
    assert_eq!(partial, FieldRejected::ScaleAxesIncomplete);
    assert_eq!(
        partial.to_string(),
        "Fill all three scale compensation axes, or leave all three empty"
    );
    let too_big = defaults(|f| {
        { f.set(ResinField::ScaleX, "0.1"); f.set(ResinField::ScaleY, "0.1"); f.set(ResinField::ScaleZ, "99"); }
    });
    assert_eq!(
        too_big.unwrap_err(),
        FieldRejected::NotAllowed {
            field: ResinField::ScaleZ
        }
    );
}

const PROFILE: &str = r#"
[identity]
id = "brand-resin"
brand = "Brand"
family = "other"
name = "Resin"
profile_version = "1.0.0"

[intent]

[defaults]
bottom_layer_count = 5

[defaults.scale_compensation_pct]
x = 0.1
y = -0.2
z = 0.0

[per_printer.printer-a]
normal_exposure_sec = 2.5
bottom_exposure_sec = 30.0
lift_distance_mm = 6.0
"#;

fn profile() -> MaterialProfile {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profile.toml");
    std::fs::write(&path, PROFILE).unwrap();
    load_material_profile_from_path(&path).unwrap()
}

#[test]
fn loaded_values_fill_the_fields_and_read_back_the_same() {
    let profile = profile();
    let block = profile.per_printer.values().next().unwrap();
    let fields = calibration_texts(block);
    assert_eq!(
        (
            fields.text(ResinField::Exposure),
            fields.text(ResinField::BottomLayersExposure),
            fields.text(ResinField::BottomLayers)
        ),
        ("2.5", "30", "")
    );
    assert_eq!(&calibration_from_texts(&fields).unwrap(), block);
    let fields = defaults_texts(&profile.defaults);
    assert_eq!(
        [
            fields.text(ResinField::ScaleX),
            fields.text(ResinField::ScaleY),
            fields.text(ResinField::ScaleZ),
            fields.text(ResinField::BottomLayers)
        ],
        ["0.1", "-0.2", "0", "5"]
    );
    assert_eq!(defaults_from_texts(&fields).unwrap(), profile.defaults);
    assert_eq!(
        defaults_texts(&MaterialDefaults::default()),
        FieldTexts::default()
    );
}

#[test]
fn a_profile_saves_to_its_own_kind_of_file() {
    let library = tempfile::tempdir().unwrap();
    let materials = library.path().join("materials");
    let full = profile();
    write_edited_resin(library.path(), &full, ResinFileKind::WholeProfile).unwrap();
    assert_eq!(
        load_material_profile_from_library(library.path(), &full.identity.id).unwrap(),
        full
    );
    assert!(!materials.join("extensions/brand-resin.toml").exists());
    // An unflagged profile is refused as an extension, with the writer's reason.
    let err = write_edited_resin(library.path(), &full, ResinFileKind::ExtensionOnly).unwrap_err();
    assert!(err.contains("extension flag"), "{err}");
    let other = tempfile::tempdir().unwrap();
    let extension = MaterialProfile {
        extension: true,
        ..profile()
    };
    write_edited_resin(other.path(), &extension, ResinFileKind::ExtensionOnly).unwrap();
    assert_eq!(
        load_material_extension_from_library(other.path(), &extension.identity.id).unwrap(),
        extension
    );
    assert!(!other.path().join("materials/brand-resin.toml").exists());
}

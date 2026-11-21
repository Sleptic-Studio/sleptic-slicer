// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
#[test]
fn scene_override_edit_reset_and_invalid_input() {
    let mut overrides = SceneOverrides::default();
    assert!(edit(&mut overrides, 0, "0.075"));
    assert_eq!(overrides.layer_height_mm.unwrap().value(), 0.075);
    assert!(!edit(&mut overrides, 0, "NaN"));
    assert!(!edit(&mut overrides, 0, "-1"));
    assert!(edit(&mut overrides, 0, ""));
    assert!(overrides.layer_height_mm.is_none());
    assert!(edit(&mut overrides, 3, "8"));
    assert!(!edit(&mut overrides, 3, "1.5"));
    assert_eq!(overrides.bottom_layer_count, Some(8));
    assert!(edit(&mut overrides, 1, "3.2"));
    assert!(edit(&mut overrides, 2, "35"));
    assert!(edit(&mut overrides, 4, "6"));
    assert!(edit(&mut overrides, 5, "80"));
    assert!(edit(&mut overrides, 6, "180"));
    assert!(edit(&mut overrides, 7, "4"));
    assert!(edit(&mut overrides, 8, "1.5"));
    assert!(!edit(&mut overrides, 8, "NaN"));
    assert!(!edit(&mut overrides, 8, "-1"));
}

#[test]
fn count_fields_display_integers_accepted_by_the_editor() {
    use slint::Model;
    let printer: sc_print_profiles::PrinterProfile = toml::from_str(include_str!(
        "../../../assets/profiles/printers/sleptic-diy-msla.toml"
    ))
    .unwrap();
    let material: sc_print_profiles::MaterialProfile = toml::from_str(include_str!(
        "../../../assets/profiles/materials/standard-405.toml"
    ))
    .unwrap();
    let config = sc_print_profiles::resolve(
        &printer,
        &sc_print_profiles::EffectiveMaterial::from(material),
        &SceneOverrides::default(),
    )
    .unwrap();
    let model = rows(&config, &SceneOverrides::default());
    assert_eq!(model.row_data(3).unwrap().value.as_str(), "5");
    assert_eq!(model.row_data(7).unwrap().value.as_str(), "0");
}

#[test]
fn editor_enforces_profile_limits_and_steps() {
    let printer: sc_print_profiles::PrinterProfile = toml::from_str(include_str!(
        "../../../assets/profiles/printers/sleptic-diy-msla.toml"
    ))
    .unwrap();
    let (min, max, step) = limits(&printer.constraints, 0);
    assert!(min > 0.0 && max >= min && step > 0.0);
    let mut overrides = SceneOverrides::default();
    assert!(!edit_constrained(
        &mut overrides,
        &printer.constraints,
        0,
        &(max + 1.0).to_string()
    ));
    assert!(overrides.layer_height_mm.is_none());
    assert!(edit_constrained(
        &mut overrides,
        &printer.constraints,
        0,
        "0.05"
    ));
}

#[test]
fn accuracy_controls_accept_signed_xy_and_bounded_integer_gray_levels() {
    let mut o = SceneOverrides::default();
    assert!(edit(&mut o, 9, "0.10"));
    assert!(!edit(&mut o, 9, "-0.1"));
    assert!(edit(&mut o, 10, "-0.25"));
    assert_eq!(o.xy_compensation_mm, Some(-0.25));
    for invalid in ["1", "257", "4.5", "NaN"] {
        assert!(!edit(&mut o, 11, invalid));
    }
    assert!(edit(&mut o, 11, "16"));
    assert_eq!(o.aa_levels, Some(16));
    for id in [9, 10, 11] {
        assert!(edit(&mut o, id, ""));
    }
    assert_eq!(o, SceneOverrides::default());
}

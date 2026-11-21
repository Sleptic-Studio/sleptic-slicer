// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_print_primitives::{BuildVolume, PrinterId};
use sc_print_profiles::{MaterialExtensionIssue, load_material_profile_from_path};
use slint::Model;

use super::*;

/// A resin calibrated for "a", "z" and the generic machine, with some
/// optional fields set and the rest left out.
const RESIN: &str = r#"
[identity]
id = "acme-grey"
brand = "Acme"
family = "standard"
name = "Grey"
profile_version = "2.0.1"

[intent]
target_layer_height_mm = 0.05

[defaults]
bottom_layer_count = 6

[per_printer.a]
normal_exposure_sec = 2.5
bottom_exposure_sec = 30.0

[per_printer.generic-machine]
normal_exposure_sec = 3.0

[per_printer.z]
normal_exposure_sec = 2.8
"#;

/// An extension to it: these calibrations, nothing else.
fn extension(blocks: &str) -> String {
    format!(
        "extension = true\n\n[identity]\nid = \"acme-grey\"\nbrand = \"Acme\"\nfamily = \"standard\"\n\
         name = \"Grey\"\nprofile_version = \"2.0.1\"\n\n[intent]\n\n[defaults]\n\n{blocks}"
    )
}

fn load(text: &str) -> MaterialProfile {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resin.toml");
    std::fs::write(&path, text).unwrap();
    load_material_profile_from_path(&path).unwrap()
}

fn resin(extra: Option<&str>) -> EffectiveMaterial {
    let (material, issues) = EffectiveMaterial::compose(load(RESIN), extra.map(load));
    assert_eq!(issues, vec![]);
    material
}

fn id(id: &str) -> PrinterId {
    PrinterId::new(id).unwrap()
}

/// Printers "a", "b", "z" and the generic machine, named so that names and
/// ids sort differently ("a" is named last).
fn catalog() -> Vec<CatalogPrinter> {
    [
        ("a", "Zeta"),
        ("b", "Beta"),
        ("z", "Alpha"),
        ("generic-machine", "Generic Machine"),
    ]
    .map(|(printer, name)| CatalogPrinter {
        id: id(printer),
        printer_maker: "Maker".into(),
        name: name.into(),
        series: String::new(),
        printer_plate_volume: BuildVolume::new(100.0, 60.0, 80.0).unwrap(),
        picture_path: None,
        profile_origin: ProfileSource::Bundled,
    })
    .to_vec()
}

fn view(material: &EffectiveMaterial, default: Option<&str>) -> Vec<ResinDisplayCard> {
    profile_view_rows(
        material,
        &catalog(),
        default.map(id).as_ref(),
        ProfileSource::Bundled,
    )
}

fn titles(sections: &[ResinDisplayCard]) -> Vec<String> {
    sections
        .iter()
        .map(|section| section.card_heading.to_string())
        .collect()
}

fn highlighted(sections: &[ResinDisplayCard]) -> Vec<String> {
    sections
        .iter()
        .filter(|section| section.is_emphasised)
        .map(|section| section.card_heading.to_string())
        .collect()
}

fn rows(section: &ResinDisplayCard) -> Vec<(String, String)> {
    section
        .display_rows
        .iter()
        .map(|field| (field.caption_text.to_string(), field.display_text.to_string()))
        .collect()
}

fn values(section: &ResinDisplayCard) -> Vec<String> {
    rows(section).into_iter().map(|(_, value)| value).collect()
}

#[test]
fn the_default_printers_calibration_leads_then_generic_then_by_name() {
    let sections = view(&resin(None), Some("a"));
    assert_eq!(
        titles(&sections),
        [
            "Identity",
            "Intent",
            "Defaults",
            "Calibration: Zeta",
            "Generic calibration",
            "Calibration: Alpha"
        ]
    );
    assert_eq!(highlighted(&sections), ["Calibration: Zeta"]);
    // With no default printer, generic still leads and nothing is marked.
    let sections = view(&resin(None), None);
    assert_eq!(
        titles(&sections)[3..],
        [
            "Generic calibration",
            "Calibration: Alpha",
            "Calibration: Zeta"
        ]
    );
    assert_eq!(highlighted(&sections), Vec::<String>::new());
}

#[test]
fn a_printer_without_its_own_calibration_uses_the_generic_one() {
    assert_eq!(
        highlighted(&view(&resin(None), Some("b"))),
        ["Generic calibration"]
    );
    let mut no_generic = load(RESIN);
    no_generic.per_printer.remove(&CalibrationTarget::Generic);
    let (material, _) = EffectiveMaterial::compose(no_generic, None);
    assert_eq!(
        highlighted(&view(&material, Some("b"))),
        Vec::<String>::new()
    );
}

#[test]
fn an_extensions_calibrations_join_the_list_by_name() {
    let material = resin(Some(&extension(
        "[per_printer.b]\nnormal_exposure_sec = 2.2\n",
    )));
    assert_eq!(
        titles(&view(&material, Some("z")))[3..],
        [
            "Calibration: Alpha",
            "Generic calibration",
            "Calibration: Beta",
            "Calibration: Zeta"
        ]
    );
}

#[test]
fn an_extension_repeating_a_calibration_shows_it_once_with_the_resins_values() {
    let repeat = load(&extension("[per_printer.a]\nnormal_exposure_sec = 9.9\n"));
    let (material, issues) = EffectiveMaterial::compose(load(RESIN), Some(repeat));
    assert_eq!(
        issues,
        [MaterialExtensionIssue::OverlappingCalibrationPair {
            target: CalibrationTarget::from(id("a"))
        }]
    );
    let sections = view(&material, Some("a"));
    let zeta: Vec<_> = sections
        .iter()
        .filter(|section| section.card_heading == "Calibration: Zeta")
        .collect();
    assert_eq!(zeta.len(), 1);
    assert_eq!(values(zeta[0])[0], "2.5");
}

#[test]
fn every_field_is_listed_and_unset_ones_say_so() {
    let sections = view(&resin(None), Some("a"));
    assert_eq!(
        rows(&sections[0]),
        [
            ("Brand", "Acme"),
            ("Name", "Grey"),
            ("Family", "standard"),
            ("Profile version", "2.0.1"),
            ("Origin", "Bundled")
        ]
        .map(|(label, value)| (label.to_owned(), value.to_owned()))
    );
    assert_eq!(values(&sections[1]), ["not set", "0.05"]);
    assert_eq!(
        rows(&sections[2]),
        [
            ("Scale compensation (x/y/z)", "not set"),
            ("Bottom layer count", "6"),
            ("Price per liter", "not set")
        ]
        .map(|(label, value)| (label.to_owned(), value.to_owned()))
    );
    // 30.0 in the file shows as 30.
    assert_eq!(
        values(&sections[3]),
        ["2.5", "30", "not set", "not set", "not set", "not set"]
    );
    assert_eq!(rows(&sections[3])[5].0, "Retract speed (mm/min)");
}

#[test]
fn set_defaults_are_shown_as_written() {
    let priced = RESIN.replace(
        "bottom_layer_count = 6",
        "price_per_liter = 34.99\nscale_compensation_pct = { x = 0.5, y = -1.0, z = 1.25 }",
    );
    let (material, _) = EffectiveMaterial::compose(load(&priced), None);
    let defaults = &view(&material, None)[2];
    assert_eq!(values(defaults), ["0.5% / -1% / 1.25%", "not set", "34.99"]);
}

#[test]
fn a_users_profile_is_marked_custom_and_unknown_printers_go_by_id() {
    let sections = profile_view_rows(&resin(None), &[], Some(&id("a")), ProfileSource::User);
    assert_eq!(
        rows(&sections[0])[4],
        ("Origin".to_owned(), "Custom".to_owned())
    );
    assert_eq!(
        titles(&sections)[3..],
        ["Calibration: a", "Generic calibration", "Calibration: z"]
    );
    assert_eq!(resin_title(&load(RESIN).identity), "Acme Grey");
}

#[test]
fn the_calibration_picker_lists_targets_in_the_same_order() {
    let profile = load(RESIN);
    let listed: Vec<(CalibrationTarget, String)> =
        ordered_calibration_targets(&profile, &catalog(), Some(&id("z")))
            .into_iter()
            .map(|(target, label)| (target, label.to_string()))
            .collect();
    assert_eq!(
        listed,
        [
            (CalibrationTarget::from(id("z")), "Alpha".to_owned()),
            (CalibrationTarget::Generic, "Generic".to_owned()),
            (CalibrationTarget::from(id("a")), "Zeta".to_owned()),
        ]
    );
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_print_primitives::BuildVolume;
use sc_print_profiles::load_material_profile_from_path;

use sc_print_profiles::{MaterialDefaults, MaterialIntent};

use super::*;
use crate::profile_catalog::ProfileSource;

const IDENTITY: &str = r#"
[identity]
id = "acme-grey"
brand = "Acme"
family = "standard"
name = "Grey"
profile_version = "2.0.1"
"#;

/// The resin: calibrated for "a" only, with an intent and defaults.
fn resin() -> MaterialProfile {
    load(&format!(
        "{IDENTITY}\n[intent]\ntarget_layer_height_mm = 0.05\n\n[defaults]\nbottom_layer_count = 5\n\n\
         [per_printer.a]\nnormal_exposure_sec = 2.5\n"
    ))
}

/// Its extension, calibrated for "b".
fn extension() -> MaterialProfile {
    load(&format!(
        "extension = true\n{IDENTITY}\n[intent]\n\n[defaults]\n\n[per_printer.b]\nnormal_exposure_sec = 2.2\n"
    ))
}

fn load(text: &str) -> MaterialProfile {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resin.toml");
    std::fs::write(&path, text).unwrap();
    load_material_profile_from_path(&path).unwrap()
}

fn id(id: &str) -> PrinterId {
    PrinterId::new(id).unwrap()
}

fn target(printer: &str) -> CalibrationTarget {
    CalibrationTarget::from(id(printer))
}

/// Printers in catalog order: z, a, generic, b.
fn catalog(ids: &[&str]) -> Vec<CatalogPrinter> {
    ids.iter()
        .map(|printer| CatalogPrinter {
            id: id(printer),
            printer_maker: "Maker".into(),
            name: format!("Printer {}", printer.to_uppercase()),
            series: String::new(),
            printer_plate_volume: BuildVolume::new(100.0, 60.0, 80.0).unwrap(),
            picture_path: None,
            profile_origin: ProfileSource::Bundled,
        })
        .collect()
}

fn offered(
    material: &EffectiveMaterial,
    printers: &[&str],
    default: Option<&str>,
) -> Vec<(String, String)> {
    printers_without_calibration(material, &catalog(printers), default.map(id).as_ref())
        .into_iter()
        .map(|(target, label)| (target.as_str().to_owned(), label.to_string()))
        .collect()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

const ALL: [&str; 4] = ["z", "a", "generic-machine", "b"];

#[test]
fn only_printers_without_a_calibration_are_offered() {
    let (plain, issues) = EffectiveMaterial::compose(resin(), None);
    assert!(issues.is_empty());
    assert_eq!(
        offered(&plain, &ALL, None),
        pairs(&[
            ("generic-machine", "Generic"),
            ("z", "Printer Z"),
            ("b", "Printer B")
        ])
    );
    // The extension's calibrations count as covered too.
    let (extended, _) = EffectiveMaterial::compose(resin(), Some(extension()));
    assert_eq!(
        offered(&extended, &ALL, None),
        pairs(&[("generic-machine", "Generic"), ("z", "Printer Z")])
    );
    assert_eq!(offered(&plain, &["a"], None), vec![]);
}

#[test]
fn the_default_printer_is_offered_first() {
    let (plain, _) = EffectiveMaterial::compose(resin(), None);
    assert_eq!(
        offered(&plain, &ALL, Some("b")),
        pairs(&[
            ("b", "Printer B"),
            ("generic-machine", "Generic"),
            ("z", "Printer Z")
        ])
    );
    // A default printer that is already covered changes nothing.
    assert_eq!(
        offered(&plain, &ALL, Some("a")),
        offered(&plain, &ALL, None)
    );
}

/// A calibration with one value set, to tell it apart.
fn values() -> MaterialPerPrinter {
    MaterialPerPrinter {
        bottom_layer_count: Some(4),
        ..Default::default()
    }
}

#[test]
fn the_first_extension_copies_only_the_identity() {
    let base = resin();
    let new = extension_with_calibration(&base, None, &target("z"), values());
    assert!(new.extension);
    assert_eq!(new.identity, base.identity);
    assert_eq!(new.intent, MaterialIntent::default());
    assert_eq!(new.defaults, MaterialDefaults::default());
    assert_eq!(
        new.per_printer.into_iter().collect::<Vec<_>>(),
        [(target("z"), values())]
    );
}

#[test]
fn later_extensions_keep_what_is_there() {
    let base = resin();
    let held = extension();
    let expected = [
        (target("b"), held.per_printer[&target("b")]),
        (target("z"), values()),
    ];
    // Added to the resin's existing extension...
    let added = extension_with_calibration(&base, Some(&held), &target("z"), values());
    assert!(added.extension);
    assert_eq!(added.per_printer.into_iter().collect::<Vec<_>>(), expected);
    // ...or, for an extension listed on its own, to itself.
    let orphan = extension_with_calibration(&held, None, &target("z"), values());
    assert_eq!(orphan.identity, held.identity);
    assert_eq!(orphan.per_printer.into_iter().collect::<Vec<_>>(), expected);
}

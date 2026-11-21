// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
fn base() -> PrinterProfile {
    toml::from_str(include_str!(
        "../../../assets/profiles/printers/sleptic-diy-msla.toml"
    ))
    .unwrap()
}
fn templates(p: &PrinterProfile) -> [String; 3] {
    let t = &p.job.as_ref().unwrap().templates;
    [t.start.clone(), t.layer.clone(), t.end.clone()]
}
#[test]
fn unknown_placeholder_and_bad_geometry_are_rejected_before_save() {
    let p = base();
    let mut t = templates(&p);
    t[1] = "G1 Z{unknown}".into();
    assert!(
        edited(&p, &fields(&p), [false, false], t)
            .unwrap_err()
            .to_string()
            .contains("unknown")
    );
    let mut f = fields(&p);
    f[1] = "0".into();
    assert!(edited(&p, &f, [false, false], templates(&p)).is_err());
    f = fields(&p);
    f[5] = "100".into();
    assert!(edited(&p, &f, [false, false], templates(&p)).is_err());
    f = fields(&p);
    f[0] = "  ".into();
    assert!(edited(&p, &f, [false, false], templates(&p)).is_err());
}
#[test]
fn authoring_round_trip_preserves_custom_values_and_all_edits() {
    let p = base();
    let mut f = fields(&p);
    f[0] = "My custom printer".into();
    f[7] = "250.0".into();
    let mut t = templates(&p);
    t[0] = "G28\n; {layers_total}\n".into();
    let p = edited(&p, &f, [true, true], t).unwrap();
    let dir = tempfile::tempdir().unwrap();
    sc_print_profiles::save_printer_profile_to_library(dir.path(), &p).unwrap();
    let loaded =
        sc_print_profiles::load_printer_profile_from_library(dir.path(), &p.identity.id).unwrap();
    assert_eq!(p, loaded);
    assert_eq!(fields(&loaded), f);
    assert!(loaded.hardware.mirror().x && loaded.hardware.mirror().y);
}

#[test]
fn adding_job_to_legacy_printer_keeps_export_defaults() {
    let mut p = base();
    p.job = None;
    let t = sc_print_job::default_job_templates();
    let profile = edited(
        &p,
        &fields(&p),
        [false, false],
        [t.start.clone(), t.layer.clone(), t.end.clone()],
    )
    .unwrap();
    assert_eq!(profile.job.unwrap().templates, t);
}

#[test]
fn duplicate_avoids_existing_ids_even_when_the_existing_file_is_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let libraries = ProfileLibraries::new(Some(dir.path().to_owned()), Vec::new());
    let original = base();
    sc_print_profiles::save_printer_profile_to_library(dir.path(), &original).unwrap();
    let original_bytes = std::fs::read(dir.path().join("printers/sleptic-diy-msla.toml")).unwrap();
    std::fs::write(
        dir.path().join("printers/sleptic-diy-msla-copy-1.toml"),
        "invalid existing file",
    )
    .unwrap();
    let copy = duplicate_profile(original.clone(), &libraries);
    assert_eq!(copy.identity.id.to_string(), "sleptic-diy-msla-copy-2");
    assert_eq!(copy.hardware, original.hardware);
    assert_eq!(copy.job, original.job);
    sc_print_profiles::save_printer_profile_to_library(dir.path(), &copy).unwrap();
    assert_eq!(
        original_bytes,
        std::fs::read(dir.path().join("printers/sleptic-diy-msla.toml")).unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("printers/sleptic-diy-msla-copy-1.toml")).unwrap(),
        "invalid existing file"
    );
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

use super::*;
use std::{fs, path::Path};

fn library(root: &Path) {
    fs::create_dir_all(root.join("printers")).unwrap();
    fs::create_dir_all(root.join("materials")).unwrap();
    fs::write(
        root.join("printers/fixture-printer.toml"),
        r#"
[identity]
id = "fixture-printer"
manufacturer = "Fixture"
family = "Test machine"
name = "Package fixture"
profile_version = "1.0.0"
[hardware]
build_volume = { x_mm = 10.0, y_mm = 10.0, z_mm = 20.0 }
pixel_resolution = { x = 100, y = 100 }
pixel_size_microns = { x = 100.0, y = 100.0 }
mirror = { x = false, y = false }
[capabilities]
output_formats = [{ format = "png_per_layer" }]
supports_grayscale = true
two_stage_lift = false
has_camera = false
[constraints]
"#,
    )
    .unwrap();
    fs::write(
        root.join("materials/fixture-resin.toml"),
        r#"
[identity]
id = "fixture-resin"
brand = "Fixture"
family = "Package material"
name = "Package resin"
profile_version = "2.0.0"
[intent]
[defaults]
"#,
    )
    .unwrap();
}

#[test]
fn export_scopes_have_the_expected_selection_and_default_names() {
    assert_eq!(PackageKind::PrinterProfiles.as_export_selection(), ExportSelection::Printers);
    assert_eq!(
        PackageKind::ResinProfiles.as_export_selection(),
        ExportSelection::Materials
    );
    assert_eq!(PackageKind::PrinterProfiles.suggested_file_name(), "printers.zip");
    assert_eq!(PackageKind::ResinProfiles.suggested_file_name(), "materials.zip");
}

#[test]
fn packages_round_trip_the_selected_kinds_and_report_overwrites() {
    let source = tempfile::tempdir().unwrap();
    library(source.path());
    for (selection, printer, material) in [
        (ExportSelection::All, true, true),
        (ExportSelection::Printers, true, false),
        (ExportSelection::Materials, false, true),
    ] {
        let destination = tempfile::tempdir().unwrap();
        let archive = destination.path().join("package.zip");
        write_package(source.path(), &selection, &archive).unwrap();
        let imported = destination.path().join("library");
        assert!(matches!(
            install_package(&imported, &archive, false).unwrap(),
            ImportResult::Added
        ));
        assert_eq!(
            imported.join("printers/fixture-printer.toml").is_file(),
            printer
        );
        assert_eq!(
            imported.join("materials/fixture-resin.toml").is_file(),
            material
        );
        if printer {
            assert_eq!(
                fs::read(imported.join("printers/fixture-printer.toml")).unwrap(),
                fs::read(source.path().join("printers/fixture-printer.toml")).unwrap()
            );
        }
        if material {
            assert_eq!(
                fs::read(imported.join("materials/fixture-resin.toml")).unwrap(),
                fs::read(source.path().join("materials/fixture-resin.toml")).unwrap()
            );
        }
        let ImportResult::WouldOverwrite(report) = install_package(&imported, &archive, false).unwrap()
        else {
            panic!("existing profiles must collide");
        };
        assert_eq!(!report.printers.is_empty(), printer);
        assert_eq!(!report.materials.is_empty(), material);
        assert!(matches!(
            install_package(&imported, &archive, true).unwrap(),
            ImportResult::Added
        ));
    }
}

#[test]
fn confirmation_requires_the_exact_custom_button() {
    for (result, expected) in [
        (MessageDialogResult::Custom("Replace".into()), true),
        (MessageDialogResult::Custom("replace".into()), false),
        (MessageDialogResult::Custom("Cancel".into()), false),
        (MessageDialogResult::Ok, false),
        (MessageDialogResult::Cancel, false),
        (MessageDialogResult::Yes, false),
        (MessageDialogResult::No, false),
    ] {
        assert_eq!(chose_overwrite(&result, "Replace"), expected);
    }
}

#[test]
fn collision_text_keeps_group_order_omits_empty_groups_and_preserves_ids() {
    let collision = |id: &str| Collision {
        id: id.into(),
        library_path: "unused".into(),
    };
    let report = CollisionReport {
        printers: vec![collision("b"), collision("a"), collision("b")],
        materials: vec![collision("resin")],
        material_extensions: vec![collision("extension")],
    };
    assert_eq!(
        overwrite_warning(&report),
        "Importing this package would overwrite existing profiles:\n\nPrinters:\n  - b\n  - a\n  - b\n\nMaterials:\n  - resin\n\nMaterial extensions:\n  - extension"
    );
    for (printers, materials, extensions) in [
        (true, false, false),
        (false, true, false),
        (false, false, true),
        (false, false, false),
    ] {
        let report = CollisionReport {
            printers: if printers {
                vec![collision("p")]
            } else {
                vec![]
            },
            materials: if materials {
                vec![collision("m")]
            } else {
                vec![]
            },
            material_extensions: if extensions {
                vec![collision("e")]
            } else {
                vec![]
            },
        };
        let text = overwrite_warning(&report);
        assert_eq!(text.contains("\n\nPrinters:"), printers);
        assert_eq!(text.contains("\n\nMaterials:"), materials);
        assert_eq!(text.contains("\n\nMaterial extensions:"), extensions);
    }
}

#[test]
fn package_errors_keep_context() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing.zip");
    assert!(
        install_package(temp.path(), &missing, false)
            .err()
            .unwrap()
            .to_string()
            .contains("could not open the package archive")
    );
    fs::write(&missing, b"invalid ZIP").unwrap();
    assert!(
        install_package(temp.path(), &missing, false)
            .err()
            .unwrap()
            .to_string()
            .contains("could not import the profile package:")
    );
}

#[test]
fn failed_export_preserves_existing_archives_and_does_not_create_new_ones() {
    let temp = tempfile::tempdir().unwrap();
    library(temp.path());
    let destination = temp.path().join("previous.zip");
    fs::write(&destination, b"previous archive").unwrap();
    let invalid = ExportSelection::Ids(vec!["does-not-exist".into()]);
    assert!(write_package(temp.path(), &invalid, &destination).is_err());
    assert_eq!(fs::read(&destination).unwrap(), b"previous archive");
    let fresh = temp.path().join("new.zip");
    assert!(write_package(temp.path(), &invalid, &fresh).is_err());
    assert!(!fresh.exists());
}

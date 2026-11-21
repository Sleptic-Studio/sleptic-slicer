// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Flag names and value spellings are what scripts depend on.
use sc_mesh_core::indexed_mesh::RepairPasses;
use sc_mesh_formats::filetypes::MeshFileType;
use sc_print_profiles::support_settings::SupportSettings;
use sc_sl1cer::profile::AaLevel;

use super::*;

fn parse(words: &[&str]) -> Result<Command, bpaf::ParseFailure> {
    parser().run_inner(words)
}

fn slice_with(extra: &[&str]) -> Result<Box<SliceArgs>, bpaf::ParseFailure> {
    let mut words = vec![
        "slice",
        "-s",
        "part.stl",
        "-t",
        "out.zip",
        "--printer-profile",
        "some-printer",
        "--material-profile",
        "./resin.toml",
    ];
    words.extend_from_slice(extra);
    match parse(&words)? {
        Command::Slice(args) => Ok(args),
        other => panic!("not a slice: {other:?}"),
    }
}

#[test]
fn mesh_commands_take_short_and_long_file_flags() {
    let Ok(Command::Convert {
        source,
        source_format,
        target,
        target_format,
    }) = parse(&[
        "convert",
        "--source",
        "a.obj",
        "--target",
        "b.bin",
        "--target-format",
        "stlb",
    ])
    else {
        panic!("convert");
    };
    assert_eq!((source.to_str(), target.to_str()), (Some("a.obj"), Some("b.bin")));
    assert_eq!(source_format, None);
    assert_eq!(target_format, Some(MeshFileType::StlBin));

    for command in ["diagnose", "inspect"] {
        let parsed = parse(&[command, "-s", "a.stl", "--source-format", "stla"]).unwrap();
        let (Command::Diagnose {
            source,
            source_format,
        }
        | Command::Inspect {
            source,
            source_format,
        }) = parsed
        else {
            panic!("{command}");
        };
        assert_eq!(source.to_str(), Some("a.stl"));
        assert_eq!(source_format, Some(MeshFileType::StlAscii));
    }
    assert!(parse(&["convert", "-s", "a.obj"]).is_err());
    assert!(parse(&["diagnose"]).is_err());
    assert!(parse(&["inspect", "-s", "a.stl", "--source-format", "step"]).is_err());
}

#[test]
fn fix_passes_default_to_the_shared_repair_and_take_joined_values() {
    let passes = |extra: &[&str]| {
        let mut words = vec!["fix", "-s", "a.stl", "-t", "b.stl"];
        words.extend_from_slice(extra);
        match parse(&words) {
            Ok(Command::Fix { passes, .. }) => Some(passes),
            _ => None,
        }
    };
    assert_eq!(passes(&[]), Some(RepairPasses::default_repair()));
    let changed = passes(&[
        "--fix-zero-length-edges=false",
        "--fix-flipped-faces=false",
        "--fix-face-normals=true",
    ])
    .unwrap();
    assert!(!changed.zero_length_edges && !changed.flipped_faces && changed.face_normals);
    // Shells are always separated; there is no flag for it.
    assert!(changed.separate_shells);
    assert_eq!(passes(&["--fix-separate-shells=false"]), None);
    // The value must be joined to its flag, and be a boolean.
    assert_eq!(passes(&["--fix-face-normals", "true"]), None);
    assert_eq!(passes(&["--fix-face-normals=maybe"]), None);
}

#[test]
fn slice_defaults_and_value_spellings() {
    let plain = slice_with(&[]).unwrap();
    assert_eq!(plain.source.to_str(), Some("part.stl"));
    assert_eq!(plain.target.to_str(), Some("out.zip"));
    assert!(plain.recipe.is_none() && plain.target_format.is_none());
    assert!(plain.layer_height.is_none() && plain.first_layer_height.is_none());
    assert_eq!(plain.aa, AaLevel::Off);
    assert!(plain.repair, "repair is on unless refused");
    assert!(!plain.verbose && !plain.no_auto_place && !plain.all_plates);
    assert!(!plain.islands && !plain.risks && !plain.supports);
    assert_eq!(plain.plate, None);
    assert_eq!(plain.minimum_feature_mm.value(), 0.3);
    assert_eq!(plain.support_settings, SupportSettings::default());

    for (given, level) in [
        ("0", AaLevel::Off),
        ("off", AaLevel::Off),
        ("2", AaLevel::X2),
        ("x4", AaLevel::X4),
        ("8", AaLevel::X8),
    ] {
        assert_eq!(slice_with(&["--aa", given]).unwrap().aa, level, "{given}");
    }
    assert!(slice_with(&["--aa", "3"]).is_err());

    let set = slice_with(&[
        "--recipe",
        "Miniature: tree supports, slice",
        "--layer-height",
        "0.05",
        "--first-layer-height",
        "0.1",
        "--plate",
        "2",
        "--all-plates",
        "--no-auto-place",
        "-v",
        "--islands",
        "--risks",
        "--minimum-feature-mm",
        "0.5",
        "--supports",
        "auto",
        "--target-format",
        "gcode-zip",
    ])
    .unwrap();
    assert_eq!(set.recipe.as_deref(), Some("Miniature: tree supports, slice"));
    assert_eq!(set.layer_height.map(|mm| mm.value()), Some(0.05));
    assert_eq!(set.first_layer_height.map(|mm| mm.value()), Some(0.1));
    assert_eq!(set.plate, Some(2));
    assert!(set.all_plates && set.no_auto_place && set.verbose);
    assert!(set.islands && set.risks && set.supports);
    assert_eq!(set.minimum_feature_mm.value(), 0.5);
    assert!(set.target_format.is_some());

    assert!(slice_with(&["--repair"]).unwrap().repair);
    assert!(!slice_with(&["--no-repair"]).unwrap().repair);
    assert!(!slice_with(&["--repair", "--no-repair"]).unwrap().repair);
    for bad in [
        &["--layer-height", "0"][..],
        &["--layer-height", "-0.05"],
        &["--minimum-feature-mm", "nan"],
        &["--supports", "on"],
        &["--target-format", "ctb"],
        &["--plate", "-1"],
    ] {
        assert!(slice_with(bad).is_err(), "{bad:?}");
    }
    // Both profiles and both paths are required.
    assert!(parse(&["slice", "-s", "a.stl", "-t", "b.zip"]).is_err());
}

#[test]
fn support_flags_set_their_setting_and_are_validated_together() {
    let d = SupportSettings::default();
    let s = slice_with(&[
        "--support-spacing-mm",
        "3",
        "--support-overhang-angle-deg",
        "40",
        "--support-lift-mm",
        "6",
        "--support-no-raft",
        "--support-skate-raft",
        "--symmetric-supports",
    ])
    .unwrap()
    .support_settings;
    assert_eq!((s.spacing_mm, s.overhang_angle_deg, s.lift_mm), (3.0, 40.0, 6.0));
    assert!(!s.raft && s.skate_raft && s.symmetric_supports);
    assert_eq!(s.tip_length_mm, d.tip_length_mm);

    let settings = |extra: &[&str]| slice_with(extra).unwrap().support_settings;
    // Trees and a solid raft are the defaults; the old positive flags still parse.
    assert!(d.tree_supports && settings(&["--tree-supports"]).tree_supports);
    assert!(!settings(&["--support-no-tree"]).tree_supports);
    assert!(settings(&["--tree-supports", "--support-no-tree"]).tree_supports);
    assert!(!d.lattice_raft && !settings(&["--support-solid-raft"]).lattice_raft);
    assert!(settings(&["--support-lattice-raft"]).lattice_raft);
    assert!(!settings(&["--support-lattice-raft", "--support-solid-raft"]).lattice_raft);

    assert!(slice_with(&["--support-spacing-mm", "0"]).is_err());
    assert!(slice_with(&["--support-tip-length-mm", "nan"]).is_err());
    assert!(slice_with(&["--support-spacing-mm", "wide"]).is_err());
}

#[test]
fn profile_commands_and_the_two_bare_commands() {
    for (words, kind) in [
        (&["profiles", "list"][..], ListKind::All),
        (&["profiles", "list", "printers"], ListKind::Printers),
        (&["profiles", "list", "materials"], ListKind::Materials),
    ] {
        assert!(matches!(parse(words), Ok(Command::ProfilesList { kind: k }) if k == kind));
    }
    assert!(parse(&["profiles", "list", "resins"]).is_err());

    let Ok(Command::ProfilesExport {
        output,
        printers,
        materials,
        ids,
    }) = parse(&["profiles", "export", "--id", "a", "--id", "b", "--printers", "out.zip"])
    else {
        panic!("export");
    };
    assert_eq!(output.to_str(), Some("out.zip"));
    assert!(printers && !materials);
    assert_eq!(ids, ["a", "b"]);
    assert!(parse(&["profiles", "export"]).is_err());

    assert!(matches!(
        parse(&["profiles", "import", "in.zip"]),
        Ok(Command::ProfilesImport { force: false, .. })
    ));
    assert!(matches!(
        parse(&["profiles", "import", "--force", "in.zip"]),
        Ok(Command::ProfilesImport { force: true, .. })
    ));
    assert!(matches!(parse(&["help"]), Ok(Command::Help)));
    assert!(matches!(parse(&["version"]), Ok(Command::Version)));
    assert!(parse(&["support-gen"]).is_err());
}

#[test]
fn help_pages_name_every_command_and_no_arguments_shows_usage() {
    let page = |words: &[&str]| parse(words).unwrap_err().unwrap_stdout();
    let top = page(&["--help"]);
    for command in [
        "convert", "diagnose", "fix", "help", "inspect", "profiles", "slice", "version",
    ] {
        assert!(top.contains(&format!("\n    {command} ")), "{command}\n{top}");
    }
    assert!(top.starts_with("Sleptic Slicer"), "{top}");
    assert_eq!(page(&[]), top, "no arguments prints the same page");
    assert_eq!(page(&["--version"]).trim(), format!("Version: {}", env!("CARGO_PKG_VERSION")));
    let slice = page(&["slice", "--help"]);
    for flag in ["--printer-profile", "--material-profile", "--supports", "--support-lift-mm"] {
        assert!(slice.contains(flag), "{flag}");
    }
}

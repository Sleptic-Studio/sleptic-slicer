// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use std::{
    fs,
    io::Read,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;
use zip::ZipArchive;

const PRINTER: &str = include_str!("data/rewrite/printer.toml");
const MATERIAL: &str = include_str!("data/rewrite/material.toml");
const FILES: [&str; 3] = [
    "printers/probe-printer.toml",
    "materials/probe-resin.toml",
    "materials/extensions/probe-resin.toml",
];

fn fixture() -> TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("printers")).unwrap();
    fs::create_dir_all(root.path().join("materials/extensions")).unwrap();
    fs::write(root.path().join(FILES[0]), PRINTER).unwrap();
    fs::write(root.path().join(FILES[1]), MATERIAL).unwrap();
    fs::write(
        root.path().join(FILES[2]),
        format!("extension = true\n{MATERIAL}"),
    )
    .unwrap();
    root
}
fn call(root: &Path, args: &[&str]) -> Output {
    let executable = std::env::var_os("SLEPTIC_CLI_CONTRACT_BIN")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_sc-slicer-cli").into());
    Command::new(executable)
        .arg("profiles")
        .args(args)
        .env("SC_SLICER_LIBRARY_DIR", root)
        .env("SC_SLICER_ASSETS_DIR", root.join("bundled-not-used"))
        .output()
        .unwrap()
}
fn success(run: Output) -> String {
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(run.stderr.is_empty());
    String::from_utf8(run.stdout).unwrap()
}
fn contents(path: &Path) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut result = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let name = entry.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        result.push((name, bytes));
    }
    result
}

#[test]
fn listing_preserves_the_tabular_contract_and_user_only_scope() {
    let root = fixture();
    let printer = "probe-printer\tProbe Printer\t100 x 60 x 150 mm, 1000 x 600 px\n";
    let material = "probe-resin\tProbe Resin\tExample, standard\n";
    assert_eq!(
        success(call(root.path(), &["list"])),
        format!("{printer}{material}")
    );
    assert_eq!(success(call(root.path(), &["list", "printers"])), printer);
    assert_eq!(success(call(root.path(), &["list", "materials"])), material);
    assert_eq!(success(call(&root.path().join("absent"), &["list"])), "");
    fs::write(root.path().join(FILES[0]), b"malformed").unwrap();
    assert!(!call(root.path(), &["list"]).status.success());
}

#[test]
fn archive_selection_preserves_profile_bytes_and_extensions() {
    let root = fixture();
    let output = root.path().join("package.zip");
    let out = output.to_str().unwrap();
    for (flags, expected) in [
        (vec![], vec![FILES[0], FILES[1], FILES[2]]),
        (
            vec!["--printers", "--materials"],
            vec![FILES[0], FILES[1], FILES[2]],
        ),
        (vec!["--printers"], vec![FILES[0]]),
        (vec!["--materials"], vec![FILES[1], FILES[2]]),
        (
            vec!["--id", "probe-resin", "--id", "probe-resin"],
            vec![FILES[1], FILES[2]],
        ),
        (vec!["--id", "probe-printer"], vec![FILES[0]]),
    ] {
        let mut args = vec!["export"];
        args.extend(flags);
        args.push(out);
        assert!(success(call(root.path(), &args)).starts_with("Exported profiles to "));
        let entries = contents(&output);
        assert_eq!(
            entries
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            [vec!["manifest.toml"], expected.clone()].concat()
        );
        let manifest = String::from_utf8(entries[0].1.clone()).unwrap();
        let version = format!("sc_slicer_version = \"{}\"", env!("CARGO_PKG_VERSION"));
        assert!(manifest.contains(&version), "{manifest}");
        for (name, bytes) in entries.iter().skip(1) {
            assert_eq!(*bytes, fs::read(root.path().join(name)).unwrap());
        }
    }
}

#[test]
fn a_failed_export_preserves_an_existing_archive() {
    let root = fixture();
    let target = root.path().join("preserve.zip");
    let previous = b"previous archive must survive";
    fs::write(&target, previous).unwrap();
    for flags in [
        vec!["--id", "unknown"],
        vec!["--id", "probe-printer", "--materials"],
        vec!["--id", "INVALID"],
    ] {
        let mut args = vec!["export"];
        args.extend(flags);
        args.push(target.to_str().unwrap());
        assert!(!call(root.path(), &args).status.success());
        assert_eq!(fs::read(&target).unwrap(), previous);
    }
    assert_eq!(
        fs::read(root.path().join(FILES[0])).unwrap(),
        PRINTER.as_bytes()
    );
}

#[test]
fn collision_refusal_preserves_every_kind_and_force_replaces_them() {
    let source = fixture();
    let archive = source.path().join("all.zip");
    let input = archive.to_str().unwrap();
    success(call(source.path(), &["export", input]));
    let dest = tempfile::tempdir().unwrap();
    success(call(dest.path(), &["import", input]));
    for file in FILES {
        assert_eq!(
            fs::read(dest.path().join(file)).unwrap(),
            fs::read(source.path().join(file)).unwrap()
        );
    }
    for file in FILES {
        fs::write(dest.path().join(file), b"keep until force").unwrap();
    }
    let rejected = call(dest.path(), &["import", input]);
    assert!(!rejected.status.success());
    let message = String::from_utf8(rejected.stderr).unwrap();
    for heading in [
        "Printer conflicts:",
        "Material conflicts:",
        "Material extension conflicts:",
        "Use --force to overwrite.",
    ] {
        assert!(message.contains(heading), "{message}");
    }
    for file in FILES {
        assert_eq!(
            fs::read(dest.path().join(file)).unwrap(),
            b"keep until force"
        );
    }
    success(call(dest.path(), &["import", input, "--force"]));
    for file in FILES {
        assert_eq!(
            fs::read(dest.path().join(file)).unwrap(),
            fs::read(source.path().join(file)).unwrap()
        );
    }
}

#[test]
fn invalid_archives_and_input_errors_leave_the_library_intact() {
    let root = fixture();
    let broken = root.path().join("broken.zip");
    fs::write(&broken, b"not an archive").unwrap();
    assert!(
        !call(root.path(), &["import", broken.to_str().unwrap()])
            .status
            .success()
    );
    assert!(
        !call(
            root.path(),
            &["import", root.path().join("missing.zip").to_str().unwrap()]
        )
        .status
        .success()
    );
    for file in FILES {
        assert!(fs::read(root.path().join(file)).unwrap().len() > 10);
    }
    assert_eq!(
        fs::read(root.path().join(FILES[0])).unwrap(),
        PRINTER.as_bytes()
    );
}

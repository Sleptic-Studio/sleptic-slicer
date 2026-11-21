// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Opt-in real-model acceptance: SC_R6_MODELS_DIR points to the local STL folder.
use assert_cmd::Command;
use std::{path::PathBuf, time::Instant};

#[test]
#[ignore = "requires the three local STL files in SC_R6_MODELS_DIR"]
fn three_real_models_export_canonical_tolerant_gcode_zip() {
    let models = PathBuf::from(std::env::var_os("SC_R6_MODELS_DIR").expect("set SC_R6_MODELS_DIR"));
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().unwrap();
    for (i, name) in [
        "Skibidi (The Rock) Toilet.stl",
        "wall_clock_07.stl",
        "butterfly done repaired.stl",
    ]
    .iter()
    .enumerate()
    {
        let output = temp.path().join(format!("{i}.zip"));
        let start = Instant::now();
        let result = Command::cargo_bin("sc-slicer-cli")
            .unwrap()
            .args(["slice", "--source"])
            .arg(models.join(name))
            .arg("--target")
            .arg(&output)
            .args(["--target-format", "gcode-zip", "--printer-profile"])
            .arg(root.join("assets/profiles/printers/sleptic-diy-msla.toml"))
            .arg("--material-profile")
            .arg(root.join("assets/profiles/materials/standard-405.toml"))
            .assert()
            .success();
        let stderr = String::from_utf8_lossy(&result.get_output().stderr);
        assert!(stderr.contains("tolerant slicing used"), "{stderr}");
        assert!(stderr.contains("dropped fragments:"), "{stderr}");
        let archive = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
        assert!(archive.file_names().filter(|n| n.as_ref().is_ok_and(|n| n.ends_with(".png"))).count() > 100);
        println!("{name}: {:.3}s\n{stderr}", start.elapsed().as_secs_f64());
    }
}

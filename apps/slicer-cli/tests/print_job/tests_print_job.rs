// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

use super::*;
use std::io::Read;

#[test]
fn default_repair_slices_cube_with_benchy_like_collapsed_faces_and_strict_refuses() {
    let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
    mesh.faces.push(IndexedTriangle::from_raw_indices(
        Normal::new([0.0, 0.0, 1.0]),
        [0, 0, 1],
    ));
    mesh.faces.push(IndexedTriangle::from_raw_indices(
        Normal::new([0.0, 0.0, 1.0]),
        [1, 1, 1],
    ));
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("degenerate.stl");
    write_stla_to(&source, &mesh);
    let out = dir.path().join("cube.svg");
    for strict in [false, true] {
        let mut cmd = Command::cargo_bin(BIN).unwrap();
        cmd.env("SC_SLICER_ASSETS_DIR", profiles_dir()).args([
            "slice",
            "-s",
            source.to_str().unwrap(),
            "-t",
            out.to_str().unwrap(),
            "--printer-profile=slicer-cli-e2e-test",
            "--material-profile=slicer-cli-e2e-test-resin",
        ]);
        if strict {
            cmd.arg("--no-repair")
                .assert()
                .failure()
                .stderr(predicate::str::contains("ZeroLengthEdge"));
        } else {
            cmd.assert()
                .success()
                .stderr(predicate::str::contains(
                    "Repair: 2 degenerate faces removed",
                ))
                .stderr(predicate::str::contains("Applied fixes").not());
        }
    }
}

#[test]
fn sleptic_profile_cube_zip_round_trip_and_cli_summary() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("cube.stl");
    write_stla_to(&source, &sc_mesh_fixtures::unit_cube_mesh());
    let out = dir.path().join("cube.zip");
    let profiles = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/profiles");
    // Override the material via a path to test an actual faded schedule with the bundled printer.
    let material_path = dir.path().join("material.toml");
    let material = fs::read_to_string(profiles.join("materials/standard-405.toml"))
        .unwrap()
        .replace("transition_layers = 0", "transition_layers = 3")
        // Drop the bundled density so the fallback diagnostic stays covered.
        .replace(
            "density_g_ml = 1.1
",
            "",
        );
    fs::write(&material_path, material).unwrap();
    let result = Command::cargo_bin(BIN)
        .unwrap()
        .env("SC_SLICER_ASSETS_DIR", &profiles)
        .args([
            "slice",
            "-s",
            source.to_str().unwrap(),
            "-t",
            out.to_str().unwrap(),
            "--target-format=gcode-zip",
            "--printer-profile=sleptic-diy-msla",
            "--material-profile",
            material_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("20 layers; print time "))
        .stdout(predicate::str::contains(
            "volume 0.00 ml; weight 0.00 g; cost unpriced",
        ))
        .stdout(predicate::str::contains(
            "Exposure: bottom 5 at 30.000s, transition 3, normal 2.500s",
        ))
        .stderr(predicate::str::contains("using 1.1 g/ml"));
    assert!(result.get_output().status.success());
    let printer = sc_print_profiles::load_printer_profile_from_path(
        &profiles.join("printers/sleptic-diy-msla.toml"),
    )
    .unwrap();
    let material = sc_print_profiles::load_material_profile_from_path(&material_path).unwrap();
    let config = sc_print_profiles::resolve(
        &printer,
        &sc_print_profiles::EffectiveMaterial::from(material),
        &sc_print_profiles::SceneOverrides::default(),
    )
    .unwrap();
    let schedule =
        sc_print_job::ExposureSchedule::uniform(&config.material_resolved, 20, 0.05, 0.05);
    let mut zip = zip::ZipArchive::new(fs::File::open(&out).unwrap()).unwrap();
    assert_eq!(zip.len(), 22);
    let mut script = String::new();
    zip.by_name("run.gcode")
        .unwrap()
        .read_to_string(&mut script)
        .unwrap();
    assert_eq!(script.matches(";LAYER_START:").count(), 20);
    let zs: Vec<f64> = script
        .lines()
        .filter_map(|line| line.strip_prefix(";Z:"))
        .map(|s| s.parse().unwrap())
        .collect();
    assert!(zs.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(zs.last(), Some(&1.0));
    let exposure_ms: Vec<u32> = script
        .lines()
        .filter(|l| l.contains("exposure dwell"))
        .map(|l| l.split_whitespace().nth(1).unwrap()[1..].parse().unwrap())
        .collect();
    assert_eq!(
        exposure_ms,
        schedule
            .layers
            .iter()
            .map(|l| (l.exposure_s * 1000.0).round() as u32)
            .collect::<Vec<_>>()
    );
    for i in 1..=20 {
        let mut png = Vec::new();
        zip.by_name(&format!("{i}.png"))
            .unwrap()
            .read_to_end(&mut png)
            .unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(png))
            .read_info()
            .unwrap();
        assert_eq!((decoder.info().width, decoder.info().height), (3840, 2400));
        assert_eq!(decoder.info().color_type, png::ColorType::Grayscale);
    }
    let mut preview = Vec::new();
    zip.by_name("preview.png")
        .unwrap()
        .read_to_end(&mut preview)
        .unwrap();
    let decoder = png::Decoder::new(std::io::Cursor::new(preview))
        .read_info()
        .unwrap();
    assert_eq!((decoder.info().width, decoder.info().height), (256, 256));
    assert_eq!(decoder.info().color_type, png::ColorType::Rgb);
    let reader = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/validate_exports.py");
    let checked = std::process::Command::new("python")
        .arg(reader)
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
}

#[test]
fn islands_flag_reports_a_floating_block_once_and_nothing_without_it() {
    // A unit cube on the plate plus a copy floating 2 mm above it: the
    // copy's first layer touches nothing below, so it is one island.
    let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
    let base = mesh.vertices.len() as u32;
    let lifted: Vec<_> = mesh
        .vertices
        .iter()
        .map(|v| {
            let [x, y, z] = v.coords();
            Vertex::new([x, y, z + 3.0])
        })
        .collect();
    let faces: Vec<_> = mesh
        .faces
        .iter()
        .map(|f| IndexedTriangle::from_raw_indices(f.normal, f.vertices.map(|v| v.get() + base)))
        .collect();
    mesh.vertices.extend(lifted);
    mesh.faces.extend(faces);
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("floating.stl");
    write_stla_to(&source, &mesh);
    let out = dir.path().join("floating.svg");
    let run = |islands: bool| {
        let mut cmd = Command::cargo_bin(BIN).unwrap();
        cmd.env("SC_SLICER_ASSETS_DIR", profiles_dir()).args([
            "slice",
            "-s",
            source.to_str().unwrap(),
            "-t",
            out.to_str().unwrap(),
            "--printer-profile=slicer-cli-e2e-test",
            "--material-profile=slicer-cli-e2e-test-resin",
        ]);
        if islands {
            cmd.arg("--islands");
        }
        cmd.assert().success()
    };
    run(true).stdout(predicate::str::contains(
        "Islands: 1 unsupported region(s) need supports:",
    ));
    run(false).stdout(predicate::str::contains("Islands").not());
    // Supports were not requested, so no support count is printed either.
    run(true).stdout(predicate::str::contains("Supports:").not());
}

#[test]
fn risks_flag_prints_shared_report_and_validates_minimum() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("cube.stl");
    write_stla_to(&source, &sc_mesh_fixtures::unit_cube_mesh());
    let out = dir.path().join("cube.svg");
    Command::cargo_bin(BIN)
        .unwrap()
        .env("SC_SLICER_ASSETS_DIR", profiles_dir())
        .args([
            "slice",
            "-s",
            source.to_str().unwrap(),
            "-t",
            out.to_str().unwrap(),
            "--printer-profile=slicer-cli-e2e-test",
            "--material-profile=slicer-cli-e2e-test-resin",
            "--risks",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Print risks: 0 errors, 0 warnings",
        ));
    Command::cargo_bin(BIN)
        .unwrap()
        .args(["slice", "--minimum-feature-mm", "0"])
        .assert()
        .failure();
}

#[test]
fn off_plate_part_is_an_error_in_cli_report() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("offplate.stl");
    let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
    for v in &mut mesh.vertices {
        let mut p = v.coords();
        p[0] += 3.0;
        p[2] += 0.5;
        *v = Vertex::new(p);
    }
    write_stla_to(&source, &mesh);
    let out = dir.path().join("offplate.svg");
    Command::cargo_bin(BIN)
        .unwrap()
        .env("SC_SLICER_ASSETS_DIR", profiles_dir())
        .args([
            "slice",
            "-s",
            source.to_str().unwrap(),
            "-t",
            out.to_str().unwrap(),
            "--printer-profile=slicer-cli-e2e-test",
            "--material-profile=slicer-cli-e2e-test-resin",
            "--no-auto-place",
            "--risks",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Error: Outside build volume"));
}

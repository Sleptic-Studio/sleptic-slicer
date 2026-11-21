// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The mesh commands as a user runs them: convert, diagnose, fix, inspect,
//! plus help and version. Exit codes, what each stream says and what lands
//! on disk.
mod common;

use std::fs;
use std::path::Path;
use std::sync::Arc;

use common::*;
use predicates::prelude::*;
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_mesh_core::{IndexedMesh, Normal, Vertex};
use sc_scene::{LeafPayload, Scene};
use tempfile::TempDir;

fn run(args: &[&str]) -> assert_cmd::assert::Assert {
    cli().args(args).assert()
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// `inspect`'s object, triangle and vertex lines for a file.
fn counts(path: &Path) -> String {
    let out = run(&["inspect", "-s", s(path)]).success().get_output().stdout.clone();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .filter(|l| l.starts_with("Objects") || l.starts_with("Triangles") || l.starts_with("Bounding"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn version_and_help() {
    run(&["version"])
        .success()
        .stdout(format!("Version: {}\n", env!("CARGO_PKG_VERSION")));
    let help = run(&["help"]).success();
    let page = String::from_utf8_lossy(&help.get_output().stdout).to_string();
    for command in ["convert", "diagnose", "fix", "inspect", "profiles", "slice", "version"] {
        assert!(page.contains(command), "{command} missing from\n{page}");
    }
    // No arguments shows the usage, as help does.
    run(&[]).success().stdout(predicate::str::contains("Available commands:"));
    run(&["convert", "-t", "x.stl"])
        .failure()
        .stderr(predicate::str::contains("--source"));
    run(&["convert", "-s", "a.stl", "-t", "b.stl", "--source-format", "step"])
        .failure()
        .stderr(predicate::str::contains("step"));
}

#[test]
fn convert_keeps_the_geometry_through_every_writable_format() {
    let dir = TempDir::new().unwrap();
    let start = dir.path().join("start.stl");
    write_stla(&start, &cuboid([0., 0., 0.], [10., 20., 5.]));
    let before = counts(&start);
    let mut previous = start;
    for (name, format) in [
        ("b.stl", Some("stlb")),
        ("c.obj", None),
        ("d.3mf", None),
        ("e.txt", Some("stla")),
    ] {
        let next = dir.path().join(name);
        let mut args = vec!["convert", "-s", s(&previous), "-t", s(&next)];
        if let Some(format) = format {
            args.extend(["--target-format", format]);
        }
        run(&args).success();
        let mut inspect = vec!["inspect", "-s", s(&next)];
        if name.ends_with(".txt") {
            inspect.extend(["--source-format", "stla"]);
        }
        let out = run(&inspect).success().get_output().stdout.clone();
        let text = String::from_utf8(out).unwrap();
        for line in before.lines() {
            assert!(text.contains(line), "{name} lost {line:?}:\n{text}");
        }
        previous = next;
    }
    // Binary STL is exactly header, count and 50 bytes per triangle.
    assert_eq!(fs::metadata(dir.path().join("b.stl")).unwrap().len(), 84 + 50 * 12);
    let obj = fs::read_to_string(dir.path().join("c.obj")).unwrap();
    assert_eq!(obj.lines().filter(|l| l.starts_with("v ")).count(), 8);
    assert_eq!(obj.lines().filter(|l| l.starts_with("f ")).count(), 12);
    assert!(fs::read_to_string(dir.path().join("e.txt")).unwrap().starts_with("solid"));
}

#[test]
fn convert_reads_by_explicit_format_and_refuses_unknown_or_unreadable_input() {
    let dir = TempDir::new().unwrap();
    let odd = dir.path().join("part.dat");
    write_stla(&odd, &tetra());
    let out = dir.path().join("out.obj");
    run(&["convert", "-s", s(&odd), "-t", s(&out)]).failure();
    assert!(!out.exists());
    run(&["convert", "-s", s(&odd), "--source-format", "stla", "-t", s(&out)]).success();
    assert!(out.is_file());

    // Bytes that are not a mesh never reach the target, even for the same format.
    let junk = dir.path().join("junk.stl");
    fs::write(&junk, b"not a mesh at all").unwrap();
    let copy = dir.path().join("copy.stl");
    run(&["convert", "-s", s(&junk), "-t", s(&copy)])
        .failure()
        .stderr(panicked().not());
    assert!(!copy.exists());

    let target = dir.path().join("x.step");
    run(&["convert", "-s", s(&odd), "--source-format", "stla", "-t", s(&target)]).failure();
    run(&["convert", "-s", s(&dir.path().join("none.stl")), "-t", s(&copy)])
        .failure()
        .stderr(panicked().not());
}

#[test]
fn a_command_never_writes_onto_its_source() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("part.stl");
    write_stla(&source, &tetra());
    let original = fs::read(&source).unwrap();
    let mut spellings = vec![
        source.clone(),
        dir.path().join(".").join("part.stl"),
        dir.path().join("sub").join("..").join("part.stl"),
    ];
    fs::create_dir(dir.path().join("sub")).unwrap();
    let linked = dir.path().join("linked.stl");
    if fs::hard_link(&source, &linked).is_ok() {
        spellings.push(linked);
    }
    if cfg!(windows) {
        spellings.push(dir.path().join("PART.STL"));
    }
    for target in &spellings {
        for command in ["convert", "fix"] {
            run(&[command, "-s", s(&source), "-t", s(target)])
                .failure()
                .stderr(panicked().not());
        }
    }
    assert_eq!(fs::read(&source).unwrap(), original);

    // A folder, or a file in a folder that does not exist, is no target.
    run(&["convert", "-s", s(&source), "-t", s(dir.path())]).failure();
    let deep = dir.path().join("missing").join("out.stl");
    run(&["fix", "-s", s(&source), "-t", s(&deep)]).failure();
    assert!(!deep.parent().unwrap().exists());
}

#[test]
fn several_objects_merge_into_one_stl_with_a_warning() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("two.obj");
    let second = shifted(tetra(), [5., 0., 0.]);
    write_obj(&source, &[("left", &tetra()), ("right", &second)]);
    let target = dir.path().join("one.stl");
    run(&["convert", "-s", s(&source), "-t", s(&target)])
        .success()
        .stderr(predicate::str::contains("WARNING"));
    assert!(counts(&target).contains("Triangles: 8"));
}

fn diagnose(mesh_file: &Path) -> assert_cmd::assert::Assert {
    run(&["diagnose", "-s", s(mesh_file)])
}

fn write_text(dir: &Path, name: &str, text: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, text).unwrap();
    path
}

#[test]
fn diagnose_names_each_kind_of_problem() {
    let dir = TempDir::new().unwrap();
    let clean = dir.path().join("clean.stl");
    write_stlb(&clean, &tetra());
    diagnose(&clean).success().stderr("");

    let p = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [2., 0., 0.]];
    let cases: Vec<(&str, IndexedMesh, &str)> = vec![
        ("open.stl", mesh(&p, &[[0, 1, 2]]), "Manifold errors"),
        ("flipped.stl", with_flipped_face(tetra()), "Manifold errors"),
        (
            "fan.stl",
            mesh(
                &[[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., -1., 0.], [0., 0., 1.]],
                &[[0, 1, 2], [1, 0, 3], [0, 1, 4]],
            ),
            "Manifold errors",
        ),
        ("flat.stl", mesh(&p, &[[0, 1, 3]]), "Normal vector errors"),
    ];
    for (name, mesh, section) in cases {
        let path = dir.path().join(name);
        write_stla(&path, &mesh);
        diagnose(&path)
            .failure()
            .stderr(predicate::str::contains(section))
            .stderr(predicate::str::contains("Found problems in diagnosed mesh"));
    }

    // A stored normal that is not a unit vector, then one pointing inwards.
    let mut long = tetra();
    long.faces[0].normal = Normal::new([0.0, 0.0, 3.0]);
    let path = write_text(dir.path(), "long.stl", &stla_text(&long, "long"));
    diagnose(&path).failure().stderr(predicate::str::contains("Normal vector errors"));
    let mut inward = tetra();
    let [x, y, z] = inward.faces[0].normal.coords();
    inward.faces[0].normal = Normal::new([-x, -y, -z]);
    let path = write_text(dir.path(), "inward.stl", &stla_text(&inward, "inward"));
    diagnose(&path).failure().stderr(predicate::str::contains("Normal vector errors"));

    // A face that reuses one corner has a zero-length edge.
    let degenerate = mesh(&p, &[[0, 1, 2], [0, 0, 1]]);
    let path = dir.path().join("degenerate.obj");
    write_obj(&path, &[("bad", &degenerate)]);
    diagnose(&path).failure().stderr(predicate::str::contains("Face errors"));

    let junk = write_text(dir.path(), "junk.stl", "solid nothing useful");
    diagnose(&junk).failure().stderr(panicked().not());
    diagnose(&dir.path().join("none.stl")).failure().stderr(panicked().not());
    run(&["diagnose", "-s", s(&write_text(dir.path(), "t.dat", &stla_text(&tetra(), "t"))), "--source-format", "stla"]).success();
}

#[test]
fn diagnose_reports_each_object_of_a_file_in_order() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("two.obj");
    let open = mesh(&[[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]], &[[0, 1, 2]]);
    let fan_points = [[5., 0., 0.], [6., 0., 0.], [5., 1., 0.], [5., -1., 0.], [5., 0., 1.]];
    let fan = mesh(&fan_points, &[[0, 1, 2], [1, 0, 3], [0, 1, 4]]);
    write_obj(&path, &[("first", &open), ("second", &fan)]);
    let out = diagnose(&path).failure().get_output().stderr.clone();
    let text = String::from_utf8(out).unwrap();
    let sections: Vec<&str> = text.lines().filter(|l| l.ends_with("):")).collect();
    // The open triangle's three edges first, then the fan's shared edge
    // and its six open ones.
    assert_eq!(sections, ["Manifold errors (3):", "Manifold errors (7):"], "{text}");
    let fan = text.find("is shared by 3 faces").unwrap();
    assert!(fan > text.find("Manifold errors (7):").unwrap(), "{text}");
}

#[test]
fn fix_repairs_writes_and_reports_what_it_did() {
    let dir = TempDir::new().unwrap();
    let clean = dir.path().join("clean.stl");
    write_stla(&clean, &tetra());
    let out = dir.path().join("out.stl");
    run(&["fix", "-s", s(&clean), "-t", s(&out)])
        .success()
        .stderr("No fixes were applied.\n");
    assert!(counts(&out).contains("Triangles: 4"));

    let flipped = dir.path().join("flipped.stl");
    write_stla(&flipped, &with_flipped_face(tetra()));
    run(&["fix", "-s", s(&flipped), "-t", s(&out)])
        .success()
        .stderr(predicate::str::starts_with("Applied fixes (1):\n\t- Flipped orientation of face with ID"));
    diagnose(&out).success();

    // With its pass switched off the same file comes through unrepaired.
    run(&["fix", "-s", s(&flipped), "-t", s(&out), "--fix-flipped-faces=false"])
        .success()
        .stderr("No fixes were applied.\n");
    diagnose(&out).failure();

    let wrong = dir.path().join("x.unknown");
    run(&["fix", "-s", s(&flipped), "-t", s(&wrong)]).failure();
    assert!(!wrong.exists());
}

#[test]
fn a_refused_mesh_is_still_written_and_the_exit_code_says_so() {
    let dir = TempDir::new().unwrap();
    let p = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
    let bad = mesh(&p, &[[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3], [0, 0, 1]]);
    let single = dir.path().join("one.obj");
    write_obj(&single, &[("bad", &bad)]);
    let out = dir.path().join("out.obj");
    run(&["fix", "-s", s(&single), "-t", s(&out), "--fix-zero-length-edges=false"])
        .failure()
        .stderr(predicate::str::contains("warning: Unable to fix mesh. Face errors correction is not implemented (yet)."))
        .stderr(predicate::str::contains("1 of 1 meshes could not be fully repaired"));
    assert!(out.is_file());

    let pair = dir.path().join("two.obj");
    let good = shifted(with_flipped_face(tetra()), [5., 0., 0.]);
    write_obj(&pair, &[("bad", &bad), ("good", &good)]);
    run(&["fix", "-s", s(&pair), "-t", s(&out), "--fix-zero-length-edges=false"])
        .failure()
        .stderr(predicate::str::contains("warning (mesh 1 of 2): Unable to fix mesh."))
        .stderr(predicate::str::contains("Flipped orientation"))
        .stderr(predicate::str::contains("1 of 2 meshes could not be fully repaired"));
    // The repairable mesh was repaired in the written file.
    let text = String::from_utf8(diagnose(&out).failure().get_output().stderr.clone()).unwrap();
    assert!(!text.contains("Manifold errors"), "{text}");
}

#[test]
fn a_degenerate_face_is_removed_by_default() {
    let dir = TempDir::new().unwrap();
    let p = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
    let bad = mesh(&p, &[[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3], [0, 0, 1]]);
    let source = dir.path().join("bad.obj");
    write_obj(&source, &[("bad", &bad)]);
    let out = dir.path().join("out.stl");
    run(&["fix", "-s", s(&source), "-t", s(&out)])
        .success()
        .stderr(predicate::str::contains("(had zero-length edge)"));
    diagnose(&out).success();
}

// Inspect: kept from the round that made the command real (it once panicked
// on every call).

#[test]
fn inspect_clean_stla_does_not_panic_and_reports_geometry() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("clean.stla");
    write_stla(&source, &tetra());
    run(&["inspect", "-s", s(&source)])
        .success()
        .stderr(panicked().not())
        .stdout(predicate::str::contains("Objects: 1"))
        .stdout(predicate::str::contains("Triangles: 4"))
        .stdout(predicate::str::contains("Vertices: 4"));
}

#[test]
fn inspect_reports_the_bounding_box_and_format() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("part.stl");
    write_stlb(&source, &tetra());
    run(&["inspect", "-s", s(&source)])
        .success()
        .stdout(predicate::str::contains("Format: Stl\n"))
        .stdout(predicate::str::contains("Bounding box: 2.000 x 2.000 x 2.000 mm\n"))
        .stdout(predicate::str::contains("  min: (-1.000, -1.000, -1.000)  max: (1.000, 1.000, 1.000)\n"));
}

#[test]
fn inspect_rotated_non_box_3mf_reports_actual_mesh_bounds() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("tilted.3mf");
    let corner = mesh(
        &[[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        &[[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
    );
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_rotation_z(std::f64::consts::FRAC_PI_4)),
            LeafPayload::Mesh(Arc::new(corner)),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    sc_mesh_formats::threemf::write_threemf(&scene, &mut fs::File::create(&path).unwrap()).unwrap();
    run(&["inspect", "-s", s(&path)])
        .success()
        .stdout(predicate::str::contains("1.414 x 0.707 x 1.000 mm"));
}

#[test]
fn inspect_counts_every_object_and_takes_an_explicit_format() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("two.obj");
    write_obj(&source, &[("a", &tetra()), ("b", &shifted(tetra(), [4., 0., 0.]))]);
    run(&["inspect", "-s", s(&source)])
        .success()
        .stdout(predicate::str::contains("Objects: 2"))
        .stdout(predicate::str::contains("Triangles: 8"));
    let odd = dir.path().join("clean.dat");
    write_stla(&odd, &tetra());
    run(&["inspect", "-s", s(&odd), "--source-format", "stla"])
        .success()
        .stdout(predicate::str::contains("Format: StlAscii"));
}

#[test]
fn inspect_fails_cleanly_on_missing_or_unreadable_files() {
    let dir = TempDir::new().unwrap();
    run(&["inspect", "-s", s(&dir.path().join("missing.stl"))])
        .failure()
        .stderr(panicked().not());
    let junk = dir.path().join("garbage.stl");
    fs::write(&junk, b"This is not an STL file").unwrap();
    run(&["inspect", "-s", s(&junk)])
        .failure()
        .stderr(panicked().not());
    let _ = Vertex::new([0.0, 0.0, 0.0]);
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! `slice` as a user runs it, on the tests' own 5 x 5 mm printer: placement,
//! repair, validation reports, the layer grid, anti-aliasing, outputs and
//! the guards that keep a slice from overwriting its input.
mod common;

use std::fs;
use std::path::Path;
use std::sync::Arc;

use assert_cmd::Command;

use common::*;
use predicates::prelude::*;
use sc_mesh_core::geometry::compute_normal_from_triangle_vertices;
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_mesh_core::{IndexedMesh, IndexedTriangle, Normal, Vertex};
use sc_scene::{LeafPayload, Scene};
use tempfile::TempDir;

fn layers_of(source: &Path, dir: &Path, extra: &[&str]) -> usize {
    let target = dir.join(format!("out{}.svg", extra.join("_").replace(['-', '.'], "")));
    let out = slice(source, &target, extra).assert().success();
    sliced_layers(&out.get_output().stdout)
}

#[test]
fn a_mesh_is_set_on_the_plate_unless_told_not_to() {
    let dir = TempDir::new().unwrap();
    // Centred on the origin: half of it is below z = 0 as saved.
    let source = dir.path().join("centred.stl");
    write_stla(&source, &shifted(tetra(), [2.5, 2.5, 0.]));
    assert_eq!(layers_of(&source, dir.path(), &["--layer-height", "0.1"]), 20);
    assert_eq!(
        layers_of(&source, dir.path(), &["--layer-height", "0.1", "--no-auto-place"]),
        10
    );

    let sunk = dir.path().join("sunk.stl");
    write_stla(&sunk, &shifted(tetra(), [2.5, 2.5, -5.]));
    slice(&sunk, &dir.path().join("x.svg"), &["--no-auto-place"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "input scene has its top at z = -4 mm. The slicer slices upward from the build plate at z = 0; reposition your model(s) above z = 0 before slicing.",
        ));
}

#[test]
fn the_layer_grid_follows_the_height_flags() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("tet.stl");
    write_stlb(&source, &tetra());
    assert_eq!(layers_of(&source, dir.path(), &["--layer-height", "0.25"]), 8);
    assert_eq!(
        layers_of(
            &source,
            dir.path(),
            &["--layer-height", "0.25", "--first-layer-height", "0.5"]
        ),
        7
    );
    slice(&source, &dir.path().join("x.svg"), &["--layer-height", "0"])
        .assert()
        .failure();
}

#[test]
fn faulty_meshes_are_repaired_unless_refused() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("flipped.stl");
    write_stla(&source, &with_flipped_face(tetra()));
    slice(&source, &dir.path().join("a.svg"), &[])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Repair: 0 degenerate faces removed, 1 faces flipped.\n",
        ));
    slice(&source, &dir.path().join("b.svg"), &["-v"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Applied fixes (1):"));
    slice(&source, &dir.path().join("c.svg"), &["--no-repair"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("mesh validation failed; slicing aborted"));
    assert!(!dir.path().join("c.svg").exists());
}

fn open_box() -> IndexedMesh {
    let mut open = cuboid([1., 1., 0.], [3., 3., 2.]);
    open.faces.truncate(10);
    open
}

#[test]
fn a_validation_failure_summarises_and_lists_everything_only_when_verbose() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("parts.obj");
    write_obj(
        &source,
        &[("whole", &shifted(tetra(), [1., 1., 1.])), ("broken", &open_box())],
    );
    let short = slice(&source, &dir.path().join("a.svg"), &["--no-repair"])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let short = String::from_utf8(short).unwrap();
    assert!(short.starts_with("mesh validation failed for 1 of the scene's leaves:\n"), "{short}");
    assert!(short.contains("(\"broken\")"), "{short}");
    assert!(!short.contains("whole"), "only the broken part is named: {short}");
    assert!(short.contains(
        "\nRun `sc-slicer-cli fix` to repair the mesh, or re-run with --verbose for the full error list.\n"
    ));

    let long = slice(&source, &dir.path().join("b.svg"), &["--no-repair", "--verbose"])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let long = String::from_utf8(long).unwrap();
    let listed = long.lines().filter(|l| l.starts_with("  - ")).count();
    assert!(long.contains("(\"broken\"):\nBoundaryEdge (4):\n"), "{long}");
    assert!(listed >= 4, "every open edge is listed: {long}");
    assert!(long.contains("\nRun `sc-slicer-cli fix` to repair the mesh.\n"));
    assert!(!long.contains("re-run with --verbose"));
}

#[test]
fn every_object_of_a_file_is_sliced_and_keeps_its_place() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("two.obj");
    let small = |dx: f32| shifted(cuboid([0., 0., 0.], [1., 1., 1.]), [dx, 0., 0.]);
    write_obj(&source, &[("a", &small(0.5)), ("b", &small(3.0))]);
    slice(&source, &dir.path().join("two.svg"), &[])
        .assert()
        .success()
        .stdout(predicate::str::contains("Sliced 2 models: "));
    // Placement moves the group, not the parts within it.
    let layers = dir.path().join("layers");
    slice(&source, &layers, &["--target-format", "png-per-layer", "--aa", "off"])
    .assert()
    .success();
    let (width, _, pixels) = png_layers(&layers).remove(0);
    let lit_columns: Vec<usize> = (0..width as usize)
        .filter(|x| pixels.chunks(width as usize).any(|row| row[*x] > 0))
        .collect();
    let gap = lit_columns.windows(2).any(|w| w[1] - w[0] > 100);
    assert!(gap, "two separate parts, 1.5 mm apart");
    assert_eq!(lit_columns.len(), 200, "two 1 mm parts at 10 micron pixels");
}

#[test]
fn anti_aliasing_greys_only_the_edges() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("tet.stl");
    write_stlb(&source, &tetra());
    for (aa, expect_grey) in [("off", false), ("x4", true)] {
        let out = dir.path().join(aa);
        slice(&source, &out, &["--target-format", "png-per-layer", "--aa", aa])
            .assert()
            .success();
        let layers = png_layers(&out);
        assert!(!layers.is_empty());
        let grey = layers
            .iter()
            .flat_map(|(_, _, p)| p.iter())
            .any(|v| *v != 0 && *v != 255);
        assert_eq!(grey, expect_grey, "--aa {aa}");
        assert!(layers.iter().all(|(w, h, _)| (*w, *h) == (500, 500)));
    }
}

#[test]
fn slice_aa_numeric_and_named_spellings_match() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("tet.stl");
    write_stlb(&source, &tetra());
    for (numeric, named) in [("0", "off"), ("2", "x2"), ("4", "x4"), ("8", "x8")] {
        let pixels: Vec<_> = [numeric, named]
            .into_iter()
            .map(|spelling| {
                let out = dir.path().join(spelling);
                slice(&source, &out, &["--target-format", "png-per-layer", "--aa", spelling])
                    .assert()
                    .success();
                png_layers(&out)
            })
            .collect();
        assert_eq!(pixels[0], pixels[1], "AA aliases {numeric}/{named}");
    }
    slice(&source, &dir.path().join("invalid"), &["--aa", "3"])
        .assert()
        .failure();
}

#[test]
fn the_format_comes_from_the_flag_before_the_name() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("tet.stl");
    write_stlb(&source, &tetra());
    let package = dir.path().join("job.bin");
    slice(&source, &package, &["--target-format", "3mf"]).assert().success();
    let zip = zip::ZipArchive::new(fs::File::open(&package).unwrap()).unwrap();
    assert!(zip.file_names().any(|n| n.is_ok_and(|n| n.ends_with(".model"))), "a 3MF package");

    slice(&source, &dir.path().join("job.bin"), &[])
        .assert()
        .failure()
        .stderr(predicate::str::contains("could not infer target format"));
    let svg = dir.path().join("all.svg");
    slice(&source, &svg, &[]).assert().success();
    assert!(fs::read_to_string(svg).unwrap().contains("<svg"));
}

#[test]
fn a_slice_never_writes_onto_its_source() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("part.3mf");
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(None, Transform3D::Identity, LeafPayload::Mesh(Arc::new(tetra())))
        .unwrap();
    scene.add_root(id).unwrap();
    sc_mesh_formats::threemf::write_threemf(&scene, &mut fs::File::create(&source).unwrap())
        .unwrap();
    let original = fs::read(&source).unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    let mut targets = vec![
        (source.clone(), "3mf"),
        (dir.path().join("sub").join("..").join("part.3mf"), "svg-combined"),
    ];
    let linked = dir.path().join("linked.3mf");
    if fs::hard_link(&source, &linked).is_ok() {
        targets.push((linked, "3mf"));
    }
    let symlink = dir.path().join("sym.3mf");
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(&source, &symlink).is_ok();
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&source, &symlink).is_ok();
    if made {
        targets.push((symlink, "3mf"));
    }
    for (target, format) in targets {
        slice(&source, &target, &["--target-format", format])
            .assert()
            .failure()
            .stderr(panicked().not());
    }
    assert_eq!(fs::read(&source).unwrap(), original);

    // A per-layer format writes into a folder, which may already exist empty.
    let layers = dir.path().join("empty");
    fs::create_dir(&layers).unwrap();
    slice(&source, &layers, &["--target-format", "png-per-layer"])
        .assert()
        .success();
    assert!(!png_layers(&layers).is_empty());
}

#[test]
fn islands_are_reported_on_request() {
    let dir = TempDir::new().unwrap();
    // A bar held up by nothing: its first layer is an island.
    let source = dir.path().join("float.obj");
    let post = cuboid([0.0, 0.0, 0.], [0.5, 0.5, 1.0]);
    let floating = cuboid([2.0, 2.0, 0.5], [3.0, 3.0, 1.0]);
    write_obj(&source, &[("post", &post), ("float", &floating)]);
    slice(&source, &dir.path().join("a.svg"), &["--islands"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Islands: 1 unsupported region(s) need supports:\n  layer "));
    let solid = dir.path().join("post.stl");
    write_stla(&solid, &post);
    slice(&solid, &dir.path().join("b.svg"), &["--islands"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Islands: none (every region touches the layer below).\n",
        ));
}

#[test]
fn plate_selection_is_checked() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("tet.stl");
    write_stlb(&source, &tetra());
    for plate in ["0", "2"] {
        slice(&source, &dir.path().join("x.svg"), &["--plate", plate])
            .assert()
            .failure()
            .stderr(predicate::str::contains(format!(
                "plate index {plate} is outside 1..=1"
            )));
    }
    slice(&source, &dir.path().join("out"), &["--all-plates", "--target-format", "svg-combined"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--all-plates requires a project file"));
}

#[test]
fn all_plates_exports_one_file_per_plate_and_plate_selects_only_its_geometry() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("multi.3mf");
    let mut scene = Scene::new();
    let mut plates = scene.plates().clone();
    let second = plates.add();
    let third = plates.add();
    for (i, plate) in [1, second, third].into_iter().enumerate() {
        let id = scene
            .add_leaf(
                None,
                Transform3D::Affine(AffineTransform3D::from_translation([
                    i as f64 * 10.0,
                    0.0,
                    1.0,
                ])),
                LeafPayload::Mesh(Arc::new(tetra())),
            )
            .unwrap();
        scene.add_root(id).unwrap();
        plates.assignments.insert(id, plate);
    }
    scene.set_plates(plates);
    sc_project_formats::write_project(
        &scene,
        &sc_project_formats::ProjectPayload::default(),
        sc_project_formats::EmbedProfiles::No,
        None,
        None,
        sc_project_formats::ProjectFileType::ThreeMf,
        &mut fs::File::create(&source).unwrap(),
    )
    .unwrap();
    let output = tmp.path().join("jobs");
    slice(&source, &output, &["--all-plates", "--target-format", "svg-combined"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Plate 3/3"));
    assert_eq!(fs::read_dir(&output).unwrap().count(), 3);
    for i in 1..=3 {
        assert!(output.join(format!("multi_plate{i}.svg")).is_file());
    }
    let one = tmp.path().join("one.svg");
    slice(&source, &one, &["--plate", "2", "--target-format", "svg-combined"])
        .assert()
        .success();
    assert_eq!(
        fs::read(one).unwrap(),
        fs::read(output.join("multi_plate2.svg")).unwrap()
    );
    slice(&source, &output, &["--all-plates", "--plate", "2", "--target-format", "svg-combined"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--plate and --all-plates cannot be combined"));
}

mod orientation_audit_tests {
    use super::*;

    /// Closed extruded L with a notch: +X foot, longer +Y stem.
    fn notched_l(centered: bool) -> IndexedMesh {
        let xy = [[0., 0.], [3., 0.], [3., 1.], [1., 1.], [1., 4.], [0., 4.]];
        let mut vertices = Vec::new();
        for z in [0., 0.5] {
            for [x, y] in xy {
                vertices.push(Vertex::new([
                    x - if centered { 1.5 } else { 0. },
                    y - if centered { 2. } else { 0. },
                    z,
                ]));
            }
        }
        let mut indices = Vec::new();
        for [a, b, c] in [[0, 1, 3], [1, 2, 3], [0, 3, 5], [3, 4, 5]] {
            indices.push([c, b, a]);
            indices.push([a + 6, b + 6, c + 6]);
        }
        for a in 0..6 {
            let b = (a + 1) % 6;
            indices.push([a, b, b + 6]);
            indices.push([a, b + 6, a + 6]);
        }
        let faces = indices
            .into_iter()
            .map(|tri| {
                let normal =
                    compute_normal_from_triangle_vertices(&tri.map(|i| vertices[i as usize]))
                        .unwrap();
                IndexedTriangle::from_raw_indices(normal, tri)
            })
            .collect();
        IndexedMesh {
            meta: None,
            vertices,
            faces,
        }
    }

    fn assert_orientation(mx: bool, my: bool) {
        let tmp = TempDir::new().unwrap();
        let profile_path = fs::read_dir(profiles_dir().join("printers"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let profile_source = fs::read_to_string(profile_path).unwrap();
        let profile = tmp.path().join("printer.toml");
        fs::write(
            &profile,
            profile_source
                .replace("x = 500, y = 500", "x = 50, y = 50")
                .replace("x = 10.0, y = 10.0", "x = 100.0, y = 100.0")
                .replace("x = false, y = false", &format!("x = {mx}, y = {my}")),
        )
        .unwrap();
        for auto_place in [false, true] {
            let source = tmp.path().join("l.stl");
            write_stla(&source, &notched_l(!auto_place));
            let output = tmp.path().join(format!("layers-{auto_place}"));
            let mut cmd = cli();
            cmd.args(["slice", "-s"])
                .arg(&source)
                .arg("-t")
                .arg(&output)
                .args(["--target-format", "png-per-layer", "--printer-profile"])
                .arg(&profile)
                .args([
                    "--material-profile",
                    "slicer-cli-e2e-test-resin",
                    "--aa",
                    "off",
                    "--layer-height",
                    "0.25",
                ]);
            if !auto_place {
                cmd.arg("--no-auto-place");
            }
            cmd.assert().success();
            let layers = png_layers(&output);
            assert_eq!(layers.len(), 2);
            for (width, height, pixels) in layers {
                assert_eq!((width, height), (50, 50));
                for y in 0..50 {
                    for x in 0..50 {
                        let ux = if mx { 49 - x } else { x };
                        let uy = if my { y } else { 49 - y };
                        let lit =
                            (10..40).contains(&ux) && (5..45).contains(&uy) && (ux < 20 || uy < 15);
                        assert_eq!(
                            pixels[y * 50 + x],
                            if lit { 255 } else { 0 },
                            "mirror=({mx},{my}), auto={auto_place}, pixel=({x},{y})"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn png_l_no_mirrors() {
        assert_orientation(false, false);
    }
    #[test]
    fn png_l_mirror_x() {
        assert_orientation(true, false);
    }
    #[test]
    fn png_l_mirror_y() {
        assert_orientation(false, true);
    }
    #[test]
    fn png_l_mirror_both() {
        assert_orientation(true, true);
    }
}

#[path = "print_job/tests_print_job.rs"]
mod tests_print_job;

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! What `slice` writes, checked in the files themselves: layer counts,
//! outlines and holes in every geometry format, pixels in PNG layers, which
//! profiles resolve, resin extensions, and project files.
//!
//! Test parts: a 1 mm cube, and 2 mm cubes with a 0.8 mm square tunnel along
//! Z or along Y (`sc-mesh-fixtures`), on the tests' 5 x 5 mm printer at
//! 10 micron pixels and 0.2 mm layers.
mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::*;
use predicates::prelude::*;
use sc_mesh_core::IndexedMesh;
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_scene::{LeafPayload, Scene};
use tempfile::TempDir;

fn part(dir: &Path, name: &str, mesh: &IndexedMesh) -> PathBuf {
    let path = dir.join(format!("{name}.stl"));
    write_stlb(&path, mesh);
    path
}

fn cube(dir: &Path) -> PathBuf {
    part(dir, "cube", &sc_mesh_fixtures::unit_cube_mesh())
}

fn z_tunnel(dir: &Path) -> PathBuf {
    part(dir, "z_tunnel", &sc_mesh_fixtures::cuboid_with_z_tunnel())
}

fn y_tunnel(dir: &Path) -> PathBuf {
    part(dir, "y_tunnel", &sc_mesh_fixtures::cuboid_with_y_tunnel())
}

/// Each layer group of an SVG: its z and the outlines (subpaths) of each
/// polygon in it.
fn svg_layers(text: &str) -> Vec<(f64, Vec<usize>)> {
    text.split("<g id=\"layer-")
        .skip(1)
        .map(|group| {
            let group = &group[..group.find("</g>").unwrap()];
            let z = group.split("data-z-mm=\"").nth(1).unwrap();
            let z: f64 = z[..z.find('"').unwrap()].parse().unwrap();
            let polygons = group
                .split("<path d=\"")
                .skip(1)
                .map(|path| path[..path.find('"').unwrap()].matches('M').count())
                .collect();
            (z, polygons)
        })
        .collect()
}

fn per_layer_svgs(dir: &Path) -> Vec<(f64, Vec<usize>)> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    files
        .iter()
        .flat_map(|f| svg_layers(&fs::read_to_string(f).unwrap()))
        .collect()
}

/// The tunnel along Y runs from z = 0.6 to 1.4 mm: there each layer is two
/// bars, elsewhere one solid square.
fn y_tunnel_polygons(z: f64) -> Vec<usize> {
    if (0.6..1.4).contains(&z) { vec![1, 1] } else { vec![1] }
}

#[test]
fn every_svg_layer_has_the_outlines_and_holes_of_its_height() {
    let dir = TempDir::new().unwrap();
    for (source, layers, expect) in [
        (cube(dir.path()), 5, (|_| vec![1]) as fn(f64) -> Vec<usize>),
        (z_tunnel(dir.path()), 10, |_| vec![2]),
        (y_tunnel(dir.path()), 10, y_tunnel_polygons),
    ] {
        let folder = dir.path().join(source.file_stem().unwrap());
        let out = slice(&source, &folder, &["--target-format", "svg-per-layer"])
            .assert()
            .success();
        assert_eq!(sliced_layers(&out.get_output().stdout), layers);
        let found = per_layer_svgs(&folder);
        assert_eq!(found.len(), layers);
        assert_eq!(fs::read_dir(&folder).unwrap().count(), layers);
        for (at, (z, polygons)) in found.iter().enumerate() {
            assert!((z - (0.1 + 0.2 * at as f64)).abs() < 1e-9, "layer {at} at {z}");
            assert_eq!(*polygons, expect(*z), "{source:?} layer {at}");
        }

        // The combined file holds the same layers, in order.
        let combined = dir.path().join(format!("{}.svg", folder.display()));
        slice(&source, &combined, &[]).assert().success();
        assert_eq!(svg_layers(&fs::read_to_string(&combined).unwrap()), found);
    }
}

/// PNG pixel of a plate position in mm (plate centred on the origin, image
/// rows running down).
fn pixel(layer: &(u32, u32, Vec<u8>), x_mm: f64, y_mm: f64) -> u8 {
    let (width, _, pixels) = layer;
    let column = ((x_mm + 2.5) * 100.0) as usize;
    let row = ((2.5 - y_mm) * 100.0) as usize;
    pixels[row * *width as usize + column]
}

#[test]
fn png_layers_are_500_px_grey_images_with_their_holes_dark() {
    let dir = TempDir::new().unwrap();
    for (source, layers) in [(cube(dir.path()), 5), (z_tunnel(dir.path()), 10), (y_tunnel(dir.path()), 10)] {
        let folder = dir.path().join(source.file_stem().unwrap());
        slice(&source, &folder, &["--target-format", "png-per-layer", "--aa", "off"])
            .assert()
            .success();
        let images = png_layers(&folder);
        assert_eq!(images.len(), layers);
        for (at, image) in images.iter().enumerate() {
            assert_eq!((image.0, image.1, image.2.len()), (500, 500, 250_000));
            let z = 0.1 + 0.2 * at as f64;
            let name = source.file_stem().unwrap().to_str().unwrap();
            let (centre, solid) = match name {
                "cube" => (255, (0.3, 0.3)),
                "z_tunnel" => (0, (-0.7, 0.0)),
                _ => (if (0.6..1.4).contains(&z) { 0 } else { 255 }, (-0.7, 0.0)),
            };
            assert_eq!(pixel(image, 0.0, 0.0), centre, "{name} layer {at} centre");
            assert_eq!(pixel(image, solid.0, solid.1), 255, "{name} layer {at} wall");
            assert_eq!(pixel(image, 2.2, 2.2), 0, "{name} layer {at} outside");
        }
    }
}

fn profile(kind: &str, name: &str) -> PathBuf {
    profiles_dir().join(kind).join(format!("{name}.toml"))
}

fn slice_with(source: &Path, target: &Path, printer: &str, material: &str) -> assert_cmd::Command {
    let mut command = cli();
    command
        .arg("slice")
        .arg("-s")
        .arg(source)
        .arg("-t")
        .arg(target)
        .args(["--printer-profile", printer, "--material-profile", material]);
    command
}

#[test]
fn profiles_resolve_by_id_or_path_and_say_why_when_they_do_not() {
    let dir = TempDir::new().unwrap();
    let source = cube(dir.path());
    let by_id = dir.path().join("id.svg");
    slice(&source, &by_id, &[]).assert().success();
    let by_path = dir.path().join("path.svg");
    let printer = profile("printers", "slicer-cli-e2e-test");
    let material = profile("materials", "slicer-cli-e2e-test-resin");
    slice_with(&source, &by_path, printer.to_str().unwrap(), material.to_str().unwrap())
        .assert()
        .success();
    assert_eq!(fs::read(by_id).unwrap(), fs::read(by_path).unwrap());

    slice_with(&source, &dir.path().join("x.svg"), "slicer-cli-e2e-test", "no-such-resin")
        .assert()
        .failure()
        .stderr(predicate::str::starts_with("Error: could not load the material profile:"));
    slice_with(&source, &dir.path().join("x.svg"), "no-such-printer", "slicer-cli-e2e-test-resin")
        .assert()
        .failure()
        .stderr(predicate::str::starts_with("Error: could not load the printer profile:"));

    // A resin without any layer height cannot slice at all...
    let incomplete = "slicer-cli-e2e-test-resin-incomplete";
    slice_with(&source, &dir.path().join("x.svg"), "slicer-cli-e2e-test", incomplete)
        .assert()
        .failure()
        .stderr(predicate::str::contains("no layer height resolves for this printer and material"));
    // ...until one is given; geometry then exports without an estimate.
    let geometry = dir.path().join("geometry.svg");
    slice_with(&source, &geometry, "slicer-cli-e2e-test", incomplete)
        .args(["--layer-height", "0.2"])
        .assert()
        .success()
        .stderr(predicate::str::contains("warning: print estimate unavailable:"));
    assert_eq!(svg_layers(&fs::read_to_string(geometry).unwrap()).len(), 5);
}

/// A user library holding the extendable resin's extension, which adds the
/// calibration for the test printer.
fn library_with_extension(text: &str) -> TempDir {
    let library = TempDir::new().unwrap();
    let folder = library.path().join("materials").join("extensions");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("slicer-cli-e2e-test-resin-extendable.toml"), text).unwrap();
    library
}

#[test]
fn a_resin_extension_completes_the_resin_by_id_or_by_path() {
    let dir = TempDir::new().unwrap();
    let source = z_tunnel(dir.path());
    let resin = "slicer-cli-e2e-test-resin-extendable";
    let job = dir.path().join("job.zip");
    // Without its extension the resin has no exposures, so no G-code job.
    slice_with(&source, &job, "slicer-cli-e2e-test", resin)
        .env("SC_SLICER_LIBRARY_DIR", dir.path().join("empty"))
        .assert()
        .failure();

    let extension = fs::read_to_string(profiles_dir().join("extensions").join(format!("{resin}.toml"))).unwrap();
    let library = library_with_extension(&extension);
    let by_path = profile("materials", resin);
    let mut jobs = Vec::new();
    for (name, material) in [("a.zip", resin), ("b.zip", by_path.to_str().unwrap())] {
        let target = dir.path().join(name);
        let out = slice_with(&source, &target, "slicer-cli-e2e-test", material)
            .env("SC_SLICER_LIBRARY_DIR", library.path())
            .assert()
            .success();
        assert_eq!(sliced_layers(&out.get_output().stdout), 10);
        jobs.push(fs::read(target).unwrap());
    }

    // The extension adds exposures only: the geometry matches the fully
    // calibrated resin's.
    let reference = dir.path().join("reference.svg");
    slice(&source, &reference, &[]).assert().success();
    let extended = dir.path().join("extended.svg");
    slice_with(&source, &extended, "slicer-cli-e2e-test", resin)
        .env("SC_SLICER_LIBRARY_DIR", library.path())
        .assert()
        .success();
    assert_eq!(
        svg_layers(&fs::read_to_string(reference).unwrap()),
        svg_layers(&fs::read_to_string(extended).unwrap())
    );
}

#[test]
fn a_broken_extension_is_reported_and_the_slice_goes_on() {
    let dir = TempDir::new().unwrap();
    let source = cube(dir.path());
    let library = library_with_extension("extension = true\n[identity]\nid = 7\n");
    slice_with(&source, &dir.path().join("x.svg"), "slicer-cli-e2e-test", "slicer-cli-e2e-test-resin-extendable")
        .env("SC_SLICER_LIBRARY_DIR", library.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("warning: "));
}

/// The 1 mm cube floating 1 mm above the plate, as one part of a scene.
fn lifted_cube() -> Scene {
    let mut scene = Scene::new();
    let lift = Transform3D::Affine(AffineTransform3D::from_translation([0.0, 0.0, 1.0]));
    let id = scene
        .add_leaf(None, lift, LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())))
        .unwrap();
    scene.add_root(id).unwrap();
    scene
}

fn write_project(path: &Path, scene: &Scene) {
    sc_project_formats::write_project(
        scene,
        &sc_project_formats::ProjectPayload::default(),
        sc_project_formats::EmbedProfiles::No,
        None,
        None,
        sc_project_formats::ProjectFileType::ThreeMf,
        &mut fs::File::create(path).unwrap(),
    )
    .unwrap();
}

#[test]
fn a_project_is_sliced_where_it_was_saved_and_a_plain_3mf_is_placed() {
    let dir = TempDir::new().unwrap();
    let project = dir.path().join("project.3mf");
    write_project(&project, &lifted_cube());
    let plain = dir.path().join("plain.3mf");
    sc_mesh_formats::threemf::write_threemf(&lifted_cube(), &mut fs::File::create(&plain).unwrap())
        .unwrap();

    let layers = |source: &Path, extra: &[&str]| {
        let target = dir.path().join("out.svg");
        let out = slice(source, &target, extra).assert().success();
        let found = svg_layers(&fs::read_to_string(target).unwrap());
        let empty = found.iter().filter(|(_, p)| p.is_empty()).count();
        (sliced_layers(&out.get_output().stdout), empty)
    };
    // Where the lifted cube really is: layers run up to its top, and those
    // below its bottom are empty.
    let z: Vec<f32> = sc_mesh_fixtures::unit_cube_mesh()
        .vertices
        .iter()
        .map(|v| v.coords()[2] + 1.0)
        .collect();
    let (bottom, top) = (z.iter().copied().fold(f32::MAX, f32::min), z.iter().copied().fold(f32::MIN, f32::max));
    // Each layer is cut at its middle; a cut exactly on a face finds nothing.
    let count = (top / 0.2).ceil() as usize;
    let empty = (0..count)
        .map(|k| 0.1 + 0.2 * k as f32)
        .filter(|z| !(*z > bottom + 1e-4 && *z < top - 1e-4))
        .count();
    let saved = (count, empty);
    assert!(bottom > 0.2, "the test needs the cube clear of the plate");
    assert_eq!(layers(&project, &[]), saved);
    assert_eq!(layers(&project, &["--no-auto-place"]), saved);
    // A plain 3MF is set down on the plate like any mesh.
    assert_eq!(layers(&plain, &[]), (5, 0));
    assert_eq!(layers(&plain, &["--no-auto-place"]), saved);
}

#[test]
fn a_project_prints_no_warnings_of_its_own() {
    let dir = TempDir::new().unwrap();
    let project = dir.path().join("project.3mf");
    write_project(&project, &lifted_cube());
    // The only line is the test resin's missing density.
    slice(&project, &dir.path().join("out.svg"), &[])
        .assert()
        .success()
        .stderr("warning: Material density unspecified; using 1.1 g/ml.\n");
}

#[test]
fn automatic_supports_slice_with_islands_and_report_count() {
    let dir = TempDir::new().unwrap();
    let source = cube(dir.path());
    let output = slice(
        &source,
        &dir.path().join("supported.svg"),
        &[
            "--supports",
            "auto",
            "--islands",
            "--support-spacing-mm",
            "2",
            "--support-no-raft",
        ],
    )
    .assert()
    .success()
    .stdout(predicate::str::contains("Islands: none"))
    .stdout(predicate::str::contains("Supports: 0").not())
    .get_output()
    .stdout
    .clone();
    assert!(String::from_utf8(output).unwrap().contains("Supports:"));
}

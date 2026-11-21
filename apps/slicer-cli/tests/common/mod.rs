// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! What the command-line tests share: the binary, the test profiles and
//! small meshes written straight to disk in a chosen format.
#![allow(dead_code)]

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use sc_mesh_core::geometry::compute_normal_from_triangle_vertices;
use sc_mesh_core::{IndexedMesh, IndexedTriangle, Normal, Vertex};

pub(crate) const BIN: &str = "sc-slicer-cli";

/// The tests' own printer and resins (no bundled profile is needed).
pub(crate) fn profiles_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/profiles")
}

pub(crate) fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

/// The binary, reading profiles from the test folder only.
pub(crate) fn cli() -> Command {
    let mut command = Command::cargo_bin(BIN).unwrap();
    command.env("SC_SLICER_ASSETS_DIR", profiles_dir());
    command
}

/// `cli slice -s source -t target` with the test printer and resin.
pub(crate) fn slice(source: &Path, target: &Path, extra: &[&str]) -> Command {
    let mut command = cli();
    command
        .arg("slice")
        .arg("-s")
        .arg(source)
        .arg("-t")
        .arg(target)
        .args([
            "--printer-profile",
            "slicer-cli-e2e-test",
            "--material-profile",
            "slicer-cli-e2e-test-resin",
        ])
        .args(extra);
    command
}

/// Fails on the text a Rust panic leaves on stderr.
pub(crate) fn panicked() -> impl Predicate<str> {
    predicate::str::contains("panicked at").or(predicate::str::contains("RUST_BACKTRACE"))
}

/// A mesh from corner points and triangles, normals from the winding.
pub(crate) fn mesh(points: &[[f32; 3]], triangles: &[[u32; 3]]) -> IndexedMesh {
    let vertices: Vec<Vertex> = points.iter().map(|p| Vertex::new(*p)).collect();
    let faces = triangles
        .iter()
        .map(|t| {
            let corners = t.map(|i| vertices[i as usize]);
            let normal = compute_normal_from_triangle_vertices(&corners)
                .unwrap_or_else(|_| Normal::new([0.0, 0.0, 1.0]));
            IndexedTriangle::from_raw_indices(normal, *t)
        })
        .collect();
    IndexedMesh {
        meta: None,
        vertices,
        faces,
    }
}

/// A closed tetrahedron filling the cube [-1, 1]³ diagonally.
pub(crate) fn tetra() -> IndexedMesh {
    mesh(
        &[[1., 1., 1.], [1., -1., -1.], [-1., 1., -1.], [-1., -1., 1.]],
        &[[0, 1, 2], [0, 3, 1], [0, 2, 3], [1, 3, 2]],
    )
}

pub(crate) fn shifted(mut mesh: IndexedMesh, [dx, dy, dz]: [f32; 3]) -> IndexedMesh {
    for v in &mut mesh.vertices {
        let [x, y, z] = v.coords();
        *v = Vertex::new([x + dx, y + dy, z + dz]);
    }
    mesh
}

/// A closed box from `min` to `max`, faces wound outwards.
pub(crate) fn cuboid(min: [f32; 3], max: [f32; 3]) -> IndexedMesh {
    let corner = |i: usize| {
        [
            if i & 1 == 0 { min[0] } else { max[0] },
            if i & 2 == 0 { min[1] } else { max[1] },
            if i & 4 == 0 { min[2] } else { max[2] },
        ]
    };
    let points: Vec<[f32; 3]> = (0..8).map(corner).collect();
    let quads = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    let triangles: Vec<[u32; 3]> = quads
        .iter()
        .flat_map(|[a, b, c, d]| [[*a, *b, *c], [*a, *c, *d]])
        .collect();
    mesh(&points, &triangles)
}

/// One face turned the wrong way.
pub(crate) fn with_flipped_face(mut mesh: IndexedMesh) -> IndexedMesh {
    let [a, b, c] = mesh.faces[0].vertices;
    let corners = [c, b, a].map(|i| mesh.vertices[usize::from(i)]);
    mesh.faces[0] = IndexedTriangle {
        normal: compute_normal_from_triangle_vertices(&corners).unwrap(),
        vertices: [c, b, a],
    };
    mesh
}

/// ASCII STL with the normals as stored (so a test can store a wrong one).
pub(crate) fn stla_text(mesh: &IndexedMesh, name: &str) -> String {
    let mut text = format!("solid {name}\n");
    for face in &mesh.faces {
        let [nx, ny, nz] = face.normal.coords();
        let _ = writeln!(text, "facet normal {nx:e} {ny:e} {nz:e}\nouter loop");
        for i in face.vertices {
            let [x, y, z] = mesh.vertices[usize::from(i)].coords();
            let _ = writeln!(text, "vertex {x:e} {y:e} {z:e}");
        }
        text += "endloop\nendfacet\n";
    }
    text + &format!("endsolid {name}\n")
}

pub(crate) fn write_stla(path: &Path, mesh: &IndexedMesh) {
    fs::write(path, stla_text(mesh, "part")).unwrap();
}

/// The name the print-job tests use.
pub(crate) fn write_stla_to(path: &Path, mesh: &IndexedMesh) {
    write_stla(path, mesh);
}

/// Binary STL: 80-byte header, count, then 50 bytes per triangle.
pub(crate) fn write_stlb(path: &Path, mesh: &IndexedMesh) {
    let mut bytes = vec![0u8; 80];
    bytes.extend((mesh.faces.len() as u32).to_le_bytes());
    for face in &mesh.faces {
        let mut floats = face.normal.coords().to_vec();
        for i in face.vertices {
            floats.extend(mesh.vertices[usize::from(i)].coords());
        }
        for value in floats {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend([0, 0]);
    }
    fs::write(path, bytes).unwrap();
}

/// OBJ with one `o` block per named mesh; indices run on across objects.
pub(crate) fn write_obj(path: &Path, objects: &[(&str, &IndexedMesh)]) {
    let mut text = String::new();
    let mut base = 1;
    for (name, mesh) in objects {
        let _ = writeln!(text, "o {name}");
        for v in &mesh.vertices {
            let [x, y, z] = v.coords();
            let _ = writeln!(text, "v {x} {y} {z}");
        }
        for face in &mesh.faces {
            let [a, b, c] = face.vertices.map(|i| usize::from(i) + base);
            let _ = writeln!(text, "f {a} {b} {c}");
        }
        base += mesh.vertices.len();
    }
    fs::write(path, text).unwrap();
}

/// The `N` of the final "Sliced … N layers" line.
pub(crate) fn sliced_layers(stdout: &[u8]) -> usize {
    let text = String::from_utf8_lossy(stdout);
    let line = text
        .lines()
        .find(|line| line.starts_with("Sliced "))
        .unwrap_or_else(|| panic!("no summary in {text}"));
    let words: Vec<&str> = line.split_whitespace().collect();
    let at = words.iter().position(|w| *w == "layers").unwrap();
    words[at - 1].parse().unwrap()
}

/// Every PNG in a folder, sorted by name, as 8-bit grey pixels.
pub(crate) fn png_layers(dir: &Path) -> Vec<(u32, u32, Vec<u8>)> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(path).unwrap()));
            let mut reader = decoder.read_info().unwrap();
            let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
            let info = reader.next_frame(&mut pixels).unwrap();
            pixels.truncate(info.buffer_size());
            (info.width, info.height, pixels)
        })
        .collect()
}

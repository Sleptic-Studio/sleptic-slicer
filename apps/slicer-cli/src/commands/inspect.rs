// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! `inspect`: what a file holds (format, objects, sizes), without judging
//! the geometry. It fails only when the file cannot be read; `diagnose` is
//! the command that looks for problems.
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use sc_mesh_formats::filetypes::{MeshFileType, infer_mesh_file_format_from_extension};

use super::{mesh_files, report};
use crate::filesystem::{display_path, validate_sourcefile_path};

pub(crate) fn inspect(
    source: OsString,
    source_format: Option<MeshFileType>,
    out: &mut dyn Write,
) -> Result<()> {
    // Shown as typed: the checked path is canonical and on Windows carries
    // a verbatim prefix nobody wrote.
    let shown = PathBuf::from(&source);
    let path = validate_sourcefile_path(PathBuf::from(source))?;
    // The reader decides the format itself without saying which; the same
    // rule is applied here to name it.
    let format = match source_format {
        Some(format) => format,
        None => infer_mesh_file_format_from_extension(&path)?,
    };
    let mesh = mesh_files::read(&path, source_format)?;
    report::display_warnings(&mesh.warnings, out);

    let scene = &mesh.graph;
    let targets = scene.mesh_targets();
    let (mut triangles, mut vertices) = (0usize, 0usize);
    for mesh in targets.iter().filter_map(|target| scene.target_mesh(*target)) {
        triangles += mesh.faces.len();
        vertices += mesh.vertices.len();
    }
    let mut text = format!(
        "File: {}\nFormat: {format:?}\nObjects: {}\nTriangles: {triangles}\nVertices: {vertices}\n",
        display_path(&shown),
        targets.len()
    );
    text += &match scene.tight_aabb()? {
        Some(bounds) => {
            let [x0, y0, z0] = bounds.min().coords();
            let [x1, y1, z1] = bounds.max().coords();
            format!(
                "Bounding box: {:.3} x {:.3} x {:.3} mm\n  min: ({x0:.3}, {y0:.3}, {z0:.3})  max: ({x1:.3}, {y1:.3}, {z1:.3})\n",
                x1 - x0,
                y1 - y0,
                z1 - z0
            )
        }
        None => String::from("Bounding box: (no geometry)\n"),
    };
    let _ = out.write_all(text.as_bytes());
    Ok(())
}

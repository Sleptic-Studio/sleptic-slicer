// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! `convert`: a mesh file read in one format and written in another.
use std::ffi::OsString;

use anyhow::Result;
use sc_mesh_formats::filetypes::{MeshFileType, infer_mesh_file_format_from_extension};

use super::mesh_files::{Transfer, open_mesh};

/// Each end is a file and, optionally, the format to treat it as.
pub(crate) fn convert(input: (OsString, Option<MeshFileType>), output: (OsString, Option<MeshFileType>)) -> Result<()> {
    let ((source, read_as), (target, write_as)) = (input, output);
    let transfer = Transfer::plan(source, target, write_as)?;
    // The input format too is settled before reading, so a name that says
    // nothing fails early.
    let read_as = match read_as {
        Some(format) => format,
        None => infer_mesh_file_format_from_extension(&transfer.from)?,
    };
    // Even into the same format the mesh is read and written again, never
    // copied: that proves the input is a mesh and replaces the target whole.
    let scene = open_mesh(&transfer.from, Some(read_as), &mut std::io::stderr())?;
    transfer.write(&scene)
}

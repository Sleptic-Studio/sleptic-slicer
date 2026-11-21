// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! `diagnose`: what is wrong with each mesh in a file.
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Result, bail};
use sc_mesh_formats::filetypes::MeshFileType;

use super::mesh_files::open_mesh;
use super::report;
use crate::filesystem::validate_sourcefile_path;

/// Prints every mesh's diagnosis, then fails if any has a problem, so the
/// exit code alone tells a script whether the file is clean.
pub(crate) fn diagnose(source: OsString, source_format: Option<MeshFileType>, notes: &mut dyn Write) -> Result<()> {
    let path = validate_sourcefile_path(PathBuf::from(source))?;
    let scene = open_mesh(&path, source_format, notes)?;
    let problems = scene
        .diagnose_meshes()
        .into_iter()
        .inspect(|(_, found)| report::show_diagnosis(found, notes))
        .filter(|(_, found)| found.has_problems())
        .count();
    if problems > 0 {
        bail!("Found problems in diagnosed mesh");
    }
    Ok(())
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! What the mesh commands (convert, fix, diagnose, inspect) share: reading a
//! mesh file and reporting what the reader noticed, and, for the commands
//! that write one, checking both ends before any work starts.
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use sc_mesh_formats::filetypes::{MeshFileType, WriteFormat, infer_mesh_file_format_from_extension};
use sc_mesh_formats::{DEFAULT_MAX_MESH_FILE_SIZE_BYTES, LoadedMesh, load_mesh_from_path, scene_writer};
use sc_scene::Scene;

use super::report;
use crate::filesystem::{ensure_distinct_files, validate_sourcefile_path, validate_targetfile_path};

/// Reads a mesh under the standard size limit; with no format given the
/// reader works it out.
pub(crate) fn read(path: &Path, format: Option<MeshFileType>) -> Result<LoadedMesh> {
    Ok(load_mesh_from_path(path, format, Some(DEFAULT_MAX_MESH_FILE_SIZE_BYTES))?)
}

/// Reads a mesh and prints the reader's warnings to `notes`.
pub(crate) fn open_mesh(path: &Path, format: Option<MeshFileType>, notes: &mut dyn Write) -> Result<Scene> {
    let loaded = read(path, format)?;
    report::display_warnings(&loaded.warnings, notes);
    Ok(loaded.graph)
}

/// A mesh file read from one place and written to another, with both
/// checked up front (each path usable, not the same file, a format known
/// for the output) so a bad name fails before anything is read.
pub(crate) struct Transfer {
    pub(crate) from: PathBuf,
    to: PathBuf,
    write_as: WriteFormat,
}

impl Transfer {
    pub(crate) fn plan(source: OsString, target: OsString, target_format: Option<MeshFileType>) -> Result<Transfer> {
        let from = validate_sourcefile_path(PathBuf::from(source))?;
        let to = validate_targetfile_path(PathBuf::from(target))?;
        ensure_distinct_files(&from, &to)?;
        let kind = match target_format {
            Some(kind) => kind,
            None => infer_mesh_file_format_from_extension(&to)?,
        };
        Ok(Transfer { from, to, write_as: WriteFormat::try_from(kind)? })
    }

    /// Writes `scene` to the target in one step (a failure never leaves half
    /// a file); what the format could not hold is reported on stderr.
    pub(crate) fn write(&self, scene: &Scene) -> Result<()> {
        for left_out in scene_writer::write_scene(scene, &self.to, self.write_as)? {
            eprintln!("WARNING: {}", left_out.user_message());
        }
        Ok(())
    }
}

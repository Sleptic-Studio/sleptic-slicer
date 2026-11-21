// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! `fix`: repair every mesh in a file and write the result.
use std::ffi::OsString;
use std::io::Write;

use anyhow::{Result, bail};
use sc_mesh_core::indexed_mesh::{MeshFix, RepairPasses};
use sc_mesh_formats::filetypes::MeshFileType;

use super::mesh_files::{Transfer, open_mesh};
use super::report;

pub(crate) fn fix(
    source: OsString,
    source_format: Option<MeshFileType>,
    target: OsString,
    target_format: Option<MeshFileType>,
    passes: RepairPasses,
    notes: &mut dyn Write,
) -> Result<()> {
    let transfer = Transfer::plan(source, target, target_format)?;
    let mut scene = open_mesh(&transfer.from, source_format, notes)?;
    let per_mesh = scene.repair_meshes(passes);
    let not_repaired = report::show_refusal_warnings(&per_mesh, notes);
    let done: Vec<MeshFix> = per_mesh.iter().flat_map(|mesh| mesh.fixes.iter().cloned()).collect();
    match done.is_empty() {
        true => {
            let _ = writeln!(notes, "No fixes were applied.");
        }
        false => report::show_applied_fixes(&done, notes),
    }
    // Written even when a mesh could not be repaired: that one is left as it
    // was and the others have fewer problems, so the output is never worse
    // than the input. The exit code still says so.
    transfer.write(&scene)?;
    if not_repaired > 0 {
        bail!("{not_repaired} of {} meshes could not be fully repaired", per_mesh.len());
    }
    Ok(())
}

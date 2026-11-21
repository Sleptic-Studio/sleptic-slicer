// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Export As: the suggested name and format, a copy of the scene taken on
//! the UI thread, and the write, which runs on a worker. The window drives
//! the dialog and the worker.
use std::path::Path;

use sc_mesh_formats::{filetypes::WriteFormat, scene_writer::WriteSceneProgress};
use sc_mesh_formats::scene_writer::{FlattenWarning, write_scene_with_progress};
use sc_scene::Scene;

use crate::{export_progress::ExportPhase, scene_document::SceneDocument};

/// The name to suggest: the opened file's, without its extension
/// (`gear.obj` gives `gear`).
pub(crate) fn suggested_export_name(document: &SceneDocument) -> Option<String> {
    let stem = document.read_file_path(|opened| opened.file_stem().and_then(|s| s.to_str()).map(String::from));
    stem.flatten()
}

/// The format to suggest: the opened file's when it is one we write
/// (`.stl`/`.stlb` as binary STL, `.stla`, `.obj`, `.3mf`), else binary
/// STL. The file dialog does not say which filter was picked, so this
/// extension is what tells the user the format; keeping the source's
/// matches "same format unless I change it".
pub(crate) fn suggested_export_suffix(document: &SceneDocument) -> &'static str {
    let opened_as = document
        .read_file_path(|opened| opened.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase))
        .flatten()
        .unwrap_or_default();
    ["obj", "stla", "3mf"].into_iter().find(|kept| *kept == opened_as).unwrap_or("stl")
}

/// A copy of the scene to write on a worker, so the document stays usable
/// meanwhile; cheap, as meshes are shared. `None` with no scene.
pub(crate) fn scene_copy_for_export(document: &SceneDocument) -> Option<Scene> {
    document.read_scene(|indexed| indexed.graph().clone())
}

/// Writes `scene` to `target` as `format`, reporting each phase, and
/// returns the warnings of merging (several parts written as one STL). It
/// does not touch the document, so it runs on a worker; the UI thread
/// shows what comes back.
pub(crate) fn write_exported_scene(
    copy: &Scene,
    dest: &Path,
    as_format: WriteFormat,
    mut report: impl FnMut(ExportPhase, u64, u64),
) -> anyhow::Result<Vec<FlattenWarning>> {
    let warnings = write_scene_with_progress(copy, dest, as_format, |step| {
        let (phase, done, total) = match step {
            WriteSceneProgress::Flatten { done, total } => (ExportPhase::Flatten, done, total),
            WriteSceneProgress::Serialize { done, total } => (ExportPhase::Serialize, done, total),
            WriteSceneProgress::Compress { done, total } => (ExportPhase::Compress, done, total),
            WriteSceneProgress::Write { done, total } => (ExportPhase::Write, done, total),
        };
        report(phase, done, total);
    })?;
    Ok(warnings)
}

/// A whole export here and now, for the tests; the window writes on a
/// worker. An error with no scene.
#[cfg(test)]
pub(crate) fn export_now(
    document: &SceneDocument,
    dest: &Path,
    as_format: WriteFormat,
) -> anyhow::Result<Vec<FlattenWarning>> {
    let Some(copy) = scene_copy_for_export(document) else {
        anyhow::bail!("No scene loaded");
    };
    write_exported_scene(&copy, dest, as_format, |_, _, _| ())
}

#[cfg(test)]
#[path = "tests_scene_export.rs"]
mod tests;

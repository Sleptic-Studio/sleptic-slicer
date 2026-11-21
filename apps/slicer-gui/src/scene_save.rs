// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Save and Save As. The window picks a file; this decides what saving the
//! document means and writes it. Every save is a 3MF project holding the
//! scene, the printer, resin and first-layer choices (an unchosen printer
//! or resin is filled from the app's defaults) and a copy of each profile
//! named: the ones the document carries already, the rest from the
//! installed folders.
use std::path::{Path, PathBuf};

use anyhow::Result;
use sc_mesh_formats::scene_writer::write_bytes_atomically;
use sc_print_primitives::{MaterialId, PrinterId};
use sc_print_profiles::ProfileLibraries;
use sc_project_formats::{EmbedProfiles, PlateSlices, ProjectFileType, ProjectPayload, WriteProgress, write_project_with_progress};
use sc_scene::{CsgPolicy, ReferencePolicy, Scene, plates::Plates};

use crate::project_profiles::with_embedded_profiles;
use crate::save_progress::WriteStep;
use crate::scene_document::{SavedAt, SceneDocument};
use crate::slice_projection::layers_for_choice;
use crate::slice_result::SliceResult;

/// What "Save" does with the document as it stands.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SaveAction {
    /// A project with changes: written over its own file, no dialog.
    WriteOver(PathBuf),
    /// Opened from a mesh (or a 3MF without project data): ask where, so an
    /// STL, OBJ or another slicer's file is never replaced unasked.
    AskWhere,
    /// The project file holds the document already.
    UpToDate,
    /// Nothing open (the menu item is off; this covers a callback that
    /// arrives while the scene closes).
    NothingOpen,
}

impl SaveAction {
    pub(crate) fn for_document(document: &SceneDocument) -> SaveAction {
        match (document.has_scene(), document.opened_from_project(), document.has_unsaved_changes()) {
            (false, _, _) => SaveAction::NothingOpen,
            (true, false, _) => SaveAction::AskWhere,
            (true, true, false) => SaveAction::UpToDate,
            (true, true, true) => SaveAction::WriteOver(
                document.read_file_path(Path::to_path_buf).expect("a loaded document always has a source path"),
            ),
        }
    }
}

/// What the Save As dialog's answer means.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SaveAsAnswer {
    Dismissed,
    /// The name has another format's extension, so a project there would
    /// not be what the name promises.
    NotAProjectName(PathBuf),
    /// Where to write; a name without an extension gets `.3mf`.
    WriteTo(PathBuf),
}

impl SaveAsAnswer {
    pub(crate) fn from_dialog(picked: Option<PathBuf>) -> SaveAsAnswer {
        let Some(path) = picked else { return SaveAsAnswer::Dismissed };
        let is_3mf = path.extension().map(|ext| ext.eq_ignore_ascii_case("3mf"));
        match is_3mf {
            None => SaveAsAnswer::WriteTo(path.with_extension("3mf")),
            Some(true) => SaveAsAnswer::WriteTo(path),
            Some(false) => SaveAsAnswer::NotAProjectName(path),
        }
    }
}

/// The document's project data with the defaults for an unchosen printer
/// or resin, and a copy of every profile it names that it does not carry
/// yet, taken from `installed`.
pub(crate) fn project_data_for_save(
    document: &SceneDocument,
    fallback_printer: Option<PrinterId>,
    fallback_resin: Option<MaterialId>,
    installed: &ProfileLibraries,
) -> ProjectPayload {
    let mut payload = document.project_payload(fallback_printer, fallback_resin);
    with_embedded_profiles(&mut payload, installed);
    payload
}

/// Everything one project file will hold, taken on the UI thread and owned
/// so another thread can write it: the scene (a cheap copy, meshes are
/// shared), the project data, the slices to keep with their key, and the
/// file to write.
pub(crate) struct ProjectSnapshot {
    scene: Scene,
    payload: ProjectPayload,
    slices: Option<(PlateSlices, String)>,
    target: PathBuf,
}

impl ProjectSnapshot {
    /// The whole document; `None` with no scene. Scene and slices are read
    /// together, so the slices kept belong to the geometry written.
    pub(crate) fn of_document(document: &SceneDocument, payload: ProjectPayload, target: PathBuf) -> Option<Self> {
        document.read_scene(|spatial| {
            let scene = spatial.graph().clone();
            let slices = slices_worth_keeping(document, &payload, &scene);
            ProjectSnapshot { scene, payload, slices, target }
        })
    }

    /// Plate `index` (counted from 1) on its own with its slices, as a
    /// project that opens by itself.
    pub(crate) fn of_plate(document: &SceneDocument, index: usize, sliced: &SliceResult, target: PathBuf) -> Option<Self> {
        let at = index.checked_sub(1)?;
        let mut scene = document
            .read_scene(|spatial| {
                let whole = spatial.graph();
                let plate_id = whole.plates().order.get(at)?.id;
                whole.plate_scene(plate_id).ok()
            })
            .flatten()?;
        // The copy keeps the original plate list; only this plate stays,
        // owning every part left in the copy.
        let plate = scene.plates().order.get(at)?.clone();
        let parts = scene.reachable_leaves(CsgPolicy::Descend, ReferencePolicy::Skip).ok()?;
        let owners = parts.into_iter().map(|part| (part, plate.id)).collect();
        scene.set_plates(Plates::from_parts(vec![plate], owners)?);
        let kept = PlateSlices { contours: sliced.merged.clone(), slab_tops: sliced.schedule.iter_slab_tops().collect() };
        Some(ProjectSnapshot {
            scene,
            payload: project_data_for_save(document, None, None, &ProfileLibraries::platform_default()),
            slices: Some((kept, sliced.token.clone())),
            target,
        })
    }

    /// A Save to `target`: the snapshot, the project data in it, and the
    /// document's state when taken, which the caller hands back once the
    /// file is written so the two cannot differ. `None` with no scene.
    pub(crate) fn for_save(
        document: &SceneDocument,
        target: PathBuf,
        fallback_printer: Option<PrinterId>,
        fallback_resin: Option<MaterialId>,
    ) -> Option<(Self, ProjectPayload, SavedAt)> {
        let installed = ProfileLibraries::platform_default();
        let payload = project_data_for_save(document, fallback_printer, fallback_resin, &installed);
        let snapshot = Self::of_document(document, payload.clone(), target)?;
        Some((snapshot, payload, document.stamp_for_save()))
    }

    /// Writes the project, replacing the target only once the file is
    /// complete, and reports each step. The document is not touched, so this
    /// runs on a worker; the UI thread records the save afterwards.
    pub(crate) fn write(&self, mut report: impl FnMut(WriteStep, u64, u64)) -> Result<()> {
        let (slices, key) = self.slices.as_ref().map(|(slices, key)| (slices, key.as_str())).unzip();
        let mut bytes = Vec::new();
        write_project_with_progress(
            &self.scene,
            &self.payload,
            EmbedProfiles::Yes,
            slices,
            key,
            ProjectFileType::ThreeMf,
            &mut bytes,
            |progress| {
                let (step, done, total) = match progress {
                    WriteProgress::Serialize { done, total } => (WriteStep::Serialize, done, total),
                    WriteProgress::Compress { done, total } => (WriteStep::Compress, done, total),
                };
                report(step, done, total);
            },
        )?;
        report(WriteStep::Writing, 0, 1);
        write_bytes_atomically(&self.target, &bytes)?;
        report(WriteStep::Writing, 1, 1);
        Ok(())
    }
}

/// The held slices, when they are still good for this project: its printer
/// and resin give the layers, and the document says whether its slices
/// match them. A profile missing or not resolving keeps none.
fn slices_worth_keeping(document: &SceneDocument, payload: &ProjectPayload, scene: &Scene) -> Option<(PlateSlices, String)> {
    let printer = payload.printer.as_ref()?;
    let resin = payload.resin.as_ref()?;
    let layers = layers_for_choice(printer, resin, document.chosen_first_layer(), scene).ok()?;
    document.slices_to_save(&layers)
}

/// A whole save at once (the window writes on a worker instead; tests use
/// this). On success the document holds what was written, saves to
/// `target` from now on and has no unsaved changes.
#[cfg(test)]
pub(crate) fn save_now(
    document: &SceneDocument,
    target: &Path,
    fallback_printer: Option<PrinterId>,
    fallback_resin: Option<MaterialId>,
) -> Result<()> {
    let (snapshot, payload, stamp) = ProjectSnapshot::for_save(document, target.to_path_buf(), fallback_printer, fallback_resin)
        .ok_or_else(|| anyhow::anyhow!("No world loaded"))?;
    snapshot.write(|_, _, _| {})?;
    document.note_project_saved(payload, target.to_path_buf(), stamp);
    Ok(())
}

#[cfg(test)]
#[path = "tests_scene_save.rs"]
mod tests;

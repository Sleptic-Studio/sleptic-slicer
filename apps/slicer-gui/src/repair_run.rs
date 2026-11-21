// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The Repair action of a window. The scene is copied on the UI thread and
//! repaired on a worker, so the window stays responsive; the worker leaves
//! its result for the UI thread (`invoke_repair_finished`). There is no
//! progress bar or cancel: it is one check per mesh, as loading already
//! does unnoticed.
//! While the window's `repairing-in-progress` is set, Repair and undo/redo
//! are off (an undo during the run would make the worker's result wrong),
//! and every way out clears it. The scene revision taken at the start
//! decides whether the result is applied: one made for geometry that has
//! since changed, or for a scene since replaced, is dropped, never merged.
//! The result is applied as one undoable edit replacing only the meshes the
//! repair changed: the repair copies a mesh only when it fixes it, so the
//! unchanged ones keep their identity.
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};

use sc_mesh_core::IndexedMesh;
use sc_mesh_core::indexed_mesh::{MeshFix, RepairPasses};
use sc_scene::mesh_targets::MeshTarget;
use sc_scene::repair::SceneRepairOutcome;
use sc_scene::{Scene, SceneError};
use sc_scene_edit::SceneEdit;
use slint::ComponentHandle;

use crate::MainWindow;
use crate::error_dialog::MessageWindows;
use crate::i18n::ctr;
use crate::mesh_loader::panic_text;
use crate::scene_document::{EditRefused, SceneDocument, Revision};
use crate::workspace_section::repair_allowed_in;

/// A mesh the repair changed: where it is stored, and the shared mesh
/// before and after, ready to become an edit.
struct MeshSwap {
    mesh_slot: MeshTarget,
    old_mesh: Arc<IndexedMesh>,
    new_mesh: Arc<IndexedMesh>,
}

impl MeshSwap {
    /// The replacement as the edit that makes it (and, undone, reverts it).
    fn into_scene_edit(&self) -> SceneEdit {
        let (before, after) = (Arc::clone(&self.old_mesh), Arc::clone(&self.new_mesh));
        match self.mesh_slot {
            MeshTarget::InlineLeaf(node_id) => SceneEdit::ReplaceLeafMesh { node_id, before, after },
            MeshTarget::Resource(rid) => SceneEdit::ReplaceMeshResource { rid, before, after },
        }
    }
}

/// What the worker found for each mesh, and the ones it changed.
struct FinishedRepair {
    repaired_parts: Vec<SceneRepairOutcome>,
    finished_swaps: Vec<MeshSwap>,
}

/// The worker's result, or its panic message.
type RepairResultCell = Arc<Mutex<Option<Result<FinishedRepair, String>>>>;

/// What became of a finished repair.
#[derive(Debug, PartialEq, Eq)]
enum RepairVerdict {
    /// The scene changed during the run; nothing was applied.
    DroppedOutdated,
    /// There was nothing to change (a clean scene, or only refusals).
    NoChange,
    /// The changes were applied as one undoable edit.
    Committed,
    /// The scene refused the edit; nothing was recorded.
    Refused(SceneError),
}

pub(crate) struct RepairJob {
    window_handle: slint::Weak<MainWindow>,
    scene_doc: Rc<SceneDocument>,
    dialog_host: Rc<MessageWindows>,
    result_cell: RepairResultCell,
    /// The revision the running repair started from; `None` when none runs.
    started_from: RefCell<Option<Revision>>,
}

impl RepairJob {
    /// Connects the window's Repair callbacks; the caller keeps the result
    /// for as long as the window lives.
    pub(crate) fn attach(
        window: &MainWindow,
        doc: Rc<SceneDocument>,
        host: Rc<MessageWindows>,
    ) -> Rc<Self> {
        let repair = Rc::new(Self {
            window_handle: window.as_weak(),
            scene_doc: doc,
            dialog_host: host,
            result_cell: RepairResultCell::default(),
            started_from: RefCell::default(),
        });
        let starter = Rc::clone(&repair);
        window.on_repair_models(move || starter.begin_repair());
        let finisher = Rc::clone(&repair);
        window.on_repair_done(move || finisher.collect_result());
        repair
    }

    /// Repair pressed. The button is off in most of these cases already;
    /// the checks cover a shortcut arriving at the wrong moment.
    fn begin_repair(self: &Rc<Self>) {
        let Some(ui) = self.window_handle.upgrade() else {
            return;
        };
        let busy =
            ui.get_busy_opening() || ui.get_slicing() || ui.get_repairing();
        let allowed = repair_allowed_in(ui.get_section());
        // The worker gets its own copy; the meshes inside stay shared.
        let copy = if busy || !allowed {
            None
        } else {
            self.scene_doc.read_scene(|spatial| spatial.graph().clone())
        };
        let Some(scene) = copy else {
            return;
        };
        self.started_from.replace(Some(self.scene_doc.revision()));
        ui.set_repairing(true);
        let (slot, window) = (Arc::clone(&self.result_cell), self.window_handle.clone());
        std::thread::spawn(move || {
            let guarded = std::panic::AssertUnwindSafe(move || repair_copy(scene));
            let result = std::panic::catch_unwind(guarded).map_err(|payload| panic_text(&payload));
            locked(&slot).replace(result);
            let _ = window.upgrade_in_event_loop(|ui| ui.invoke_repair_done());
        });
    }

    /// On the UI thread: takes the result, applies it if still current and
    /// reports in a dialog.
    fn collect_result(&self) {
        let result = locked(&self.result_cell).take();
        let started_from = self.started_from.take();
        // Every way out ends the run in the window and returns to Transform.
        if let Some(ui) = self.window_handle.upgrade() {
            ui.set_repairing(false);
            ui.invoke_section_picked(crate::WorkspaceSection::Transform);
        }
        let (Some(result), Some(started_from)) = (result, started_from) else {
            return;
        };
        let finished = match result {
            Ok(finished) => finished,
            Err(crash) => {
                let heading = ctr!("RepairDialog", "Repair failed");
                let message = ctr!("RepairDialog", "The repair worker crashed.").to_string();
                return self.dialog_host.show_failure(&heading, None, message, crash);
            }
        };
        match decide_repair(&self.scene_doc, &finished, started_from) {
            RepairVerdict::DroppedOutdated => self.dialog_host.show_notice(
                &ctr!("RepairDialog", "Repair discarded"),
                ctr!(
                    "RepairDialog",
                    "The scene changed while repairing; nothing was applied."
                )
                .to_string(),
                String::new(),
            ),
            RepairVerdict::NoChange | RepairVerdict::Committed => {
                let outcomes = &finished.repaired_parts;
                let lines = self
                    .scene_doc
                    .read_scene(|spatial| repair_report_lines(outcomes, spatial.graph()))
                    .unwrap_or_default();
                let heading = ctr!("RepairDialog", "Mesh repair");
                self.dialog_host
                    .show_notice(&heading, repair_summary_text(outcomes), lines.join("\n"));
            }
            RepairVerdict::Refused(error) => self.dialog_host.show_failure(
                &ctr!("RepairDialog", "Repair failed"),
                None,
                ctr!(
                    "RepairDialog",
                    "The scene rejected the repair edit; nothing was changed."
                )
                .to_string(),
                error.to_string(),
            ),
        }
    }
}

/// The worker: repairs every mesh of the copy and returns the ones whose
/// shared mesh changed. The copy shares its meshes with the live scene, so
/// one still the same object after the run needs no change in the live
/// scene either.
fn repair_copy(mut copy: Scene) -> FinishedRepair {
    let mut held = Vec::new();
    for target in copy.mesh_targets() {
        if let Some(mesh) = current_mesh_of(&copy, target) {
            held.push((target, mesh));
        }
    }
    let fixed = copy.repair_meshes(RepairPasses::default_repair());
    let mut replacements = Vec::new();
    for (target, before) in held {
        // A mesh the repair left alone is still the very same object.
        let Some(after) = current_mesh_of(&copy, target) else {
            continue;
        };
        if !Arc::ptr_eq(&before, &after) {
            replacements.push(MeshSwap { mesh_slot: target, old_mesh: before, new_mesh: after });
        }
    }
    FinishedRepair { repaired_parts: fixed, finished_swaps: replacements }
}

/// The shared mesh stored at `target`, if it still exists.
fn current_mesh_of(scene: &Scene, target: MeshTarget) -> Option<Arc<IndexedMesh>> {
    let shared = match target {
        MeshTarget::InlineLeaf(leaf) => scene.leaf_mesh_arc(leaf),
        MeshTarget::Resource(resource) => scene.mesh_resource_arc(resource),
    };
    shared.map(Arc::clone)
}

/// The value behind `mutex`, also after a panic while it was held: the
/// slot only ever holds a whole result or nothing.
fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Applies a result if the scene is still the one it was made for, as one
/// undoable edit; drops it otherwise.
fn decide_repair(
    doc: &Rc<SceneDocument>,
    finished: &FinishedRepair,
    started_from: Revision,
) -> RepairVerdict {
    let outdated = doc.slice_outdated_by_geometry(started_from);
    if outdated {
        return RepairVerdict::DroppedOutdated;
    }
    let parts: Vec<SceneEdit> = finished.finished_swaps.iter().map(MeshSwap::into_scene_edit).collect();
    if parts.is_empty() {
        return RepairVerdict::NoChange;
    }
    // One edit for the whole repair, so a single undo reverts it.
    let edit = SceneEdit::Compound(parts);
    match doc.commit_edit(edit.clone()) {
        Ok(()) => {
            doc.record_edit(edit);
            RepairVerdict::Committed
        }
        // No scene any more is the outdated case; the revision check above
        // makes it unreachable in practice.
        Err(EditRefused::Empty) => RepairVerdict::DroppedOutdated,
        Err(EditRefused::Refused(error)) => RepairVerdict::Refused(error),
    }
}

/// The dialog's one-line summary.
fn repair_summary_text(outcomes: &[SceneRepairOutcome]) -> String {
    let count = |keep: fn(&SceneRepairOutcome) -> bool| outcomes.iter().filter(|o| keep(o)).count();
    let repaired = count(|o| !o.fixes.is_empty() && o.refusal.is_none());
    let refused = count(|o| o.refusal.is_some());
    let still_holed = count(|o| o.refusal.is_none() && !o.edge_manifold_after);
    if repaired + refused + still_holed == 0 {
        return ctr!("RepairDialog", "No problems found.").to_string();
    }
    // Each count that is not zero, in this order.
    let parts = [
        (repaired, ctr!("RepairDialog", "{} mesh(es) repaired", repaired)),
        (refused, ctr!("RepairDialog", "{} mesh(es) could not be fully repaired", refused)),
        (still_holed, ctr!("RepairDialog", "{} mesh(es) still not edge-manifold", still_holed)),
    ];
    let said: Vec<String> = parts
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(_, text)| text.to_string())
        .collect();
    format!("{}.", said.join(", "))
}

/// A line per mesh with something to say (fixes, a refusal, or still not
/// edge-manifold); clean meshes are left out.
fn repair_report_lines(outcomes: &[SceneRepairOutcome], graph: &Scene) -> Vec<String> {
    let mut lines = Vec::new();
    for (i, outcome) in outcomes.iter().enumerate() {
        // Degenerate, reoriented, renormalised, duplicate, detached.
        let mut counts = [0usize; 5];
        for fix in &outcome.fixes {
            counts[match fix {
                MeshFix::RemovedZeroLengthEdgeFace { .. } => 0,
                MeshFix::FlippedFaceOrientation { .. } => 1,
                MeshFix::RecomputedNormal { .. } => 2,
                MeshFix::RemovedDuplicateFace { .. } => 3,
                MeshFix::DetachedSharedVertex { .. } => 4,
            }] += 1;
        }
        let [removed, reoriented, renormaled, duplicates, detached] = counts;
        let mut notes = Vec::new();
        if removed > 0 {
            notes.push(ctr!("RepairDialog", "removed {} degenerate face(s)", removed).to_string());
        }
        if reoriented > 0 {
            notes.push(ctr!("RepairDialog", "reoriented {} face(s)", reoriented).to_string());
        }
        if renormaled > 0 {
            notes.push(ctr!("RepairDialog", "recomputed {} normal(s)", renormaled).to_string());
        }
        if duplicates > 0 {
            notes
                .push(ctr!("RepairDialog", "removed {} duplicate face(s)", duplicates).to_string());
        }
        if detached > 0 {
            notes.push(
                ctr!(
                    "RepairDialog",
                    "separated touching shells ({} shared vertices)",
                    detached
                )
                .to_string(),
            );
        }
        match &outcome.refusal {
            Some(refusal) => notes
                .push(ctr!("RepairDialog", "could not be fully repaired: {}", refusal).to_string()),
            None if !outcome.edge_manifold_after => {
                notes.push(ctr!("RepairDialog", "still not edge-manifold").to_string())
            }
            None => {}
        }
        if !notes.is_empty() {
            let name = part_label(graph, outcome.target, i);
            lines.push(format!("{name}: {}", notes.join("; ")));
        }
    }
    lines
}

/// A mesh's name as the object list shows it: the first part using it, else
/// the mesh's own name, else "Mesh n" counting from 1.
fn part_label(graph: &Scene, target: MeshTarget, ordinal: usize) -> String {
    let first_part = graph.leaves_referencing(target).into_iter().next();
    let part_name = first_part.and_then(|id| graph.node(id)?.meta.as_ref()?.name.clone());
    let mesh_name = || graph.target_mesh(target)?.meta.as_ref()?.name.clone();
    let numbered = || ctr!("RepairDialog", "Mesh {}", ordinal + 1).to_string();
    part_name.or_else(mesh_name).unwrap_or_else(numbered)
}

#[cfg(test)]
#[path = "tests_repair_run.rs"]
mod tests;

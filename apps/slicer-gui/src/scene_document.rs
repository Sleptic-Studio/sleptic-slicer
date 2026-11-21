// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Editable scenes, committed history, and per-plate session results.
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_mesh_spatial::SceneSpatial;
use sc_print_primitives::{MaterialId, PositiveFiniteMm, PrinterId};
use sc_project_formats::{EmbeddedProfile, PlateSlices, ProjectPayload};
use sc_scene::Scene;
use sc_scene::node::{LeafAttachment, LeafPayload, NodeKind};
use sc_scene::{MeshResourceId, NodeId, SceneError};
use sc_scene_edit::SceneFragment;
use sc_sl1cer::contour::{ContourLayer, ContourStack};
use sc_sl1cer::profile::{LayerSchedule, SliceProfile};

pub(crate) use sc_scene_edit::SceneEdit;

use crate::selection::{Selection, SelectionState};
use crate::slice_result::{SliceParams, SliceResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Revision {
    install: u64,
    head: Option<u64>,
}

impl Revision {
    pub(crate) fn same_document(self, other: Self) -> bool {
        self.install == other.install
    }

    #[cfg(test)]
    pub(crate) fn with_head(top_edit: Option<u64>) -> Self {
        Self { install: 0, head: top_edit }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SavedAt {
    install: u64,
    head: Option<u64>,
    payload: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SaveCheck {
    Current,
    ChangedSince,
    Superseded,
}

pub(crate) const UNDO_DEPTH: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DocumentEvent {
    Opened,
    Emptied,
    ChoiceChanged,
    Edited,
    Regrouped,
    HistoryChanged,
    UnsavedChanged,
    /// Printer, material or scene process inputs changed.
    SettingsChanged,
}

type Observer = Rc<dyn Fn(DocumentEvent)>;

#[derive(Clone, Debug)]
struct PastEdit {
    serial: u64,
    change: SceneEdit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpenedFrom {
    MeshFile,
    ProjectFile,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Choices {
    machine: Option<PrinterId>,
    material: Option<MaterialId>,
    first_layer: Option<PositiveFiniteMm>,
}

impl Choices {
    fn unpack(payload: ProjectPayload) -> (Self, Vec<EmbeddedProfile>) {
        let choices = Self {
            machine: payload.printer,
            material: payload.resin,
            first_layer: payload.first_layer_height,
        };
        (choices, payload.embedded)
    }
}

pub(crate) struct SceneDocument {
    active_plate: Cell<u32>,
    base_process: RefCell<sc_print_profiles::scene_overrides::PlateSettings>,
    pub(crate) plate_width: Cell<f32>,
    /// Support settings Auto prepare tuned for the parts, used once by the
    /// support run it starts and never shown in the user's settings (R106).
    pub(crate) auto_support_settings:
        RefCell<Option<sc_print_profiles::support_settings::SupportSettings>>,
    /// The printer the plate's supports were generated for (R108): their
    /// island check holds on that printer's pixels and layers only.
    pub(crate) supports_printer: RefCell<Option<PrinterId>>,
    pub(crate) plate_depth: Cell<f32>,
    plate_results: RefCell<std::collections::BTreeMap<u32, SliceResult>>,
    plate_previews: RefCell<std::collections::BTreeMap<u32, crate::plate_preview::PlatePreview>>,
    pub(crate) plate_ui: RefCell<Option<slint::Weak<crate::MainWindow>>>,
    pub(crate) scene_aa: Cell<Option<sc_print_primitives::AaLevel>>,
    pub(crate) process_overrides: RefCell<sc_print_profiles::SceneOverrides>,
    pub(crate) slice_state: RefCell<crate::slice_state::SliceState>,
    model: RefCell<Option<SceneSpatial>>,
    input_path: RefCell<Option<PathBuf>>,
    file_kind: Cell<OpenedFrom>,
    choices: RefCell<Choices>,
    profile_documents: RefCell<Vec<EmbeddedProfile>>,
    picked: SelectionState,
    observers: RefCell<Vec<(u64, Observer)>>,
    observer_serial: Cell<u64>,
    past: RefCell<VecDeque<PastEdit>>,
    future: RefCell<VecDeque<PastEdit>>,
    edit_serial: Cell<u64>,
    saved_head: Cell<Option<u64>>,
    save_history_lost: Cell<bool>,
    payload_unsaved: Cell<bool>,
    held_slice: RefCell<Option<SliceResult>>,
    slice_history_lost: Cell<bool>,
    slice_serial: Cell<u64>,
    installation_serial: Cell<u64>,
    payload_serial: Cell<u64>,
}

/// `(row, x0, x1)` island spans of one layer plus the `(width, height)`
/// raster resolution they are expressed in; see
/// [`SceneDocument::slice_island_spans`].
pub(crate) type IslandSpans = (Vec<(u32, u32, u32)>, u32, u32);

impl SceneDocument {
    // Loading and clearing retire saved/sliced baselines without reusing edit IDs.
    fn reset_baselines(&self, unsaved: bool) -> bool {
        let changed = self.discard_history();
        self.saved_head.set(None);
        self.save_history_lost.set(false);
        self.payload_unsaved.set(unsaved);
        self.held_slice.take();
        self.slice_state
            .replace(crate::slice_state::SliceState::Idle);
        self.slice_history_lost.set(false);
        changed
    }

    fn read_slice<R>(&self, read: impl FnOnce(&SliceResult) -> R) -> Option<R> {
        let stored = self.held_slice.borrow();
        stored.as_ref().map(read)
    }

    fn append_history(
        &self,
        stack: &RefCell<VecDeque<PastEdit>>,
        entry: PastEdit,
        applied: bool,
    ) {
        let mut entries = stack.borrow_mut();
        if entries.len() == UNDO_DEPTH {
            entries.pop_front();
            if applied {
                self.save_history_lost.set(true);
                self.slice_history_lost.set(true);
            }
        }
        entries.push_back(entry);
    }

    fn history_changed(&self, previous_dirty: bool) {
        self.tell(DocumentEvent::HistoryChanged);
        self.dirty_transition(previous_dirty);
    }

    fn replay_history(&self, forward: bool) -> Result<bool, SceneError> {
        let (source, destination) = if forward {
            (&self.future, &self.past)
        } else {
            (&self.past, &self.future)
        };
        let previous_dirty = self.has_unsaved_changes();
        let Some(entry) = source.borrow_mut().pop_back() else {
            return Ok(false);
        };
        let edit = if forward {
            entry.change.clone()
        } else {
            entry.change.inverse()
        };
        if let Err(failure) = self.commit_edit(edit) {
            source.borrow_mut().push_back(entry);
            return match failure {
                EditRefused::Refused(error) => Err(error),
                EditRefused::Empty => Ok(false),
            };
        }
        self.append_history(destination, entry, forward);
        self.history_changed(previous_dirty);
        Ok(true)
    }

    fn commit_scene_edit(&self, edit: SceneEdit) -> Result<bool, SceneError> {
        match self.commit_edit(edit.clone()) {
            Err(EditRefused::Refused(error)) => Err(error),
            Err(EditRefused::Empty) => Ok(false),
            Ok(()) => {
                self.record_edit(edit);
                Ok(true)
            }
        }
    }

    pub(crate) fn all_plate_slices(&self) -> Vec<(usize, SliceResult)> {
        self.read_scene(|s| {
            s.graph()
                .plates()
                .order
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    let result = if p.id == self.active_plate() {
                        self.held_slice.borrow().clone()
                    } else {
                        self.plate_results.borrow().get(&p.id).cloned()
                    }?;
                    (!self.slice_outdated_by_geometry(result.params.geometry))
                        .then_some((i + 1, result))
                })
                .collect()
        })
        .unwrap_or_default()
    }

    pub(crate) fn finish_plate_drag(&self, edits: &mut Vec<SceneEdit>, lead: NodeId) {
        let Some(graph) = self.read_scene(|s| s.graph().clone()) else {
            return;
        };
        let width = self.plate_width.get();
        let old = graph.plate_of(lead);
        let Ok(mesh) = sc_scene::bake_leaf_to_world(&graph, lead) else {
            return;
        };
        let (min, max) = mesh
            .vertices
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| {
                (a.min(v.coords()[0]), b.max(v.coords()[0]))
            });
        let (min_y, max_y) = mesh
            .vertices
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| {
                (a.min(v.coords()[1]), b.max(v.coords()[1]))
            });
        // World-space centre of the dropped object: plate-local centre plus
        // its current plate's grid offset. The plate whose rectangle holds
        // it becomes its new plate; a drop between plates stays put.
        let depth = self.plate_depth.get();
        let [old_x, old_y] = graph.plate_offset(old, width, depth);
        let center = [(min + max) * 0.5 + old_x, (min_y + max_y) * 0.5 + old_y];
        let Some(target) = graph
            .plates()
            .order
            .iter()
            .find(|p| {
                let [x, y] = graph.plate_offset(p.id, width, depth);
                (center[0] - x).abs() <= width * 0.5 && (center[1] - y).abs() <= depth * 0.5
            })
            .filter(|p| !p.locked)
            .map(|p| p.id)
        else {
            return;
        };
        if target == old {
            return;
        }
        let before = graph.plates().clone();
        let mut after = before.clone();
        let mut corrections = Vec::new();
        for edit in edits.iter_mut() {
            if let SceneEdit::SetLocalTransform {
                node_id,
                after: final_transform,
                ..
            } = edit
            {
                let [from_x, from_y] = graph.plate_offset(graph.plate_of(*node_id), width, depth);
                let [to_x, to_y] = graph.plate_offset(target, width, depth);
                let delta = Transform3D::Affine(AffineTransform3D::from_translation([
                    f64::from(from_x - to_x),
                    f64::from(from_y - to_y),
                    0.0,
                ]));
                if let Some(correction) = crate::layout_tools::world_edit(&graph, *node_id, delta) {
                    if let SceneEdit::SetLocalTransform {
                        after: normalized, ..
                    } = &correction
                    {
                        *final_transform = *normalized;
                    }
                    corrections.push(correction);
                }
                after.assignments.insert(*node_id, target);
                for support in owned_support_ids(&graph, *node_id) {
                    after.assignments.insert(support, target);
                }
            }
        }
        let membership = SceneEdit::SetPlates { before, after };
        corrections.push(membership.clone());
        if self.commit_edit(SceneEdit::Compound(corrections)).is_ok() {
            edits.push(membership);
            self.set_active_plate(target);
        }
    }

    /// Viewport offset (X, Y in mm) of the plate holding `id`.
    pub(crate) fn plate_offset(&self, id: NodeId) -> [f32; 2] {
        self.read_scene(|s| {
            s.graph().plate_offset(
                s.graph().plate_of(id),
                self.plate_width.get(),
                self.plate_depth.get(),
            )
        })
        .unwrap_or([0.0, 0.0])
    }

    pub(crate) fn local_camera(
        &self,
        camera: &crate::camera::ViewCamera,
        id: NodeId,
    ) -> crate::camera::ViewCamera {
        let mut local = camera.clone();
        let [x, y] = self.plate_offset(id);
        local.look_at.x -= x;
        local.look_at.y -= y;
        local
    }

    fn ensure_empty_scene(&self) {
        if self.has_scene() {
            return;
        }
        let selection = self.choices.borrow().clone();
        let overrides = *self.process_overrides.borrow();
        let aa = self.scene_aa.get();
        self.take_spatial(SceneSpatial::from_graph(Scene::new()), PathBuf::new(), None);
        *self.choices.borrow_mut() = selection;
        *self.process_overrides.borrow_mut() = overrides;
        self.scene_aa.set(aa);
        *self.base_process.borrow_mut() = sc_print_profiles::scene_overrides::PlateSettings {
            name: None,
            process: overrides,
            aa,
            first_layer_height: self.chosen_first_layer(),
        };
        self.tell(DocumentEvent::SettingsChanged);
    }

    pub(crate) fn apply_batch(&self, edit: SceneEdit, node_slots: usize) -> Result<(), String> {
        self.ensure_empty_scene();
        {
            let mut slot = self.model.borrow_mut();
            let spatial = slot.as_mut().ok_or("Missing scene")?;
            while spatial.graph().nodes_len() < node_slots {
                spatial.allocate_unbound_node_id();
            }
        }
        self.commit_edit(edit.clone())
            .map_err(|e| format!("{e:?}"))?;
        self.record_edit(edit);
        Ok(())
    }

    pub(crate) fn active_plate(&self) -> u32 {
        self.read_scene(|s| {
            let plates = &s.graph().plates().order;
            plates
                .iter()
                .find(|p| p.id == self.active_plate.get())
                .unwrap_or(&plates[0])
                .id
        })
        .unwrap_or(1)
    }

    fn restore_active_settings(&self) {
        let settings = self
            .read_scene(|s| {
                s.graph()
                    .plates()
                    .order
                    .iter()
                    .find(|p| p.id == self.active_plate())
                    .and_then(|p| p.settings.as_deref())
                    .and_then(|s| sc_print_profiles::scene_overrides::PlateSettings::parse(s).ok())
            })
            .flatten()
            .unwrap_or_else(|| self.base_process.borrow().clone());
        *self.process_overrides.borrow_mut() = settings.process;
        self.scene_aa.set(settings.aa);
        self.choices.borrow_mut().first_layer = settings.first_layer_height;
    }

    pub(crate) fn set_active_plate(&self, plate: u32) {
        if matches!(
            *self.slice_state.borrow(),
            crate::slice_state::SliceState::Slicing { .. }
        ) {
            return;
        }
        if !self
            .read_scene(|s| s.graph().plates().order.iter().any(|p| p.id == plate))
            .unwrap_or(false)
        {
            return;
        }

        let old = self.active_plate.get();
        if old == plate {
            return;
        }
        let ui = self.plate_ui.borrow().as_ref().and_then(|w| w.upgrade());
        if let Some(ui) = &ui {
            self.plate_previews
                .borrow_mut()
                .insert(old, crate::plate_preview::PlatePreview::capture(ui));
        }
        if let Some(result) = self.held_slice.borrow_mut().take() {
            self.plate_results.borrow_mut().insert(old, result);
        }
        *self.held_slice.borrow_mut() = self.plate_results.borrow().get(&plate).cloned();
        self.active_plate.set(plate);
        self.restore_active_settings();

        self.slice_serial
            .set(self.slice_serial.get().wrapping_add(1));
        *self.slice_state.borrow_mut() = if self.slicing_inputs().is_some() {
            if self.slice_is_geometry_stale() {
                crate::slice_state::SliceState::Stale
            } else {
                crate::slice_state::SliceState::Sliced
            }
        } else {
            crate::slice_state::SliceState::Idle
        };
        self.tell(DocumentEvent::ChoiceChanged);
        if let Some(ui) = &ui {
            self.plate_previews
                .borrow()
                .get(&plate)
                .cloned()
                .unwrap_or_default()
                .apply(ui);
            self.slice_state.borrow().apply(ui);
            ui.invoke_layer_picked();
        }
    }

    pub(crate) fn edit_plates(&self, change: impl FnOnce(&mut sc_scene::plates::Plates)) {
        let Some(before) = self.read_scene(|s| s.graph().plates().clone()) else {
            return;
        };
        let mut after = before.clone();
        change(&mut after);
        let edit = SceneEdit::SetPlates { before, after };
        if !edit.is_noop() && self.commit_edit(edit.clone()).is_ok() {
            self.record_edit(edit);
        }
    }

    /// Display name is stored with per-plate settings so existing project
    /// persistence and the plate-table undo transaction preserve it together.
    pub(crate) fn plate_name(&self) -> Option<String> {
        self.read_scene(|s| {
            s.graph()
                .plates()
                .order
                .iter()
                .find(|p| p.id == self.active_plate())
                .and_then(|p| p.settings.as_deref())
                .and_then(|s| sc_print_profiles::scene_overrides::PlateSettings::parse(s).ok())
                .and_then(|s| s.name)
        })
        .flatten()
    }

    pub(crate) fn rename_plate(&self, name: &str) {
        let name = name.trim();
        let active = self.active_plate();
        let mut settings = self
            .read_scene(|s| {
                s.graph()
                    .plates()
                    .order
                    .iter()
                    .find(|p| p.id == active)
                    .and_then(|p| p.settings.as_deref())
                    .and_then(|s| sc_print_profiles::scene_overrides::PlateSettings::parse(s).ok())
            })
            .flatten()
            .unwrap_or_else(|| sc_print_profiles::scene_overrides::PlateSettings {
                name: None,
                process: *self.process_overrides.borrow(),
                aa: self.scene_aa.get(),
                first_layer_height: self.chosen_first_layer(),
            });
        settings.name = (!name.is_empty()).then(|| name.to_string());
        let Ok(serialized) = settings.serialize() else {
            return;
        };
        self.edit_plates(|p| {
            if let Some(plate) = p.order.iter_mut().find(|p| p.id == active) {
                plate.settings = Some(serialized);
            }
        });
    }

    pub(crate) fn add_plate(&self) {
        self.ensure_empty_scene();
        let mut added = None;
        let settings = sc_print_profiles::scene_overrides::PlateSettings {
            name: None,
            process: *self.process_overrides.borrow(),
            aa: self.scene_aa.get(),
            first_layer_height: self.chosen_first_layer(),
        }
        .serialize()
        .ok();
        let active = self.active_plate();
        self.edit_plates(|p| {
            if let Some(existing) = p
                .order
                .iter_mut()
                .find(|p| p.id == active && p.settings.is_none())
            {
                existing.settings = settings.clone();
            }
            let id = p.add();
            p.order.last_mut().unwrap().settings = settings;
            added = Some(id);
        });
        if let Some(id) = added {
            self.set_active_plate(id);
        }
    }

    pub(crate) fn move_selection_to_plate(&self, plate: u32) {
        if self
            .read_scene(|s| {
                self.chosen().ids().iter().any(|id| {
                    s.graph()
                        .plates()
                        .order
                        .iter()
                        .any(|p| p.id == s.graph().plate_of(*id) && p.locked)
                })
            })
            .unwrap_or(false)
        {
            return;
        }

        let mut ids = self.chosen().ids();
        if let Some(supports) = self.read_scene(|s| {
            ids.iter()
                .flat_map(|id| owned_support_ids(s.graph(), *id))
                .collect::<Vec<_>>()
        }) {
            ids.extend(supports);
        }
        self.edit_plates(|p| {
            if p.order.iter().any(|v| v.id == plate && !v.locked) {
                for id in ids {
                    p.assignments.insert(id, plate);
                }
            }
        });
        self.set_active_plate(plate);
    }

    pub(crate) fn select_active_plate(&self) {
        let active = self.active_plate();
        let ids = self
            .read_scene(|s| {
                s.graph()
                    .reachable_leaves(
                        sc_scene::CsgPolicy::Descend,
                        sc_scene::ReferencePolicy::Skip,
                    )
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|id| s.graph().plate_of(*id) == active)
                    .collect()
            })
            .unwrap_or_default();
        self.choose(Selection::from_ids(ids));
    }

    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self {
            active_plate: Cell::new(1),
            base_process: RefCell::default(),
            plate_width: Cell::new(192.0),
            auto_support_settings: RefCell::default(),
            supports_printer: RefCell::default(),
            plate_depth: Cell::new(120.0),
            plate_results: RefCell::default(),
            plate_previews: RefCell::default(),
            plate_ui: RefCell::default(),
            model: RefCell::default(),
            input_path: RefCell::default(),
            file_kind: Cell::new(OpenedFrom::MeshFile),
            choices: RefCell::new(Choices::default()),
            slice_state: RefCell::default(),
            process_overrides: RefCell::default(),
            scene_aa: Cell::default(),
            profile_documents: RefCell::default(),
            picked: SelectionState::new(),
            observers: RefCell::default(),
            observer_serial: Cell::default(),
            past: RefCell::default(),
            future: RefCell::default(),
            edit_serial: Cell::new(1),
            saved_head: Cell::default(),
            save_history_lost: Cell::default(),
            payload_unsaved: Cell::default(),
            held_slice: RefCell::default(),
            slice_history_lost: Cell::default(),
            slice_serial: Cell::default(),
            installation_serial: Cell::default(),
            payload_serial: Cell::default(),
        })
    }

    #[cfg(test)]
    pub(crate) fn take_mesh_scene(&self, graph: Scene, source_path: PathBuf) {
        self.take_spatial(SceneSpatial::from_graph(graph), source_path, None);
    }

    #[cfg(test)]
    pub(crate) fn take_project_scene(
        &self,
        graph: Scene,
        source_path: PathBuf,
        payload: ProjectPayload,
    ) {
        self.take_spatial(SceneSpatial::from_graph(graph), source_path, Some(payload));
    }

    pub(crate) fn take_spatial(
        &self,
        spatial: SceneSpatial,
        source_path: PathBuf,
        project_payload: Option<ProjectPayload>,
    ) {
        let previous_dirty = self.has_unsaved_changes();
        self.advance_installation();
        self.active_plate.set(spatial.graph().plates().order[0].id);
        self.plate_results.borrow_mut().clear();
        self.plate_previews.borrow_mut().clear();
        self.model.replace(Some(spatial));
        let recovery_file = crate::recovery::directory()
            .is_some_and(|directory| source_path.parent() == Some(directory.as_path()));
        self.input_path.replace(Some(source_path));
        let is_project = project_payload.is_some();
        let payload = project_payload.unwrap_or_default();
        let overrides = payload
            .process_overrides
            .as_deref()
            .and_then(|text| toml::from_str(text).ok())
            .unwrap_or_default();
        let aa = payload.aa_level;
        let (choices, embedded) = Choices::unpack(payload);
        self.file_kind.set(if is_project && !recovery_file {
            OpenedFrom::ProjectFile
        } else {
            OpenedFrom::MeshFile
        });
        self.choices.replace(choices);
        self.profile_documents.replace(embedded);
        self.picked.set(Selection::None);
        let history_existed = self.reset_baselines(recovery_file);
        self.process_overrides.replace(overrides);
        self.scene_aa.set(aa);
        self.base_process
            .replace(sc_print_profiles::scene_overrides::PlateSettings {
                name: None,
                process: overrides,
                aa,
                first_layer_height: self.chosen_first_layer(),
            });
        self.restore_active_settings();
        self.tell(DocumentEvent::Opened);
        if history_existed {
            self.tell(DocumentEvent::HistoryChanged);
        }
        self.dirty_transition(previous_dirty);
    }

    pub(crate) fn empty(&self) {
        let previous_dirty = self.has_unsaved_changes();
        self.advance_installation();
        self.model.take();
        self.input_path.take();
        self.file_kind.set(OpenedFrom::MeshFile);
        self.choices.replace(Choices::default());
        self.profile_documents.borrow_mut().clear();
        self.picked.set(Selection::None);
        let history_existed = self.reset_baselines(false);
        self.process_overrides.replace(Default::default());
        self.scene_aa.set(None);
        self.tell(DocumentEvent::Emptied);
        if history_existed {
            self.tell(DocumentEvent::HistoryChanged);
        }
        self.dirty_transition(previous_dirty);
    }

    pub(crate) fn has_unsaved_changes(&self) -> bool {
        let head = self.history_head();
        self.has_scene()
            && (self.payload_unsaved.get()
                || head != self.saved_head.get()
                || (head.is_none() && self.save_history_lost.get()))
    }

    pub(crate) fn note_saved(&self) {
        let dirty_before = self.has_unsaved_changes();
        self.saved_head.set(self.history_head());
        self.payload_unsaved.set(false);
        self.save_history_lost.set(false);
        self.dirty_transition(dirty_before);
    }

    /// Select the sole imported leaf; multi-object imports retain a placeholder state.
    pub(crate) fn select_sole_object(&self) {
        let initial_selection = self
            .model
            .borrow()
            .as_ref()
            .and_then(|spatial| {
                let first = spatial.leaf_id_at_index(0)?;
                spatial
                    .leaf_id_at_index(1)
                    .is_none()
                    .then_some(Selection::One(crate::selection::LeafId(first)))
            })
            .unwrap_or(Selection::None);
        self.choose(initial_selection);
    }

    pub(crate) fn project_payload(
        &self,
        default_printer: Option<PrinterId>,
        default_resin: Option<MaterialId>,
    ) -> ProjectPayload {
        let choices = self.choices.borrow();
        let process = self.process_overrides.borrow();
        let overrides = (*process != Default::default())
            .then(|| toml::to_string(&*process).ok())
            .flatten();
        ProjectPayload {
            embedded: self.profile_documents.borrow().to_vec(),
            aa_level: self.scene_aa.get(),
            process_overrides: overrides,
            first_layer_height: choices.first_layer,
            resin: choices.material.clone().or(default_resin),
            printer: choices.machine.clone().or(default_printer),
        }
    }

    pub(crate) fn opened_from_project(&self) -> bool {
        self.file_kind.get() == OpenedFrom::ProjectFile && self.has_scene()
    }

    pub(crate) fn stamp_for_save(&self) -> SavedAt {
        let revision = self.revision();
        SavedAt {
            install: revision.install,
            head: revision.head,
            payload: self.payload_serial.get(),
        }
    }

    pub(crate) fn note_project_saved(
        &self,
        payload: ProjectPayload,
        target: PathBuf,
        prepared: SavedAt,
    ) -> SaveCheck {
        let now = self.stamp_for_save();
        if !self.revision().same_document(Revision {
            install: prepared.install,
            head: prepared.head,
        }) {
            return SaveCheck::Superseded;
        }
        self.input_path.replace(Some(target));
        self.file_kind.set(OpenedFrom::ProjectFile);
        if prepared == now {
            let (choices, documents) = Choices::unpack(payload);
            self.choices.replace(choices);
            self.profile_documents.replace(documents);
            self.note_saved();
            SaveCheck::Current
        } else {
            SaveCheck::ChangedSince
        }
    }

    pub(crate) fn chosen_printer(&self) -> Option<PrinterId> {
        self.choices.borrow().machine.clone()
    }

    pub(crate) fn chosen_resin(&self) -> Option<MaterialId> {
        self.choices.borrow().material.clone()
    }

    pub(crate) fn chosen_first_layer(&self) -> Option<PositiveFiniteMm> {
        self.choices.borrow().first_layer
    }

    pub(crate) fn process_changed(&self) {
        let settings = sc_print_profiles::scene_overrides::PlateSettings {
            name: self.plate_name(),
            process: *self.process_overrides.borrow(),
            aa: self.scene_aa.get(),
            first_layer_height: self.chosen_first_layer(),
        }
        .serialize()
        .ok();
        let active = self.active_plate();
        self.edit_plates(|p| {
            if let Some(p) = p.order.iter_mut().find(|p| p.id == active) {
                p.settings = settings;
            }
        });

        self.slice_state.borrow_mut().changed();
        self.touch_payload();
        self.tell(DocumentEvent::SettingsChanged);
    }

    pub(crate) fn choose_printer(&self, printer: Option<PrinterId>) -> bool {
        self.update_choice(|selection| &mut selection.machine, printer, true)
    }

    /// Fills in the default printer for a scene that names none. Not a
    /// user edit, so the document stays clean (opening a project without
    /// a saved printer read as "unsaved changes" straight away).
    pub(crate) fn fill_default_printer(&self, printer: Option<PrinterId>) -> bool {
        self.chosen_printer().is_none()
            && self.update_choice(|selection| &mut selection.machine, printer, false)
    }

    /// Fills in the default resin; see [`Self::fill_default_printer`].
    pub(crate) fn fill_default_resin(&self, resin: Option<MaterialId>) -> bool {
        self.chosen_resin().is_none()
            && self.update_choice(|selection| &mut selection.material, resin, false)
    }

    pub(crate) fn choose_resin(&self, resin: Option<MaterialId>) -> bool {
        self.update_choice(|selection| &mut selection.material, resin, true)
    }

    pub(crate) fn choose_first_layer(&self, height: Option<PositiveFiniteMm>) -> bool {
        self.update_choice(|selection| &mut selection.first_layer, height, true)
    }

    /// save-dirty (when `user_edit`) only when the value actually changes.
    /// Returns whether it changed.
    fn update_choice<T: PartialEq>(
        &self,
        field: impl FnOnce(&mut Choices) -> &mut Option<T>,
        value: Option<T>,
        user_edit: bool,
    ) -> bool {
        {
            let mut choices = self.choices.borrow_mut();
            let destination = field(&mut choices);
            if destination == &value {
                return false;
            }
            *destination = value;
        }
        self.slice_state.borrow_mut().changed();
        if user_edit {
            self.touch_payload();
        }
        self.tell(DocumentEvent::SettingsChanged);
        true
    }

    pub(crate) fn revision(&self) -> Revision {
        Revision {
            install: self.installation_serial.get(),
            head: self.history_head(),
        }
    }

    pub(crate) fn slice_outdated_by_geometry(&self, since: Revision) -> bool {
        since != self.revision()
            || (since.head.is_none() && self.slice_history_lost.get())
    }

    pub(crate) fn keep_slice(&self, result: SliceResult) {
        let plate = self.active_plate();
        self.plate_results
            .borrow_mut()
            .insert(plate, result.clone());
        self.held_slice.replace(Some(result));
        self.slice_history_lost.set(false);
        self.slice_serial
            .set(self.slice_serial.get().wrapping_add(1));
    }

    /// The layer grid of the held slice.
    pub(crate) fn slice_layer_schedule(&self) -> Option<LayerSchedule> {
        self.read_slice(|result| result.schedule)
    }

    pub(crate) fn slicing_inputs(&self) -> Option<SliceParams> {
        self.read_slice(|result| result.params.clone())
    }

    /// Whether the scene's geometry has changed since the held slice
    /// ran, i.e. whether anything derived from that slice (the layer
    /// view, the print-time and resin-volume estimates) no longer
    /// reflects the live scene. `false` while no slice is held: there
    /// is nothing yet to be stale relative to.
    ///
    /// Geometry only, deliberately: a printer/resin/AA-level change
    /// without a re-slice also makes a held slice stale, but nothing
    /// in the GUI reacts to that today, so this does not claim to
    /// catch it. [`SliceAgain::compare`](crate::slice_result::SliceAgain::compare)
    /// is the fuller check the Slice button itself already runs at
    /// press time, comparing every scalar plus geometry together.
    pub(crate) fn slice_is_geometry_stale(&self) -> bool {
        self.slicing_inputs()
            .is_some_and(|params| self.slice_outdated_by_geometry(params.geometry))
    }

    pub(crate) fn layer_total(&self) -> Option<u32> {
        self.read_slice(|result| result.schedule.total_layers())
    }

    pub(crate) fn layer_top_mm(&self, index: u32) -> Option<f64> {
        self.read_slice(|result| result.schedule.iter_slab_tops().nth(index as usize))
            .flatten()
    }

    /// Returns the held slice's merged contours and the slicer-side
    /// profile they were produced under, for an "Export Sliced File"
    /// flow to snapshot off the event loop. `None` when no slice is
    /// held. The clone is cheap relative to a write: `ContourStack`
    /// holds its own owned geometry, so this is one allocation, not a
    /// deep per-vertex copy of shared mesh buffers.
    pub(crate) fn slice_export_data(&self) -> Option<(ContourStack, SliceProfile)> {
        self.read_slice(|result| (result.merged.clone(), result.profile.clone()))
    }

    pub(crate) fn slice_serial_now(&self) -> u64 {
        self.slice_serial.get()
    }

    pub(crate) fn slice_accuracy(
        &self,
    ) -> (
        sc_sl1cer::raster::accuracy::Accuracy,
        sc_print_primitives::AaLevel,
    ) {
        self.held_slice
            .borrow()
            .as_ref()
            .map(|r| (r.profile.accuracy(), r.profile.aa_level()))
            .unwrap_or((Default::default(), sc_print_primitives::AaLevel::Off))
    }

    pub(crate) fn read_layer<R>(
        &self,
        index: u32,
        f: impl FnOnce(&ContourLayer) -> R,
    ) -> Option<R> {
        self.read_slice(|result| result.merged.layers().get(index as usize).map(f))
            .flatten()
    }

    /// Thin-feature pixels at export resolution, unmirrored for the 2D view.
    pub(crate) fn slice_thin_spans(&self, index: u32, minimum_mm: f64) -> Option<IslandSpans> {
        let held = self.held_slice.borrow();
        let result = held.as_ref()?;
        let layer = result.merged.layers().get(index as usize)?;
        let b = result.profile.build_volume();
        let res = *result.profile.pixel_resolution();
        let map =
            sc_sl1cer::raster::pixel_map::PixelMap::new(b.x(), b.y(), res, Default::default());
        let raster = sc_sl1cer::raster::accuracy::rasterize_accurate(
            layer,
            &map,
            result.profile.aa_level(),
            0.0,
            result.profile.accuracy(),
            index as usize,
        );
        Some((
            sc_print_job::risks::thin_feature_spans(
                &raster,
                (b.x_mm() / f64::from(res.x()), b.y_mm() / f64::from(res.y())),
                minimum_mm,
            ),
            res.x(),
            res.y(),
        ))
    }

    /// Unsupported-island spans of held layer `index`, computed at the
    /// printer's own resolution (the same resolution the island count
    /// uses; a coarser display raster can hide thin overhangs) and
    /// un-mirrored to match the on-screen cap. Returns the `(row, x0, x1)`
    /// spans in bottom-to-top row order plus the resolution they are in.
    /// `None` for layer 0 (it sits on the plate) or with no slice held.
    pub(crate) fn slice_island_spans(&self, index: u32) -> Option<IslandSpans> {
        if index == 0 {
            return None;
        }
        let index = usize::try_from(index).ok()?;
        let held = self.held_slice.borrow();
        let result = held.as_ref()?;
        let layers = result.merged.layers();
        let (layer, below) = (layers.get(index)?, layers.get(index - 1)?);
        let build = result.profile.build_volume();
        let resolution = *result.profile.pixel_resolution();
        let map = sc_sl1cer::raster::pixel_map::PixelMap::new(
            build.x(),
            build.y(),
            resolution,
            sc_print_primitives::Mirror { x: false, y: false },
        );
        let spans = sc_print_job::island_spans(
            &sc_sl1cer::raster::accuracy::rasterize_accurate(
                layer,
                &map,
                result.profile.aa_level(),
                0.0,
                result.profile.accuracy(),
                index,
            ),
            &sc_sl1cer::raster::accuracy::rasterize_accurate(
                below,
                &map,
                result.profile.aa_level(),
                0.0,
                result.profile.accuracy(),
                index - 1,
            ),
            sc_print_job::self_supporting_reach_px(&result.profile),
            sc_print_job::cure_bleed_px(&result.profile),
        );
        Some((spans, resolution.x(), resolution.y()))
    }

    pub(crate) fn slices_to_save(
        &self,
        current_schedule: &LayerSchedule,
    ) -> Option<(PlateSlices, String)> {
        self.read_slice(|result| {
            if &result.schedule != current_schedule
                || self.slice_outdated_by_geometry(result.params.geometry)
            {
                return None;
            }
            Some((
                PlateSlices {
                    slab_tops: result.schedule.iter_slab_tops().collect(),
                    contours: result.merged.clone(),
                },
                result.token.to_owned(),
            ))
        })
        .flatten()
    }

    fn history_head(&self) -> Option<u64> {
        self.past.borrow().iter().next_back().map(|item| item.serial)
    }

    fn dirty_transition(&self, was_dirty: bool) {
        match (was_dirty, self.has_unsaved_changes()) {
            (false, true) | (true, false) => self.tell(DocumentEvent::UnsavedChanged),
            _ => {}
        }
    }

    fn touch_payload(&self) {
        let previous = self.has_unsaved_changes();
        self.payload_serial
            .set(self.payload_serial.get().wrapping_add(1));
        self.payload_unsaved.set(true);
        self.dirty_transition(previous);
    }

    fn advance_installation(&self) {
        self.installation_serial
            .set(self.installation_serial.get().wrapping_add(1));
    }

    fn discard_history(&self) -> bool {
        let past = self.past.take();
        let future = self.future.take();
        !(past.is_empty() && future.is_empty())
    }

    pub(crate) fn chosen(&self) -> Selection {
        self.picked.current()
    }

    /// Toggle membership without changing the document or undo history.
    pub(crate) fn toggle_selection(&self, id: NodeId) {
        let mut ids = self.chosen().ids();
        if let Some(index) = ids.iter().position(|&v| v == id) {
            ids.remove(index);
        } else {
            ids.push(id);
        }
        self.choose(Selection::from_ids(ids));
    }

    pub(crate) fn select_all(&self) {
        let ids = self
            .read_scene(|s| {
                s.graph()
                    .reachable_leaves(
                        sc_scene::CsgPolicy::Descend,
                        sc_scene::ReferencePolicy::Skip,
                    )
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        self.choose(Selection::from_ids(ids));
    }

    pub(crate) fn choose(&self, sel: Selection) {
        let ids = sel.ids();
        if let Some(plate) = ids
            .first()
            .and_then(|id| self.read_scene(|s| s.graph().plate_of(*id)))
        {
            self.set_active_plate(plate);
        }
        if sel != self.chosen() {
            self.picked.set(sel);
            self.tell(DocumentEvent::ChoiceChanged);
        }
    }

    pub(crate) fn main_chosen_node(&self) -> Option<NodeId> {
        self.picked.main_node()
    }

    pub(crate) fn commit_edit(&self, edit: SceneEdit) -> Result<(), EditRefused> {
        let (event, removed_selection, changed_plates) = {
            let mut storage = self.model.borrow_mut();
            let scene = storage.as_mut().ok_or(EditRefused::Empty)?;
            let prepared = with_following_supports(scene.graph(), edit);
            let event = if prepared.is_structural() {
                DocumentEvent::Regrouped
            } else {
                DocumentEvent::Edited
            };
            let removed = removes_selection(&prepared, &self.picked);
            let mut plates = false;
            visit_edits(&prepared, &mut |item| {
                plates |= matches!(item, SceneEdit::SetPlates { .. })
            });
            scene
                .apply_edit(prepared)
                .map_err(EditRefused::Refused)?;
            (event, removed, plates)
        };
        if changed_plates {
            self.restore_active_settings();
        }
        if removed_selection {
            let remaining = self
                .chosen()
                .ids()
                .into_iter()
                .filter(|id| {
                    self.read_scene(|s| s.graph().node(*id).is_some())
                        .unwrap_or(false)
                })
                .collect();
            self.picked.set(Selection::from_ids(remaining));
        }
        self.tell(event);
        if removed_selection {
            self.tell(DocumentEvent::ChoiceChanged);
        }
        Ok(())
    }

    pub(crate) fn record_edit(&self, edit: SceneEdit) {
        if !edit.is_noop() {
            let previous = self.has_unsaved_changes();
            let serial = self.edit_serial.get();
            self.edit_serial.set(serial + 1);
            self.append_history(&self.past, PastEdit { serial, change: edit }, true);
            self.future.borrow_mut().clear();
            self.history_changed(previous);
        }
    }

    /// Atomically replace generated supports and model transforms as one undo entry.
    pub(crate) fn apply_support_scene(
        &self,
        generated: &Scene,
        model_roots: &[NodeId],
    ) -> Result<(), EditRefused> {
        self.apply_support_scene_scoped(generated, model_roots, None)
    }

    pub(crate) fn apply_support_scene_scoped(
        &self,
        generated: &Scene,
        model_roots: &[NodeId],
        selected: Option<NodeId>,
    ) -> Result<(), EditRefused> {
        let edits = {
            let mut slot = self.model.borrow_mut();
            let spatial = slot.as_mut().ok_or(EditRefused::Empty)?;
            let graph = spatial.graph();
            let mut edits = Vec::new();
            let old: Vec<_> = crate::auto_support_run::support_ids(graph)
                .into_iter()
                .filter(|&id| graph.plate_of(id) == self.active_plate.get())
                .filter(|&id| {
                    selected.is_none_or(|owner| {
                        graph
                            .node(id)
                            .and_then(|n| n.meta.as_ref())
                            .and_then(|m| m.comment.as_deref())
                            .is_some_and(|c| {
                                c.lines().any(|l| l == format!("owner = {}", owner.get()))
                            })
                    })
                })
                .collect();
            for id in old.iter().rev() {
                let node = graph.node(*id).unwrap();
                let NodeKind::Leaf(payload) = &node.kind else {
                    continue;
                };
                edits.push(SceneEdit::RemoveLeaf(sc_scene_edit::LeafRecord {
                    node_id: *id,
                    attachment: graph.attachment_of(*id).unwrap(),
                    payload: payload.clone(),
                    local_transform: node.local_transform,
                    meta: node.meta.clone(),
                }));
            }
            let targets = selected.map_or_else(|| model_roots.to_vec(), |id| vec![id]);
            for id in &targets {
                if let (Some(before), Some(after)) = (graph.node(*id), generated.node(*id)) {
                    edits.push(SceneEdit::SetLocalTransform {
                        node_id: *id,
                        before: before.local_transform,
                        after: if selected.is_some() {
                            let parent = graph
                                .world_transform_of(*id)
                                .map_err(EditRefused::Refused)?
                                .compose(
                                    &before
                                        .local_transform
                                        .inverse()
                                        .map_err(|_| EditRefused::Empty)?,
                                );
                            parent
                                .inverse()
                                .map_err(|_| EditRefused::Empty)?
                                .compose(
                                    &generated
                                        .world_transform_of(*id)
                                        .map_err(EditRefused::Refused)?,
                                )
                        } else {
                            after.local_transform
                        },
                    });
                }
            }
            let first_position =
                graph.roots().len() - old.iter().filter(|id| graph.roots().contains(id)).count();
            let plate_before = spatial.graph().plates().clone();
            let mut plate_after = plate_before.clone();
            let mut new_meshes = Vec::new();
            let support_ids = crate::auto_support_run::support_ids(generated);
            generated
                .for_each_expanded_leaf::<SceneError, _>(
                    sc_scene::CsgPolicy::Descend,
                    |id, transform, mesh| {
                        if support_ids.contains(&id)
                            || (generated.roots().contains(&id) && !model_roots.contains(&id))
                        {
                            let mut meta = generated.node(id).and_then(|n| n.meta.clone());
                            if let Some(owner) = support_owner(generated, id)
                                && let Some(comment) =
                                    meta.as_mut().and_then(|m| m.comment.as_mut())
                                && !comment.lines().any(|l| l.starts_with("owner_linear = "))
                            {
                                let linear = match generated.world_transform_of(owner)? {
                                    Transform3D::Identity => {
                                        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
                                    }
                                    Transform3D::Affine(a) => *a.linear(),
                                };
                                if !comment.ends_with('\n') {
                                    comment.push('\n');
                                }
                                comment.push_str(&format!("owner_linear = {linear:?}\n"));
                            }
                            new_meshes.push((Arc::new(mesh.clone()), meta, *transform));
                        }
                        Ok(())
                    },
                )
                .map_err(EditRefused::Refused)?;
            for (position, (mesh, generated_meta, local_transform)) in
                (first_position..).zip(new_meshes)
            {
                let id = spatial.allocate_unbound_node_id();
                plate_after.assignments.insert(id, self.active_plate.get());
                edits.push(SceneEdit::AddLeaf(sc_scene_edit::LeafRecord {
                    node_id: id,
                    attachment: LeafAttachment::Root { position },
                    payload: LeafPayload::Mesh(mesh),
                    local_transform,
                    meta: Some(sc_scene::node::SceneMetadata {
                        name: Some(format!("Support {}", position + 1)),
                        comment: generated_meta
                            .and_then(|m| m.comment)
                            .or_else(|| Some("sc-generated-support".into())),
                    }),
                }));
            }
            edits.push(SceneEdit::SetPlates {
                before: plate_before,
                after: plate_after,
            });
            SceneEdit::Compound(edits)
        };
        self.commit_edit(edits.clone())?;
        self.record_edit(edits);
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn add_model(
        &self,
        mesh: MeshResourceId,
        local_transform: Transform3D,
    ) -> Result<Option<NodeId>, SceneError> {
        let edit = {
            let mut storage = self.model.borrow_mut();
            let Some(scene) = storage.as_mut() else {
                return Ok(None);
            };
            let attachment = LeafAttachment::Root {
                position: scene.graph().roots().len(),
            };
            SceneEdit::AddLeaf(sc_scene_edit::LeafRecord {
                node_id: scene.allocate_unbound_node_id(),
                attachment,
                payload: LeafPayload::MeshResource(mesh),
                local_transform,
                meta: None,
            })
        };
        let SceneEdit::AddLeaf(sc_scene_edit::LeafRecord { node_id, .. }) = edit else {
            unreachable!()
        };
        self.commit_scene_edit(edit)
            .map(|done| done.then_some(node_id))
    }

    pub(crate) fn delete_chosen(&self) -> Result<bool, SceneError> {
        let selection = self.chosen().ids();
        let planned = self
            .read_scene(|scene| {
                let mut targets = selection.clone();
                for id in selection {
                    for support in owned_support_ids(scene.graph(), id) {
                        if !targets.contains(&support) {
                            targets.push(support);
                        }
                    }
                }
                let mut scratch = scene.graph().clone();
                let mut removals = Vec::new();
                for id in targets {
                    if let Some(removal) = SceneEdit::remove_leaf_with_gc(&scratch, id) {
                        removal.apply_to_scene(&mut scratch).ok()?;
                        removals.push(removal);
                    }
                }
                pack_edits(removals)
            })
            .flatten();
        match planned {
            Some(edit) => self.commit_scene_edit(edit),
            None => Ok(false),
        }
    }

    pub(crate) fn paste_parts(
        &self,
        fragment: &SceneFragment,
        placement_translation: [f64; 3],
    ) -> Result<Option<NodeId>, SceneError> {
        let Some((insertion, primary)) = self.stage_paste(fragment, placement_translation) else {
            return Ok(None);
        };
        let mut inserted = Vec::new();
        visit_edits(&insertion, &mut |item| {
            if let SceneEdit::AddLeaf(sc_scene_edit::LeafRecord { node_id, .. }) = item {
                inserted.push(*node_id);
            }
        });
        let before = self
            .read_scene(|s| s.graph().plates().clone())
            .unwrap_or_default();
        let mut after = before.clone();
        for id in &inserted {
            after.assignments.insert(*id, self.active_plate());
        }
        if !self.commit_scene_edit(SceneEdit::Compound(vec![
            insertion,
            SceneEdit::SetPlates { before, after },
        ]))? {
            return Ok(None);
        }
        let models = self
            .read_scene(|s| {
                inserted
                    .iter()
                    .copied()
                    .filter(|id| support_owner(s.graph(), *id).is_none())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.choose(Selection::from_ids(if models.is_empty() {
            inserted
        } else {
            models
        }));
        Ok(Some(primary))
    }

    fn stage_paste(
        &self,
        fragment: &SceneFragment,
        placement_translation: [f64; 3],
    ) -> Option<(SceneEdit, NodeId)> {
        let mut storage = self.model.borrow_mut();
        let scene = storage.as_mut()?;
        let source = fragment.scene();
        let mut resources = HashMap::new();
        let mut transaction = Vec::new();
        for (old_id, _) in source.mesh_resources_iter() {
            let mesh = source
                .mesh_resource_arc(old_id)
                .expect("frag_scene yielded a live rid");
            let new_id = scene
                .graph()
                .find_mesh_resource_by_arc(mesh)
                .unwrap_or_else(|| {
                    let rid = scene.allocate_unbound_mesh_resource_id();
                    transaction.push(SceneEdit::AddMeshResource(sc_scene_edit::MeshRecord {
                        rid,
                        mesh: mesh.clone(),
                    }));
                    rid
                });
            resources.insert(old_id, new_id);
        }
        // Reserve even unsupported roots, keeping the allocator's observable sequence.
        let mapping: HashMap<_, _> = source
            .roots()
            .iter()
            .map(|id| (*id, scene.allocate_unbound_node_id()))
            .collect();
        let mut root_position = scene.graph().roots().len();
        let mut primary = None;
        for source_id in source.roots() {
            let node = source
                .node(*source_id)
                .expect("frag_scene.roots() yielded a live id");
            let NodeKind::Leaf(LeafPayload::MeshResource(resource)) = node.kind else {
                continue;
            };
            let node_id = mapping[source_id];
            primary.get_or_insert(node_id);
            let transform = match source.roots().len() {
                1 => reanchor(node.local_transform, placement_translation),
                _ => {
                    Transform3D::Affine(AffineTransform3D::from_translation(placement_translation))
                        .compose(&node.local_transform)
                }
            };
            let mut metadata = node.meta.clone();
            if let Some(new_owner) =
                support_owner(source, *source_id).and_then(|id| mapping.get(&id))
                && let Some(text) = metadata.as_mut().and_then(|meta| meta.comment.as_mut())
            {
                let mut lines = text
                    .lines()
                    .filter(|line| !line.starts_with("owner = "))
                    .collect::<Vec<_>>();
                lines.push("");
                *text = lines.join("\n");
                text.push_str(&format!("owner = {}\n", new_owner.get()));
            }
            transaction.push(SceneEdit::AddLeaf(sc_scene_edit::LeafRecord {
                node_id,
                attachment: LeafAttachment::Root {
                    position: root_position,
                },
                payload: LeafPayload::MeshResource(resources[&resource]),
                local_transform: transform,
                meta: metadata,
            }));
            root_position += 1;
        }
        Some((pack_edits(transaction)?, primary?))
    }

    pub(crate) fn has_past(&self) -> bool {
        !self.past.borrow().is_empty()
    }

    pub(crate) fn has_future(&self) -> bool {
        !self.future.borrow().is_empty()
    }

    pub(crate) fn step_back(&self) -> Result<bool, SceneError> {
        self.replay_history(false)
    }

    pub(crate) fn step_forward(&self) -> Result<bool, SceneError> {
        self.replay_history(true)
    }

    pub(crate) fn watch<F>(self: &Rc<Self>, listener: F) -> Watch
    where
        F: Fn(DocumentEvent) + 'static,
    {
        let token = self.observer_serial.get();
        self.observer_serial.set(token.wrapping_add(1));
        let callback: Observer = Rc::new(listener);
        self.observers.borrow_mut().push((token, callback));
        Watch {
            owner: Rc::downgrade(self),
            key: token,
        }
    }

    fn tell(&self, event: DocumentEvent) {
        if event == DocumentEvent::Edited || event == DocumentEvent::Regrouped {
            self.slice_state.borrow_mut().changed();
        }
        // Dispatch outside the borrow: observers may change subscriptions.
        let callbacks = self.observers.borrow().clone();
        for (_, callback) in callbacks {
            callback(event);
        }
    }

    pub(crate) fn file_path(&self) -> Option<PathBuf> {
        self.input_path.borrow().clone()
    }

    pub(crate) fn read_file_path<R>(&self, f: impl FnOnce(&Path) -> R) -> Option<R> {
        self.input_path.borrow().as_deref().map(f)
    }

    pub(crate) fn file_name(&self) -> Option<String> {
        self.read_file_path(|path| {
            path.file_name()
                .and_then(|part| part.to_str())
                .map(String::from)
        })
        .flatten()
    }

    pub(crate) fn read_scene<R>(&self, f: impl FnOnce(&SceneSpatial) -> R) -> Option<R> {
        self.model.borrow().as_ref().map(f)
    }

    pub(crate) fn has_scene(&self) -> bool {
        self.model.borrow().is_some()
    }

    /// Append a batch, placing only the new leaves around existing objects.
    /// Planning is atomic: a batch that cannot fit leaves scene/history untouched.
    pub(crate) fn import_geometry(
        &self,
        sources: &[Scene],
        volume: crate::build_volume::BuildVolume,
    ) -> Result<(), String> {
        let mut trial = self.read_scene(|s| s.graph().clone()).unwrap_or_default();
        let (edit, ids) =
            crate::layout_tools::import_geometry(&mut trial, sources, volume, self.active_plate())?;
        if !self.has_scene() {
            self.take_spatial(SceneSpatial::from_graph(Scene::new()), PathBuf::new(), None);
        }
        {
            let mut slot = self.model.borrow_mut();
            let spatial = slot.as_mut().ok_or("Missing scene")?;
            for &id in &ids {
                assert_eq!(spatial.allocate_unbound_node_id(), id);
            }
        }
        self.commit_edit(edit.clone())
            .map_err(|e| format!("{e:?}"))?;
        self.record_edit(edit);
        self.choose(Selection::from_ids(ids));
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn fill_plate(
        &self,
        volume: crate::build_volume::BuildVolume,
        gap: f64,
    ) -> Option<usize> {
        self.fill_plate_after(None, volume, gap)
    }

    /// Fill plate preceded by `first` (Fill plate's re-orientation), recorded
    /// as one undo step. `first` is rolled back if no fill can be planned.
    pub(crate) fn fill_plate_after(
        &self,
        first: Option<SceneEdit>,
        volume: crate::build_volume::BuildVolume,
        gap: f64,
    ) -> Option<usize> {
        if let Some(edit) = &first {
            self.commit_edit(edit.clone()).ok()?;
        }
        let planned = self.plan_fill_plate(volume, gap);
        let applied = planned.and_then(|(edit, count)| {
            self.commit_edit(edit.clone()).ok()?;
            Some((edit, count))
        });
        let Some((edit, count)) = applied else {
            if let Some(first) = first {
                let _ = self.commit_edit(first.inverse());
            }
            return None;
        };
        self.record_edit(match first {
            Some(first) => SceneEdit::Compound(vec![first, edit]),
            None => edit,
        });
        Some(count)
    }

    fn plan_fill_plate(
        &self,
        volume: crate::build_volume::BuildVolume,
        gap: f64,
    ) -> Option<(SceneEdit, usize)> {
        let selected = self.main_chosen_node()?;
        let (edit, count) = {
            let mut slot = self.model.borrow_mut();
            let spatial = slot.as_mut()?;
            // A selected support fills the plate with its model (and the
            // model's supports), never with bare support columns.
            let selected =
                if crate::auto_support_run::support_ids(spatial.graph()).contains(&selected) {
                    support_owner(spatial.graph(), selected)?
                } else {
                    selected
                };
            let active = self.active_plate.get();
            let mut planning = spatial.graph().plate_scene(active).ok()?;
            let (mut edit, count) =
                crate::layout_tools::fill_plate(&mut planning, selected, volume, gap)?;
            fn allocate(spatial: &mut SceneSpatial, edit: &SceneEdit) {
                match edit {
                    SceneEdit::AddLeaf(sc_scene_edit::LeafRecord { node_id, .. }) => {
                        assert_eq!(spatial.allocate_unbound_node_id(), *node_id)
                    }
                    SceneEdit::Compound(edits) => {
                        for edit in edits {
                            allocate(spatial, edit);
                        }
                    }
                    _ => {}
                }
            }
            allocate(spatial, &edit);
            let before = spatial.graph().plates().clone();
            let mut after = before.clone();
            fn assign(edit: &SceneEdit, after: &mut sc_scene::plates::Plates, active: u32) {
                match edit {
                    SceneEdit::AddLeaf(sc_scene_edit::LeafRecord { node_id, .. }) => {
                        after.assignments.insert(*node_id, active);
                    }
                    SceneEdit::Compound(edits) => {
                        for edit in edits {
                            assign(edit, after, active);
                        }
                    }
                    _ => {}
                }
            }
            assign(&edit, &mut after, active);
            edit = SceneEdit::Compound(vec![edit, SceneEdit::SetPlates { before, after }]);
            (edit, count)
        };
        Some((edit, count))
    }
}

fn node_comment(graph: &Scene, id: NodeId) -> Option<&str> {
    graph
        .node(id)
        .and_then(|n| n.meta.as_ref())
        .and_then(|m| m.comment.as_deref())
}

/// Generated support leaves that belong to `model`: columns tagged with
/// its `owner = <id>` line. Legacy unowned leaves resolve to the nearest model.
pub(crate) fn owned_support_ids(graph: &Scene, model: NodeId) -> Vec<NodeId> {
    if node_comment(graph, model).is_some_and(|c| c.starts_with("sc-generated-support")) {
        return Vec::new();
    }
    crate::auto_support_run::support_ids(graph)
        .into_iter()
        .filter(|&id| support_owner(graph, id) == Some(model))
        .collect()
}

/// The model a generated support leaf belongs to: the leaf named by its
/// `owner = <id>` line, or the nearest model for legacy unowned geometry.
/// `None` for non-support leaves and for supports with no live owner.
pub(crate) fn support_owner(graph: &Scene, support: NodeId) -> Option<NodeId> {
    let comment = node_comment(graph, support)?;
    if !comment.starts_with("sc-generated-support") {
        return None;
    }
    let owner = comment
        .lines()
        .find_map(|l| l.strip_prefix("owner = ")?.parse::<u32>().ok());
    let models = graph
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .ok()?
        .into_iter()
        .filter(|&id| {
            !node_comment(graph, id).is_some_and(|c| c.starts_with("sc-generated-support"))
        });
    match owner {
        Some(owner) => models.into_iter().find(|id| id.get() == owner),
        None => {
            let centre = |id| {
                let mesh = sc_scene::bake_leaf_to_world(graph, id).ok()?;
                let b = mesh.local_aabb()?;
                let a = b.min().coords();
                let z = b.max().coords();
                Some([(a[0] + z[0]) / 2.0, (a[1] + z[1]) / 2.0])
            };
            let position = centre(support).unwrap_or([0.0; 2]);
            models.min_by(|&a, &b| {
                let distance = |id| {
                    centre(id).map_or(f32::INFINITY, |p| {
                        (p[0] - position[0]).powi(2) + (p[1] - position[1]).powi(2)
                    })
                };
                distance(a).total_cmp(&distance(b))
            })
        }
    }
}

/// Whether the owner's linear transform differs from the contact-generation baseline.
/// Translation is deliberately excluded: supports follow translations.
pub(crate) fn support_is_stale(graph: &Scene, support: NodeId) -> bool {
    let Some(owner) = support_owner(graph, support) else {
        return false;
    };
    let Some(comment) = node_comment(graph, support) else {
        return false;
    };
    if comment.lines().any(|l| l == "geometry_stale = true") {
        return true;
    }
    let baseline = comment
        .lines()
        .find_map(|l| l.strip_prefix("owner_linear = "))
        .and_then(|text| toml::from_str::<toml::Value>(&format!("value = {text}")).ok())
        .and_then(|value| {
            value
                .get("value")?
                .as_array()?
                .iter()
                .map(|row| {
                    row.as_array()?
                        .iter()
                        .map(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
                        .collect::<Option<Vec<_>>>()
                })
                .collect::<Option<Vec<_>>>()
        });
    let Some(baseline) =
        baseline.filter(|rows| rows.len() == 3 && rows.iter().all(|r| r.len() == 3))
    else {
        return false;
    };
    let linear = match graph.world_transform_of(owner) {
        Ok(Transform3D::Identity) => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        Ok(Transform3D::Affine(a)) => *a.linear(),
        Err(_) => return false,
    };
    (0..3).any(|r| (0..3).any(|c| (linear[r][c] - baseline[r][c]).abs() > 1e-8))
}

/// Generated supports are separate root leaves with world-space meshes, so
/// a transform edit on a supported model used to leave its supports standing
/// where they were. Every `SetLocalTransform` in `edit` (top level or inside
/// a compound) whose world change is a pure translation now also translates
/// the model's owned supports by the same offset, unless the edit already
/// touches those supports itself (arrange moves them explicitly; support
/// regeneration removes them). Undo/redo replay edits through `apply_edit`,
/// so the supports follow the inverse the same way. Rotations and scales
/// change the contact geometry, so the supports stay put for those and need
/// regenerating.
fn with_following_supports(graph: &Scene, edit: SceneEdit) -> SceneEdit {
    fn walk(edit: &SceneEdit, moves: &mut Vec<(NodeId, Transform3D)>, touched: &mut Vec<NodeId>) {
        match edit {
            SceneEdit::SetLocalTransform { node_id, after, .. } => {
                moves.push((*node_id, *after));
                touched.push(*node_id);
            }
            SceneEdit::RemoveLeaf(sc_scene_edit::LeafRecord { node_id, .. }) | SceneEdit::ReplaceLeafMesh { node_id, .. } => {
                touched.push(*node_id)
            }
            SceneEdit::Compound(edits) => edits.iter().for_each(|e| walk(e, moves, touched)),
            _ => {}
        }
    }
    let (mut moves, mut touched) = (Vec::new(), Vec::new());
    walk(&edit, &mut moves, &mut touched);
    let translation_of = |node_id: NodeId, after: &Transform3D| -> Option<[f64; 3]> {
        let current = graph.node(node_id)?.local_transform;
        let world = graph.world_transform_of(node_id).ok()?;
        let parent = world.compose(&current.inverse().ok()?);
        let Transform3D::Affine(delta) = parent.compose(after).compose(&world.inverse().ok()?)
        else {
            return None;
        };
        let linear = delta.linear();
        let pure_translation = (0..3)
            .all(|r| (0..3).all(|c| (linear[r][c] - if r == c { 1.0 } else { 0.0 }).abs() < 1e-9));
        let moves = delta.translation().iter().any(|v| v.abs() > 1e-12);
        (pure_translation && moves).then(|| *delta.translation())
    };
    let mut follows = Vec::new();
    for (node_id, after) in &moves {
        // A node edited twice in one compound has no single "from" state.
        if moves.iter().filter(|(id, _)| id == node_id).count() > 1 {
            continue;
        }
        let Some(delta) = translation_of(*node_id, after) else {
            // Old projects have no baseline. Capture the pre-edit transform on
            // their first linear edit so they participate in stale warnings too.
            let linear = match graph.world_transform_of(*node_id) {
                Ok(Transform3D::Identity) => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                Ok(Transform3D::Affine(a)) => *a.linear(),
                Err(_) => continue,
            };
            for id in owned_support_ids(graph, *node_id) {
                if touched.contains(&id)
                    || node_comment(graph, id)
                        .is_some_and(|c| c.lines().any(|l| l.starts_with("owner_linear = ")))
                {
                    continue;
                }
                let Some(node) = graph.node(id) else {
                    continue;
                };
                let NodeKind::Leaf(payload) = &node.kind else {
                    continue;
                };
                let Some(attachment) = graph.attachment_of(id) else {
                    continue;
                };
                let mut meta = node.meta.clone().unwrap_or_default();
                let comment = meta.comment.get_or_insert_with(String::new);
                if !comment.ends_with('\n') {
                    comment.push('\n');
                }
                comment.push_str(&format!("owner_linear = {linear:?}\n"));
                follows.push(SceneEdit::RemoveLeaf(sc_scene_edit::LeafRecord {
                    node_id: id,
                    attachment,
                    payload: payload.clone(),
                    local_transform: node.local_transform,
                    meta: node.meta.clone(),
                }));
                follows.push(SceneEdit::AddLeaf(sc_scene_edit::LeafRecord {
                    node_id: id,
                    attachment,
                    payload: payload.clone(),
                    local_transform: node.local_transform,
                    meta: Some(meta),
                }));
                touched.push(id);
            }
            continue;
        };
        let shift = Transform3D::Affine(AffineTransform3D::from_translation(delta));
        for id in owned_support_ids(graph, *node_id) {
            if touched.contains(&id)
                || !matches!(graph.attachment_of(id), Some(LeafAttachment::Root { .. }))
            {
                continue;
            }
            let Some(before) = graph.node(id).map(|n| n.local_transform) else {
                continue;
            };
            touched.push(id);
            follows.push(SceneEdit::SetLocalTransform {
                node_id: id,
                before,
                after: shift.compose(&before),
            });
        }
    }
    if follows.is_empty() {
        return edit;
    }
    match edit {
        SceneEdit::Compound(mut edits) => {
            edits.extend(follows);
            SceneEdit::Compound(edits)
        }
        edit => SceneEdit::Compound(std::iter::once(edit).chain(follows).collect()),
    }
}

fn reanchor(xform: Transform3D, translation: [f64; 3]) -> Transform3D {
    let linear = match xform {
        Transform3D::Affine(matrix) => *matrix.linear(),
        Transform3D::Identity => {
            return Transform3D::Affine(AffineTransform3D::from_translation(translation));
        }
    };
    Transform3D::Affine(AffineTransform3D::from_linear_and_translation(
        linear,
        translation,
    ))
}

#[derive(Debug)]
pub(crate) enum EditRefused {
    Empty,
    Refused(SceneError),
}

fn removes_selection(edit: &SceneEdit, selection: &SelectionState) -> bool {
    let selected = selection.current().ids();
    let mut removed = false;
    visit_edits(edit, &mut |item| {
        if let SceneEdit::RemoveLeaf(sc_scene_edit::LeafRecord { node_id, .. }) = item {
            removed |= selected.contains(node_id);
        }
    });
    removed
}

pub(crate) struct Watch {
    key: u64,
    owner: Weak<SceneDocument>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        owner
            .observers
            .borrow_mut()
            .retain(|entry| entry.0 != self.key);
    }
}

#[cfg(test)]
#[path = "tests_project_process.rs"]
mod tests_project_process;

// A compound's order matters; use the same depth-first order for every inspection.
fn visit_edits(edit: &SceneEdit, inspect: &mut impl FnMut(&SceneEdit)) {
    match edit {
        SceneEdit::Compound(children) => {
            for child in children {
                visit_edits(child, inspect);
            }
        }
        item => inspect(item),
    }
}

fn pack_edits(mut edits: Vec<SceneEdit>) -> Option<SceneEdit> {
    match edits.len() {
        0 => None,
        1 => Some(edits.pop().expect("len == 1")),
        _ => Some(SceneEdit::Compound(edits)),
    }
}

#[cfg(test)]
#[path = "tests_scene_document.rs"]
mod tests;

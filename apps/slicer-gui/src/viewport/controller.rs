// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! One window's 3D view: it owns the session (document, loader, dialogs),
//! the camera, the frame host and the input router, and most of its
//! methods hand a call to one of them. `graphics_ready` needs two at once:
//! the session gets the new GPU device (and opens a file that was waiting
//! for one), the frame host gets the renderer and a redraw. Slint hands
//! the device to whatever answers `rendering-setup`, so the wgpu types are
//! imported here, where both halves meet.

use std::{cell::Cell, rc::Rc};

use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use slint::{ComponentHandle, SharedString, wgpu_29::wgpu};

use super::{
    orbit::ViewHolder,
    handle_drag::{DragKind, HandleDragState},
    handle_frames::HandleFramer,
    router::{ViewportInputRouter, ViewportReleaseResult},
    arrow_keys::KeyboardTransformFocus,
    parts_panel::{build_object_list_names, fill_object_list, format_scene_stats, scene_stats},
    frame_host::{FrameHost, Owed},
    session::ViewSession,
    move_limits::{clamp_move_edit, settle_on_plate},
    screen::{ScreenDelta, ScreenPoint},
};
use crate::{
    MainWindow, PointerModifiers, WorkspaceSection,
    build_volume::BuildVolume,
    camera::ViewAxis,
    clipboard::{self, Clipboard},
    error_dialog::MessageWindows,
    export_progress_panel::ExportProgressPanel,
    gizmo,
    i18n::ctr,
    mesh_health::opening_section,
    mesh_loader::begin_ticketed_load,
    mesh_render::{DeviceLoss, FrameRefusal, SceneRender, StartRefusal},
    pending_load::WaitingFile,
    picking,
    save_progress_panel::SaveProgressPanel,
    scene_document::{DocumentEvent, SceneDocument, SceneEdit},
    scene_loader::{LoadedSceneBundle, SceneLoadCoordinator},
    selection::{LeafId, Selection},
};
use crate::gizmo::SIZES;

/// A transform's translation component, `[0.0, 0.0, 0.0]` for
/// `Identity`. Used by the Position field mirror/commit path, which
/// only ever reads or replaces this one component of a leaf's local
/// transform.
fn translation_of(t: &Transform3D) -> [f64; 3] {
    match t {
        Transform3D::Identity => [0.0, 0.0, 0.0],
        Transform3D::Affine(a) => *a.translation(),
    }
}

/// Structural commands can move an existing model too (support lift, cut,
/// hollow and their undo). Selection need not change for its fields to change.
fn position_refreshes_on(event: DocumentEvent) -> bool {
    matches!(
        event,
        DocumentEvent::ChoiceChanged | DocumentEvent::Edited | DocumentEvent::Regrouped
    )
}

/// Where the object sits on its plate, as the Position fields show it: X/Y
/// are the centre of its world bounding box (plate centre = 0, 0) and Z is
/// its lowest point above the plate. The raw local translation is not
/// useful there: it compensates for wherever the STL's own coordinates
/// happened to be (a camera mount read X -202, Y 1480 while sitting on the
/// plate), so typing 0 would throw the part far off the plate.
fn displayed_position(
    spatial: &sc_mesh_spatial::SceneSpatial,
    id: sc_scene::NodeId,
) -> Option<[f64; 3]> {
    let query = spatial.leaf(id)?;
    let world = query.parent_world().compose(&query.local_transform());
    let aabb = query.transformed_aabb(world)?;
    let min = aabb.min().coords();
    let max = aabb.max().coords();
    Some([
        0.5 * (f64::from(min[0]) + f64::from(max[0])),
        0.5 * (f64::from(min[1]) + f64::from(max[1])),
        f64::from(min[2]),
    ])
}

/// Two-decimal text for the object panel's Position/Rotation/Scale
/// fields. Values that round to zero print as `0.00`, never `-0.00`:
/// decomposing a rotation yields tiny negative residues (or a true
/// negative zero) that would otherwise show a stray minus sign.
fn format_transform_value(v: f64) -> String {
    let text = format!("{v:.2}");
    if text == "-0.00" {
        "0.00".to_owned()
    } else {
        text
    }
}

/// Orca Slicer's test for a model exported in inches or meters: its
/// bounding box encloses less than 8 cubic millimetres (a 2 mm cube). Far
/// below any real resin part, so it never nags about genuine miniatures.
const TINY_OBJECT_VOLUME_MM3: f64 = 8.0;

fn looks_like_wrong_units(size: [f64; 3]) -> bool {
    let volume = size[0] * size[1] * size[2];
    volume > 0.0 && volume < TINY_OBJECT_VOLUME_MM3
}

/// Place on face cache: flat sides of `node` at `pose` on a plate drawn at
/// `offset`.
struct PlaceFaces {
    node: sc_scene::node::NodeId,
    pose: Transform3D,
    offset: [f32; 2],
    faces: Vec<crate::place_on_face::Face>,
}

pub(crate) struct ViewController {
    window: slint::Weak<MainWindow>,
    state: ViewSession,
    orbit: ViewHolder,
    renderer: FrameHost,
    pointer: ViewportInputRouter,
    /// The layer key the slice-view panel's image was last made for (the
    /// 3D cut's keys, so both change together); `None` while it is blank.
    /// A frame with the same key leaves the image alone.
    layer_image_key: Cell<Option<u64>>,
    measure: std::cell::RefCell<crate::measure::Measure>,
    /// Place on face: the selected model's flat sides, cached for its
    /// current pose, and the one under the cursor.
    place_faces: std::cell::RefCell<Option<PlaceFaces>>,
    place_hover: Cell<Option<usize>>,
    benchmark_start: Cell<Option<std::time::Instant>>,
    hidden_objects: std::cell::RefCell<Vec<usize>>,
    paint_drag: Cell<bool>,
    plate_lock: Cell<bool>,
    /// True while the Preview 3D view is showing; entering it frames the models.
    preview_3d_shown: Cell<bool>,
    /// Screen quads (viewport px) of the plate tool tiles, in the order of
    /// the `plate-overlays` model: hover and clicks are hit-tested here.
    plate_tile_hits: std::cell::RefCell<Vec<[glam::Vec2; 4]>>,
    volume_cache: Cell<Option<((usize, usize), f64)>>,
    /// This controller, for continuations of in-window confirmations
    /// (set once it is wrapped in `Rc`).
    self_weak: std::cell::RefCell<std::rc::Weak<ViewController>>,
    thumbnail_timer: slint::Timer,
    /// Polls auto prepare's background orient (R92).
    prepare_timer: slint::Timer,
    thumbnail_generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl ViewController {
    pub(crate) fn new(window: &MainWindow, scene: Rc<SceneDocument>, waiting_file: Rc<WaitingFile>, gpu_loss: DeviceLoss) -> Self {
        let session = ViewSession::for_window(window, scene, waiting_file, gpu_loss);
        Self::assemble(window.as_weak(), session, FrameHost::new(window))
    }

    /// A controller with no window: everything it would show is skipped,
    /// so its own state is what tests observe (as with
    /// `SceneLoader::new_for_test`). It starts in Transform, where most of
    /// the tested tools live; the app starts in Prepare and loading picks
    /// the section, and the section tests choose their own.
    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        let session = ViewSession::detached();
        let controller =
            Self::assemble(slint::Weak::default(), session, FrameHost::without_window());
        controller.state.place.set_section(WorkspaceSection::Transform);
        controller
    }

    /// A controller around `session` and `render`, with every other part
    /// in its starting state.
    fn assemble(ui: slint::Weak<MainWindow>, session: ViewSession, render: FrameHost) -> Self {
        Self {
            window: ui,
            state: session,
            orbit: ViewHolder::new(),
            renderer: render,
            pointer: ViewportInputRouter::new(),
            layer_image_key: Cell::new(None),
            measure: Default::default(),
            place_faces: Default::default(),
            place_hover: Cell::new(None),
            benchmark_start: Cell::new(None),
            hidden_objects: Default::default(),
            paint_drag: Cell::new(false),
            plate_lock: Cell::new(false),
            preview_3d_shown: Cell::new(false),
            plate_tile_hits: Default::default(),
            volume_cache: Cell::new(None),
            self_weak: Default::default(),
            thumbnail_timer: slint::Timer::default(),
            prepare_timer: slint::Timer::default(),
            thumbnail_generation: Default::default(),
        }
    }

    /// Puts `renderer` (in tests, a `ScriptedRenderer`) where the GPU
    /// renderer would go, so painting and the GPU lifecycle run without a
    /// device.
    #[cfg(test)]
    pub(crate) fn put_renderer_for_test(&self, scripted: Box<dyn crate::mesh_render::MeshRenderer>) {
        *self.renderer.renderer_slot().borrow_mut() = Some(scripted);
    }

    /// Forces the frame flags, for tests of painting that need an upload
    /// or a redraw owed without making the document edits that owe them.
    #[cfg(test)]
    pub(super) fn put_frame_for_test(&self, state: Owed) {
        self.renderer.owe_exactly(state);
    }

    /// Whether frames of `size` are being skipped after a refusal.
    #[cfg(test)]
    pub(super) fn refused_size_for_test(&self, size: (u32, u32)) -> bool {
        self.renderer.size_was_refused(size)
    }

    #[cfg(test)]
    pub(crate) fn put_graph_for_test(&self, graph: sc_scene::Scene) {
        // A scene always comes with a source path; "<test>" makes it plain
        // where this one came from.
        self.state
            .document
            .take_mesh_scene(graph, std::path::PathBuf::from("<test>"));
    }

    #[cfg(test)]
    pub(crate) fn forget_frame_for_test(&self) {
        self.renderer.owe_exactly(Owed::NOTHING);
    }

    #[cfg(test)]
    pub(crate) fn redraw_due_for_test(&self) -> bool {
        self.renderer.owes(Owed::REDRAW)
    }

    #[cfg(test)]
    pub(super) fn frame_for_test(&self) -> Owed {
        self.renderer.owed()
    }

    #[cfg(test)]
    pub(crate) fn camera_ready_for_test(&self) -> bool {
        self.orbit.has_view()
    }

    #[cfg(test)]
    pub(super) fn renderer_up_for_test(&self) -> bool {
        self.renderer.renderer_slot().borrow().is_some()
    }

    #[cfg(test)]
    pub(crate) fn put_viewport_for_test(&self, w: f32, h: f32) {
        self.renderer.put_view_extent(w, h);
    }

    #[cfg(test)]
    pub(crate) fn viewport_size_for_test(&self) -> Option<(f32, f32)> {
        self.renderer.view_extent().map(|vp| (vp.width(), vp.height()))
    }

    #[cfg(test)]
    pub(super) fn key_focus_for_test(&self) -> KeyboardTransformFocus {
        self.pointer.viewport_keyboard_motion().keyboard_transform_focus()
    }

    #[cfg(test)]
    pub(crate) fn leaf_total_for_test(&self) -> usize {
        let document = &self.state.document;
        document.read_scene(|s| s.graph().reachable_leaf_count()).unwrap_or_default()
    }

    /// The window's scene loader; `wiring.rs` takes tickets and delivers
    /// results on it itself.
    pub(crate) fn scene_loader(&self) -> Rc<SceneLoadCoordinator> {
        Rc::clone(&self.state.loader)
    }

    /// Where a file picked before the window has a GPU waits; it is opened
    /// once the renderer is in (`graphics_ready`).
    pub(crate) fn queued_load(&self) -> Rc<WaitingFile> {
        Rc::clone(&self.state.waiting_file)
    }

    /// The window's scene document.
    pub(crate) fn scene_doc(&self) -> Rc<SceneDocument> {
        Rc::clone(&self.state.document)
    }

    /// The window's error and message dialogs, for flows outside the
    /// controller (a save finishing on a worker, for one) that must report.
    pub(crate) fn dialog_manager(&self) -> Rc<MessageWindows> {
        Rc::clone(&self.state.messages)
    }

    /// The save-progress panel driver, shared with the async save flow.
    pub(crate) fn save_panel(&self) -> Rc<SaveProgressPanel> {
        self.state.panels.save.clone()
    }

    pub(crate) fn export_panel(&self) -> Rc<ExportProgressPanel> {
        self.state.panels.export.clone()
    }

    /// Listens to the document (once the controller is in an `Rc`,
    /// `app_state` calls this). The listener holds a `Weak`, so the
    /// document does not keep the controller alive.
    pub(crate) fn listen_to_document(self: &Rc<Self>) {
        *self.self_weak.borrow_mut() = Rc::downgrade(self);
        let me = Rc::downgrade(self);
        let subscription = self.state.document.watch(move |event| {
            if let Some(me) = me.upgrade() {
                me.scene_event(event);
            }
        });
        self.state.keep_watch(subscription);
    }

    /// Re-renders the plate tabs' thumbnails a moment from now in the
    /// current theme's background; a newer request supersedes one still
    /// waiting.
    pub(crate) fn refresh_plate_thumbnails(&self) {
        use std::sync::atomic::Ordering;
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        let background = if ui.get_theme() == crate::ThemePreference::Light {
            [242, 242, 242]
        } else {
            [38, 38, 38]
        };
        let weak = self.window.clone();
        let doc = self.scene_doc().clone();
        let generation = self.thumbnail_generation.clone();
        let ticket = generation.fetch_add(1, Ordering::Relaxed) + 1;
        self.thumbnail_timer.start(
            slint::TimerMode::SingleShot,
            std::time::Duration::from_millis(200),
            move || {
                let Some(scene) = doc.read_scene(|s| s.graph().clone()) else {
                    return;
                };
                let weak = weak.clone();
                let generation = generation.clone();
                std::thread::spawn(move || {
                    let images: Vec<_> = scene
                        .plates()
                        .order
                        .iter()
                        .map(|plate| {
                            let mesh = scene
                                .plate_scene(plate.id)
                                .ok()
                                .and_then(|s| s.merge_meshes().ok().flatten());
                            mesh.map_or_else(
                                || background.repeat(64 * 48),
                                |mesh| {
                                    sc_mesh_formats::preview::render_mesh(&mesh, 64, 48, background)
                                },
                            )
                        })
                        .collect();
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        if generation.load(Ordering::Relaxed) != ticket {
                            return;
                        }
                        let images: Vec<_> = images
                            .into_iter()
                            .map(|pixels| {
                                let mut buffer =
                                    slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(64, 48);
                                buffer.make_mut_bytes().copy_from_slice(&pixels);
                                slint::Image::from_rgb8(buffer)
                            })
                            .collect();
                        ui.set_plate_thumbnails(Rc::new(slint::VecModel::from(images)).into());
                    });
                });
            },
        );
    }

    fn scene_event(&self, change: DocumentEvent) {
        self.refresh_plate_thumbnails();
        // Home-tab preview of this file: refreshed on open and on save (the
        // document turning clean with a path).
        let doc = self.scene_doc();
        if (change == DocumentEvent::Opened
            || (change == DocumentEvent::UnsavedChanged && !doc.has_unsaved_changes()))
            && let Some(path) = doc.file_path()
            && let Some(scene) = doc.read_scene(|s| s.graph().clone())
        {
            crate::thumbnails::write_in_background(path, scene);
        }
        if position_refreshes_on(change) {
            self.mirror_position_to_ui();
        }
        if let Some(ui) = self.window.upgrade() {
            let doc = self.scene_doc();
            doc.plate_width.set(self.printer_volume().width_mm());
            doc.plate_depth.set(self.printer_volume().depth_mm());
            let ids = doc
                .read_scene(|s| {
                    s.graph()
                        .plates()
                        .order
                        .iter()
                        .map(|p| p.id as i32)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| vec![1]);
            if slint::Model::row_count(&ui.get_plate_ids()) != ids.len() {
                self.orbit.forget_view();
            }
            let names = doc
                .read_scene(|s| {
                    s.graph()
                        .plates()
                        .order
                        .iter()
                        .map(|p| {
                            p.settings
                                .as_deref()
                                .and_then(|s| {
                                    sc_print_profiles::scene_overrides::PlateSettings::parse(s).ok()
                                })
                                .and_then(|s| s.name)
                                .unwrap_or_default()
                                .into()
                        })
                        .collect::<Vec<SharedString>>()
                })
                .unwrap_or_default();
            ui.set_plate_names(Rc::new(slint::VecModel::from(names)).into());
            ui.set_plate_ids(Rc::new(slint::VecModel::from(ids)).into());
            ui.set_active_plate_id(doc.active_plate() as i32);
            let locked = self.plate_locked();
            if ui.get_plate_locked() != locked {
                ui.set_plate_locked(locked);
            }
        }

        if matches!(
            change,
            DocumentEvent::Opened
                | DocumentEvent::Emptied
                | DocumentEvent::Edited
                | DocumentEvent::Regrouped
        ) && let Some(ui) = self.window.upgrade()
        {
            ui.set_plate_action_message("".into());
        }
        if matches!(
            change,
            DocumentEvent::ChoiceChanged | DocumentEvent::Opened | DocumentEvent::Regrouped
        ) && let Some(ui) = self.window.upgrade()
        {
            let document = &self.state.document;
            let thickness = document
                .read_scene(|s| {
                    crate::hollow_tools::wall_thickness(
                        s.graph(),
                        document.main_chosen_node()?,
                    )
                })
                .flatten()
                .unwrap_or(2.0);
            ui.set_hollow_wall_mm(format!("{thickness:.2}").into());
            let rows = document
                .read_scene(|s| {
                    document
                        .main_chosen_node()
                        .map(|id| crate::hollow_tools::hole_rows(s.graph(), id))
                })
                .flatten()
                .unwrap_or_default();
            ui.set_drain_hole_rows(
                Rc::new(slint::VecModel::from(
                    rows.into_iter().map(SharedString::from).collect::<Vec<_>>(),
                ))
                .into(),
            );
        }
        if let Some(ui) = self.window.upgrade() {
            let (saved, warnings) = self
                .state
                .document
                .read_scene(|s| crate::hollow_tools::stats(s.graph()))
                .unwrap_or_default();
            ui.set_hollow_saved_text(format!("{:.2} mL", saved / 1000.0).into());
            ui.set_suction_cup_count(warnings as i32);
        }
        if matches!(
            change,
            DocumentEvent::Opened | DocumentEvent::Emptied | DocumentEvent::Regrouped
        ) {
            self.hidden_objects.borrow_mut().clear();
        }
        if let Some(ui) = self.window.upgrade() {
            self.state.document.slice_state.borrow().apply(&ui);
        }
        match change {
            DocumentEvent::Opened => self.scene_put(),
            DocumentEvent::Emptied => self.scene_emptied(),
            DocumentEvent::ChoiceChanged => {
                self.show_selection();
                // With nothing selected the Move / Rotate / Scale tool is
                // put away, so the next selection starts without one. Only
                // the window and the handles change: no edit from inside
                // the document's own listener.
                if self.state.document.chosen().ids().is_empty()
                    && let Some(ui) = self.window.upgrade()
                    && ui.get_active_tool() >= 0
                {
                    ui.set_active_tool(-1);
                    self.pointer.viewport_gizmo_machine().set_handles(false);
                }
                self.note_unsaved();
            }
            // A moved part (every drag tick and nudge comes here too): the
            // transforms are read live each frame, so a redraw is enough.
            DocumentEvent::Edited => {
                self.mirror_slice_staleness_to_ui();
                self.note_unsaved();
            }
            // Parts or meshes added, removed or replaced: the GPU copy and
            // the object list both mirror the scene part for part, so both
            // are rebuilt (drawing a stale GPU copy trips `render`'s count
            // check).
            DocumentEvent::Regrouped => {
                self.renderer.owe(Owed::UPLOAD);
                self.with_ui(|ui| self.refresh_parts_list(ui));
                self.mirror_slice_staleness_to_ui();
                self.note_unsaved();
            }
            DocumentEvent::HistoryChanged => self.show_undo_redo_state(),
            DocumentEvent::UnsavedChanged | DocumentEvent::SettingsChanged => {
                self.show_unsaved_state();
                if let Some(id) = self.state.document.chosen_printer()
                    && let Ok((printer, _)) = crate::slice_projection::load_chosen_profiles(
                        &id,
                        &self.state.document.chosen_resin().unwrap_or_else(|| {
                            sc_print_primitives::MaterialId::new("standard-405").unwrap()
                        }),
                    )
                {
                    if crate::mesh_render::set_plate_printer(printer.identity.manufacturer.as_str(), printer.identity.name.as_str()) {
                        self.note_unsaved();
                    }
                    let v = printer.hardware.build_volume();
                    self.set_build_volume(BuildVolume::new(
                        v.x_mm() as f32,
                        v.y_mm() as f32,
                        v.z_mm() as f32,
                    ));
                }
            }
        }
    }

    /// Runs `apply` on the window while it is still open.
    fn with_ui(&self, apply: impl FnOnce(&MainWindow)) {
        if let Some(ui) = self.window.upgrade() {
            apply(&ui);
        }
    }

    /// Owes a frame when a camera or input handler reports a change.
    fn redraw_if(&self, changed: bool) {
        if changed {
            self.note_unsaved();
        }
    }

    /// Everything the window shows about the document itself: history,
    /// unsaved state, name and the held slice (its layers and freshness).
    fn mirror_document_to_ui(&self) {
        self.show_undo_redo_state();
        self.show_unsaved_state();
        self.show_title();
        self.show_layer_total();
        self.mirror_slice_staleness_to_ui();
    }

    /// Undo and Redo enabled as the history allows.
    fn show_undo_redo_state(&self) {
        let document = &self.state.document;
        let (undo, redo) = (document.has_past(), document.has_future());
        self.with_ui(|ui| {
            ui.set_undo_available(undo);
            ui.set_redo_available(redo);
        });
    }

    /// Unsaved changes (the window title then starts with `*`).
    fn show_unsaved_state(&self) {
        let dirty = self.state.document.has_unsaved_changes();
        self.with_ui(|ui| ui.set_unsaved_changes(dirty));
    }

    /// The held slice's layers: the slider's range, with the top layer
    /// selected. With no slice (after an install or close drops it) the
    /// slider is off and the risks and estimates are cleared. Estimates
    /// are not recomputed here for a slice that survived: only a Slice run
    /// computes them, and a stale or guessed number is worse than none.
    fn show_layer_total(&self) {
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        let count = self.state.document.layer_total().unwrap_or(0);
        ui.set_layer_total(count as i32);
        ui.set_layer_shown(count.saturating_sub(1) as f32);
        if count == 0 {
            ui.set_print_risks(Default::default());
            ui.set_risk_error_count(0);
            ui.set_selected_risk(-1);
            ui.set_estimated_print_time_text(SharedString::default());
            ui.set_estimated_resin_volume_text(SharedString::default());
            ui.set_estimated_resin_cost_text(SharedString::default());
        }
    }

    /// Whether the scene moved on from the held slice, so the Slice panel
    /// marks its estimates out of date (what that catches:
    /// [`SceneDocument::slice_is_geometry_stale`]).
    fn mirror_slice_staleness_to_ui(&self) {
        if let Some(ui) = self.window.upgrade() {
            let document = &self.state.document;
            if document.slice_is_geometry_stale() {
                document.slice_state.borrow_mut().changed();
            }
            document.slice_state.borrow().apply(&ui);
        }
    }

    /// The open file's name, the window title (empty with no scene).
    fn show_title(&self) {
        let name = self.state.document.file_name();
        self.with_ui(|ui| ui.set_project_title(name.unwrap_or_default().into()));
    }

    /// The title again, after Save As moved the document (which fires no
    /// scene event).
    pub(crate) fn refresh_title(&self) {
        self.show_title();
    }

    /// The bounds to frame: `bounds` alone with one plate; with several,
    /// also the whole grid of plates (the build volume repeated, the grid
    /// growing right and towards the front), so every plate is in view.
    fn project_view_bounds(
        &self,
        bounds: Option<(glam::Vec3, glam::Vec3)>,
    ) -> Option<(glam::Vec3, glam::Vec3)> {
        let count = self
            .scene_doc()
            .read_scene(|s| s.graph().plates().order.len())
            .unwrap_or(1);
        if count <= 1 {
            return bounds;
        }
        let volume = self.printer_volume();
        let (mut lo, mut hi) = (volume.envelope_min_mm(), volume.envelope_max_mm());
        let (width, depth) = (volume.width_mm(), volume.depth_mm());
        let last = sc_scene::plate_grid_offset(count - 1, count, width, depth);
        let columns = (count as f64).sqrt().ceil() as usize;
        let last_in_row = sc_scene::plate_grid_offset(columns - 1, count, width, depth);
        hi.x += last_in_row[0];
        lo.y += last[1];
        Some(bounds.map_or((lo, hi), |(min, max)| (min.min(lo), max.max(hi))))
    }

    /// A new scene: measurements go, the camera is framed afresh on the
    /// next frame, and the window shows the scene's list, history, name,
    /// slice and saved state.
    fn scene_put(&self) {
        self.measure.borrow_mut().clear();
        self.orbit.forget_view();
        self.with_ui(|ui| {
            ui.set_measure_text("".into());
            show_scene_open(ui, true);
            self.refresh_parts_list(ui);
        });
        self.mirror_position_to_ui();
        self.mirror_document_to_ui();
        self.note_unsaved();
    }

    /// Rebuilds the object list from the scene: on install, and after
    /// every edit that adds or removes parts (a move does not change it).
    fn refresh_parts_list(&self, ui: &MainWindow) {
        use super::parts_panel::{support_row_flags, triangle_counts};
        let list = self.state.document.read_scene(|spatial| {
            let graph = spatial.graph();
            let stats = scene_stats(graph);
            (
                build_object_list_names(graph),
                format_scene_stats(stats),
                stats.object_count,
                triangle_counts(graph),
                support_row_flags(graph),
            )
        });
        let (names, stats_text, models, triangles, support_rows) = list.unwrap_or_default();
        ui.set_object_model_count(models as i32);
        let triangles: Vec<SharedString> = triangles.into_iter().map(SharedString::from).collect();
        ui.set_object_triangle_counts(slint::ModelRc::new(slint::VecModel::from(triangles)));
        ui.set_object_visibility(slint::ModelRc::new(slint::VecModel::from(vec![
            true;
            names.len()
        ])));
        ui.set_object_row_support(slint::ModelRc::new(slint::VecModel::from(support_rows)));
        fill_object_list(ui, names);
        ui.set_object_list_stats_text(stats_text.into());
    }

    /// The scene closed: the view, the frame flags and the input state
    /// start over. A drag or arrow-key move in progress is dropped
    /// without reverting: its part is gone.
    fn scene_emptied(&self) {
        self.orbit.forget_view();
        self.renderer.owe_exactly(Owed::NOTHING);
        self.pointer.clear_viewport_input();
        self.renderer.drop_renderer_scene();
        self.with_ui(|ui| {
            show_scene_open(ui, false);
            ui.set_view_ready(slint::Image::default());
            fill_object_list(ui, Vec::new());
            ui.set_object_list_stats_text(SharedString::default());
            ui.set_object_model_count(0);
        });
        self.mirror_document_to_ui();
    }

    fn ask_redraw(&self) {
        self.renderer.queue_redraw();
    }

    /// The build volume, which loading places new parts on.
    pub(crate) fn printer_volume(&self) -> BuildVolume {
        self.state.place.volume()
    }

    /// Letters the plate with printer `id`'s brand (R118).
    pub(crate) fn set_plate_brand_from(&self, id: Option<&sc_print_primitives::PrinterId>) {
        let Some(id) = id else {
            return;
        };
        let resin = sc_print_primitives::MaterialId::new("standard-405").unwrap();
        if let Ok((printer, _)) = crate::slice_projection::load_chosen_profiles(id, &resin)
            && crate::mesh_render::set_plate_printer(printer.identity.manufacturer.as_str(), printer.identity.name.as_str())
        {
            self.note_unsaved();
        }
    }

    /// A new build volume, drawn on the next frame (the plate geometry is
    /// cached per volume) with the camera left where it is. It must owe a
    /// redraw: a frame with none owed draws nothing.
    pub(crate) fn set_build_volume(&self, volume: BuildVolume) {
        if self.state.place.set_volume(volume) {
            self.note_unsaved();
        }
    }

    /// Shows `section` (from the switcher, or the load picking one by mesh
    /// health). Leaving Transform ends a transform in progress, so none is
    /// left half done where editing is not allowed.
    pub(crate) fn switch_section(&self, section: WorkspaceSection) {
        let switched = self.state.place.set_section(section);
        if switched {
            if !self.layout_tools_allowed() {
                self.drop_transform_drags();
            }
            self.with_ui(|ui| ui.set_section(section));
            self.note_unsaved();
        }
    }

    /// Whether the section allows the gizmo, delete, copy and paste
    /// (selection and the camera work everywhere).
    pub(crate) fn layout_tools_allowed(&self) -> bool {
        crate::workspace_section::layout_tools_allowed_in(self.state.place.section())
    }

    /// Whether the section allows undo and redo: everywhere but Slice
    /// (repairs in Prepare are undoable).
    pub(crate) fn edits_unblocked(&self) -> bool {
        crate::workspace_section::undo_allowed_in(self.state.place.section())
    }

    /// Reverts a gizmo drag and drops the keyboard gizmo, keeping the
    /// selection.
    fn drop_transform_drags(&self) {
        self.revert_drag_in_progress();
        if self.keyboard_gizmo_on() {
            self.take_keyboard_focus(KeyboardTransformFocus::NoTransformFocus);
        }
    }

    /// Reverts a gizmo drag if one is running; true when it did.
    fn revert_drag_in_progress(&self) -> bool {
        let dragging = matches!(self.pointer.viewport_gizmo_machine().drag_snapshot(), HandleDragState::HandleHeld { .. });
        if dragging {
            self.pointer.cancel_viewport_gizmo_drag(&self.state.document);
        }
        dragging
    }

    /// Whether the arrow keys currently drive the gizmo.
    fn keyboard_gizmo_on(&self) -> bool {
        self.pointer.viewport_keyboard_motion().keyboard_transform_focus() != KeyboardTransformFocus::NoTransformFocus
    }

    fn note_unsaved(&self) {
        self.renderer.flag_dirty();
    }

    /// Commands shared by viewport menus and selection shortcuts.
    /// Plate hit in the same logical viewport coordinates as object picking.
    pub(crate) fn activate_plate_at(&self, x: f32, y: f32) {
        let hit = self.node_under(x, y);
        self.activate_plate_for_click(ScreenPoint::new(x, y), hit);
    }

    pub(crate) fn hits_plate_at(&self, x: f32, y: f32) -> bool {
        self.plate_at(x, y).is_some()
    }

    fn plate_at(&self, x: f32, y: f32) -> Option<u32> {
        let vp = self.renderer.view_extent()?;
        let slot = self.orbit.view_cell().borrow();
        let camera = slot.as_ref()?;
        let ray = picking::ray_through_pixel(
            camera,
            crate::mesh_render::starting_fov_y(),
            vp.width(),
            vp.height(),
            x,
            y,
        );
        self.scene_doc()
            .read_scene(|s| {
                for p in &s.graph().plates().order {
                    let mut local = ray;
                    let volume = self.printer_volume();
                    let [x, y] = s
                        .graph()
                        .plate_offset(p.id, volume.width_mm(), volume.depth_mm());
                    local.start.x -= x;
                    local.start.y -= y;
                    if picking::lands_on_plate(&local, self.printer_volume()) {
                        return Some(p.id);
                    }
                }
                None
            })
            .flatten()
    }

    /// The View setting: perspective, or orthographic (the default).
    pub(crate) fn set_perspective(&self, perspective: bool) {
        self.orbit.set_perspective(perspective);
        self.note_unsaved();
    }

    pub(crate) fn navigation_view(&self, index: i32) {
        if let Some(view) = super::view_steps::face_view(index - 1) {
            self.orbit.look_along(view);
        } else {
            self.orbit.go_home();
        }
        self.note_unsaved();
    }

    /// Verification: a close, low look across the previewed layer's edge
    /// (where its side walls show), as when zooming in on the cut.
    pub(crate) fn layer_closeup_evidence(&self) {
        let Some((min, max)) = self.active_plate_model_bounds() else {
            return;
        };
        let layer = self.window.upgrade().map_or(0, |ui| {
            ui.get_layer_shown().round().max(0.0) as u32
        });
        let Some(top) = self.state.document.layer_top_mm(layer) else {
            return;
        };
        if let Some(camera) = self.orbit.view_cell().borrow_mut().as_mut() {
            camera.look_at = glam::Vec3::new(0.5 * (min.x + max.x), min.y, top as f32);
            camera.azimuth = -std::f32::consts::FRAC_PI_2 + 0.35;
            camera.elevation = 0.18;
            camera.eye_distance = 0.06 * (max - min).length();
        }
        self.note_unsaved();
    }

    /// Verification: the plate's front tab from close in front, to read the
    /// printer's brand and model lettered on it.
    pub(crate) fn plate_label_closeup_evidence(&self) {
        let volume = self.state.place.volume();
        if let Some(camera) = self.orbit.view_cell().borrow_mut().as_mut() {
            let front = -0.5 * volume.depth_mm();
            camera.look_at = glam::Vec3::new(0.0, front - 0.04 * volume.depth_mm(), 0.0);
            camera.azimuth = -std::f32::consts::FRAC_PI_2;
            camera.elevation = 0.9;
            camera.eye_distance = 0.55 * volume.width_mm();
        }
        self.note_unsaved();
    }

    /// Verification: a close, low look at the supports under the part, to
    /// judge how round they render (R90).
    pub(crate) fn support_closeup_evidence(&self) {
        let Some((min, max)) = self.active_plate_model_bounds() else {
            return;
        };
        if let Some(camera) = self.orbit.view_cell().borrow_mut().as_mut() {
            camera.look_at = glam::Vec3::new(
                min.x + 0.3 * (max.x - min.x),
                0.5 * (min.y + max.y),
                min.z + 0.2 * (max.z - min.z),
            );
            camera.azimuth = -std::f32::consts::FRAC_PI_2 + 0.5;
            camera.elevation = 0.12;
            camera.eye_distance = 0.22 * (max - min).length();
        }
        self.note_unsaved();
    }

    /// Verification: hover the view cube's front face (0), its front-top
    /// edge (1) or the front-right-top corner (2), as the pointer would.
    pub(crate) fn cube_hover_evidence(&self, which: i32) {
        let target = match which {
            0 => glam::Vec3::new(0.0, -1.0, 0.0),
            1 => glam::Vec3::new(0.0, -1.0, 1.0),
            _ => glam::Vec3::new(1.0, -1.0, 1.0),
        };
        let point = self
            .orbit
            .view_cell()
            .borrow()
            .as_ref()
            .and_then(|c| super::view_steps::screen_point(c, target * 0.97));
        if let Some(p) = point {
            self.cube_hovered(p.x, p.y);
        }
    }

    /// Highlight the cube face, edge or corner under the pointer.
    pub(crate) fn cube_hovered(&self, x: f32, y: f32) {
        let path = self
            .orbit
            .view_cell()
            .borrow()
            .as_ref()
            .and_then(|c| {
                let target = super::view_steps::pick_target(c, glam::Vec2::new(x, y))?;
                Some(super::view_steps::target_path(c, target))
            })
            .unwrap_or_default();
        if let Some(ui) = self.window.upgrade() {
            ui.set_cube_hover_face(if path.is_empty() { -1 } else { 1 });
            if ui.get_cube_highlight().as_str() != path {
                ui.set_cube_highlight(path.into());
            }
        }
    }

    /// Click a view-cube face, edge or corner: look at it head on.
    pub(crate) fn cube_clicked(&self, x: f32, y: f32) {
        let target = self
            .orbit
            .view_cell()
            .borrow()
            .as_ref()
            .and_then(|c| super::view_steps::pick_target(c, glam::Vec2::new(x, y)));
        if let Some(target) = target {
            self.orbit.look_from(target);
            self.note_unsaved();
            if let Some(ui) = self.window.upgrade() {
                ui.set_cube_hover_face(-1);
                ui.set_cube_highlight("".into());
            }
        }
    }

    pub(crate) fn set_plate_locked(&self, locked: bool) {
        self.pointer.cancel_viewport_gizmo_drag(&self.state.document);
        let doc = self.scene_doc();
        let active = doc.active_plate();
        doc.edit_plates(|p| {
            if let Some(p) = p.order.iter_mut().find(|p| p.id == active) {
                p.locked = locked;
            }
        });
        self.plate_lock.set(locked);
        self.note_unsaved();
    }

    fn plate_locked(&self) -> bool {
        self.scene_doc()
            .read_scene(|s| {
                s.graph()
                    .plates()
                    .order
                    .iter()
                    .any(|p| p.id == self.scene_doc().active_plate() && p.locked)
            })
            .unwrap_or(false)
    }

    pub(crate) fn context_action(&self, action: i32) {
        let doc = &self.state.document;
        if action == 1 {
            doc.select_active_plate();
            return;
        }
        if !self.layout_tools_allowed() {
            return;
        }
        if self.plate_locked() && matches!(action, 3 | 4 | 6) {
            return;
        }
        if action == 6 {
            doc.select_active_plate();
            self.delete_selected();
            return;
        }
        let edit = doc
            .read_scene(|s| match action {
                3 => Some(SceneEdit::Compound(
                    s.graph()
                        .reachable_leaves(
                            sc_scene::CsgPolicy::Descend,
                            sc_scene::ReferencePolicy::Skip,
                        )
                        .ok()?
                        .into_iter()
                        .filter(|&id| s.graph().plate_of(id) == doc.active_plate())
                        .filter(|&id| {
                            !s.graph()
                                .node(id)
                                .and_then(|n| n.meta.as_ref())
                                .and_then(|m| m.comment.as_deref())
                                .is_some_and(|c| c.starts_with("sc-generated-support"))
                        })
                        .filter_map(|id| {
                            crate::layout_tools::auto_orient(
                                s.graph(),
                                id,
                                45.0,
                                self.printer_volume(),
                                self.window
                                    .upgrade()
                                    .map(|ui| crate::layout_tools::orient_weights(&ui))
                                    .unwrap_or_else(|| {
                                        sc_print_glue::orient::Preset::Balanced.weights()
                                    }),
                                self.orient_context(),
                            )
                        })
                        .collect(),
                )),
                18 => crate::hollow_tools::remove(s.graph(), doc.main_chosen_node()?),
                20 => {
                    let id = doc.main_chosen_node()?;
                    Some(SceneEdit::SetLocalTransform {
                        node_id: id,
                        before: s.graph().node(id)?.local_transform,
                        after: Transform3D::Identity,
                    })
                }
                _ => None,
            })
            .flatten();
        if let Some(edit) = edit
            && doc.commit_edit(edit.clone()).is_ok()
        {
            doc.record_edit(edit);
        }
    }

    fn orient_context(&self) -> sc_print_glue::orient::Context {
        let document = &self.state.document;
        let mut context = document
            .chosen_printer()
            .zip(document.chosen_resin())
            .and_then(|(p, m)| crate::slice_projection::load_chosen_profiles(&p, &m).ok())
            .and_then(|(p, m)| {
                sc_print_glue::orient::Context::for_profiles(
                    &p,
                    &m,
                    &document.process_overrides.borrow(),
                )
                .ok()
            })
            .unwrap_or_default();
        context.aa = document.scene_aa.get().unwrap_or(context.aa);
        context.first_layer_height_mm = document
            .chosen_first_layer()
            .map_or(0.0, |v| v.value());
        if let Some(ui) = self.window.upgrade() {
            context.zero_support_head_start =
                sc_print_profiles::orient::ZERO_SUPPORT_HEAD_START_COPIES;
            context.supports = ui.get_orient_for_supports();
            context.convenience_rate = 1.0;
            context.gap_mm = ui.get_arrange_gap_mm().parse().unwrap_or(3.0);
            context.support = crate::auto_support_run::settings(&ui).unwrap_or_default();
        }
        context
    }

    /// Auto prepare (Sleptic: one button that does everything, the separate
    /// tools kept, all working toward the same result): orient every part
    /// on the plate for printing (the Auto orient search, which already
    /// weighs supports, their reach and copies per plate), arrange them with
    /// room for the supports they will need, then generate the supports.
    /// Parts that already have supports keep their pose.
    pub(crate) fn auto_prepare_action(&self) {
        if self.plate_locked() || !self.layout_tools_allowed() {
            return;
        }
        let document = &self.state.document;
        let ui = self.window.upgrade();
        let angle = ui
            .as_ref()
            .and_then(|ui| ui.get_support_overhang_angle_deg().parse::<f64>().ok())
            .unwrap_or(45.0);
        let weights = ui
            .as_ref()
            .map(crate::layout_tools::orient_weights)
            .filter(|w| w.iter().sum::<f64>() > 0.0)
            .unwrap_or(sc_print_profiles::orient::Preset::default().weights());
        let volume = self.printer_volume();
        let Ok(volume) = sc_print_primitives::BuildVolume::new(
            f64::from(volume.width_mm()),
            f64::from(volume.depth_mm()),
            f64::from(volume.height_mm()),
        ) else {
            return;
        };
        let context = self.orient_context();
        let active = document.active_plate();
        // The parts to orient, on a snapshot of the scene the worker can own.
        // A part too big for the printer whichever way it is turned is left as
        // it is: the support step refuses it with a message, and orienting it
        // first only kept the user waiting (a 516 mm part: 4 s).
        let snapshot = document.read_scene(|spatial| {
            let graph = spatial.graph();
            let supports = crate::auto_support_run::support_ids(graph);
            let parts: Vec<_> = graph
                .reachable_leaves(
                    sc_scene::CsgPolicy::Descend,
                    sc_scene::ReferencePolicy::Skip,
                )
                .unwrap_or_default()
                .into_iter()
                .filter(|&id| {
                    graph.plate_of(id) == active
                        && !supports.contains(&id)
                        && crate::scene_document::owned_support_ids(graph, id).is_empty()
                        && crate::auto_support_run::part_fits_some_way(graph, id, volume)
                })
                .collect();
            (graph.clone(), parts)
        });
        let Some((scene, parts)) = snapshot.filter(|(_, parts)| !parts.is_empty()) else {
            self.finish_auto_prepare(Vec::new(), document.revision());
            return;
        };
        // Orient runs on a worker, so the window keeps drawing (R92: it ran on
        // the UI thread, and the app froze for seconds on a large part).
        let revision = document.revision();
        let slot: std::sync::Arc<std::sync::Mutex<Option<Vec<(sc_scene::NodeId, Transform3D)>>>> =
            Default::default();
        let worker = slot.clone();
        if let Some(ui) = &ui {
            ui.set_making_supports(true);
            ui.set_plate_action_message(crate::i18n::tr!("Orienting…").into());
        }
        std::thread::spawn(move || {
            let deltas = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::layout_tools::orient_once_per_part(&scene, &parts, |id| {
                    let proposal = sc_print_glue::orient::search_with_context(
                        &scene, id, volume, angle, weights, context,
                    )?;
                    (!proposal.retained).then_some(proposal.delta)
                })
            }))
            .unwrap_or_default();
            *worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(deltas);
        });
        let weak = self.self_weak.borrow().clone();
        self.prepare_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(30),
            move || {
                let Some(deltas) = slot.lock().unwrap_or_else(|e| e.into_inner()).take() else {
                    return;
                };
                if let Some(controller) = weak.upgrade() {
                    controller.prepare_timer.stop();
                    controller.finish_auto_prepare(deltas, revision);
                }
            },
        );
    }

    /// Auto prepare after orient: apply the poses (unless the scene changed
    /// meanwhile), lay the plate out and generate supports.
    fn finish_auto_prepare(
        &self,
        deltas: Vec<(sc_scene::NodeId, Transform3D)>,
        revision: crate::scene_document::Revision,
    ) {
        let document = &self.state.document;
        let ui = self.window.upgrade();
        if let Some(ui) = &ui {
            ui.set_making_supports(false);
            ui.set_plate_action_message("".into());
        }
        if document.revision() != revision {
            if let Some(ui) = &ui {
                ui.set_plate_action_message(
                    crate::i18n::tr!("The scene changed while orienting. Run Auto prepare again.")
                        .into(),
                );
            }
            return;
        }
        let edits = document
            .read_scene(|spatial| {
                deltas
                    .into_iter()
                    .filter_map(|(id, delta)| {
                        crate::layout_tools::world_edit(spatial.graph(), id, delta)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !edits.is_empty() {
            let edit = SceneEdit::Compound(edits);
            if document.commit_edit(edit.clone()).is_ok() {
                document.record_edit(edit);
            }
        }
        self.layout_action(true);
        if let Some(ui) = ui {
            self.pick_support_settings(&ui);
            ui.set_supports_enabled(true);
            ui.set_supports_tool_active(true);
            ui.invoke_auto_supports_requested();
        }
        self.note_unsaved();
    }

    /// Auto prepare tunes every support setting to the active plate's parts as
    /// they now sit (R106), for its own support run only: the user's settings
    /// stay as they are (the maintainer: all done inside the app).
    fn pick_support_settings(&self, ui: &MainWindow) {
        let document = &self.state.document;
        let active = document.active_plate();
        let parts: Vec<sc_mesh_core::IndexedMesh> = document
            .read_scene(|spatial| {
                let graph = spatial.graph();
                let supports = crate::auto_support_run::support_ids(graph);
                graph
                    .reachable_leaves(
                        sc_scene::CsgPolicy::Descend,
                        sc_scene::ReferencePolicy::Skip,
                    )
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|&id| graph.plate_of(id) == active && !supports.contains(&id))
                    .filter_map(|id| sc_scene::bake_leaf_to_world(graph, id).ok())
                    .collect()
            })
            .unwrap_or_default();
        if parts.is_empty() {
            return;
        }
        let volume = self.printer_volume();
        let plate = f64::from(volume.width_mm()) * f64::from(volume.depth_mm());
        let user = crate::auto_support_run::settings(ui).unwrap_or_default();
        let choice = sc_print_glue::supports::choose_supports(&parts, plate, &user);
        document
            .auto_support_settings
            .replace(Some(choice.settings));
        ui.set_plate_action_message(support_choice_text(&choice).into());
    }

    pub(crate) fn auto_orient_action(&self) {
        if self.plate_locked() {
            return;
        }
        if !self.layout_tools_allowed() {
            return;
        }
        let document = &self.state.document;
        let angle = self
            .window
            .upgrade()
            .and_then(|ui| ui.get_support_overhang_angle_deg().parse::<f64>().ok())
            .unwrap_or(45.0);
        let started = std::time::Instant::now();
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        let volume = self.printer_volume();
        let Ok(volume) = sc_print_primitives::BuildVolume::new(
            f64::from(volume.width_mm()),
            f64::from(volume.depth_mm()),
            f64::from(volume.height_mm()),
        ) else {
            return;
        };
        let weights = crate::layout_tools::orient_weights(&ui);
        if weights.iter().sum::<f64>() <= 0.0 {
            ui.set_plate_action_message(crate::i18n::tr!(
                "Choose at least one nonzero Orient weight."
            ));
            return;
        }
        let proposal = document
            .read_scene(|spatial| {
                let id = document.main_chosen_node()?;
                let proposal = sc_print_glue::orient::search_with_context(
                    spatial.graph(),
                    id,
                    volume,
                    angle,
                    weights,
                    self.orient_context(),
                )?;
                let edit = crate::layout_tools::world_edit(spatial.graph(), id, proposal.delta)?;
                Some((proposal, edit))
            })
            .flatten();
        eprintln!("Auto orient: {:.3} s", started.elapsed().as_secs_f64());
        if let Some((proposal, edit)) = proposal {
            if !proposal.retained && document.commit_edit(edit.clone()).is_ok() {
                document.record_edit(edit);
            }
            let a = proposal.original;
            let b = proposal.chosen;
            let message = if !b.fits {
                crate::i18n::tr!("Kept original: no fitting orientation found.").to_string()
            } else if b.zero_support {
                crate::layout_tools::direct_orient_summary(b.print_minutes, b.copies)
            } else if !a.fits && b.height > a.height * 2.0 && b.print_minutes > 90.0 {
                crate::i18n::tr!("Only fits standing up: {}. Consider scaling or splitting it; fits {} per plate.",
                    sc_print_job::format_duration(b.print_minutes * 60.0), b.copies).to_string()
            } else if proposal.retained {
                crate::i18n::tr!("Kept original: already as good within search tolerance. {} print; fits {} per plate.",
                    sc_print_job::format_duration(b.print_minutes * 60.0), b.copies).to_string()
            } else {
                crate::layout_tools::orient_summary(
                    b.print_minutes - a.print_minutes,
                    a.contacts - b.contacts,
                    a.expected_blocked_contacts + a.plate_recessed_contacts as f64,
                    b.expected_blocked_contacts + b.plate_recessed_contacts as f64,
                    b.copies,
                )
            };
            ui.set_plate_action_message(message.into());
        }
    }

    pub(crate) fn arrange_all_action(&self) {
        let doc = self.scene_doc();
        match doc
            .read_scene(|s| crate::layout_tools::arrange_all(s.graph(), self.printer_volume(), 3.0))
        {
            Some(Ok(edit)) => {
                if doc.commit_edit(edit.clone()).is_ok() {
                    doc.record_edit(edit);
                }
            }
            Some(Err(ids)) => self.state.messages.show_failure(
                &crate::i18n::tr!("Placement"),
                None,
                crate::i18n::tr!("Objects that do not fit:").to_string(),
                ids.iter()
                    .map(|id| id.get().to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            None => {}
        }
    }

    /// Remove plate `id` when it holds no objects and is not the only
    /// plate (no question to ask: nothing on it is lost). One undo step.
    /// Returns whether the plate was removed.
    pub(crate) fn delete_empty_plate(&self, id: u32) -> bool {
        let doc = self.scene_doc();
        let Some(edit) = doc
            .read_scene(|s| {
                let graph = s.graph();
                let before = graph.plates().clone();
                if before.order.len() <= 1 || !before.order.iter().any(|p| p.id == id) {
                    return None;
                }
                let occupied = graph
                    .reachable_leaves(
                        sc_scene::CsgPolicy::Descend,
                        sc_scene::ReferencePolicy::Skip,
                    )
                    .unwrap_or_default()
                    .into_iter()
                    .any(|leaf| graph.plate_of(leaf) == id);
                if occupied {
                    return None;
                }
                let mut after = before.clone();
                after.order.retain(|p| p.id != id);
                after.assignments.retain(|_, p| *p != id);
                Some(SceneEdit::SetPlates { before, after })
            })
            .flatten()
        else {
            return false;
        };
        let next = doc.read_scene(|s| {
            s.graph()
                .plates()
                .order
                .iter()
                .find(|p| p.id != id)
                .map(|p| p.id)
        });
        if doc.commit_edit(edit.clone()).is_err() {
            return false;
        }
        doc.record_edit(edit);
        if let Some(Some(next)) = next {
            doc.set_active_plate(next);
        }
        true
    }

    /// The plate tile's delete button. An empty plate goes at once; one
    /// with objects asks first, then removes the plate and its objects. The
    /// last plate always stays: it is cleared instead.
    pub(crate) fn delete_plate_from_tile(&self, id: u32) {
        if self.delete_empty_plate(id) {
            return;
        }
        let doc = self.scene_doc();
        let Some((objects, plates, index)) = doc.read_scene(|s| {
            let graph = s.graph();
            let objects = graph
                .reachable_leaves(
                    sc_scene::CsgPolicy::Descend,
                    sc_scene::ReferencePolicy::Skip,
                )
                .unwrap_or_default()
                .into_iter()
                .filter(|&leaf| graph.plate_of(leaf) == id)
                .filter(|&leaf| {
                    !graph
                        .node(leaf)
                        .and_then(|n| n.meta.as_ref())
                        .and_then(|m| m.comment.as_deref())
                        .is_some_and(|c| c.starts_with("sc-generated-support"))
                })
                .count();
            let plates = graph.plates().order.len();
            let index = graph
                .plates()
                .order
                .iter()
                .position(|p| p.id == id)
                .map_or(1, |i| i + 1);
            (objects, plates, index)
        }) else {
            return;
        };
        if objects == 0 {
            return;
        }
        let question = if plates > 1 {
            crate::i18n::tr!(
                "Delete plate {} and the 1 object on it?",
                "Delete plate {} and the {n} objects on it?",
                objects,
                index
            )
        } else {
            crate::i18n::tr!(
                "Plate {} is the only plate, so it stays. Delete the 1 object on it?",
                "Plate {} is the only plate, so it stays. Delete the {n} objects on it?",
                objects,
                index
            )
        };
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        let title = crate::i18n::tr!("Delete plate {}?", index);
        let weak = self.self_weak.borrow().clone();
        crate::confirm::ask(
            &ui,
            crate::confirm::Question {
                title: &title,
                body: &question,
                primary: &if plates > 1 {
                    crate::i18n::tr!("Delete plate")
                } else {
                    crate::i18n::tr!("Delete objects")
                },
                secondary: "",
                destructive: true,
            },
            move |choice| {
                let Some(ctrl) = weak.upgrade() else {
                    return;
                };
                if choice != crate::confirm::Choice::Primary {
                    return;
                }
                ctrl.scene_doc().set_active_plate(id);
                if plates > 1 {
                    ctrl.remove_active_plate(true);
                } else {
                    ctrl.context_action(6);
                }
            },
        );
    }

    pub(crate) fn delete_plate_action(&self) {
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        let weak = self.self_weak.borrow().clone();
        crate::confirm::ask(
            &ui,
            crate::confirm::Question {
                title: &crate::i18n::tr!("Delete this plate?"),
                body: &crate::i18n::tr!(
                    "Delete the plate with its objects, or keep the objects by moving them to another plate."
                ),
                primary: &crate::i18n::tr!("Delete with objects"),
                secondary: &crate::i18n::tr!("Move objects"),
                destructive: true,
            },
            move |choice| {
                let Some(ctrl) = weak.upgrade() else {
                    return;
                };
                match choice {
                    crate::confirm::Choice::Primary => ctrl.remove_active_plate(true),
                    crate::confirm::Choice::Secondary => ctrl.remove_active_plate(false),
                    crate::confirm::Choice::Cancel => {}
                }
            },
        );
    }

    /// Remove the active plate (not the last one), deleting its objects or
    /// moving them to the next plate; one undo step.
    fn remove_active_plate(&self, delete_objects: bool) {
        let doc = self.scene_doc();
        let active = doc.active_plate();
        let Some(mut trial) = doc.read_scene(|s| s.graph().clone()) else {
            return;
        };
        let before = trial.plates().clone();
        if before.order.len() <= 1 {
            return;
        }
        let first = before
            .order
            .iter()
            .find(|p| p.id != active)
            .map(|p| p.id)
            .unwrap();
        let mut edits = Vec::new();
        let ids = trial
            .reachable_leaves(
                sc_scene::CsgPolicy::Descend,
                sc_scene::ReferencePolicy::Skip,
            )
            .unwrap_or_default();
        let mut after = before.clone();
        let ids: Vec<_> = ids
            .into_iter()
            .filter(|id| trial.plate_of(*id) == active)
            .collect();
        for id in ids {
            {
                if delete_objects {
                    if let Some(edit) = SceneEdit::remove_leaf_with_gc(&trial, id)
                        && edit.apply_to_scene(&mut trial).is_ok()
                    {
                        edits.push(edit);
                    }
                    after.assignments.remove(&id);
                } else {
                    after.assignments.insert(id, first);
                }
            }
        }
        after.assignments.retain(|_, p| *p != active);
        after.order.retain(|p| p.id != active);
        edits.push(SceneEdit::SetPlates { before, after });
        let edit = SceneEdit::Compound(edits);
        if doc.commit_edit(edit.clone()).is_ok() {
            doc.record_edit(edit);
            doc.set_active_plate(first);
        }
    }

    pub(crate) fn layout_action(&self, arrange: bool) {
        if self.plate_locked() {
            return;
        }
        if !self.layout_tools_allowed() {
            return;
        }
        let document = &self.state.document;
        let edit = if arrange {
            let gap = self
                .window
                .upgrade()
                .and_then(|ui| ui.get_arrange_gap_mm().parse::<f64>().ok())
                .unwrap_or(3.0);
            match document.read_scene(|spatial| {
                crate::layout_tools::arrange_with_gap(
                    &spatial
                        .graph()
                        .plate_scene(document.active_plate())
                        .map_err(|_| Vec::new())?,
                    self.printer_volume(),
                    gap,
                )
                .map_err(|ids| {
                    ids.iter()
                        .map(|&id| {
                            spatial
                                .graph()
                                .node(id)
                                .and_then(|n| n.meta.as_ref())
                                .and_then(|m| m.name.clone())
                                .unwrap_or_else(|| format!("{}", id.get()))
                        })
                        .collect::<Vec<_>>()
                })
            }) {
                Some(Ok(edit)) => Some(edit),
                Some(Err(names)) => {
                    self.state.messages.show_failure(
                        &crate::i18n::tr!("Placement"),
                        None,
                        crate::i18n::tr!("Objects that do not fit:").to_string(),
                        names.join(", "),
                    );
                    return;
                }
                None => None,
            }
        } else {
            // Lay flat is now the Place on face tool (see `place_face_*`).
            return;
        };
        if let Some(edit) = edit {
            if document.commit_edit(edit.clone()).is_ok() {
                document.record_edit(edit);
            }
        } else {
            self.state.messages.show_failure(
                &crate::i18n::tr!("Placement"),
                None,
                crate::i18n::tr!("The objects could not be placed within the build volume.")
                    .to_string(),
                String::new(),
            );
        }
    }

    pub(crate) fn mirror_action(&self, axis: i32) {
        if !self.layout_tools_allowed() {
            return;
        }
        let document = &self.state.document;
        let edit = document
            .read_scene(|spatial| {
                let axis = usize::try_from(axis).ok()?;
                Some(SceneEdit::Compound(
                    document
                        .chosen()
                        .ids()
                        .into_iter()
                        .filter_map(|id| crate::layout_tools::mirror(spatial.graph(), id, axis))
                        .collect(),
                ))
            })
            .flatten();
        if let Some(edit) = edit
            && document.commit_edit(edit.clone()).is_ok()
        {
            document.record_edit(edit);
        }
    }

    pub(crate) fn fill_plate_action(&self) {
        if self.plate_locked() {
            return;
        }
        if !self.layout_tools_allowed() {
            return;
        }
        let gap = self
            .window
            .upgrade()
            .and_then(|ui| ui.get_arrange_gap_mm().parse().ok())
            .unwrap_or(3.0);
        let orient = self.fill_plate_orientation();
        let reoriented = orient.is_some();
        let direct = orient.as_ref().is_some_and(|(_, direct)| *direct);
        let orient = orient.map(|(edit, _)| edit);
        if let Some(count) =
            self.state
                .document
                .fill_plate_after(orient, self.printer_volume(), gap)
            && let Some(ui) = self.window.upgrade()
        {
            ui.set_plate_action_message(if direct {
                crate::i18n::tr!(
                    "Re-oriented to print without supports and added {} copies.",
                    count
                )
            } else {
                match (reoriented, count) {
                    (true, 0) => {
                        crate::i18n::tr!("Re-oriented to save space; the plate is already full.")
                    }
                    (true, _) => {
                        crate::i18n::tr!("Re-oriented to save space and added {} copies.", count)
                    }
                    (false, 0) => {
                        crate::i18n::tr!("The plate is already full; no copies were added.")
                    }
                    (false, _) => crate::i18n::tr!("Added {} copies to fill the plate.", count),
                }
            });
        }
    }

    /// Fill plate re-orients the part first, exactly as the Auto orient
    /// button would (its search, context and the chosen Orient weights;
    /// Sleptic: Fill plate used an older, fixed orientation). A part that
    /// already has generated supports keeps its pose (turning it would
    /// strand them), as does one whose current pose is already as good.
    fn fill_plate_orientation(&self) -> Option<(SceneEdit, bool)> {
        let document = &self.state.document;
        let selected = document.main_chosen_node()?;
        let volume = self.printer_volume();
        let volume = sc_print_primitives::BuildVolume::new(
            f64::from(volume.width_mm()),
            f64::from(volume.depth_mm()),
            f64::from(volume.height_mm()),
        )
        .ok()?;
        let angle = self
            .window
            .upgrade()
            .and_then(|ui| ui.get_support_overhang_angle_deg().parse::<f64>().ok())
            .unwrap_or(45.0);
        let context = self.orient_context();
        let weights = self
            .window
            .upgrade()
            .map(|ui| crate::layout_tools::orient_weights(&ui))
            .filter(|w| w.iter().sum::<f64>() > 0.0)
            .unwrap_or(sc_print_profiles::orient::FILL_PLATE_WEIGHTS);
        document
            .read_scene(|spatial| {
                let graph = spatial.graph();
                if crate::auto_support_run::support_ids(graph).contains(&selected)
                    || !crate::scene_document::owned_support_ids(graph, selected).is_empty()
                {
                    return None;
                }
                let proposal = sc_print_glue::orient::search_with_context(
                    graph, selected, volume, angle, weights, context,
                )?;
                if proposal.retained {
                    return None;
                }
                crate::layout_tools::world_edit(graph, selected, proposal.delta)
                    .map(|edit| (edit, proposal.chosen.zero_support))
            })
            .flatten()
    }

    /// The Hollow button (`auto` false) or Auto drain holes (true).
    pub(crate) fn hollow_requested(&self, auto: bool) {
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        self.hollow_action(if auto {
            crate::hollow_tools::HoleChange::Auto(
                ui.get_drain_diameter_mm().parse().unwrap_or(f64::NAN),
            )
        } else {
            crate::hollow_tools::HoleChange::Keep
        });
    }

    pub(crate) fn hollow_action(&self, change: crate::hollow_tools::HoleChange) {
        if !self.layout_tools_allowed() {
            return;
        }
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        let document = &self.state.document;
        let Some(id) = document.main_chosen_node() else {
            return;
        };
        let thickness = ui.get_hollow_wall_mm().parse::<f64>().unwrap_or(f64::NAN);
        let Some(result) =
            document.read_scene(|s| crate::hollow_tools::edit(s.graph(), id, thickness, change))
        else {
            return;
        };
        match result {
            Ok(edit) => {
                if document.commit_edit(edit.clone()).is_ok() {
                    document.record_edit(edit);
                }
            }
            Err(error) => self.state.messages.show_failure(
                &crate::i18n::tr!("The object could not be hollowed"),
                None,
                error,
                String::new(),
            ),
        }
        if let Some((saved, warnings)) =
            document.read_scene(|s| crate::hollow_tools::stats(s.graph()))
        {
            ui.set_hollow_saved_text(format!("{:.2} mL", saved / 1000.0).into());
            ui.set_suction_cup_count(warnings as i32);
        }
    }

    /// Adds a drain hole at the point typed into the hole panel.
    pub(crate) fn drain_at_point(&self) {
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        let point = [
            ui.get_drain_x_mm(),
            ui.get_drain_y_mm(),
            ui.get_drain_z_mm(),
        ]
        .map(|v| v.parse::<f64>().unwrap_or(f64::NAN));
        if point.iter().any(|v| !v.is_finite()) {
            self.state.messages.show_failure(
                &crate::i18n::tr!("No drain hole position"),
                None,
                crate::i18n::tr!("Enter the hole's X, Y and Z position in millimetres.")
                    .to_string(),
                String::new(),
            );
            return;
        }
        // Typed points have no view: "Along the view" runs straight down.
        self.hollow_action(crate::hollow_tools::HoleChange::Add(drain_request(
            &ui,
            point,
            [0.0, 0.0, -1.0],
        )));
    }

    pub(crate) fn model_tool_action(&self, action: i32) {
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        use crate::model_tools::Operation;
        use sc_print_glue::model_tools::BooleanOp;
        let operation = match action {
            0 => Operation::Cut {
                z: ui.get_cut_z_mm().parse().unwrap_or(f64::NAN),
                keep: ui.get_cut_keep(),
                cap: ui.get_cut_cap(),
            },
            1 => Operation::Split,
            3 => Operation::Simplify {
                target: ui.get_simplify_target().parse().unwrap_or(f64::NAN),
                percentage: ui.get_simplify_percentage(),
                deviation: ui.get_simplify_deviation().parse().unwrap_or(f64::NAN),
            },
            _ => Operation::Boolean(match ui.get_boolean_operation() {
                0 => BooleanOp::Union,
                2 => BooleanOp::Intersect,
                _ => BooleanOp::Subtract,
            }),
        };
        self.apply_model_operation(operation);
    }

    /// Desktop acceptance drives the same callbacks and, for labels, the
    /// actual screen-ray pointer path used by interactive placement.
    pub(crate) fn model_tool_evidence(&self, kind: i32) {
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        let document = self.scene_doc();
        if kind == 4 {
            document.choose(Selection::None);
            self.frame_models_for_evidence(1.1);
            if let Some(camera) = self.orbit.view_cell().borrow_mut().as_mut() {
                camera.azimuth = std::f32::consts::FRAC_PI_2;
                camera.elevation = 0.45;
            }
            return;
        }
        let ids = document
            .read_scene(|s| {
                s.graph()
                    .reachable_leaves(
                        sc_scene::CsgPolicy::Descend,
                        sc_scene::ReferencePolicy::Skip,
                    )
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let Some(&first) = ids.first() else {
            return;
        };
        document.choose(Selection::One(LeafId(first)));
        ui.set_model_tools_active(true);
        ui.set_supports_tool_active(false);
        ui.set_measure_tool_active(false);
        ui.set_place_face_tool_active(false);
        let mut label_focus = None;
        match kind {
            0 => {
                ui.set_model_tool_mode(0);
                ui.set_cut_z_mm("10".into());
                ui.set_cut_keep(2);
                ui.set_cut_cap(true);
                if let Some((_, scale)) = self.cut_screen_geometry() {
                    ui.invoke_cut_plane_dragged(scale * 2.);
                    ui.invoke_cut_plane_dragged(-scale * 2.);
                }
                ui.set_cut_z_mm("10".into());
                ui.invoke_model_tool_requested(0);
                // Separate the upper half after the one-step cut so both cap
                // surfaces are visible in the acceptance image.
                if let Some(id) = document.main_chosen_node() {
                    let edit = document
                        .read_scene(|s| {
                            crate::layout_tools::world_edit(
                                s.graph(),
                                id,
                                Transform3D::Affine(AffineTransform3D::from_translation([
                                    35., 0., -10.,
                                ])),
                            )
                        })
                        .flatten();
                    if let Some(edit) = edit {
                        let _ = document.commit_edit(edit.clone());
                        document.record_edit(edit);
                    }
                }
                let mut selection = document.chosen().ids();
                selection.reverse();
                document.choose(Selection::from_ids(selection));
            }
            1 => {
                ui.invoke_model_tool_requested(1);
                ui.set_model_tools_active(false);
            }
            2 => {
                ui.set_model_tool_mode(1);
                ui.set_label_text("Sleptic O".into());
                let point = document
                    .read_scene(|s| {
                        let mesh = sc_scene::bake_leaf_to_world(s.graph(), first).ok()?;
                        let b = mesh.local_aabb()?;
                        Some(glam::Vec3::new(
                            b.min()[0] + 8.,
                            b.min()[1] + 6.,
                            b.max()[2],
                        ))
                    })
                    .flatten();
                let Some(mut point) = point else {
                    return;
                };
                let [x, y] = document.plate_offset(first);
                point.x += x;
                point.y += y;
                let cursor = {
                    let slot = self.orbit.view_cell().borrow();
                    let (Some(camera), Some(viewport)) = (slot.as_ref(), self.renderer.view_extent())
                    else {
                        return;
                    };
                    picking::screen_point_of(
                        camera,
                        crate::mesh_render::starting_fov_y(),
                        viewport.width(),
                        viewport.height(),
                        point,
                    )
                };
                if let Some((x, y)) = cursor {
                    self.on_pointer_down(ScreenPoint::new(x, y), PointerModifiers::default());
                    self.on_pointer_up(ScreenPoint::new(x, y), PointerModifiers::default());
                }
                label_focus = Some(point + glam::Vec3::new(5.5, 1.25, 0.));
            }
            _ => {
                ui.set_model_tool_mode(2);
                document.choose(Selection::from_ids(ids));
                ui.set_boolean_operation(1);
                ui.invoke_model_tool_requested(2);
            }
        }
        self.frame_models_for_evidence(1.4);
        if let Some(focus) = label_focus {
            // A close-up must show the raised outlines and counters without
            // the floating tools panel obscuring them on the small window.
            ui.set_model_tools_active(false);
            if let Some(camera) = self.orbit.view_cell().borrow_mut().as_mut() {
                camera.look_at = focus;
                camera.azimuth = -std::f32::consts::FRAC_PI_2;
                camera.elevation = 1.05;
                camera.eye_distance = 28.;
            }
            self.note_unsaved();
        }
    }

    fn frame_models_for_evidence(&self, distance: f32) {
        let doc = self.scene_doc();
        let bounds = doc
            .read_scene(|s| {
                s.graph()
                    .reachable_leaves(
                        sc_scene::CsgPolicy::Descend,
                        sc_scene::ReferencePolicy::Skip,
                    )
                    .ok()?
                    .into_iter()
                    .filter_map(|id| {
                        let b = sc_scene::bake_leaf_to_world(s.graph(), id)
                            .ok()?
                            .local_aabb()?;
                        let [x, y] = s.graph().plate_offset(
                            s.graph().plate_of(id),
                            doc.plate_width.get(),
                            doc.plate_depth.get(),
                        );
                        let offset = glam::Vec3::new(x, y, 0.);
                        Some((
                            glam::Vec3::from_array(b.min().coords()) + offset,
                            glam::Vec3::from_array(b.max().coords()) + offset,
                        ))
                    })
                    .reduce(|(a, b), (c, d)| (a.min(c), b.max(d)))
            })
            .flatten();
        if let Some((lo, hi)) = bounds {
            if let Some(camera) = self.orbit.view_cell().borrow_mut().as_mut() {
                camera.zoom_to_box(lo, hi);
                camera.eye_distance *= distance;
            }
            self.note_unsaved();
        }
    }

    fn apply_model_operation(&self, operation: crate::model_tools::Operation) {
        if !self.layout_tools_allowed() || self.plate_locked() {
            return;
        }
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        let document = self.scene_doc();
        let ids = document.chosen().ids();
        if ui.get_simplify_busy() {
            return;
        }
        if matches!(operation, crate::model_tools::Operation::Simplify { .. }) {
            let Some(scene) = document.read_scene(|s| s.graph().clone()) else {
                return;
            };
            let revision = document.revision();
            let token = sc_sl1cer::CancelToken::new();
            let cancel = token.clone();
            ui.on_model_tool_cancel(move || cancel.cancel());
            ui.set_simplify_busy(true);
            ui.set_simplify_progress(0.0);
            ui.set_model_tool_error("".into());
            let progress_ui = ui.as_weak();
            let weak = ui.as_weak();
            let (tx, rx) = futures_channel::oneshot::channel();
            std::thread::spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut reported = -1.0_f32;
                    crate::model_tools::plan_with_progress(&scene, &ids, operation, |fraction| {
                        if fraction - reported >= 0.002 || fraction == 1.0 {
                            reported = fraction;
                            let _ = progress_ui.clone().upgrade_in_event_loop(move |ui| {
                                ui.set_simplify_progress(fraction);
                            });
                        }
                        !token.is_cancelled()
                    })
                }))
                .unwrap_or_else(|e| Err(crate::mesh_loader::panic_text(&e)));
                let _ = tx.send((result, token.is_cancelled()));
            });
            let _ = slint::spawn_local(async move {
                let result = rx.await;
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                ui.set_simplify_busy(false);
                let result = match result {
                    Ok((_, true)) => Err(crate::i18n::tr!(
                        "Simplification cancelled; no changes were applied."
                    )
                    .to_string()),
                    Ok((Ok(plan), false)) if revision == document.revision() => {
                        document.apply_batch(plan.edit, plan.slots).map(|()| {
                            document.choose(Selection::from_ids(plan.selected));
                            if let Some(summary) = plan.summary {
                                ui.set_plate_action_message(summary.into());
                            }
                        })
                    }
                    Ok((Ok(_), false)) => {
                        Err(crate::i18n::tr!("The project changed. Run Simplify again.")
                            .to_string())
                    }
                    Ok((Err(error), false)) => Err(error),
                    Err(error) => Err(error.to_string()),
                };
                ui.set_model_tool_error(result.err().unwrap_or_default().into());
            });
            return;
        }
        let result = document.read_scene(|s| crate::model_tools::plan(s.graph(), &ids, operation));
        let Some(result) = result else {
            return;
        };
        let result = result.and_then(|plan| {
            document.apply_batch(plan.edit, plan.slots)?;
            document.choose(Selection::from_ids(plan.selected));
            if let Some(summary) = plan.summary {
                ui.set_plate_action_message(summary.into());
            }
            Ok(())
        });
        ui.set_model_tool_error(result.err().unwrap_or_default().into());
    }

    fn label_at(&self, id: sc_scene::NodeId, point: [f64; 3]) {
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        let document = self.scene_doc();
        let normal = document
            .read_scene(|s| {
                let mesh = sc_scene::bake_leaf_to_world(s.graph(), id).ok()?;
                crate::model_tools::face_normal(&mesh, point)
            })
            .flatten();
        let Some(normal) = normal else {
            return;
        };
        document.choose(Selection::One(LeafId(id)));
        self.apply_model_operation(crate::model_tools::Operation::Label {
            text: ui.get_label_text().to_string(),
            point,
            normal,
            size: ui.get_label_size_mm().parse().unwrap_or(f64::NAN),
            depth: ui.get_label_depth_mm().parse().unwrap_or(f64::NAN),
            engrave: ui.get_label_engrave(),
        });
    }

    fn cut_screen_geometry(&self) -> Option<([(f32, f32); 4], f32)> {
        let ui = self.window.upgrade()?;
        if !ui.get_model_tools_active() || ui.get_model_tool_mode() != 0 {
            return None;
        }
        let doc = self.scene_doc();
        let id = doc.main_chosen_node()?;
        let bounds = doc.read_scene(|s| {
            sc_scene::bake_leaf_to_world(s.graph(), id)
                .ok()?
                .local_aabb()
        })??;
        let lo = bounds.min();
        let hi = bounds.max();
        let z = ui.get_cut_z_mm().parse::<f32>().ok()?;
        if !z.is_finite() {
            return None;
        }
        let [ox, oy] = doc.plate_offset(id);
        let viewport = self.renderer.view_extent()?;
        let slot = self.orbit.view_cell().borrow();
        let camera = slot.as_ref()?;
        let project = |x, y, z| {
            picking::screen_point_of(
                camera,
                crate::mesh_render::starting_fov_y(),
                viewport.width(),
                viewport.height(),
                glam::Vec3::new(x + ox, y + oy, z),
            )
        };
        let corners = [
            (lo[0] - 2., lo[1] - 2.),
            (hi[0] + 2., lo[1] - 2.),
            (hi[0] + 2., hi[1] + 2.),
            (lo[0] - 2., hi[1] + 2.),
        ];
        let points: Vec<_> = corners
            .iter()
            .map(|&(x, y)| project(x, y, z))
            .collect::<Option<Vec<_>>>()?;
        let next = project(corners[0].0, corners[0].1, z + 1.)?;
        Some((
            points.try_into().ok()?,
            next.1 - project(corners[0].0, corners[0].1, z)?.1,
        ))
    }

    pub(crate) fn drag_cut_plane(&self, dy: f32) {
        let Some((_, pixels_per_mm)) = self.cut_screen_geometry() else {
            return;
        };
        if pixels_per_mm.abs() < 0.1 {
            return;
        }
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        let z = ui.get_cut_z_mm().parse::<f32>().unwrap_or(0.);
        ui.set_cut_z_mm(
            format!(
                "{:.3}",
                (z + dy / pixels_per_mm).clamp(0., self.printer_volume().height_mm())
            )
            .into(),
        );
        self.note_unsaved();
    }

    fn update_cut_plane(&self) {
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        let points: Vec<_> = self
            .cut_screen_geometry()
            .map(|(p, _)| {
                p.into_iter()
                    .map(|(x, y)| crate::MeasureMarker { x, y })
                    .collect()
            })
            .unwrap_or_default();
        use slint::Model;
        let current = ui.get_cut_plane_markers();
        if current.row_count() != points.len()
            || points
                .iter()
                .enumerate()
                .any(|(i, p)| current.row_data(i).is_none_or(|q| p.x != q.x || p.y != q.y))
        {
            ui.set_cut_plane_markers(slint::ModelRc::new(slint::VecModel::from(points)));
        }
    }

    pub(crate) fn toggle_object_visibility(&self, index: i32) {
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        use slint::Model;
        if index >= ui.get_parts_list().row_count() {
            return;
        }
        let mut hidden = self.hidden_objects.borrow_mut();
        let rows = self
            .state
            .document
            .read_scene(|s| super::parts_panel::object_rows(s.graph()))
            .unwrap_or_default();
        let Some(row) = rows.get(index) else {
            return;
        };
        super::parts_panel::toggle_row_visibility(&mut hidden, &row.leaf_indices);
        ui.set_object_visibility(slint::ModelRc::new(slint::VecModel::from(
            rows.iter()
                .map(|row| row.leaf_indices.iter().any(|i| !hidden.contains(i)))
                .collect::<Vec<_>>(),
        )));
        drop(hidden);
        self.note_unsaved();
    }

    pub(crate) fn layer_changed(&self) {
        if let Some(ui) = self.window.upgrade() {
            let index = ui.get_layer_shown() as u32;
            if let Some(z) = self.state.document.layer_top_mm(index) {
                ui.set_layer_z_text(format!("{z:.3} mm").into());
                let doc = &self.state.document;
                let area = crate::print_estimate::lit_area(
                    doc.read_layer(index, |layer| {
                        layer
                            .polygons()
                            .iter()
                            .map(|p| {
                                p.outer().signed_area()
                                    + p.holes().iter().map(|h| h.signed_area()).sum::<f64>()
                            })
                            .sum::<f64>()
                    })
                    .unwrap_or(0.0),
                );
                if let (Some(printer), Some(material)) =
                    (doc.chosen_printer(), doc.chosen_resin())
                    && let Ok((p, m)) =
                        crate::slice_projection::load_chosen_profiles(&printer, &material)
                    && let Ok(config) =
                        sc_print_profiles::resolve(&p, &m, &doc.process_overrides.borrow())
                {
                    let schedule = sc_print_job::ExposureSchedule::uniform(
                        &config.material_resolved,
                        index + 1,
                        doc.layer_top_mm(0).unwrap_or(0.05),
                        doc.layer_top_mm(1).unwrap_or(0.1)
                            - doc.layer_top_mm(0).unwrap_or(0.05),
                    );
                    if let Some(regions) = doc
                        .read_scene(|s| {
                            s.graph()
                                .plate_scene(doc.active_plate())
                                .ok()
                                .and_then(|s| sc_print_glue::calibration::regions(&s).ok())
                        })
                        .flatten()
                        .filter(|r| !r.is_empty())
                    {
                        let text = regions
                            .iter()
                            .enumerate()
                            .map(|(tile, r)| {
                                let mut m = config.material_resolved.clone();
                                m.normal_exposure_sec =
                                    sc_print_primitives::PositiveFiniteSec::new(r.exposure_s)
                                        .expect("validated tile");
                                let s = sc_print_job::ExposureSchedule::uniform(
                                    &m,
                                    index + 1,
                                    doc.layer_top_mm(0).unwrap_or(0.05),
                                    m.layer_height_mm.value(),
                                );
                                crate::i18n::tr!(
                                    "Tile {}: {} s",
                                    tile + 1,
                                    format!("{:.3}", s.layers[index as usize].exposure_s)
                                )
                                .to_string()
                            })
                            .collect::<Vec<_>>()
                            .join(" / ");
                        ui.set_layer_detail_text(
                            format!(
                                "{}\n{text}\n{} {area:.2} mm2",
                                crate::i18n::tr!("Sequential exposure masks"),
                                crate::i18n::tr!("Lit area:")
                            )
                            .into(),
                        );
                    } else if let Some(layer) = schedule.layers.last() {
                        let material = &config.material_resolved;
                        ui.set_layer_cure_shade(crate::slice_cap::cure_shade(
                            layer.exposure_s,
                            material.normal_exposure_sec.value(),
                            material
                                .bottom_exposure_sec
                                .value()
                                .max(material.normal_exposure_sec.value()),
                        ));
                        ui.set_layer_detail_text(crate::i18n::tr!(
                            "Exposure: {} s\nLit area: {} mm\u{b2}",
                            format!("{:.3}", layer.exposure_s),
                            format!("{area:.2}")
                        ));
                    }
                }
            }
        }
        self.note_unsaved();
    }

    pub(crate) fn view_resized(&self, w: f32, h: f32) {
        self.renderer.take_view_size(w, h);
    }

    /// The window stopped resizing (`RESIZE_QUIET` without a new size).
    /// While dragging, targets come in rounded-up sizes and the image is
    /// stretched to fit; now they are remade at the exact size.
    pub(crate) fn resize_done(&self) {
        self.renderer.resize_settled();
    }

    pub(crate) fn orbit_by(&self, delta: ScreenDelta) {
        self.redraw_if(self.orbit.orbit_by(delta, self.renderer.view_extent()));
    }

    pub(crate) fn pan_by(&self, delta: ScreenDelta) {
        self.redraw_if(self.orbit.pan_by(delta, self.renderer.view_extent()));
    }

    pub(crate) fn zoom_by(&self, wheel_delta: f32, cursor: ScreenPoint) {
        let viewport = self.renderer.view_extent();
        self.redraw_if(self.orbit.zoom_at(wheel_delta, cursor, viewport));
    }

    fn refresh_camera_framing(&self) {
        if self.orbit.has_view() {
            let bounds = self
                .scene_doc()
                .read_scene(|s| crate::mesh_render::display_bounds(s, self.printer_volume()))
                .flatten();
            self.orbit
                .frame(self.project_view_bounds(bounds), self.printer_volume());
        }
    }

    pub(crate) fn frame_models(&self) {
        self.refresh_camera_framing();
        self.redraw_if(self.orbit.go_home());
    }

    pub(crate) fn look_along_axis(&self, view: crate::AxisView) {
        self.refresh_camera_framing();
        self.redraw_if(self.orbit.look_along(view.into()));
    }

    pub(crate) fn close_scene(&self) {
        // A load still on its way belongs to the close gesture, so the
        // loader drops it here; clearing the document then announces
        // `Cleared`, and `on_scene_cleared` resets the view and the window.
        self.state.loader.cancel_and_clear_loads();
        self.state.document.empty();
    }

    /// A load finished on the worker: install its scene, make it the
    /// document's, and frame it.
    pub(crate) fn scene_opened(&self) {
        let Some(loaded) = self.state.loader.take_completed_scene_load() else {
            return;
        };
        let LoadedSceneBundle {
            placement_message,
            loaded_spatial_index: spatial,
            gpu_scene: uploaded,
            loaded_project_data: project_payload,
            restored_slice_stack: reloaded_slices,
            loaded_source_path: source_path,
            loaded_mesh_health: mesh_health,
            ..
        } = loaded;
        // The worker's GPU buffers go straight in, unless the renderer is
        // gone or was rebuilt on another device meanwhile (the scene's
        // device token tells); then the scene is uploaded on this thread.
        let gpu_ready = {
            let mut renderer = self.renderer.renderer_slot().borrow_mut();
            renderer
                .as_mut()
                .is_some_and(|renderer| renderer.swap_in_scene(uploaded))
        };
        // Installing resets the camera (framed below), the object list
        // and the UI.
        let document = &self.state.document;
        document.take_spatial(spatial, source_path, project_payload);
        document.select_sole_object();
        self.check_import_units();
        if let Some(ui) = self.window.upgrade() {
            ui.set_plate_action_message(placement_message.into());
        }
        // A reopened project's still-valid slices become the held slice at
        // once, so it opens on them without slicing again. A stored slice
        // fires no scene event, so its layers and freshness are shown here.
        if let Some(saved) = reloaded_slices {
            document.keep_slice(saved.into_slice_result(document.revision()));
            self.show_layer_total();
            self.mirror_slice_staleness_to_ui();
        }
        // Clean meshes open in Transform, others in Prepare, where repair
        // lives (after installing: the section switcher needs a scene).
        self.switch_section(opening_section(mesh_health));
        if !gpu_ready {
            self.renderer.owe(Owed::UPLOAD);
            return;
        }
        self.state.loader.record_upload_success();
        // Framed on the scene and the plate together, so the plate is in
        // view and the orbit is anchored to the printer, however big or
        // small the model (as the first frame does with no camera).
        let bounds = self
            .renderer
            .renderer_slot()
            .borrow()
            .as_ref()
            .and_then(|renderer| renderer.shown_bounds());
        self.orbit.frame(
            self.project_view_bounds(bounds),
            self.state.place.volume(),
        );
        self.renderer.owe(Owed::REDRAW);
    }

    /// The main selected part as its position among the scene's parts in
    /// pre-order (tests only; the document keeps it as a node id,
    /// [`SceneDocument::selection_primary_node_id`]).
    #[cfg(test)]
    pub(crate) fn chosen(&self) -> Option<usize> {
        let document = &self.state.document;
        let primary = document.main_chosen_node()?;
        document.read_scene(|scene| scene.leaf_index_of(primary)).flatten()
    }

    /// The object list's selection: row `row` (a model), or nothing for
    /// a support row or a row that no longer exists (a click can race a
    /// reload). The document announces only real changes, so showing it
    /// again does not echo back here.
    pub(crate) fn on_select_by_index(&self, row: usize) {
        let document = &self.state.document;
        let part = document
            .read_scene(|spatial| {
                let rows = super::parts_panel::object_rows(spatial.graph());
                // A support row groups supports for visibility; it has no
                // single model to transform.
                let model = rows.get(row).filter(|row| !row.support)?;
                spatial.leaf_id_at_index(model.leaf_indices[0]).map(LeafId)
            })
            .flatten();
        document.choose(part.map_or(Selection::None, Selection::One));
    }

    /// Tab and Shift+Tab: the next or previous model (`delta` counts only
    /// by its sign), wrapping; the first model when none is selected.
    /// A drag in progress is not aborted.
    pub(crate) fn cycle_selection(&self, delta: i32) {
        let document = &self.state.document;
        let selected = document.main_chosen_node();
        let row = document
            .read_scene(|spatial| {
                let rows = super::parts_panel::object_rows(spatial.graph());
                let models: Vec<(usize, &super::parts_panel::ObjectRow)> = rows
                    .iter()
                    .enumerate()
                    .filter(|(_, row)| !row.support)
                    .collect();
                if models.is_empty() {
                    return None;
                }
                let leaf = selected.and_then(|id| spatial.leaf_index_of(id));
                let current = leaf.and_then(|leaf| {
                    models
                        .iter()
                        .position(|(_, row)| row.leaf_indices.contains(&leaf))
                });
                let count = models.len() as i32;
                let next = current.map_or(0, |at| {
                    (at as i32 + delta.signum()).rem_euclid(count) as usize
                });
                Some(models[next].0)
            })
            .flatten();
        if let Some(row) = row {
            self.on_select_by_index(row);
        }
    }

    /// Undo, unless the section forbids it or a repair is running. The
    /// menu greys Undo out with nothing to undo, but a shortcut can race
    /// that, so the document decides.
    pub(crate) fn undo(&self) {
        if self.history_allowed()
            && let Err(error) = self.state.document.step_back()
        {
            self.report_refused_edit(error);
        }
    }

    pub(crate) fn redo(&self) {
        if self.history_allowed()
            && let Err(error) = self.state.document.step_forward()
        {
            self.report_refused_edit(error);
        }
    }

    /// Undo and redo wait while a repair runs: an edit meanwhile would
    /// make its result stale (it would be thrown away, wasting the run).
    fn history_allowed(&self) -> bool {
        let repairing = self
            .window
            .upgrade()
            .is_some_and(|ui| ui.get_repairing());
        self.edits_unblocked() && !repairing
    }

    /// Edits are built against the live scene, so a refusal is a bug; it
    /// is shown rather than silently losing the gesture.
    fn report_refused_edit(&self, error: sc_scene::SceneError) {
        let message = ctr!(
            "ErrorDialog",
            "The scene rejected the last edit; nothing was changed."
        );
        let heading = ctr!("ErrorDialog", "Edit failed");
        self.state
            .messages
            .show_failure(&heading, None, message.to_string(), error.to_string());
    }

    /// Delete (the key, the context menu and Edit > Delete): the selected
    /// part, if the section allows editing.
    pub(crate) fn delete_selected(&self) {
        if self.layout_tools_allowed()
            && let Err(error) = self.state.document.delete_chosen()
        {
            self.report_refused_edit(error);
        }
    }

    /// Frame acceptance captures using the ordinary pan and wheel handlers.
    pub(crate) fn zoom_model_evidence(&self) {
        let Some(viewport) = self.renderer.view_extent() else {
            return;
        };
        let Some(bounds) = self
            .scene_doc()
            .read_scene(|s| s.graph().tight_aabb().ok().flatten())
            .flatten()
        else {
            return;
        };
        let mut point = (glam::Vec3::from(bounds.min().coords())
            + glam::Vec3::from(bounds.max().coords()))
            * 0.5;
        if let Some(ui) = self.window.upgrade()
            && ui.get_section() == WorkspaceSection::Slice
        {
            point.z = self
                .scene_doc()
                .layer_top_mm(ui.get_layer_shown() as u32)
                .unwrap_or(f64::from(point.z) * 2.0) as f32
                * 0.5;
        }
        for pass in 0..4 {
            let projected = self.orbit.view_cell().borrow().as_ref().and_then(|c| {
                picking::screen_point_of(
                    c,
                    crate::mesh_render::starting_fov_y(),
                    viewport.width(),
                    viewport.height(),
                    point,
                )
            });
            if let Some((x, y)) = projected {
                self.pan_by(ScreenDelta::new(viewport.width() * 0.5 - x, viewport.height() * 0.5 - y));
            }
            if pass == 1 {
                self.zoom_by(
                    280.0,
                    ScreenPoint::new(viewport.width() * 0.5, viewport.height() * 0.5),
                );
            }
        }
    }

    /// Find a real picking hit/miss for repeatable desktop input evidence.
    pub(crate) fn context_evidence_point(&self, kind: i32) -> Option<(f32, f32)> {
        let v = self.renderer.view_extent()?;
        for y in (100..v.height() as usize - 100).step_by(12) {
            for x in (80..v.width() as usize - 280).step_by(12) {
                let object = self.node_under(x as f32, y as f32).is_some();
                let plate = self.hits_plate_at(x as f32, y as f32);
                if match kind {
                    1 | 5 => object,
                    3 => !object && !plate,
                    _ => !object && plate,
                } {
                    return Some((x as f32, y as f32));
                }
            }
        }
        None
    }

    /// Verification sends two surface clicks through the ordinary picking path.
    pub(crate) fn measure_evidence(&self) {
        let Some(viewport) = self.renderer.view_extent() else {
            return;
        };
        let centre = {
            let camera = self.orbit.view_cell().borrow();
            camera.as_ref().and_then(|c| {
                picking::screen_point_of(
                    c,
                    crate::mesh_render::starting_fov_y(),
                    viewport.width(),
                    viewport.height(),
                    glam::Vec3::new(0.0, 0.0, 10.0),
                )
            })
        };
        if let Some((x, y)) = centre {
            self.zoom_by(220.0, ScreenPoint::new(x, y));
        }
        let points = {
            let camera = self.orbit.view_cell().borrow();
            let Some(camera) = camera.as_ref() else {
                return;
            };
            [[-5.0, -5.0, 20.0], [5.0, 5.0, 20.0]].map(|p| {
                picking::screen_point_of(
                    camera,
                    crate::mesh_render::starting_fov_y(),
                    viewport.width(),
                    viewport.height(),
                    glam::Vec3::from_array(p),
                )
            })
        };
        for point in points.into_iter().flatten() {
            let cursor = ScreenPoint::new(point.0, point.1);
            self.on_pointer_down(cursor, PointerModifiers::default());
            self.on_pointer_up(cursor, PointerModifiers::default());
        }
    }

    fn update_measure_markers(&self) {
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        let Some(viewport) = self.renderer.view_extent() else {
            return;
        };
        let camera = self.orbit.view_cell().borrow();
        let Some(camera) = camera.as_ref() else {
            return;
        };
        let markers = self
            .measure
            .borrow()
            .points
            .iter()
            .filter_map(|&p| {
                picking::screen_point_of(
                    camera,
                    crate::mesh_render::starting_fov_y(),
                    viewport.width(),
                    viewport.height(),
                    glam::Vec3::from_array(p),
                )
                .map(|(x, y)| crate::MeasureMarker { x, y })
            })
            .collect::<Vec<_>>();
        use slint::Model;
        let current = ui.get_measure_markers();
        if current.row_count() != markers.len()
            || markers
                .iter()
                .enumerate()
                .any(|(i, m)| current.row_data(i).is_none_or(|p| p.x != m.x || p.y != m.y))
        {
            ui.set_measure_markers(slint::ModelRc::new(slint::VecModel::from(markers)));
        }
    }

    fn place_face_active(&self) -> bool {
        self.layout_tools_allowed()
            && self
                .window
                .upgrade()
                .is_some_and(|ui| ui.get_place_face_tool_active())
    }

    /// Launch evidence: the first model selected, Place on face on, and the
    /// cursor over the model so one side shows its hover highlight.
    pub(crate) fn place_face_evidence(&self) {
        self.on_select_by_index(0);
        self.zoom_model_evidence();
        if let Some(ui) = self.window.upgrade() {
            ui.set_place_face_tool_active(true);
        }
        self.place_face_tool_changed();
        self.refresh_place_faces();
        // Aim at the middle of the first side facing the camera.
        let target = {
            let camera = self.orbit.view_cell().borrow();
            let cache = self.place_faces.borrow();
            camera
                .as_ref()
                .zip(cache.as_ref())
                .zip(self.renderer.view_extent())
                .and_then(|((camera, p), vp)| {
                    let offset =
                        glam::DVec3::new(f64::from(p.offset[0]), f64::from(p.offset[1]), 0.0);
                    p.faces.iter().find_map(|f| {
                        let mid =
                            f.outline.iter().sum::<glam::DVec3>() / f.outline.len() as f64 + offset;
                        (f.normal.dot(mid - camera.eye_position().as_dvec3()) < 0.0)
                            .then(|| {
                                picking::screen_point_of(
                                    camera,
                                    crate::mesh_render::starting_fov_y(),
                                    vp.width(),
                                    vp.height(),
                                    mid.as_vec3(),
                                )
                            })
                            .flatten()
                    })
                })
        };
        if let Some((x, y)) = target {
            self.on_pointer_move(ScreenPoint::new(x, y), PointerModifiers::default());
        }
    }

    /// The Place on face button or Escape turned the tool on or off.
    pub(crate) fn place_face_tool_changed(&self) {
        self.place_hover.set(None);
        if !self
            .window
            .upgrade()
            .is_some_and(|ui| ui.get_place_face_tool_active())
        {
            *self.place_faces.borrow_mut() = None;
        } else {
            self.pointer.viewport_gizmo_machine().remove_hover();
        }
        self.note_unsaved();
    }

    /// Keep the flat sides of the selected model current: recomputed only
    /// when the selection, its pose or its plate changes.
    fn refresh_place_faces(&self) {
        let document = &self.state.document;
        let size = (document.plate_width.get(), document.plate_depth.get());
        let key = document.main_chosen_node().and_then(|id| {
            document
                .read_scene(|spatial| {
                    let graph = spatial.graph();
                    let pose = graph.world_transform_of(id).ok()?;
                    Some((
                        id,
                        pose,
                        graph.plate_offset(graph.plate_of(id), size.0, size.1),
                    ))
                })
                .flatten()
        });
        let Some((node, pose, offset)) = key else {
            *self.place_faces.borrow_mut() = None;
            self.place_hover.set(None);
            return;
        };
        if self
            .place_faces
            .borrow()
            .as_ref()
            .is_some_and(|p| p.node == node && p.pose == pose && p.offset == offset)
        {
            return;
        }
        let faces = document
            .read_scene(|spatial| crate::layout_tools::place_faces(spatial.graph(), node))
            .unwrap_or_default();
        *self.place_faces.borrow_mut() = Some(PlaceFaces {
            node,
            pose,
            offset,
            faces,
        });
        self.place_hover.set(None);
    }

    /// The flat side under the cursor, if any.
    fn place_face_at(&self, cursor: ScreenPoint) -> Option<usize> {
        self.refresh_place_faces();
        let held_camera = self.orbit.view_cell().borrow();
        let camera = held_camera.as_ref()?;
        let viewport = self.renderer.view_extent()?;
        let ray = picking::ray_through_pixel(
            camera,
            crate::mesh_render::starting_fov_y(),
            viewport.width(),
            viewport.height(),
            cursor.x,
            cursor.y,
        );
        let cache = self.place_faces.borrow();
        let p = cache.as_ref()?;
        // Faces are plate-local; the model is drawn at its plate's offset.
        let origin = ray.start.as_dvec3()
            - glam::DVec3::new(f64::from(p.offset[0]), f64::from(p.offset[1]), 0.0);
        crate::place_on_face::pick(&p.faces, origin, ray.heading.as_dvec3())
    }

    /// Turn the selected model so flat side `index` rests on the plate.
    fn place_face(&self, index: usize) {
        if self.plate_locked() {
            return;
        }
        let Some((node, normal)) = self
            .place_faces
            .borrow()
            .as_ref()
            .and_then(|p| Some((p.node, p.faces.get(index)?.normal)))
        else {
            return;
        };
        let document = &self.state.document;
        if let Some(Some(edit)) = document.read_scene(|spatial| {
            crate::layout_tools::place_face_down(spatial.graph(), node, normal)
        }) && document.commit_edit(edit.clone()).is_ok()
        {
            document.record_edit(edit);
        }
        self.place_hover.set(None);
        self.note_unsaved();
    }

    pub(crate) fn measure_point(&self, point: [f32; 3]) {
        let result = self.measure.borrow_mut().click(point);
        if let Some(ui) = self.window.upgrade() {
            ui.set_measure_text(match result {
                Some([distance, x, y, z]) => format!(
                    "{} {distance:.3} mm   dX {x:.3}   dY {y:.3}   dZ {z:.3}",
                    ctr!("Measure", "Distance:")
                )
                .into(),
                None => ctr!(
                    "Measure",
                    "First point selected. Click a second surface point."
                ),
            });
        }
    }

    /// Esc, and a click on empty space: undoes one thing per press, the
    /// most temporary first. Place on face is put away, then a
    /// measurement cleared, then a drag aborted (the part goes back),
    /// then the keyboard gizmo dropped, and last the selection cleared.
    pub(crate) fn select_nothing(&self) {
        if let Some(ui) = self.window.upgrade()
            && ui.get_place_face_tool_active()
        {
            ui.set_place_face_tool_active(false);
            self.place_face_tool_changed();
            return;
        }
        if !self.measure.borrow().points.is_empty() {
            self.measure.borrow_mut().clear();
            if let Some(ui) = self.window.upgrade() {
                ui.set_measure_text("".into());
            }
            return;
        }
        if self.revert_drag_in_progress() {
            return;
        }
        if self.keyboard_gizmo_on() {
            return self.take_keyboard_focus(KeyboardTransformFocus::NoTransformFocus);
        }
        let document = &self.state.document;
        if !matches!(document.chosen(), Selection::None) {
            // The gizmo goes with the selection, so its hovered handle is
            // forgotten first; the document then announces the change.
            self.pointer.viewport_gizmo_machine().remove_hover();
            document.choose(Selection::None);
        }
    }

    /// Select the transform mode used by both pointer input and rendering.
    pub(crate) fn keys_move(&self) {
        self.select_gizmo_tool(0, KeyboardTransformFocus::MoveFocus);
    }

    pub(crate) fn keys_rotate(&self) {
        self.select_gizmo_tool(1, KeyboardTransformFocus::RotationFocus);
    }

    pub(crate) fn pick_scale_tool(&self) {
        self.select_gizmo_tool(2, KeyboardTransformFocus::ScalingFocus);
    }

    fn select_gizmo_tool(&self, index: i32, focus: KeyboardTransformFocus) {
        if !self.layout_tools_allowed()
            || self
                .state
                .document
                .main_chosen_node()
                .is_none()
        {
            return;
        }
        self.pointer.cancel_viewport_gizmo_drag(&self.state.document);
        self.pointer.viewport_gizmo_machine().set_handles(true);
        self.pointer.viewport_gizmo_machine().set_mode(match index {
            1 => crate::mesh_render::HandleShowing::Turn,
            2 => crate::mesh_render::HandleShowing::Resize,
            _ => crate::mesh_render::HandleShowing::Move,
        });
        if let Some(ui) = self.window.upgrade() {
            ui.set_active_tool(index);
            ui.set_model_tools_active(false);
            ui.set_supports_tool_active(false);
            ui.set_hollow_tool_active(false);
            ui.set_drain_tool_active(false);
            ui.set_measure_tool_active(false);
            ui.set_place_face_tool_active(false);
        }
        self.take_keyboard_focus(focus);
        self.note_unsaved();
    }

    pub(crate) fn on_gizmo_toolbar(&self, index: i32) {
        let active = self.window.upgrade().map_or_else(
            || match self.pointer.viewport_gizmo_machine().mode() {
                crate::mesh_render::HandleShowing::Turn => 1,
                crate::mesh_render::HandleShowing::Resize => 2,
                _ => 0,
            },
            |ui| ui.get_active_tool(),
        );
        // Clicking the active tool puts it away: no tool, like a fresh
        // selection (Sleptic: "no tools should be selected by default").
        if active == index {
            self.clear_gizmo_tool();
            return;
        }
        match index {
            1 => self.keys_rotate(),
            2 => self.pick_scale_tool(),
            _ => self.keys_move(),
        }
    }

    /// No Move / Rotate / Scale tool: no gizmo or tool panel; the part is
    /// still dragged on the plate. The state a selection starts in, and the
    /// one clicking off a model returns to, so reselecting it does not
    /// reopen the last tool (Sleptic).
    fn clear_gizmo_tool(&self) {
        self.pointer.cancel_viewport_gizmo_drag(&self.state.document);
        if let Some(ui) = self.window.upgrade() {
            ui.set_active_tool(-1);
        }
        self.pointer.viewport_gizmo_machine().set_handles(false);
        self.take_keyboard_focus(KeyboardTransformFocus::NoTransformFocus);
        self.note_unsaved();
    }

    fn sync_gizmo_mode(&self) {
        if let Some(ui) = self.window.upgrade() {
            self.pointer
                .viewport_gizmo_machine()
                .set_handles(ui.get_active_tool() >= 0);
            let mode = match ui.get_active_tool() {
                1 => crate::mesh_render::HandleShowing::Turn,
                2 => crate::mesh_render::HandleShowing::Resize,
                _ => crate::mesh_render::HandleShowing::Move,
            };
            if self.pointer.viewport_gizmo_machine().mode() != mode {
                self.pointer.viewport_gizmo_machine().set_mode(mode);
            }
        }
    }

    /// Gives the arrow keys to a gizmo tool, or takes them back with
    /// `None`. Arming needs a section that allows transforms; dropping
    /// always works, so leaving Transform can drop it. The window and a
    /// redraw follow a real change.
    fn take_keyboard_focus(&self, focus: KeyboardTransformFocus) {
        // Dropping the focus is always allowed; taking it only where the
        // layout may be edited.
        let allowed = focus == KeyboardTransformFocus::NoTransformFocus || self.layout_tools_allowed();
        let changed = allowed
            .then(|| self.pointer.change_viewport_keyboard_focus(&self.state.document, focus))
            .flatten();
        if let Some(now) = changed {
            self.with_ui(|ui| ui.set_keys_drive_gizmo(now != KeyboardTransformFocus::NoTransformFocus));
            self.note_unsaved();
        }
    }

    pub(crate) fn gizmo_key_down(&self, code: i32) {
        let document = &self.state.document;
        self.redraw_if(self.pointer.press_viewport_gizmo_arrow(document, code));
    }

    pub(crate) fn gizmo_key_up(&self, code: i32) {
        let document = &self.state.document;
        self.redraw_if(self.pointer.release_viewport_gizmo_arrow(document, code));
    }

    /// Returns `true` when the press grabbed a gizmo handle; otherwise the
    /// viewer treats a left drag as a camera pan.
    pub(crate) fn on_pointer_down(
        &self,
        at: ScreenPoint,
        mut keys: PointerModifiers,
    ) -> bool {
        if self
            .window
            .upgrade()
            .is_some_and(|ui| ui.get_supports_tool_active())
            && (keys.control || keys.alt)
        {
            self.paint_drag.set(true);
            self.paint_at(at, keys.alt);
            return true;
        }
        self.sync_gizmo_mode();
        if self.pointer.viewport_gizmo_machine().mode() == crate::mesh_render::HandleShowing::Resize
            && self.window.upgrade().is_some_and(|ui| ui.get_scale_uniform())
        {
            keys.shift = true;
        }
        self.pointer.begin_viewport_pointer_press(
            &self.state.document,
            self.orbit.view_cell(),
            self.renderer.view_extent(),
            at,
            keys.clone(),
            self.layout_tools_allowed()
                && !self.plate_locked()
                && !keys.control
                && !self.window.upgrade().is_some_and(|ui| {
                    ui.get_supports_tool_active()
                        || ui.get_drain_tool_active()
                        || ui.get_measure_tool_active()
                        || ui.get_model_tools_active()
                        || ui.get_place_face_tool_active()
                }),
        )
    }

    pub(crate) fn on_pointer_move(&self, at: ScreenPoint, mut keys: PointerModifiers) {
        if self.paint_drag.get() {
            self.paint_at(at, keys.alt);
            return;
        }
        // The pointer position is remembered everywhere (a paste lands under it),
        // even where the gizmo below does not react to it.
        self.pointer.record_viewport_hover_position(at);
        self.update_plate_tile_hover(Some(at));
        if self.place_face_active() {
            let hover = self.place_face_at(at);
            if self.place_hover.replace(hover) != hover {
                self.note_unsaved();
            }
            return;
        }
        if !self.layout_tools_allowed()
            || self
                .window
                .upgrade()
                .is_some_and(|ui| ui.get_supports_tool_active())
        {
            return;
        }
        if self.pointer.viewport_gizmo_machine().mode() == crate::mesh_render::HandleShowing::Resize
            && self.window.upgrade().is_some_and(|ui| ui.get_scale_uniform())
        {
            keys.shift = true;
        }
        let moved = self.pointer.move_viewport_pointer(
            &self.state.document,
            self.orbit.view_cell(),
            self.renderer.view_extent(),
            self.state.place.volume(),
            at,
            keys,
        );
        self.redraw_if(moved);
    }

    pub(crate) fn on_pointer_up(&self, cursor: ScreenPoint, modifiers: PointerModifiers) {
        if self.paint_drag.replace(false) {
            return;
        }

        match self.pointer.finish_viewport_pointer_press(&self.state.document, cursor) {
            ViewportReleaseResult::ViewportDragCommitted => {
                // A dragged object may have crossed onto another plate.
                if let Some(id) = self.scene_doc().main_chosen_node() {
                    self.activate_plate_for_click(cursor, Some(id));
                }
                self.note_unsaved()
            }
            ViewportReleaseResult::HandleClick(cursor) => {
                if let Some(Some(node_id)) = self.pointer.pick_viewport_position(
                    &self.state.document,
                    self.orbit.view_cell(),
                    self.renderer.view_extent(),
                    cursor,
                ) {
                    self.state
                        .document
                        .choose(Selection::One(LeafId(node_id)));
                    self.activate_plate_for_click(cursor, Some(node_id));
                }
                self.note_unsaved();
            }
            ViewportReleaseResult::ViewportSelectionClick(cursor) => {
                if self.click_plate_tile(cursor) {
                    self.note_unsaved();
                    return;
                }
                if self.place_face_active()
                    && let Some(index) = self.place_face_at(cursor)
                {
                    self.place_face(index);
                    return;
                }
                if let Some(ui) = self.window.upgrade()
                    && (ui.get_supports_tool_active()
                        || ui.get_drain_tool_active()
                        || ui.get_measure_tool_active()
                        || (ui.get_model_tools_active() && ui.get_model_tool_mode() == 1))
                {
                    let camera_slot = self.orbit.view_cell().borrow();
                    if let Some(camera) = camera_slot.as_ref() {
                        let Some(viewport) = self.renderer.view_extent() else {
                            return;
                        };
                        let ray = picking::ray_through_pixel(
                            camera,
                            crate::mesh_render::starting_fov_y(),
                            viewport.width(),
                            viewport.height(),
                            cursor.x,
                            cursor.y,
                        );
                        // Objects live in plate-local coordinates and are
                        // drawn at their plate's grid offset, so the pick and
                        // the hit point must undo that offset: `position` is
                        // plate-local (drain holes, support contacts), `shown`
                        // is where it appears on screen (measure markers).
                        let doc = &self.state.document;
                        let size = (doc.plate_width.get(), doc.plate_depth.get());
                        if let Some(Some((id, position, shown))) = doc.read_scene(|spatial| {
                            let id = picking::part_under_ray_on_plates(&ray, spatial, Some(size))?;
                            let [ox, oy] = spatial.graph().plate_offset(
                                spatial.graph().plate_of(id),
                                size.0,
                                size.1,
                            );
                            let mut local = ray;
                            local.start.x -= ox;
                            local.start.y -= oy;
                            let query = spatial.leaf(id)?;
                            let t = picking::part_hit_distance(&local, &query)?;
                            let position = local.start + local.heading * t;
                            Some((id, position, position + glam::Vec3::new(ox, oy, 0.0)))
                        }) {
                            if ui.get_model_tools_active() && ui.get_model_tool_mode() == 1 {
                                drop(camera_slot);
                                self.label_at(
                                    id,
                                    [
                                        f64::from(position.x),
                                        f64::from(position.y),
                                        f64::from(position.z),
                                    ],
                                );
                                return;
                            }
                            if ui.get_measure_tool_active() {
                                self.measure_point([shown.x, shown.y, shown.z]);
                                return;
                            }
                            if ui.get_drain_tool_active() {
                                let direction = ray.heading;
                                drop(camera_slot);
                                self.state
                                    .document
                                    .choose(Selection::One(LeafId(id)));
                                let point =
                                    [position.x as f64, position.y as f64, position.z as f64];
                                // The clicked point fills the point fields, so
                                // a hole can be moved by editing them.
                                ui.set_drain_x_mm(format!("{:.2}", point[0]).into());
                                ui.set_drain_y_mm(format!("{:.2}", point[1]).into());
                                ui.set_drain_z_mm(format!("{:.2}", point[2]).into());
                                let view =
                                    [direction.x as f64, direction.y as f64, direction.z as f64];
                                self.hollow_action(crate::hollow_tools::HoleChange::Add(
                                    drain_request(&ui, point, view),
                                ));
                                return;
                            }
                            ui.invoke_support_contact_requested(
                                position.x,
                                position.y,
                                position.z,
                                id.get() as i32,
                                modifiers.shift,
                            );
                        }
                    }
                } else {
                    if modifiers.control {
                        if let Some(id) = self.node_under(cursor.x, cursor.y) {
                            self.state.document.toggle_selection(id);
                            self.activate_plate_for_click(cursor, Some(id));
                        }
                    } else {
                        self.select_under(cursor);
                    }
                }
            }
            ViewportReleaseResult::ViewportReleaseIgnored => (),
        }
    }

    fn paint_at(&self, cursor: ScreenPoint, avoid: bool) {
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        let slot = self.orbit.view_cell().borrow();
        let (Some(camera), Some(viewport)) = (slot.as_ref(), self.renderer.view_extent()) else {
            return;
        };
        let ray = picking::ray_through_pixel(
            camera,
            crate::mesh_render::starting_fov_y(),
            viewport.width(),
            viewport.height(),
            cursor.x,
            cursor.y,
        );
        if let Some(Some((id, p))) = self.state.document.read_scene(|spatial| {
            let doc = self.scene_doc();
            picking::part_surface_point(
                &ray,
                spatial,
                (doc.plate_width.get(), doc.plate_depth.get()),
            )
        }) {
            ui.invoke_support_paint_requested(p.x, p.y, p.z, id.get() as i32, avoid);
        }
    }

    /// The pointer left the 3D view: forget where it hovered, so a paste
    /// now goes to the view's centre and not to a point it is no longer
    /// over.
    pub(crate) fn pointer_left(&self) {
        self.paint_drag.set(false);
        self.pointer.leave_viewport_pointer();
        self.update_plate_tile_hover(None);
    }

    /// Index (into `plate-overlays`) of the plate tool tile under `cursor`.
    /// Tiles are drawings without a TouchArea, so the viewport keeps every
    /// pointer event: drags that start on a tile still pan or orbit, and
    /// only a left click (no drag) reaches [`Self::click_plate_tile`].
    fn plate_tile_at(&self, cursor: ScreenPoint) -> Option<usize> {
        if self.state.place.section() == WorkspaceSection::Slice {
            return None;
        }
        let p = glam::Vec2::new(cursor.x, cursor.y);
        self.plate_tile_hits.borrow().iter().position(|quad| {
            // Inside a convex quad: on the same side of all four edges.
            let (mut left, mut right) = (false, false);
            for i in 0..4 {
                let (a, b) = (quad[i], quad[(i + 1) % 4]);
                let side = (b - a).perp_dot(p - a);
                left |= side > 0.0;
                right |= side < 0.0;
            }
            !(left && right)
        })
    }

    fn update_plate_tile_hover(&self, cursor: Option<ScreenPoint>) {
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        let hovered = cursor
            .and_then(|c| self.plate_tile_at(c))
            .map_or(-1, |i| i as i32);
        if ui.get_hovered_plate_tile() != hovered {
            ui.set_hovered_plate_tile(hovered);
        }
    }

    /// Runs the plate tool tile under a left click; false when the click
    /// missed every tile. A click on a disabled tile is still consumed, so
    /// it does not fall through and clear the selection.
    fn click_plate_tile(&self, cursor: ScreenPoint) -> bool {
        use slint::Model as _;
        let Some(index) = self.plate_tile_at(cursor) else {
            return false;
        };
        let Some(ui) = self.window.upgrade() else {
            return false;
        };
        let Some(tile) = ui.get_plate_overlays().row_data(index) else {
            return false;
        };
        let usable = tile.slot == 0 || tile.slot >= 4 || (ui.get_scene_present() && !tile.locked);
        match tile.slot {
            0 => ui.invoke_plate_selected(tile.id),
            // The trash tile deletes the plate (Sleptic): at once when it is
            // empty, after an "are you sure" when it holds objects.
            1 if !tile.locked => self.delete_plate_from_tile(tile.id as u32),
            6 => ui.invoke_add_plate_requested(),
            slot if usable => ui.invoke_plate_action(
                tile.id,
                match slot {
                    1 => 6,
                    2 => 3,
                    3 => 2,
                    4 => 26,
                    _ => 22,
                },
            ),
            _ => {}
        }
        true
    }

    /// Plate tile screen quads without a GPU frame to project them.
    #[cfg(test)]
    pub(crate) fn set_plate_tile_hits_for_test(&self, hits: Vec<[glam::Vec2; 4]>) {
        *self.plate_tile_hits.borrow_mut() = hits;
    }

    /// The right button went down; its release decides between the
    /// context menu (a click) and an orbit (a drag).
    pub(crate) fn right_down(&self, x: f32, y: f32) {
        self.pointer.begin_context_pointer_press(ScreenPoint::new(x, y));
    }

    /// Where to open the context menu: the press point, for a click; none
    /// after a drag (the orbit used it) or without a press.
    pub(crate) fn right_up(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let press = self.pointer.finish_context_pointer_press(ScreenPoint::new(x, y))?;
        Some((press.x, press.y))
    }

    /// The part under `(x, y)`, leaving the selection alone (the context
    /// menu decides from it whether to select and offer Copy). `None` on
    /// a miss, or with no scene or camera yet.
    pub(crate) fn node_under(&self, x: f32, y: f32) -> Option<sc_scene::NodeId> {
        self.pick(ScreenPoint::new(x, y)).flatten()
    }

    /// The picker's answer at `cursor`: `None` when it could not look (no
    /// scene, camera or viewport), else the part hit, if any.
    fn pick(&self, at: ScreenPoint) -> Option<Option<sc_scene::NodeId>> {
        let document = &self.state.document;
        self.pointer.pick_viewport_position(
            document,
            self.orbit.view_cell(),
            self.renderer.view_extent(),
            at,
        )
    }

    /// A click: selects the part under `cursor`, or on a miss clears as
    /// Esc does; then the plate under it becomes active.
    fn select_under(&self, cursor: ScreenPoint) {
        let Some(hit) = self.pick(cursor) else {
            return;
        };
        match hit {
            Some(part) => self
                .state
                .document
                .choose(Selection::One(LeafId(part))),
            None => self.select_nothing(),
        }
        self.activate_plate_for_click(cursor, hit);
    }

    /// Plate activation for a click: a clicked object makes its own plate
    /// active (not whichever plate lies behind it on screen); a click on
    /// empty plate surface activates that plate, but only when there is more
    /// than one plate to choose from. Camera drags never change the plate.
    pub(crate) fn activate_plate_for_click(
        &self,
        cursor: ScreenPoint,
        hit: Option<sc_scene::NodeId>,
    ) {
        let document = self.scene_doc();
        if let Some(id) = hit {
            if let Some(plate) = document.read_scene(|s| s.graph().plate_of(id)) {
                document.set_active_plate(plate);
            }
            return;
        }
        let several = document
            .read_scene(|s| s.graph().plates().order.len() > 1)
            .unwrap_or(false);
        if several && let Some(plate) = self.plate_at(cursor.x, cursor.y) {
            document.set_active_plate(plate);
        }
    }

    /// Paste the clipboard's fragment onto the active plate in the most
    /// compact free spot (see [`clipboard::placement`]). The search
    /// starts next to the copied parts when they stand on the active
    /// plate, as in Orca, and at the plate centre otherwise; the cursor
    /// plays no part (Sleptic: Ctrl+V dropped copies "way off the build
    /// plate" from the cursor's ray). Returns `true` when the paste
    /// landed.
    pub(crate) fn paste_at_cursor(&self, clipboard: &Clipboard) -> bool {
        let active = self.scene_doc().active_plate();
        let anchor = clipboard
            .source()
            .filter(|&(plate, _)| plate == active)
            .map_or([0.0, 0.0], |(_, xy)| xy);
        self.paste_compact(clipboard, anchor)
    }

    /// Duplicate (Ctrl+D): the most compact free spot, searched from
    /// the selected object; the mouse position is irrelevant.
    pub(crate) fn duplicate_clipboard(&self, clipboard: &Clipboard) -> bool {
        let doc = &self.state.document;
        let Some((plate, anchor)) = clipboard::selection_anchor(&doc) else {
            return false;
        };
        if plate != doc.active_plate() {
            doc.set_active_plate(plate);
        }
        self.paste_compact(clipboard, anchor)
    }

    /// Paste from the canvas context menu. The right-clicked point
    /// (the cursor may have drifted inside the menu since) is the
    /// search anchor when it lies on the active plate, so the copy
    /// lands in the compact free spot nearest where the user clicked.
    pub(crate) fn paste_at_pixel(&self, clipboard: &Clipboard, x: f32, y: f32) -> bool {
        let clicked = self.renderer.view_extent().and_then(|viewport| {
            let camera_slot = self.orbit.view_cell().borrow();
            let ray = picking::ray_through_pixel(
                camera_slot.as_ref()?,
                crate::mesh_render::starting_fov_y(),
                viewport.width(),
                viewport.height(),
                x,
                y,
            );
            picking::level_plane_crossing(&ray, 0.0)
        });
        let doc = &self.state.document;
        let active = doc.active_plate();
        let [ox, oy] = doc
            .read_scene(|s| {
                s.graph()
                    .plate_offset(active, doc.plate_width.get(), doc.plate_depth.get())
            })
            .unwrap_or([0.0, 0.0]);
        let volume = self.printer_volume();
        let half = [
            f64::from(volume.width_mm()) * 0.5,
            f64::from(volume.depth_mm()) * 0.5,
        ];
        // The plate's objects are stored plate-local: undo the plate's
        // grid offset on the clicked point.
        let on_plate = clicked
            .map(|(wx, wy)| [f64::from(wx - ox), f64::from(wy - oy)])
            .filter(|p| p[0].abs() <= half[0] && p[1].abs() <= half[1]);
        match on_plate {
            Some(anchor) => self.paste_compact(clipboard, anchor),
            None => self.paste_at_cursor(clipboard),
        }
    }

    /// Pastes into the most compact free spot of the active plate, the one
    /// nearest `anchor` (plate XY) on a tie, as one undo step.
    fn paste_compact(&self, clipboard: &Clipboard, anchor: [f64; 2]) -> bool {
        let document = &self.state.document;
        let plate = document.active_plate();
        let footprint = clipboard::copied_box(clipboard.fragment());
        let taken = document.read_scene(|spatial| clipboard::placement::obstacle_aabbs_on_plate(spatial, plate));
        let (true, Some(footprint), Some(taken)) = (self.layout_tools_allowed(), footprint, taken) else { return false };
        // The gap the Arrange panel holds, 3 mm when it does not parse.
        let spacing = self.window.upgrade().and_then(|ui| ui.get_arrange_gap_mm().parse::<f64>().ok());
        let volume = self.printer_volume();
        let middle = [volume.width_mm(), volume.depth_mm()].map(|side| f64::from(side) / 2.0);
        let shift = clipboard::placement::compact_paste_translation(
            &footprint,
            anchor,
            middle,
            spacing.unwrap_or(3.0),
            &taken,
        );
        let pasted = document.paste_parts(clipboard.fragment(), shift);
        if let Err(refused) = &pasted {
            self.report_refused_edit(refused.clone());
        }
        let added = matches!(pasted, Ok(Some(_)));
        if added {
            self.note_unsaved();
        }
        added
    }
    /// The selection in the object list: the row of the last selected
    /// part, and every model row holding a selected part.
    fn show_selection(&self) {
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        let document = &self.state.document;
        let primary = document.main_chosen_node();
        let selected = document.chosen().ids();
        let (row, marked) = document
            .read_scene(|spatial| {
                let rows = super::parts_panel::object_rows(spatial.graph());
                let leaf = primary.and_then(|id| spatial.leaf_index_of(id));
                let row = leaf
                    .and_then(|leaf| rows.iter().position(|row| row.leaf_indices.contains(&leaf)));
                let holds_selected = |row: &super::parts_panel::ObjectRow| {
                    !row.support
                        && row.leaf_indices.iter().any(|&i| {
                            spatial
                                .leaf_id_at_index(i)
                                .is_some_and(|id| selected.contains(&id))
                        })
                };
                (row, rows.iter().map(holds_selected).collect::<Vec<_>>())
            })
            .unwrap_or_default();
        ui.set_chosen_part_row(row.map_or(-1, |row| row as i32));
        ui.set_selected_object_rows(slint::ModelRc::new(slint::VecModel::from(marked)));
    }

    /// Pushes the primary selection's local-transform translation into
    /// the Transform section's Position fields, formatted to 2 decimal
    /// places (mm), and its world-space bounding-box size into the
    /// read-only "Bounding box" row alongside them. Empty strings
    /// while nothing is selected; the panel itself is hidden in that
    /// case, but a stale non-empty value would otherwise briefly flash
    /// the previous object's numbers. Called on every
    /// `SelectionChanged` and `Mutated` scene event, so both rows
    /// track a gizmo drag or keyboard nudge live, not just this
    /// panel's own commits -- deliberately bundled into one function
    /// rather than two, so every one of this function's many call
    /// sites can never update one row and forget the other: a rotate
    /// or scale gizmo drag changes the bounding box exactly as much as
    /// a translate drag changes the position, and both already fire
    /// through this same set of scene events.
    /// The selected object's world-space bounding-box size in mm.
    fn selected_object_size(&self) -> Option<[f64; 3]> {
        let document = &self.state.document;
        let id = document.main_chosen_node()?;
        document
            .read_scene(|spatial| {
                let query = spatial.leaf(id)?;
                let world = query.parent_world().compose(&query.local_transform());
                query.transformed_aabb(world).map(|aabb| {
                    let (min, max) = (aabb.min().coords(), aabb.max().coords());
                    [0, 1, 2].map(|i| f64::from(max[i] - min[i]))
                })
            })
            .flatten()
    }

    /// After a load, offer a unit conversion when the object is implausibly
    /// small (Orca Slicer's "object too small, may be in inches or meters"
    /// check): see [`looks_like_wrong_units`].
    fn check_import_units(&self) {
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        match self
            .selected_object_size()
            .filter(|size| looks_like_wrong_units(*size))
        {
            Some([x, y, z]) => {
                ui.set_units_hint_text(crate::i18n::tr!(
                    "This model is very small ({} x {} x {} mm). It may have been saved in inches or meters.",
                    format_transform_value(x),
                    format_transform_value(y),
                    format_transform_value(z)
                ));
                ui.set_units_hint_visible(true);
            }
            None => ui.set_units_hint_visible(false),
        }
    }

    /// Scale the selected object uniformly by `factor` (25.4 for inches,
    /// 1000 for meters) as one undoable edit, then centre it and drop it on
    /// the plate.
    pub(crate) fn on_units_conversion_requested(&self, factor: f32) {
        let document = &self.state.document;
        if let Some(node_id) = document.main_chosen_node()
            && let Some(before) = document
                .read_scene(|spatial| spatial.leaf(node_id).map(|q| q.local_transform()))
                .flatten()
            && let Some((_, scales)) = crate::object_transform::components(before)
        {
            let after = (0..3).try_fold(before, |t, axis| {
                crate::object_transform::edit(t, true, axis, scales[axis] * f64::from(factor))
            });
            if let Some(after) = after {
                let edit = SceneEdit::SetLocalTransform {
                    node_id,
                    before,
                    after,
                };
                if document.commit_edit(edit.clone()).is_ok() {
                    document.record_edit(edit);
                }
            }
        }
        self.on_center_on_plate_requested();
        self.on_drop_to_plate_requested();
        if let Some(ui) = self.window.upgrade() {
            ui.set_units_hint_visible(false);
        }
        self.mirror_position_to_ui();
    }

    fn mirror_position_to_ui(&self) {
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        let document = &self.state.document;
        ui.set_selected_is_hollow(
            document
                .main_chosen_node()
                .and_then(|id| {
                    document
                        .read_scene(|s| crate::hollow_tools::wall_thickness(s.graph(), id))
                        .flatten()
                })
                .is_some(),
        );
        let leaf_data = document.main_chosen_node().and_then(|id| {
            document
                .read_scene(|spatial| {
                    let query = spatial.leaf(id)?;
                    let world = query.parent_world().compose(&query.local_transform());
                    let aabb = query.transformed_aabb(world);
                    let size = aabb.map(|aabb| {
                        let min = aabb.min().coords();
                        let max = aabb.max().coords();
                        [
                            f64::from(max[0] - min[0]),
                            f64::from(max[1] - min[1]),
                            f64::from(max[2] - min[2]),
                        ]
                    });
                    let position = displayed_position(spatial, id)
                        .unwrap_or_else(|| translation_of(&query.local_transform()));
                    Some((position, size))
                })
                .flatten()
        });
        let text = |v: Option<f64>| match v {
            Some(v) => SharedString::from(format_transform_value(v)),
            None => SharedString::default(),
        };
        let [x, y, z] = leaf_data.map(|(t, _)| t).unwrap_or_default();
        let present = leaf_data.is_some();
        ui.set_position_x_text(text(present.then_some(x)));
        ui.set_position_y_text(text(present.then_some(y)));
        ui.set_position_z_text(text(present.then_some(z)));
        // The selected model's own plate, numbered like the plate tabs (the
        // panel showed the active plate's internal id, R55).
        let plate_number = document.main_chosen_node().and_then(|id| {
            document
                .read_scene(|spatial| {
                    let graph = spatial.graph();
                    let plate = graph.plate_of(id);
                    graph.plates().order.iter().position(|p| p.id == plate)
                })
                .flatten()
        });
        ui.set_object_plate_number(plate_number.map_or(0, |i| i as i32 + 1));
        let components = document.main_chosen_node().and_then(|id| {
            document
                .read_scene(|spatial| {
                    spatial
                        .leaf(id)
                        .and_then(|q| crate::object_transform::components(q.local_transform()))
                })
                .flatten()
        });
        ui.set_object_rotation(slint::ModelRc::new(slint::VecModel::from(
            (0..3)
                .map(|i| text(components.map(|(r, _)| r[i])))
                .collect::<Vec<_>>(),
        )));
        ui.set_object_scale(slint::ModelRc::new(slint::VecModel::from(
            (0..3)
                .map(|i| text(components.map(|(_, s)| s[i])))
                .collect::<Vec<_>>(),
        )));

        let size = leaf_data.and_then(|(_, size)| size);
        ui.set_bbox_x_text(text(size.map(|s| s[0])));
        ui.set_bbox_y_text(text(size.map(|s| s[1])));
        ui.set_bbox_z_text(text(size.map(|s| s[2])));
        ui.set_object_volume_text(
            self.selected_volume_mm3()
                .map(|v| SharedString::from(format!("{:.2} mL", v / 1000.0)))
                .unwrap_or_default(),
        );
    }

    /// Enclosed volume of the selected leaf in world space (mm³): the local
    /// mesh volume, cached per mesh, times the world transform's scale.
    fn selected_volume_mm3(&self) -> Option<f64> {
        let id = self.state.document.main_chosen_node()?;
        self.state
            .document
            .read_scene(|spatial| {
                let graph = spatial.graph();
                let sc_scene::node::NodeKind::Leaf(payload) = &graph.node(id)?.kind else {
                    return None;
                };
                let mesh: &sc_mesh_core::IndexedMesh = match payload {
                    sc_scene::node::LeafPayload::Mesh(mesh) => mesh,
                    sc_scene::node::LeafPayload::MeshResource(rid) => graph.mesh_resource(*rid)?,
                };
                let key = (std::ptr::from_ref(mesh) as usize, mesh.faces.len());
                let cached = self.volume_cache.get().filter(|(k, _)| *k == key);
                let local = cached.map_or_else(
                    || {
                        let v = crate::layout_tools::mesh_volume_mm3(mesh);
                        self.volume_cache.set(Some((key, v)));
                        v
                    },
                    |(_, v)| v,
                );
                let scale = graph.world_transform_of(id).ok()?.determinant().abs();
                Some(local * scale)
            })
            .flatten()
    }

    /// Commits an edit from one of the Transform section's Position
    /// fields: `axis` is 0/1/2 for X/Y/Z, `text` the field's raw text
    /// at Enter or focus-loss. Unparseable text or an inactive gate
    /// (no selection, not in the Transform section) just re-syncs the
    /// field to the real value rather than erroring, since a stray
    /// blur with half-typed input is a normal thing to happen while
    /// editing, not a mistake worth surfacing.
    ///
    /// The written transform keeps the object's existing rotation and
    /// scale exactly: only the translation component changes, via the
    /// same `AffineTransform3D::from_translation(delta).compose(before)`
    /// composition the gizmo's mouse-drag translate uses (see
    /// `handle_drag::drag_to_transform`), so this is provably a pure
    /// translation, not a decomposition of the existing linear part.
    /// Clamped against the build envelope exactly like a drag or a
    /// keyboard nudge, then applied and pushed as a single undo step
    /// (there is no live-preview phase to separate the two, unlike a
    /// drag gesture).
    pub(crate) fn on_position_committed(&self, axis: i32, text: SharedString) {
        if !self.layout_tools_allowed() {
            self.mirror_position_to_ui();
            return;
        }
        let Ok(axis) = usize::try_from(axis) else {
            self.mirror_position_to_ui();
            return;
        };
        let Ok(value) = text.trim().parse::<f64>() else {
            self.mirror_position_to_ui();
            return;
        };
        let document = &self.state.document;
        let Some(node_id) = document.main_chosen_node() else {
            return;
        };
        let Some(before) = document
            .read_scene(|spatial| spatial.leaf(node_id).map(|q| q.local_transform()))
            .flatten()
        else {
            return;
        };
        let current = document
            .read_scene(|spatial| displayed_position(spatial, node_id))
            .flatten()
            .unwrap_or_else(|| translation_of(&before));
        if axis >= current.len() {
            self.mirror_position_to_ui();
            return;
        }
        let mut target = current;
        target[axis] = value;
        let delta = [
            target[0] - current[0],
            target[1] - current[1],
            target[2] - current[2],
        ];
        let proposed =
            Transform3D::Affine(AffineTransform3D::from_translation(delta)).compose(&before);
        let after = document
            .read_scene(|spatial| {
                let clamped =
                    clamp_move_edit(spatial, node_id, before, proposed, self.printer_volume());
                settle_on_plate(spatial, node_id, clamped)
            })
            .unwrap_or(proposed);
        if document
            .commit_edit(SceneEdit::SetLocalTransform {
                node_id,
                before,
                after,
            })
            .is_ok()
        {
            document.record_edit(SceneEdit::SetLocalTransform {
                node_id,
                before,
                after,
            });
        }
        self.mirror_position_to_ui();
    }

    pub(crate) fn on_object_transform_committed(&self, scale: bool, axis: i32, text: SharedString) {
        if !self.layout_tools_allowed() {
            return;
        }
        let document = &self.state.document;
        let Some(node_id) = document.main_chosen_node() else {
            return;
        };
        let before = document
            .read_scene(|spatial| spatial.leaf(node_id).map(|q| q.local_transform()))
            .flatten();
        if let (Some(before), Ok(value)) = (before, text.parse::<f64>())
            && let Some(after) = crate::object_transform::edit(before, scale, axis as usize, value)
        {
            // Typed rotation/scale also leaves the part resting on the plate.
            let after = document
                .read_scene(|spatial| settle_on_plate(spatial, node_id, after))
                .unwrap_or(after);
            let edit = SceneEdit::SetLocalTransform {
                node_id,
                before,
                after,
            };
            if document.commit_edit(edit.clone()).is_ok() {
                document.record_edit(edit);
            }
        }
        self.mirror_position_to_ui();
    }

    /// "Center on plate": moves the selected object so its world-space
    /// AABB is horizontally centred over the plate origin (world X=0,
    /// Y=0), leaving Z untouched. A pure world-space translation, so
    /// rotation and scale survive exactly, same as a Position field
    /// commit; undoable as a single step.
    /// Scale to fit: the selected model, scaled uniformly to the largest
    /// size the build volume holds, centred on its plate, as one undo step.
    pub(crate) fn scale_to_fit_action(&self) {
        if !self.layout_tools_allowed() || self.plate_locked() {
            return;
        }
        let document = &self.state.document;
        let Some(id) = document.main_chosen_node() else {
            return;
        };
        let volume = self.printer_volume();
        if let Some(Some(edit)) = document
            .read_scene(|spatial| crate::layout_tools::scale_to_fit(spatial.graph(), id, volume))
            && document.commit_edit(edit.clone()).is_ok()
        {
            document.record_edit(edit);
        }
        self.note_unsaved();
    }

    pub(crate) fn on_center_on_plate_requested(&self) {
        self.apply_plate_alignment(|aabb| {
            let min = aabb.min().coords();
            let max = aabb.max().coords();
            let center_x = f64::from(min[0] + max[0]) / 2.0;
            let center_y = f64::from(min[1] + max[1]) / 2.0;
            [-center_x, -center_y, 0.0]
        });
    }

    /// "Drop to plate": moves the selected object so its world-space
    /// AABB's lowest point rests exactly on Z=0, leaving X/Y
    /// untouched. Same commit shape as [`Self::on_center_on_plate_requested`].
    pub(crate) fn on_drop_to_plate_requested(&self) {
        self.apply_plate_alignment(|aabb| [0.0, 0.0, -f64::from(aabb.min().coords()[2])]);
    }

    /// Shared plumbing for the plate-alignment actions: reads the
    /// selected leaf's world-space AABB, asks `delta_for` for the
    /// world-space offset (in the object's parent's frame, which for
    /// an ungrouped object at the scene root is the same as world
    /// space) that satisfies the alignment, then composes and commits
    /// that delta exactly like [`Self::on_position_committed`] does
    /// for a single axis. No-ops without a selection, outside the
    /// Transform section, or for a leaf with no renderable geometry
    /// (an empty mesh has no AABB to align).
    fn apply_plate_alignment(&self, delta_for: impl FnOnce(sc_mesh_core::Aabb) -> [f64; 3]) {
        if !self.layout_tools_allowed() {
            return;
        }
        let document = &self.state.document;
        let Some(node_id) = document.main_chosen_node() else {
            return;
        };
        let Some((before, delta)) = document
            .read_scene(|spatial| {
                let query = spatial.leaf(node_id)?;
                let before = query.local_transform();
                let world = query.parent_world().compose(&before);
                let mut aabb = query.transformed_aabb(world)?;
                // Supports move with their model, so align the model and
                // its supports as one unit (Drop to plate rests the raft,
                // not the model, on the plate).
                for id in crate::scene_document::owned_support_ids(spatial.graph(), node_id) {
                    if let Some(support) = spatial.leaf(id)
                        && let Some(b) = support.transformed_aabb(support.world_transform())
                    {
                        aabb.union(&b);
                    }
                }
                Some((before, delta_for(aabb)))
            })
            .flatten()
        else {
            return;
        };
        let proposed =
            Transform3D::Affine(AffineTransform3D::from_translation(delta)).compose(&before);
        let after = document
            .read_scene(|spatial| {
                clamp_move_edit(spatial, node_id, before, proposed, self.printer_volume())
            })
            .unwrap_or(proposed);
        if document
            .commit_edit(SceneEdit::SetLocalTransform {
                node_id,
                before,
                after,
            })
            .is_ok()
        {
            document.record_edit(SceneEdit::SetLocalTransform {
                node_id,
                before,
                after,
            });
        }
        self.mirror_position_to_ui();
    }

    /// Slint has a device for this window. A new token marks this setup:
    /// the loader (for worker-built scenes) and the renderer both carry
    /// it, so a scene built before a later teardown and setup is refused
    /// by `Renderer::install_scene`.
    pub(crate) fn graphics_ready(&self, device: wgpu::Device, queue: wgpu::Queue) {
        let token = crate::mesh_render::DeviceStamp::issue();
        let loss = self.state.loader.lost_device_state();
        if loss.is_lost() {
            self.state.loader.record_device_loss();
            return;
        }
        // The renderer comes before the device is published: a refused
        // renderer leaves no device to load into.
        let started = self
            .renderer
            .gpu_arrived(device.clone(), token, queue.clone(), loss);
        if let Err(err) = started {
            return self.renderer_refused(err);
        }
        self.state.loader.install_graphics_device(device, token, queue);
        self.renderer_ready();
    }

    /// The window renders through a graphics API this build cannot draw
    /// with (it draws with wgpu), so it never gets a device; recorded so
    /// whatever needs one can say why.
    pub(crate) fn graphics_unsupported(&self, api: String) {
        self.state.loader.record_unsupported_graphics_api(api);
    }

    /// The renderer could not be built: that becomes the window's reason
    /// for having no device, shown once, and a load waiting for this
    /// setup ends with it.
    pub(super) fn renderer_refused(&self, err: StartRefusal) {
        let (message, detail) = (err.to_string(), err.source.to_string());
        self.state.loader.record_renderer_refusal(message.clone());
        let heading = ctr!("ErrorDialog", "The 3D view could not start").to_string();
        let consequence = ctr!("ErrorDialog", "Files cannot be opened in this window.").to_string();
        self.state
            .messages
            .show_failure_later(heading, Some(consequence), message, detail);
        self.start_parked_load();
    }

    /// The renderer is ready: earlier failures are forgotten, a loaded
    /// scene is owed an upload, a frame is asked for (the plate shows even
    /// without a scene), and a file waiting for a device is loaded.
    pub(super) fn renderer_ready(&self) {
        // A fresh renderer starts with a clean failure record and, when a
        // scene is already open, owes it a first upload.
        let loader = &self.state.loader;
        loader.clear_upload_failure_gate();
        loader.clear_render_failure_gate();
        let scene_open = self.state.document.has_scene();
        if scene_open {
            self.renderer.owe(Owed::UPLOAD);
        }
        self.ask_redraw();
        self.start_parked_load();
    }

    /// Loads the file that waited for a device (one given at startup, for
    /// instance). If a load is somehow running already (a teardown and
    /// setup racing Open), the waiting file is dropped, as a drop onto a
    /// busy window is.
    fn start_parked_load(&self) {
        if let Some(waiting) = self.state.waiting_file.take()
            && let Some(pass) = self.state.loader.request_load_ticket()
        {
            begin_ticketed_load(
                pass,
                waiting,
                &self.state.loader,
                self.state.place.volume(),
            );
        }
    }

    /// The device is going: GPU state goes with it, while the scene and
    /// the camera stay, so the next setup uploads again (it owes the
    /// upload itself) with the view unchanged. The loader forgets the
    /// device too, so Open meanwhile is refused instead of building
    /// buffers for a device nobody owns.
    pub(crate) fn graphics_gone(&self) {
        // The device goes away: neither the loader nor the renderer may
        // keep using it.
        let loader = &self.state.loader;
        loader.remove_graphics_device();
        self.renderer.gpu_departed();
    }

    /// Before painting: steps the arrow keys (which move parts), uploads
    /// the scene if owed, and frames the camera the first time there is a
    /// renderer. `false` skips the paint: no renderer, or a lost device.
    /// A failed upload still paints, without the scene.
    fn scene_changed(&self) -> bool {
        // First, so the paint sees this frame's moves.
        self.pointer.apply_viewport_keyboard_arrows(
            &self.state.document,
            self.orbit.view_cell(),
            self.renderer.view_extent(),
            self.state.place.volume(),
        );
        let mut held = self.renderer.renderer_slot().borrow_mut();
        let lost = self.state.loader.lost_device_state().is_lost();
        let renderer = match held.as_mut() {
            Some(renderer) if !lost => renderer,
            Some(_) => {
                // Nothing owed and no dialog: a lost device is told once.
                self.renderer.settle(Owed::UPLOAD);
                self.renderer.settle(Owed::REDRAW);
                return false;
            }
            None => return false,
        };
        if self.renderer.owes(Owed::UPLOAD) {
            self.upload_scene(renderer.as_mut());
        }
        // A camera kept across a teardown and setup keeps the user's view.
        if !self.orbit.has_view() {
            let shown = self.project_view_bounds(renderer.shown_bounds());
            self.orbit.frame(self.project_view_bounds(shown), self.state.place.volume());
        }
        true // paint
    }

    /// Uploads the scene. On failure the renderer's old scene no longer
    /// matches, so it is dropped and the frame shows the plate, keeping
    /// the window responsive. The upload is then no longer owed: retrying
    /// every frame would rebuild GPU memory for as long as the failure
    /// lasts, and everything that changes the scene owes a new upload
    /// itself (renderer install, the install fallback, each edit).
    fn upload_scene(&self, renderer: &mut dyn crate::mesh_render::MeshRenderer) {
        match self
            .state
            .document
            .read_scene(|spatial| renderer.prepare_scene(spatial.graph()))
        {
            Some(Ok(())) => {
                self.renderer.uploaded();
                self.state.loader.record_upload_success();
            }
            Some(Err(err)) => {
                renderer.drop_scene();
                self.renderer.settle(Owed::UPLOAD);
                self.state.loader.record_upload_failure(&err);
            }
            // The scene was closed before the upload came round.
            None => self.renderer.settle(Owed::UPLOAD),
        }
    }

    /// Adds drain holes at the point the risk row `row` names (its layer
    /// and pixel) as one undoable edit, with the diameter the panel holds
    /// (3 mm when it does not parse).
    pub(crate) fn risk_auto_drain(&self, row: &crate::RiskRow) {
        let Some(ui) = self.window.upgrade() else {
            return;
        };
        let document = &self.state.document;
        let Some((stack, _profile)) = document.slice_export_data() else {
            return;
        };
        let Some(layer) = stack.layers().get(row.layer.max(0) as usize) else {
            return;
        };
        let point = [f64::from(row.px), f64::from(row.py), layer.z_mm()];
        let diameter = ui.get_drain_diameter_mm().parse::<f64>().unwrap_or(3.0);
        let result = document.read_scene(|spatial| {
            crate::hollow_tools::drain_risk(spatial.graph(), point, diameter)
        });
        match result {
            Some(Ok(edit)) => {
                if document.commit_edit(edit.clone()).is_ok() {
                    document.record_edit(edit);
                }
            }
            Some(Err(error)) => self.state.messages.show_failure(
                &crate::i18n::tr!("Sleptic Slicer drain holes"),
                None,
                crate::i18n::tr!("The drain holes could not be added.").to_string(),
                error,
            ),
            None => {}
        }
    }

    pub(crate) fn risk_selection_changed(&self) {
        self.layer_image_key.set(None);
        self.layer_changed();
    }

    /// Keeps the slice-view panel's image on the layer shown. `cap_key` is
    /// set exactly while the 3D cut is on (Slice section, slices held, a
    /// layer with a top). A new key makes the cross-section again, no key
    /// blanks it, the same key does nothing. It follows the layer whenever
    /// slices are held, so opening the panel shows it at once.
    fn refresh_layer_image(&self, window: &MainWindow, cut_key: Option<u64>, layer_no: u32, volume: &BuildVolume) {
        if self.layer_image_key.get() == cut_key {
            return;
        }
        let document = &self.state.document;
        let (accurate, fine) = document.slice_accuracy();
        let cap_of = |index: u32| {
            let draw = |layer: &_| crate::slice_cap::build_cap_accurate(layer, volume, accurate, fine, index as usize);
            document.read_layer(index, draw).unwrap_or(None)
        };
        let lo = volume.envelope_min_mm();
        let hi = volume.envelope_max_mm();
        let (plate_w, plate_h) = (f64::from(hi.x - lo.x), f64::from(hi.y - lo.y));
        // The layer itself is drawn as a vector Path (crisp at any zoom);
        // the image only carries the red island overlay on top of it.
        let (path, overlay) = match cut_key {
            Some(_) => {
                let path = document
                    .read_layer(layer_no, |l| {
                        crate::slice_cap::layer_path_commands(l, plate_w, plate_h)
                    })
                    .unwrap_or_default();
                let overlay = cap_of(layer_no)
                    .map(|raster| {
                        // Islands are found at the printer's resolution and
                        // scaled onto the display cap.
                        let islands = document
                            .slice_island_spans(layer_no)
                            .map(|(spans, w, h)| {
                                crate::slice_cap::scale_spans(
                                    &spans,
                                    (w, h),
                                    (raster.width, raster.height),
                                )
                            })
                            .unwrap_or_default();
                        use slint::Model;
                        let selected = usize::try_from(window.get_selected_risk())
                            .ok()
                            .and_then(|i| window.get_print_risks().row_data(i));
                        if let Some(risk) =
                            selected.filter(|r| layer_no as i32 >= r.layer && layer_no as i32 <= r.last)
                        {
                            let spans = if risk.kind == sc_print_job::RiskKind::ThinWall as i32 {
                                document
                                    .slice_thin_spans(
                                        layer_no,
                                        window.get_minimum_feature_mm().parse().unwrap_or(0.3),
                                    )
                                    .map(|(spans, w, h)| {
                                        crate::slice_cap::scale_spans(
                                            &spans,
                                            (w, h),
                                            (raster.width, raster.height),
                                        )
                                    })
                                    .unwrap_or_default()
                            } else {
                                islands.clone()
                            };
                            slint::Image::from_rgba8(crate::slice_cap::risk_overlay_rgba(
                                &raster, &risk, &spans,
                            ))
                        } else {
                            slint::Image::from_rgba8(crate::slice_cap::island_overlay_rgba(
                                &raster, &islands,
                            ))
                        }
                    })
                    .unwrap_or_default();
                (path, overlay)
            }
            None => (String::new(), slint::Image::default()),
        };
        window.set_preview_plate_aspect((hi.x - lo.x) / (hi.y - lo.y));
        window.set_slice_view_path(path.into());
        window.set_slice_view_plate_w(plate_w as f32);
        window.set_slice_view_plate_h(plate_h as f32);
        window.set_layer_image(overlay);
        self.layer_image_key.set(cut_key);
    }

    /// World-space bounds of the active plate's leaves (models and
    /// supports), including the plate's viewport offset.
    fn active_plate_model_bounds(&self) -> Option<(glam::Vec3, glam::Vec3)> {
        let doc = self.scene_doc();
        let active = doc.active_plate();
        doc.read_scene(|spatial| {
            let mut bounds: Option<(glam::Vec3, glam::Vec3)> = None;
            for id in spatial.leaf_ids() {
                if spatial.graph().plate_of(id) != active {
                    continue;
                }
                let Some(query) = spatial.leaf(id) else {
                    continue;
                };
                let Some(aabb) = query.transformed_aabb(query.world_transform()) else {
                    continue;
                };
                let [ox, oy] = spatial.graph().plate_offset(
                    active,
                    doc.plate_width.get(),
                    doc.plate_depth.get(),
                );
                let offset = glam::Vec3::new(ox, oy, 0.0);
                let min = glam::Vec3::from(aabb.min().coords()) + offset;
                let max = glam::Vec3::from(aabb.max().coords()) + offset;
                bounds = Some(match bounds {
                    Some((a, b)) => (a.min(min), b.max(max)),
                    None => (min, max),
                });
            }
            bounds
        })
        .flatten()
    }

    /// Draws a frame, when one is owed and there is a renderer, a window
    /// and a viewport of at least a pixel.
    fn paint_frame(&self) {
        // A frame is drawn only when one is owed and there is a renderer,
        // an open window and a viewport of at least one pixel.
        if !self.renderer.owes(Owed::REDRAW) {
            return;
        }
        let mut slot = self.renderer.renderer_slot().borrow_mut();
        let (Some(renderer), Some(ui)) = (slot.as_mut(), self.window.upgrade()) else {
            return;
        };
        let Some((vp, width, height)) = self.viewport_pixels(&ui) else {
            return;
        };
        let view_height = vp.height();
        let (eff_w, eff_h) = self.renderer.rounded_size(width, height);

        // Entering the Preview 3D view zooms onto the active plate's models:
        // a small part in a large build volume was a speck in the cut view.
        let preview_3d = ui.get_preview_3d() && self.state.place.section() == WorkspaceSection::Slice;
        if preview_3d
            && !self.preview_3d_shown.get()
            && let Some((min, max)) = self.active_plate_model_bounds()
            && let Some(camera) = self.orbit.view_cell().borrow_mut().as_mut()
        {
            camera.zoom_to_box(min, max);
        }
        self.preview_3d_shown.set(preview_3d);

        let camera_slot = self.orbit.view_cell().borrow();
        let Some(camera) = &*camera_slot else {
            return;
        };

        let build_volume = self.state.place.volume();
        let lo = build_volume.envelope_min_mm();
        let hi = build_volume.envelope_max_mm();
        // An empty window still shows plate 1 with its tiles, so build the
        // glyphs from a default one-plate scene when nothing is loaded.
        let plate_size = (build_volume.width_mm(), build_volume.depth_mm());
        // The Slice section shows only the sliced (active) plate, so
        // the other plates' tiles have no plate under them.
        let sliced_only = self.state.place.section() == WorkspaceSection::Slice;
        let build_overlays =
            |graph: &sc_scene::Scene, spatial: Option<&sc_mesh_spatial::SceneSpatial>| {
                graph
                    .plates()
                    .order
                    .iter()
                    .enumerate()
                    // Preview hides the tiles (R110: their targets stayed live
                    // there and the plate buttons misbehaved in the 3D slice view).
                    .filter(|_| !sliced_only)
                    .flat_map(|(i, plate)| {
                        // Glyphs and click targets sit on the projections of
                        // the plate's own tool tiles (drawn in the 3D plate
                        // mesh), so they move, scale and tilt with the plate.
                        let [ox, oy] = graph.plate_offset(
                            plate.id,
                            build_volume.width_mm(),
                            build_volume.depth_mm(),
                        );
                        build_volume
                            .plate_tile_rects()
                            .into_iter()
                            .enumerate()
                            .filter_map(move |(slot, [x0, y0, x1, y1])| {
                                let corners =
                                    [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(x, y)| {
                                        picking::screen_point_of(
                                            camera,
                                            crate::mesh_render::starting_fov_y(),
                                            vp.width(),
                                            view_height,
                                            glam::Vec3::new(x + ox, y + oy, 0.0),
                                        )
                                    });
                                let mut lo = (f32::MAX, f32::MAX);
                                let mut hi = (f32::MIN, f32::MIN);
                                for corner in corners {
                                    let (x, y) = corner?;
                                    lo = (lo.0.min(x), lo.1.min(y));
                                    hi = (hi.0.max(x), hi.1.max(y));
                                }
                                let quad = corners.map(|c| {
                                    let (x, y) = c.unwrap_or_default();
                                    glam::Vec2::new(x, y)
                                });
                                let (w, h) = (hi.0 - lo.0, hi.1 - lo.1);
                                // The icons are drawn in the plate mesh (R110); this
                                // is only the click target, kept while any of the
                                // tile is on the canvas, however small it looks.
                                if !(hi.0 > 0.0
                                    && hi.1 > 0.0
                                    && lo.0 < vp.width()
                                    && lo.1 < view_height)
                                {
                                    return None;
                                }
                                // The glyph is drawn over the 3D image, so a model
                                // standing between the camera and the tile would not
                                // hide it: hide (and stop hit-testing) a tile whose
                                // centre is behind a model.
                                let centre_world = glam::Vec3::new(
                                    0.5 * (x0 + x1) + ox,
                                    0.5 * (y0 + y1) + oy,
                                    0.0,
                                );
                                let to_tile = centre_world - camera.eye_position();
                                let ray = picking::PickRay {
                                    start: camera.eye_position(),
                                    heading: to_tile.normalize_or_zero(),
                                };
                                if spatial.is_some_and(|s| {
                                    picking::first_part_hit(&ray, s, Some(plate_size))
                                        .is_some_and(|(_, t)| t < to_tile.length() - 0.5)
                                }) {
                                    return None;
                                }
                                // The glyph is printed on the tile (plate plane,
                                // read upright from the front: +Y is up), not drawn
                                // upright at its screen box: tile-local path data.
                                let on_tile = |p: glam::Vec2| {
                                    picking::screen_point_of(
                                        camera,
                                        crate::mesh_render::starting_fov_y(),
                                        vp.width(),
                                        view_height,
                                        glam::Vec3::new(p.x + ox, p.y + oy, 0.0),
                                    )
                                    .map(|(x, y)| glam::Vec2::new(x - lo.0, y - lo.1))
                                    .unwrap_or_default()
                                };
                                let size = (x1 - x0).min(y1 - y0);
                                let centre = glam::Vec2::new(0.5 * (x0 + x1), 0.5 * (y0 + y1));
                                let glyph_path = if slot == 0 {
                                    let text = crate::surface_text::outline(&(i + 1).to_string());
                                    let s = (size * 0.55).min(size * 0.8 / text.width.max(0.01));
                                    crate::surface_text::path_commands(&text, |p| {
                                        on_tile(centre + p * s)
                                    })
                                } else {
                                    let g = size * 0.72;
                                    crate::surface_text::path_commands(
                                        crate::surface_text::plate_icon(slot),
                                        |p| {
                                            on_tile(
                                                centre
                                                    + glam::Vec2::new(
                                                        p.x / 24.0 - 0.5,
                                                        0.5 - p.y / 24.0,
                                                    ) * g,
                                            )
                                        },
                                    )
                                };
                                let [a, b, c, d] = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
                                    .map(|(x, y)| on_tile(glam::Vec2::new(x, y)));
                                let tile_path = format!(
                                    "M{} {} L{} {} L{} {} L{} {} Z",
                                    a.x, a.y, b.x, b.y, c.x, c.y, d.x, d.y
                                );
                                Some((
                                    crate::PlateOverlay {
                                        id: plate.id as i32,
                                        index: i as i32 + 1,
                                        slot: slot as i32,
                                        x: lo.0,
                                        y: lo.1,
                                        w,
                                        h,
                                        locked: plate.locked,
                                        glyph_path: glyph_path.into(),
                                        tile_path: tile_path.into(),
                                    },
                                    quad,
                                ))
                            })
                    })
                    .collect::<Vec<_>>()
            };
        let (overlays, hits): (Vec<_>, Vec<_>) = self
            .scene_doc()
            .read_scene(|s| build_overlays(s.graph(), Some(s)))
            .unwrap_or_else(|| build_overlays(&sc_scene::Scene::new(), None))
            .into_iter()
            .unzip();
        *self.plate_tile_hits.borrow_mut() = hits;
        ui.set_plate_overlays(Rc::new(slint::VecModel::from(overlays)).into());
        let (faces_path, hover_path, face_count) = if ui.get_place_face_tool_active() {
            self.refresh_place_faces();
            let cache = self.place_faces.borrow();
            let hovered = self.place_hover.get();
            let mut all = String::new();
            let mut hover = String::new();
            let mut count = -1;
            if let Some(p) = cache.as_ref() {
                count = p.faces.len() as i32;
                let eye = camera.eye_position().as_dvec3();
                for (i, face) in p.faces.iter().enumerate() {
                    let shown: Vec<glam::DVec3> = face
                        .outline
                        .iter()
                        .map(|q| {
                            *q + glam::DVec3::new(
                                f64::from(p.offset[0]),
                                f64::from(p.offset[1]),
                                0.0,
                            )
                        })
                        .collect();
                    // Only sides facing the camera: on a convex hull those
                    // are exactly the visible ones, never hidden by the model.
                    if face.normal.dot(shown[0] - eye) >= 0.0 {
                        continue;
                    }
                    let Some(screen) = shown
                        .iter()
                        .map(|q| {
                            picking::screen_point_of(
                                camera,
                                crate::mesh_render::starting_fov_y(),
                                vp.width(),
                                view_height,
                                q.as_vec3(),
                            )
                        })
                        .collect::<Option<Vec<_>>>()
                    else {
                        continue;
                    };
                    let mut path = String::new();
                    for (k, (x, y)) in screen.iter().enumerate() {
                        path.push_str(&format!("{}{x:.1} {y:.1} ", if k == 0 { "M" } else { "L" }));
                    }
                    path.push_str("Z ");
                    if hovered == Some(i) {
                        hover = path;
                    } else {
                        all.push_str(&path);
                    }
                }
            }
            (all, hover, count)
        } else {
            (String::new(), String::new(), -1)
        };
        if ui.get_place_face_path().as_str() != faces_path {
            ui.set_place_face_path(faces_path.into());
        }
        if ui.get_place_face_hover_path().as_str() != hover_path {
            ui.set_place_face_hover_path(hover_path.into());
        }
        ui.set_place_face_count(face_count);
        let corner = [
            glam::Vec3::new(lo.x, lo.y, 0.0),
            glam::Vec3::new(lo.x, hi.y, 0.0),
            glam::Vec3::new(hi.x, lo.y, 0.0),
            glam::Vec3::new(hi.x, hi.y, 0.0),
        ]
        .into_iter()
        .filter_map(|p| {
            picking::screen_point_of(
                camera,
                crate::mesh_render::starting_fov_y(),
                vp.width(),
                view_height,
                p,
            )
        })
        .max_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((x, y)) = corner {
            ui.set_plate_toolbar_x(x + 16.0);
            ui.set_plate_toolbar_y(y - 64.0);
        }
        if ui.get_nav_yaw() != camera.azimuth || ui.get_nav_pitch() != camera.elevation {
            let labels = [
                crate::i18n::tr!("Top"),
                crate::i18n::tr!("Bottom"),
                crate::i18n::tr!("Front"),
                crate::i18n::tr!("Back"),
                crate::i18n::tr!("Left"),
                crate::i18n::tr!("Right"),
            ];
            let faces = super::view_steps::faces(camera)
                .into_iter()
                .map(|f| {
                    let [a, b, c, d] = f.points;
                    let center = (a + b + c + d) * 0.25;
                    // Uppercase like the Fusion / Inventor view cube.
                    let label: SharedString = labels[f.id as usize].to_uppercase().into();
                    let label_path = super::view_steps::face_label_path(
                        camera,
                        f.id,
                        &crate::surface_text::outline(&label),
                    );
                    crate::CubeFace {
                        id: f.id,
                        facing: f.facing,
                        label,
                        label_path: label_path.into(),
                        x0: a.x,
                        y0: a.y,
                        x1: b.x,
                        y1: b.y,
                        x2: c.x,
                        y2: c.y,
                        x3: d.x,
                        y3: d.y,
                        cx: center.x,
                        cy: center.y,
                    }
                })
                .collect::<Vec<_>>();
            ui.set_cube_faces(slint::ModelRc::new(slint::VecModel::from(faces)));
            let round = super::view_steps::rounding(camera)
                .into_iter()
                .map(|piece| {
                    let mut path = String::new();
                    for (k, p) in piece.points.iter().enumerate() {
                        path.push_str(&format!(
                            "{}{:.2} {:.2} ",
                            if k == 0 { "M" } else { "L" },
                            p.x,
                            p.y
                        ));
                    }
                    path.push('Z');
                    crate::CubeRound {
                        path: path.into(),
                        facing: piece.facing,
                    }
                })
                .collect::<Vec<_>>();
            ui.set_cube_round(slint::ModelRc::new(slint::VecModel::from(round)));
            let axes = super::view_steps::axes(camera)
                .into_iter()
                .zip(["X", "Y", "Z"])
                .map(|(a, label)| crate::NavAxis {
                    label: label.into(),
                    x0: a.from.x,
                    y0: a.from.y,
                    x: a.to.x,
                    y: a.to.y,
                    visible: a.visible,
                })
                .collect::<Vec<_>>();
            ui.set_nav_axes(slint::ModelRc::new(slint::VecModel::from(axes)));
            ui.set_nav_yaw(camera.azimuth);
            ui.set_nav_pitch(camera.elevation);
        }

        let (clip_z, selected_layer) = self.update_slice_cut(renderer.as_mut(), &ui, &build_volume);

        // A size the GPU already refused is not tried again every frame.
        if self.renderer.size_was_refused((eff_w, eff_h)) {
            return self.renderer.settle(Owed::REDRAW);
        }

        // With a scene open the gizmo frame is composed against it and the
        // plate and parts are drawn together; with none, `with_scene`
        // yields nothing and the plate alone is drawn, without a gizmo.
        let only_active_plate = self.state.place.section() == WorkspaceSection::Slice;
        let layer_band = clip_z.map_or(0.0, |top| {
            layer_band_thickness(top, |i| self.state.document.layer_top_mm(i), selected_layer)
        });
        // Preview shows only the plate that was sliced: models on other
        // plates would otherwise appear cut at a layer height that has
        // nothing to do with them.
        let mut hidden_objects = self.hidden_objects.borrow().clone();
        if only_active_plate {
            let doc = self.scene_doc();
            let active = doc.active_plate();
            if let Some(others) = doc.read_scene(|spatial| {
                spatial
                    .leaf_ids()
                    .into_iter()
                    .enumerate()
                    .filter(|&(_, id)| spatial.graph().plate_of(id) != active)
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>()
            }) {
                hidden_objects.extend(others);
            }
        }
        let active_plate = self.scene_doc().active_plate();
        if let Some(look) = renderer.look() {
            look.active_plate = active_plate;
            look.only_active_plate = only_active_plate;
            look.layer_band = layer_band;
            look.hidden_objects = hidden_objects;
            // The colours follow the window's theme and resin; without a
            // window they stay as they were.
            if let Some(ui) = self.window.upgrade() {
                look.light_scene = ui.get_theme() == crate::ThemePreference::Light;
                look.cure_shade = ui.get_layer_cure_shade();
                let resin = ui.get_resin_color();
                look.model_color = crate::mesh_render::model_shade([resin.red(), resin.green(), resin.blue()]);
                let warning = ui.get_support_warning_color();
                look.support_warning_rgb = [warning.red(), warning.green(), warning.blue()].map(u32::from);
            }
        }
        let texture = {
            self.state
                .document
                .read_scene(|spatial_for_render| {
                    let selection_node_id = self.state.document.main_chosen_node();
                    // The selection highlight stays visible in every section
                    // (selection is live everywhere). The gizmo is a
                    // transform tool, so its handles only compose in the
                    // Transform section: gate the selection handed to the
                    // composer, not the one that drives the object tint.
                    let gizmo_selection = if self.layout_tools_allowed()
                        && ui.get_active_tool() >= 0
                        && !ui.get_measure_tool_active()
                        && !ui.get_model_tools_active()
                        && !ui.get_place_face_tool_active()
                        && !ui.get_supports_tool_active()
                    {
                        selection_node_id
                    } else {
                        None
                    };
                    self.sync_gizmo_mode();
                    let mut gizmo_frame = HandleFramer::frame_for(
                        &self.pointer,
                        spatial_for_render,
                        camera,
                        gizmo_selection,
                        view_height,
                    );

                    if let (Some(frame), Some(id)) = (&mut gizmo_frame, gizmo_selection) {
                        let [x, y] = self.scene_doc().plate_offset(id);
                        frame.anchor.x += x;
                        frame.anchor.y += y;
                    }
                    // Rotate drag: the protractor of 15 degree ticks the
                    // angle sticks to, and the turned angle beside it.
                    let protractor = match (self.pointer.viewport_gizmo_machine().drag_snapshot(), &gizmo_frame) {
                        (
                            HandleDragState::HandleHeld {
                                op:
                                    DragKind::Rotate {
                                        axis,
                                        start_angle,
                                        current_unwrapped,
                                        ..
                                    },
                                ..
                            },
                            Some(frame),
                        ) => rotate_protractor(
                            camera,
                            (vp.width(), view_height),
                            frame.anchor,
                            frame.arm_length * SIZES.ring_radius,
                            axis,
                            start_angle,
                            current_unwrapped - start_angle,
                        ),
                        _ => Protractor::default(),
                    };
                    if ui.get_rotate_ticks_path().as_str() != protractor.ticks {
                        ui.set_rotate_ticks_path(protractor.ticks.into());
                        ui.set_rotate_tick_hot_path(protractor.hot.into());
                        ui.set_rotate_readout(protractor.label.into());
                        ui.set_rotate_readout_x(protractor.label_at.0);
                        ui.set_rotate_readout_y(protractor.label_at.1);
                    } else if ui.get_rotate_readout().as_str() != protractor.label {
                        ui.set_rotate_tick_hot_path(protractor.hot.into());
                        ui.set_rotate_readout(protractor.label.into());
                        ui.set_rotate_readout_x(protractor.label_at.0);
                        ui.set_rotate_readout_y(protractor.label_at.1);
                    }
                    // Preview draws the models in their plain material
                    // colour so the blue cut face (the current layer) is
                    // the only highlighted surface.
                    let in_preview = self.state.place.section() == WorkspaceSection::Slice;
                    let selected_ids = if in_preview {
                        Vec::new()
                    } else {
                        self.state.document.chosen().ids()
                    };
                    renderer.draw_frame(
                        Some(SceneRender {
                            world_bounds: crate::mesh_render::display_bounds(
                                spatial_for_render,
                                build_volume,
                            ),
                            selected_ids: &selected_ids,
                            scene: spatial_for_render.graph(),
                            outlined: selection_node_id.filter(|_| !in_preview),
                            handles: gizmo_frame,
                            cut_height: clip_z,
                        }),
                        build_volume,
                        camera,
                        eff_w,
                        eff_h,
                    )
                })
                .unwrap_or_else(|| renderer.draw_frame(None, build_volume, camera, eff_w, eff_h))
        };

        self.show_frame(&ui, texture, (eff_w, eff_h));
    }

    /// Hands a rendered frame to the window. A lost device draws nothing
    /// and owes nothing; a refused frame marks its size as refused and is
    /// counted as a render failure.
    pub(super) fn show_frame(
        &self,
        ui: &MainWindow,
        rendered: Result<Option<wgpu::Texture>, FrameRefusal>,
        size: (u32, u32),
    ) {
        let texture = match rendered {
            Ok(Some(drawn)) => drawn,
            Ok(None) => return, // nothing new to show this time
            // Told once elsewhere; this frame is simply settled.
            Err(FrameRefusal::Lost) => {
                return self.renderer.settle(Owed::REDRAW);
            }
            Err(failure) => {
                self.renderer.record_refused_size(size);
                return self.state.loader.record_render_failure(&failure);
            }
        };
        self.renderer.record_drawn();
        self.state.loader.record_render_success();
        self.renderer.settle(Owed::REDRAW);
        let image = match slint::Image::try_from(texture) {
            Ok(image) => image,
            Err(failure) => return self.texture_refused_by_ui(failure),
        };
        ui.set_view_ready(image);
        if self.state.document.has_scene() {
            crate::mesh_loader::time_first_frame();
        }
    }

    /// The window would not take the rendered texture: said in a dialog.
    fn texture_refused_by_ui(&self, failure: impl std::fmt::Display + std::fmt::Debug) {
        let heading = ctr!("ErrorDialog", "Failed to display the 3D model").to_string();
        let consequence =
            ctr!("ErrorDialog", "The rendered texture could not be handed to the UI.").to_string();
        let (message, detail) = (failure.to_string(), format!("{failure:?}"));
        self.state
            .messages
            .show_failure_later(heading, Some(consequence), message, detail);
    }

    /// This frame's slice cut: in Slice, with a slice held, the selected
    /// layer's top clips the parts and the layer fills the cut face so it
    /// reads solid; the slice view panel shows the same cut. Returns the
    /// clip height and the layer.
    fn update_slice_cut(
        &self,
        renderer: &mut dyn crate::mesh_render::MeshRenderer,
        ui: &MainWindow,
        build_volume: &BuildVolume,
    ) -> (Option<f32>, u32) {
        // The slider holds whole layers, as a float.
        let layer = ui.get_layer_shown().round().max(0.0) as u32;
        let document = &self.state.document;
        let clip_z = crate::workspace_section::cut_height_for(
            self.state.place.section(),
            document.layer_top_mm(layer),
        );
        // The fill is kept per slice and layer: an orbit reuses it, a new
        // slice or another layer rebuilds it, no cut clears it.
        let key = clip_z.map(|_| (document.slice_serial_now() << 20) ^ u64::from(layer));
        let mut fill = || {
            let accuracy = document.slice_accuracy().0;
            document
                .read_layer(layer, |cut| {
                    crate::slice_cap::build_cap_distance(
                        cut,
                        build_volume,
                        accuracy,
                        layer as usize,
                    )
                })
                .flatten()
        };
        match renderer.refresh_cut_face(key, &mut fill) {
            Ok(()) | Err(FrameRefusal::Lost) => {}
            Err(err) => self.state.loader.record_render_failure(&err),
        }
        // Rebuilt only when the key changes; every repaint (slider, section,
        // scene, a finished slice) brings it up to date.
        self.refresh_layer_image(ui, key, layer, build_volume);
        (clip_z, layer)
    }

    /// The viewport and its size in physical pixels; `None` without a
    /// viewport or when either side rounds to nothing.
    fn viewport_pixels(&self, ui: &MainWindow) -> Option<(super::screen::ViewportSize, u32, u32)> {
        let viewport = self.renderer.view_extent()?;
        let scale = ui.window().scale_factor();
        let pixels = |logical: f32| (logical * scale).round() as u32;
        let (width, height) = (pixels(viewport.width()), pixels(viewport.height()));
        (width > 0 && height > 0).then_some((viewport, width, height))
    }

    pub(crate) fn before_frame(&self) {
        // Pick up a new view size before painting (R116).
        if let Some(ui) = self.window.upgrade() {
            let (w, h) = (ui.get_viewer_width(), ui.get_viewer_height());
            if w > 0.0 && h > 0.0 {
                self.renderer.take_view_size(w, h);
            }
        }
        self.update_measure_markers();
        self.update_cut_plane();
        if self.scene_changed() {
            self.paint_frame();
            self.count_benchmark_frame();
        }
    }

    /// The preview benchmark (automation only): counts the first 120
    /// frames and keeps owing more until it has them.
    fn count_benchmark_frame(&self) {
        if let Some(ui) = self.window.upgrade()
            && ui.get_benchmark_preview()
            && ui.get_benchmark_frames() < 120
        {
            let start = self.benchmark_start.get().unwrap_or_else(|| {
                let now = std::time::Instant::now();
                self.benchmark_start.set(Some(now));
                now
            });
            let frames = ui.get_benchmark_frames() + 1;
            ui.set_benchmark_frames(frames);
            if frames > 1 {
                ui.set_benchmark_fps((frames - 1) as f32 / start.elapsed().as_secs_f32());
            }
            self.renderer.flag_dirty();
        }
    }
}

/// Thickness of the previewed layer `index` whose top is at `top`: down to
/// the previous layer's top, or to the plate for the first layer.
fn layer_band_thickness(top: f32, top_of: impl Fn(u32) -> Option<f64>, index: u32) -> f32 {
    let below = index
        .checked_sub(1)
        .and_then(top_of)
        .map_or(0.0, |z| z as f32);
    (top - below).max(0.0)
}

/// Screen-space drawing of the rotate tool's ticks (SVG path data in
/// viewport px), the tick or angle the part is turned to, and its label.
#[derive(Default)]
struct Protractor {
    ticks: String,
    hot: String,
    label: String,
    label_at: (f32, f32),
}

/// Ticks every `ROTATION_STEP_DEG` around the rotate arc, counted from where
/// the drag started (so they mark turns of 15, 30, 45 ... degrees), longer
/// every 45; `turned` is the raw drag angle in radians.
fn rotate_protractor(
    camera: &crate::camera::ViewCamera,
    (w, h): (f32, f32),
    pivot: glam::Vec3,
    radius: f32,
    axis: gizmo::axis::Axis,
    start_angle: f32,
    turned: f32,
) -> Protractor {
    let (u, v) = gizmo::axis::axis_basis(axis);
    let at = |angle: f32, r: f32| {
        picking::screen_point_of(
            camera,
            crate::mesh_render::starting_fov_y(),
            w,
            h,
            pivot + (u * angle.cos() + v * angle.sin()) * (radius * r),
        )
    };
    let mut ticks = String::new();
    let line = |path: &mut String, angle: f32, r0: f32, r1: f32| {
        if let (Some(a), Some(b)) = (at(angle, r0), at(angle, r1)) {
            path.push_str(&format!("M{:.1} {:.1} L{:.1} {:.1} ", a.0, a.1, b.0, b.1));
        }
    };
    let step = super::handle_drag::ROTATION_STEP_DEG;
    let count = (360.0 / step).round() as usize;
    for k in 0..count {
        let major = (k as f32 * step) % 45.0 == 0.0;
        let angle = start_angle + (k as f32 * step).to_radians();
        line(&mut ticks, angle, 1.06, if major { 1.24 } else { 1.14 });
    }
    // The ring the ticks stand on.
    let ring: Vec<_> = (0..=96)
        .filter_map(|i| at(start_angle + i as f32 * std::f32::consts::TAU / 96.0, 1.06))
        .collect();
    for (i, (x, y)) in ring.iter().enumerate() {
        ticks.push_str(&format!("{}{x:.1} {y:.1} ", if i == 0 { "M" } else { "L" }));
    }
    let shown = super::handle_drag::snap_rotation(turned);
    let snapped = shown != turned;
    let mut hot = String::new();
    line(&mut hot, start_angle + shown, 0.92, 1.32);
    let degrees = shown.to_degrees();
    let label = if snapped {
        format!("{degrees:.0}°")
    } else {
        format!("{degrees:.1}°")
    };
    let label_at = at(start_angle + shown, 1.48).unwrap_or((-100.0, -100.0));
    Protractor {
        ticks,
        hot,
        label,
        label_at,
    }
}

/// The window's view-cube face as the camera's own axis view.
impl From<crate::AxisView> for ViewAxis {
    fn from(face: crate::AxisView) -> Self {
        use crate::AxisView as Face;
        match face {
            Face::Top => Self::Top,
            Face::Bottom => Self::Bottom,
            Face::Front => Self::Front,
            Face::Back => Self::Back,
            Face::Left => Self::Left,
            Face::Right => Self::Right,
        }
    }
}

/// Both "scene open" and "scene closed" leave nothing selected and the
/// arrow keys off the gizmo.
fn show_scene_open(ui: &MainWindow, open: bool) {
    ui.set_scene_present(open);
    ui.set_chosen_part_row(-1);
    ui.set_keys_drive_gizmo(false);
}

#[cfg(test)]
#[path = "tests_controller_misc.rs"]
mod tests_controller_misc;

#[cfg(test)]
#[path = "tests_controller_selection.rs"]
mod tests_controller_selection;

#[cfg(test)]
#[path = "tests_controller_position.rs"]
mod tests_controller_position;

#[cfg(test)]
#[path = "tests_controller_alignment.rs"]
mod tests_controller_alignment;

#[cfg(test)]
#[path = "tests_controller_clicks.rs"]
mod tests_controller_clicks;

#[cfg(test)]
#[path = "tests_controller_keyboard.rs"]
mod tests_controller_keyboard;

#[cfg(test)]
#[path = "tests_canvas_ux.rs"]
mod tests_canvas_ux;

#[cfg(test)]
#[path = "tests_controller_redraw.rs"]
mod tests_controller_redraw;

#[cfg(test)]
#[path = "tests_controller_window.rs"]
mod tests_controller_window;

#[cfg(test)]
mod r21_refresh_tests {
    use super::*;
    #[test]
    fn frame_view_uses_live_rotated_mesh_after_edit_and_undo() {
        let ctrl = ViewController::for_test();
        let graph = crate::tests_r21::tilted_octahedron();
        let id = graph.first_leaf_id().unwrap();
        ctrl.put_graph_for_test(graph.clone());
        let volume = BuildVolume::new(2.1, 1.5, 1.5);
        ctrl.set_build_volume(volume);
        ctrl.orbit.frame(None, volume);
        let doc = ctrl.scene_doc();
        let before = graph.node(id).unwrap().local_transform;
        let after = Transform3D::Affine(AffineTransform3D::from_translation([10.0, 0.0, 0.0]))
            .compose(&before);
        let edit = SceneEdit::SetLocalTransform {
            node_id: id,
            before,
            after,
        };
        doc.commit_edit(edit.clone()).unwrap();
        doc.record_edit(edit);
        ctrl.frame_models();
        let expected = doc
            .read_scene(|s| crate::mesh_render::display_bounds(s, volume))
            .flatten();
        let (center, _) = volume.framing_sphere(expected);
        assert_eq!(
            ctrl.orbit.view_cell().borrow().as_ref().unwrap().look_at,
            center
        );
        assert!(center.x > 4.0);
        doc.step_back().unwrap();
        ctrl.frame_models();
        assert!(
            ctrl.orbit
                .view_cell()
                .borrow()
                .as_ref()
                .unwrap()
                .look_at
                .x
                .abs()
                < 1e-6
        );
    }
    #[test]
    fn supported_model_lift_and_undo_refresh_position_without_reselection() {
        let ctrl = ViewController::for_test();
        let mut graph = sc_scene_edit::SceneFragment::from_leaf_snapshot(
            std::sync::Arc::new(sc_mesh_fixtures::unit_cube_mesh()),
            Transform3D::Affine(AffineTransform3D::from_translation([0.0, 0.0, 0.5])),
        )
        .into_scene();
        let model = graph.first_leaf_id().unwrap();
        ctrl.put_graph_for_test(graph.clone());
        let doc = ctrl.scene_doc();
        doc.select_sole_object();
        let changed = Rc::new(std::cell::RefCell::new(Vec::new()));
        let values = changed.clone();
        let weak = Rc::downgrade(&doc);
        let _sub = doc.watch(move |event| {
            if position_refreshes_on(event)
                && let Some(doc) = weak.upgrade()
            {
                values.borrow_mut().push(
                    doc.read_scene(|s| displayed_position(s, model))
                        .flatten()
                        .unwrap()[2],
                );
            }
        });
        graph = sc_print_glue::supports::geometric_supported_scene(&graph, &Default::default())
            .unwrap()
            .0;
        doc.apply_support_scene(&graph, &[model]).unwrap();
        assert_eq!(doc.main_chosen_node(), Some(model));
        doc.step_back().unwrap();
        doc.step_forward().unwrap();
        assert_eq!(*changed.borrow(), vec![5.0, 0.0, 5.0]);
    }
}

/// What Auto prepare tuned and why, in a line (R106).
pub(crate) fn support_choice_text(choice: &sc_print_glue::supports::SupportChoice) -> String {
    let s = &choice.settings;
    let n = crate::slice_panel::short_number;
    if choice.lifted == 0 {
        return crate::i18n::tr!(
            "Auto prepare kept your support settings: the parts print on the plate, which carries them."
        )
        .to_string();
    }
    let raft = if choice.lattice {
        crate::i18n::tr!(
            "lattice raft (solid rafts would cover {}% of the plate)",
            (choice.raft_share * 100.0).round()
        )
        .to_string()
    } else {
        crate::i18n::tr!("solid raft").to_string()
    };
    crate::i18n::tr!(
        "Auto prepare tuned the supports to these parts: {} mm tips, {} mm shafts, {} mm apart, {}.",
        n(s.tip_contact_diameter_mm),
        n(s.shaft_diameter_mm),
        n(s.spacing_mm),
        raft
    )
    .to_string()
}

/// The drain hole panel's settings as a request at `point`; `view` is the
/// direction "Along the view" uses.
fn drain_request(
    ui: &MainWindow,
    point: [f64; 3],
    view: [f64; 3],
) -> crate::hollow_tools::DrainRequest {
    use crate::hollow_tools::DrainAim;
    let mirror = ui.get_drain_mirror();
    crate::hollow_tools::DrainRequest {
        point,
        aim: match ui.get_drain_aim() {
            1 => DrainAim::Along(view),
            2 => DrainAim::Along([0.0, 0.0, -1.0]),
            _ => DrainAim::Surface,
        },
        diameter_mm: ui.get_drain_diameter_mm().parse().unwrap_or(f64::NAN),
        depth_mm: ui
            .get_drain_custom_depth()
            .then(|| ui.get_drain_depth_mm().parse().unwrap_or(f64::NAN)),
        mirror: [mirror == 1 || mirror == 3, mirror == 2 || mirror == 3],
    }
}

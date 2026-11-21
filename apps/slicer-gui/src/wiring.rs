// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Connects a main window's callbacks to its `ViewportController`, and runs
//! the file flows behind them: Open, Save, Save As, Export As, Export
//! sliced file and Export all. Most callbacks only forward to the
//! controller or the document; the flows ask for a path, write on a worker
//! thread so the window keeps responding, and report the result in a
//! dialog.
use std::path::{Path, PathBuf};
use std::rc::Rc;

use futures_channel::oneshot;
use sc_mesh_formats::filetypes::{WriteFormat, infer_mesh_file_format_from_extension};
use sc_print_primitives::{MaterialId, PrinterId};
use slint::{ComponentHandle, Model};

use crate::MainWindow;
use crate::error_dialog::{MessageWindows, error_with_causes};
use crate::export_progress_panel::ExportProgressPanel;
use crate::i18n::{ctr, tr};
use crate::mesh_loader::{self, OpenVerdict};
use crate::preferences_bus::{PreferenceHub, PreferenceEdit};
use crate::save_progress_panel::SaveProgressPanel;
use crate::scene_document::{SaveCheck, SceneDocument};
use crate::scene_export;
use crate::scene_loader::SceneLoadTicket;
use crate::scene_save::{self, SaveAction, SaveAsAnswer};
use crate::viewport::{RESIZE_QUIET, ScreenDelta, ScreenPoint, ViewController};

/// Wires window callbacks to a shared value: for each `on_x |args| body`
/// (or `on_x || body`), `on_x` gets a closure owning its own clone of
/// `source`, named `name` in `body`.
macro_rules! forward {
    ($ui:expr, $source:ident as $name:ident {}) => {};
    ($ui:expr, $source:ident as $name:ident { $on:ident || $body:expr; $($rest:tt)* }) => {
        {
            let $name = Rc::clone(&$source);
            $ui.$on(move || $body);
        }
        forward!($ui, $source as $name { $($rest)* });
    };
    ($ui:expr, $source:ident as $name:ident { $on:ident |$($arg:ident),*| $body:expr; $($rest:tt)* }) => {
        {
            let $name = Rc::clone(&$source);
            $ui.$on(move |$($arg),*| $body);
        }
        forward!($ui, $source as $name { $($rest)* });
    };
}

/// What the Export As dialog's answer allows: nothing, a refusal, or a
/// write in the format the name's extension asks for.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ExportDispatch {
    Cancelled,
    /// No extension, or one we read but do not write (`.ply`, `.amf`).
    UnsupportedExtension(PathBuf),
    Ready {
        target: PathBuf,
        format: WriteFormat,
    },
}

pub(crate) fn decide_export_dispatch(target: Option<PathBuf>) -> ExportDispatch {
    let Some(target) = target else {
        return ExportDispatch::Cancelled;
    };
    let format = infer_mesh_file_format_from_extension(&target)
        .ok()
        .and_then(|f| WriteFormat::try_from(f).ok());
    match format {
        Some(format) => ExportDispatch::Ready { target, format },
        None => ExportDispatch::UnsupportedExtension(target),
    }
}

/// The active plate's place in the plate list, from 1.
fn plate_position(doc: &SceneDocument) -> usize {
    let active = doc.active_plate();
    doc.read_scene(|s| {
        s.graph()
            .plates()
            .order
            .iter()
            .position(|p| p.id == active)
            .unwrap_or(0)
            + 1
    })
    .unwrap_or(1)
}

/// The name an export of the plate at `position` starts from:
/// `<file stem>_plate<position>`, `project` without a file.
fn plate_file_stem(doc: &SceneDocument, position: usize) -> String {
    let stem = scene_export::suggested_export_name(doc).unwrap_or_else(|| "project".into());
    format!("{stem}_plate{position}")
}

/// The auto orient preset at `index` in the preset list.
fn orient_preset(index: i32) -> sc_print_profiles::orient::Preset {
    use sc_print_profiles::orient::Preset;
    match index {
        1 => Preset::FewestSupports,
        2 => Preset::Fastest,
        3 => Preset::BestQuality,
        4 => Preset::SmallestFootprint,
        _ => Preset::default(),
    }
}

/// Wires every callback of a new main window (`AppState::open_main_window`).
/// `prefs` gives the flows the default printer and resin at the time.
pub(crate) fn connect_main_window_actions(
    ui: &MainWindow,
    ctrl: Rc<ViewController>,
    prefs: Rc<PreferenceHub>,
) -> Result<(), slint::SetRenderingNotifierError> {
    crate::hub::wire(ui, ctrl.clone(), prefs.clone());
    wire_plates(ui, &ctrl);
    crate::calibration::wire(ui, ctrl.clone(), prefs.clone());
    wire_view_and_orient(ui, &ctrl, &prefs);
    wire_model_tools(ui, &ctrl);
    {
        let ctrl = ctrl.clone();
        crate::batch_prepare::install(ui, ctrl.scene_doc(), prefs.clone(), move || {
            ctrl.printer_volume()
        });
    }
    forward!(ui, ctrl as ctrl {
        on_close_scene || ctrl.close_scene();
    });
    connect_mesh_open_action(ui, ctrl.clone(), prefs.clone());
    connect_project_save_actions(ui, ctrl.clone(), prefs);
    connect_mesh_export_action(ui, ctrl.clone());
    wire_export_sliced(ui, &ctrl);
    wire_export_all(ui, &ctrl);
    forward!(ui, ctrl as ctrl {
        on_undo || ctrl.undo();
        on_redo || ctrl.redo();
        on_delete_key || ctrl.delete_selected();
        on_section_picked |section| ctrl.switch_section(section);
    });
    wire_layers_and_risks(ui, &ctrl);
    wire_viewport(ui, &ctrl);
    attach_viewport_render_notifications(ui, ctrl)
}

fn wire_plates(ui: &MainWindow, ctrl: &Rc<ViewController>) {
    let doc = ctrl.scene_doc();
    let weak = ui.as_weak();
    ui.on_plate_settings_requested({
        let doc = doc.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_plate_display_name(doc.plate_name().unwrap_or_default().into());
                ui.set_plate_editor_locked(ui.get_plate_locked());
                ui.set_plate_editor_open(true);
            }
        }
    });
    forward!(ui, doc as doc {
        on_plate_renamed |name| doc.rename_plate(&name);
    });
    forward!(ui, ctrl as ctrl {
        on_plate_lock_changed |locked| ctrl.set_plate_locked(locked);
        on_cube_clicked |x, y| ctrl.cube_clicked(x, y);
        on_cube_hovered |x, y| ctrl.cube_hovered(x, y);
        on_navigation_view |index| ctrl.navigation_view(index);
        on_context_action |action| ctrl.context_action(action);
        on_object_visibility_toggled |index| ctrl.toggle_object_visibility(index);
        on_place_face_tool_changed || ctrl.place_face_tool_changed();
        on_arrange_requested || ctrl.layout_action(true);
    });
}

fn wire_view_and_orient(
    ui: &MainWindow,
    ctrl: &Rc<ViewController>,
    prefs: &Rc<PreferenceHub>,
) {
    // Orthographic unless the user chose perspective (View menu, or 5).
    let saved = prefs.current_values();
    ui.set_perspective_view(saved.perspective_view);
    ctrl.set_perspective(saved.perspective_view);
    {
        let (ctrl, prefs) = (ctrl.clone(), prefs.clone());
        ui.on_projection_toggled(move |perspective| {
            ctrl.set_perspective(perspective);
            prefs.edit_preferences(move |s| {
                s.perspective_view = perspective;
                PreferenceEdit::Projection
            });
        });
    }
    ui.set_orient_preset(saved.orient_preset);
    let [support, height, quality, footprint] = orient_preset(saved.orient_preset)
        .weights()
        .map(|w| (w * 25.0) as f32);
    ui.set_orient_support_weight(support);
    ui.set_orient_height_weight(height);
    ui.set_orient_quality_weight(quality);
    ui.set_orient_footprint_weight(footprint);
    forward!(ui, prefs as prefs {
        on_orient_preset_changed |index| prefs.edit_preferences(move |s| {
            s.orient_preset = index;
            PreferenceEdit::OrientPreset
        });
    });
}

fn wire_model_tools(ui: &MainWindow, ctrl: &Rc<ViewController>) {
    let doc = ctrl.scene_doc();
    forward!(ui, ctrl as ctrl {
        on_auto_orient_requested || ctrl.auto_orient_action();
        on_auto_prepare_requested || ctrl.auto_prepare_action();
        on_measure_evidence_requested || ctrl.measure_evidence();
    });
    forward!(ui, doc as doc {
        on_add_plate_requested || doc.add_plate();
        on_plate_selected |id| doc.set_active_plate(id as u32);
    });
    forward!(ui, ctrl as ctrl {
        on_plate_thumbnails_refresh || ctrl.refresh_plate_thumbnails();
    });
    forward!(ui, doc as doc {
        on_move_to_plate_requested |id| doc.move_selection_to_plate(id as u32);
    });
    forward!(ui, ctrl as ctrl {
        on_arrange_all_requested || ctrl.arrange_all_action();
    });
    forward!(ui, doc as doc {
        on_reorder_plate_requested |direction| {
            let active = doc.active_plate();
            doc.edit_plates(|plates| {
                let last = plates.order.len() as i32 - 1;
                if let Some(from) = plates.order.iter().position(|p| p.id == active) {
                    plates.order.swap(from, (from as i32 + direction).clamp(0, last) as usize);
                }
            });
        };
    });
    let weak = ui.as_weak();
    forward!(ui, ctrl as ctrl {
        on_delete_plate_requested || ctrl.delete_plate_action();
        on_plate_action |id, action| {
            ctrl.scene_doc().set_active_plate(id as u32);
            match action {
                2 => ctrl.layout_action(true),
                26 => {
                    if let Some(ui) = weak.upgrade() {
                        ctrl.set_plate_locked(!ui.get_plate_locked());
                    }
                }
                22 => {
                    if let Some(ui) = weak.upgrade() {
                        ui.invoke_plate_settings_requested();
                    }
                }
                _ => ctrl.context_action(action),
            }
        };
        on_model_tool_requested |action| ctrl.model_tool_action(action);
        on_model_tool_evidence |kind| ctrl.model_tool_evidence(kind);
        on_cut_plane_dragged |dy| ctrl.drag_cut_plane(dy);
        on_mirror_requested |axis| ctrl.mirror_action(axis);
        on_fill_plate_requested || ctrl.fill_plate_action();
        on_hollow_requested |auto| ctrl.hollow_requested(auto);
        on_drain_hole_remove |index| {
            ctrl.hollow_action(crate::hollow_tools::HoleChange::Remove(index.max(0) as usize))
        };
        on_drain_holes_clear || ctrl.hollow_action(crate::hollow_tools::HoleChange::Clear);
        on_drain_at_point || ctrl.drain_at_point();
    });
}

fn wire_layers_and_risks(ui: &MainWindow, ctrl: &Rc<ViewController>) {
    let weak = ui.as_weak();
    forward!(ui, ctrl as ctrl {
        on_risk_jump_requested |index| {
            let Some((ui, row)) = risk_row(&weak, index) else {
                return;
            };
            ui.set_selected_risk(index);
            ui.set_expanded_risk_kind(row.kind);
            ui.set_preview_3d(false);
            ui.set_layer_shown(row.layer as f32);
            ctrl.risk_selection_changed();
        };
    });
    let weak = ui.as_weak();
    forward!(ui, ctrl as ctrl {
        on_risk_fix_requested |index| {
            let Some((ui, row)) = risk_row(&weak, index) else {
                return;
            };
            // A fix is offered only for an analysis of the current slice.
            if ui.get_slice_estimates_stale() {
                return;
            }
            match row.action {
                1 => {
                    ui.set_supports_enabled(true);
                    ui.invoke_auto_supports_requested();
                }
                2 => ctrl.risk_auto_drain(&row),
                _ => {}
            }
        };
    });
    let weak = ui.as_weak();
    ui.on_risk_settings_changed(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_slice_estimates_stale(true);
        }
    });
    let weak = ui.as_weak();
    forward!(ui, ctrl as ctrl {
        on_next_island_requested || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let islands: Vec<usize> = ui.get_island_layers().iter().filter_map(|l| usize::try_from(l).ok()).collect();
            let current = ui.get_layer_shown().max(0.0) as usize;
            if let Some(layer) = next_island_layer(&islands, current) {
                ui.set_layer_shown(layer as f32);
                ctrl.layer_changed();
            }
        };
        on_layer_picked || ctrl.layer_changed();
    });
}

/// The window and its risk row at `index`, if both are there.
fn risk_row(ui: &slint::Weak<MainWindow>, index: i32) -> Option<(MainWindow, crate::RiskRow)> {
    let ui = ui.upgrade()?;
    let row = ui.get_print_risks().row_data(index.max(0) as usize)?;
    Some((ui, row))
}

/// The island layer after `current`: the lowest above it, else the lowest
/// of all (the jump wraps). `None` without islands.
fn next_island_layer(islands: &[usize], current: usize) -> Option<usize> {
    let lowest_above = islands
        .iter()
        .copied()
        .filter(|&layer| layer > current)
        .min();
    lowest_above.or_else(|| islands.iter().copied().min())
}

fn wire_viewport(ui: &MainWindow, ctrl: &Rc<ViewController>) {
    // Each resize restarts the timer; once resizing pauses for
    // `RESIZE_QUIET`, the renderer resizes to the exact pixels.
    let settle = slint::Timer::default();
    forward!(ui, ctrl as ctrl {
        on_view_size_changed |w, h| {
            ctrl.view_resized(w, h);
            let ctrl = ctrl.clone();
            settle.start(slint::TimerMode::SingleShot, RESIZE_QUIET, move || ctrl.resize_done());
        };
        on_orbit_dragged |dx, dy| ctrl.orbit_by(ScreenDelta::new(dx, dy));
        on_pan_dragged |dx, dy| ctrl.pan_by(ScreenDelta::new(dx, dy));
        on_wheel_zoom |delta, x, y| ctrl.zoom_by(delta, ScreenPoint::new(x, y));
        on_fit_view_asked || ctrl.frame_models();
        on_axis_view_asked |code| ctrl.look_along_axis(code);
        // A negative index means no selection.
        on_part_row_picked |index| match usize::try_from(index) {
            Ok(index) => ctrl.on_select_by_index(index),
            Err(_) => ctrl.select_nothing(),
        };
        on_viewport_pointer_down |x, y, m| ctrl.on_pointer_down(ScreenPoint::new(x, y), m);
        on_pointer_moved |x, y, m| ctrl.on_pointer_move(ScreenPoint::new(x, y), m);
        on_pointer_released |x, y, m| ctrl.on_pointer_up(ScreenPoint::new(x, y), m);
        on_pointer_left_view || ctrl.pointer_left();
        on_gizmo_toolbar |index| ctrl.on_gizmo_toolbar(index);
        on_move_tool_key || ctrl.keys_move();
        on_rotate_tool_key || ctrl.keys_rotate();
        on_scale_tool_key || ctrl.pick_scale_tool();
        on_object_transform_committed |scale, axis, text| ctrl.on_object_transform_committed(scale, axis, text);
        on_units_conversion_requested |factor| ctrl.on_units_conversion_requested(factor);
        on_position_x_committed |text| ctrl.on_position_committed(0, text);
        on_position_y_committed |text| ctrl.on_position_committed(1, text);
        on_position_z_committed |text| ctrl.on_position_committed(2, text);
        on_center_on_plate_requested || ctrl.on_center_on_plate_requested();
        on_scale_to_fit_requested || ctrl.scale_to_fit_action();
        on_drop_to_plate_requested || ctrl.on_drop_to_plate_requested();
        on_arrow_key_down |code| ctrl.gizmo_key_down(code);
        on_arrow_key_up |code| ctrl.gizmo_key_up(code);
        on_step_selection |delta| ctrl.cycle_selection(delta);
        on_selection_dropped || ctrl.select_nothing();
    });
}

fn attach_viewport_render_notifications(
    ui: &MainWindow,
    ctrl: Rc<ViewController>,
) -> Result<(), slint::SetRenderingNotifierError> {
    ui.window()
        .set_rendering_notifier(move |state, api| match state {
            slint::RenderingState::RenderingSetup => match api {
                slint::GraphicsAPI::WGPU29 { device, queue, .. } => {
                    ctrl.graphics_ready(device.clone(), queue.clone());
                }
                // The viewport draws with wgpu only. Any other API leaves the
                // window without a device for good; recording which one came
                // lets the user be told, and a report name it.
                other => ctrl.graphics_unsupported(format!("{other:?}")),
            },
            slint::RenderingState::BeforeRendering => ctrl.before_frame(),
            slint::RenderingState::RenderingTeardown => ctrl.graphics_gone(),
            _ => {}
        })
}

/// Runs `work` on a new thread and returns its result on the UI thread;
/// `None` if the event loop ended first.
async fn on_worker<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = oneshot::channel();
    std::thread::spawn(move || {
        let result = work();
        let _ = slint::invoke_from_event_loop(move || {
            let _ = tx.send(result);
        });
    });
    rx.await.ok()
}

/// [`on_worker`], with a panic in `work` returned as an error.
async fn on_worker_caught<T: Send + 'static>(
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Option<anyhow::Result<T>> {
    on_worker(move || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
            .unwrap_or_else(|payload| Err(anyhow::anyhow!(mesh_loader::panic_text(&payload))))
    })
    .await
}

fn connect_mesh_open_action(ui: &MainWindow, ctrl: Rc<ViewController>, prefs: Rc<PreferenceHub>) {
    let weak = ui.as_weak();
    forward!(ui, ctrl as ctrl {
        on_scene_opened || {
            ctrl.scene_opened();
            if weak.upgrade().is_some_and(|ui| ui.get_scene_present()) {
                ctrl.switch_section(crate::WorkspaceSection::Transform);
            }
        };
    });
    ui.on_open_file(move || {
        // Callbacks keep running while the file dialog is open, so the
        // load gate is taken before it opens: `loading` shows at once and
        // a second Open does nothing. A cancelled pick drops the ticket.
        let Some(ticket) = ctrl.scene_loader().request_load_ticket() else {
            return;
        };
        if let Err(err) =
            slint::spawn_local(open_chosen_mesh_file(ticket, ctrl.clone(), prefs.clone()))
        {
            // The ticket went down with the future.
            eprintln!("Failed to spawn file dialog task: {err}");
        }
    });
}

/// Open: pick a file (starting in the last folder opened from), remember
/// it, and load it now, once the viewport starts, or straight into the
/// usual load error. The build volume is read after the pick, so a
/// setting changed meanwhile applies.
async fn open_chosen_mesh_file(
    ticket: SceneLoadTicket,
    ctrl: Rc<ViewController>,
    prefs: Rc<PreferenceHub>,
) {
    let start = mesh_dialog_start_directory(prefs.current_values().last_open_dir, sc_app_paths::home_dir());
    let Some(path) = prompt_for_mesh_path(start).await else {
        return;
    };
    retain_mesh_parent_directory(&prefs, &path);
    remember_recent_file(&prefs, &path);
    let loader = ctrl.scene_loader();
    match mesh_loader::open_verdict(loader.unavailable_graphics_reason()) {
        OpenVerdict::LoadNow => {
            mesh_loader::begin_ticketed_load(ticket, path, &loader, ctrl.printer_volume())
        }
        // The viewport has not had its first paint yet: the file waits
        // for `handle_rendering_setup`, which takes its own ticket.
        OpenVerdict::HoldUntilGpu => {
            ctrl.queued_load().hold(path);
            drop(ticket);
        }
        OpenVerdict::CannotLoad(reason) => {
            ticket.deliver_loaded_scene(path, Err(anyhow::Error::msg(reason.explain())))
        }
    }
}

/// Where the open dialog starts: the last folder opened from if it still
/// exists, else the home folder if it exists, else the platform's choice.
fn mesh_dialog_start_directory(
    last_open_dir: Option<PathBuf>,
    home_dir: Option<PathBuf>,
) -> Option<PathBuf> {
    last_open_dir
        .filter(|dir| dir.is_dir())
        .or_else(|| home_dir.filter(|home| home.is_dir()))
}

/// Remembers the folder of an opened file for the next open dialog,
/// writing the settings only when it changed.
fn retain_mesh_parent_directory(prefs: &Rc<PreferenceHub>, opened_file: &Path) {
    let Some(dir) = opened_file.parent().map(Path::to_path_buf) else {
        return;
    };
    if prefs.current_values().last_open_dir.as_ref() != Some(&dir) {
        prefs.edit_preferences(move |snap| {
            snap.last_open_dir = Some(dir);
            PreferenceEdit::LastOpenDir
        });
    }
}

/// Puts an opened file first in Open Recent. Every way of opening a file
/// comes here (the dialog, drag and drop, the command line, Open Recent);
/// a path that is not a file is left out.
pub(crate) fn remember_recent_file(prefs: &Rc<PreferenceHub>, opened_file: &Path) {
    if !opened_file.is_file() {
        return;
    }
    let path = opened_file.to_path_buf();
    prefs.edit_preferences(move |snap| {
        snap.recent_files = recent_files_with_pushed(std::mem::take(&mut snap.recent_files), path);
        PreferenceEdit::RecentFiles
    });
}

/// `path` first, once, then the rest, up to `MAX_RECENT_FILES`.
fn recent_files_with_pushed(mut existing: Vec<PathBuf>, path: PathBuf) -> Vec<PathBuf> {
    existing.retain(|p| p != &path);
    existing.insert(0, path);
    existing.truncate(crate::app_settings::MAX_RECENT_FILES);
    existing
}

async fn prompt_for_mesh_path(start_dir: Option<PathBuf>) -> Option<PathBuf> {
    let filters: [(SharedText, &[&str]); 5] = [
        (
            ctr!("FileDialog", "All supported files"),
            mesh_loader::OPENABLE_EXTENSIONS,
        ),
        (ctr!("FileDialog", "3MF files"), &["3mf"]),
        (ctr!("FileDialog", "OBJ files"), &["obj"]),
        (ctr!("FileDialog", "STL files"), &["stl", "stla", "stlb"]),
        (ctr!("FileDialog", "All files"), &["*"]),
    ];
    let mut dialog = rfd::AsyncFileDialog::new();
    for (name, extensions) in filters {
        dialog = dialog.add_filter(name.to_string(), extensions);
    }
    if let Some(dir) = start_dir {
        dialog = dialog.set_directory(dir);
    }
    dialog.pick_file().await.map(|h| h.path().to_path_buf())
}

type SharedText = slint::SharedString;

fn connect_project_save_actions(ui: &MainWindow, ctrl: Rc<ViewController>, prefs: Rc<PreferenceHub>) {
    let (save_view, save_prefs) = (ctrl.clone(), prefs.clone());
    ui.on_save_project(move || start_project_save(&save_view, &save_prefs, false));
    ui.on_save_project_as(move || start_project_save(&ctrl, &prefs, true));
}

/// Runs Save, or Save As when `ask_where`, for the window's document. Both
/// can move the document (a first Save asks where, like Save As, and a
/// write may finish after the document moved on), so the title is
/// refreshed afterwards whatever happened.
fn start_project_save(view: &Rc<ViewController>, prefs: &PreferenceHub, ask_where: bool) {
    let saver = ProjectSaver::for_window(view, prefs);
    let offered = if ask_where { scene_export::suggested_export_name(&saver.doc) } else { None };
    let view = view.clone();
    let task = async move {
        let _ = if ask_where { saver.save_as(offered).await } else { saver.save().await };
        view.refresh_title();
    };
    if let Err(err) = slint::spawn_local(task) {
        eprintln!("Failed to start the save task: {err}");
    }
}

/// What a Save or Save As came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SaveResult {
    /// The document as it is now is on disk: the only result that lets a
    /// caller go on to close the scene.
    Saved,
    /// A file was written, but the document changed while it was (an edit,
    /// a printer change, another file opened), so the file lacks that.
    SavedButChanged,
    /// Nothing written: no scene, a cancelled dialog, or a failure that
    /// showed its own dialog.
    NotSaved,
}

impl SaveResult {
    fn from_check(check: SaveCheck) -> Self {
        if check == SaveCheck::Current { Self::Saved } else { Self::SavedButChanged }
    }
}

/// Everything a project save needs from its window: the document, where
/// errors and progress show, and the printer and resin a project without
/// its own records under.
pub(crate) struct ProjectSaver {
    pub(crate) doc: Rc<SceneDocument>,
    pub(crate) dialogs: Rc<MessageWindows>,
    pub(crate) progress: Rc<SaveProgressPanel>,
    pub(crate) printer: Option<PrinterId>,
    pub(crate) resin: Option<MaterialId>,
}

impl ProjectSaver {
    /// The window's document and panels, with the preferred printer and
    /// resin as they are now.
    pub(crate) fn for_window(view: &ViewController, prefs: &PreferenceHub) -> Self {
        let preferred = prefs.current_values();
        Self {
            doc: view.scene_doc(),
            dialogs: view.dialog_manager(),
            progress: view.save_panel(),
            printer: preferred.default_printer,
            resin: preferred.default_resin,
        }
    }

    /// Save: write over the project file, ask where (Save As) when there is
    /// none, and do nothing for a project that is already saved.
    pub(crate) async fn save(&self) -> SaveResult {
        match SaveAction::for_document(&self.doc) {
            SaveAction::NothingOpen => SaveResult::NotSaved,
            SaveAction::UpToDate => SaveResult::Saved,
            SaveAction::WriteOver(target) => self.write_to(&target).await,
            SaveAction::AskWhere => {
                let offered = plate_file_stem(&self.doc, plate_position(&self.doc));
                self.save_as(Some(offered)).await
            }
        }
    }

    /// Save As: ask for a `.3mf` name, offering `offered`, and write there.
    pub(crate) async fn save_as(&self, offered: Option<String>) -> SaveResult {
        let mut ask = rfd::AsyncFileDialog::new().add_filter(ctr!("FileDialog", "3MF files").to_string(), &["3mf"]);
        if let Some(stem) = offered {
            ask = ask.set_file_name(format!("{stem}.3mf"));
        }
        let picked = ask.save_file().await.map(|file| file.path().to_path_buf());
        match SaveAsAnswer::from_dialog(picked) {
            SaveAsAnswer::Dismissed => SaveResult::NotSaved,
            SaveAsAnswer::WriteTo(target) => self.write_to(&target).await,
            SaveAsAnswer::NotAProjectName(target) => {
                let refusal = anyhow::anyhow!("Project files are saved as .3mf packages.");
                show_save_error(&self.dialogs, &target, &refusal);
                SaveResult::NotSaved
            }
        }
    }

    /// Writes the project to `target` on a worker with the progress panel
    /// up. The window keeps running meanwhile, so the result says whether
    /// the document is still what was written.
    async fn write_to(&self, target: &Path) -> SaveResult {
        let destination = target.to_path_buf();
        // None: a close raced the menu and there is no scene to write.
        let Some((bundle, payload, stamp)) =
            scene_save::ProjectSnapshot::for_save(&self.doc, destination.clone(), self.printer.clone(), self.resin.clone())
        else {
            return SaveResult::NotSaved;
        };
        self.progress.begin();
        let sink = self.progress.sink();
        let report = move |phase, done, total| sink.set(phase, done, total);
        let written = on_worker_caught(move || bundle.write(report)).await;
        self.progress.end();
        match written {
            None => SaveResult::NotSaved,
            Some(Err(err)) => {
                show_save_error(&self.dialogs, target, &err);
                SaveResult::NotSaved
            }
            Some(Ok(())) => SaveResult::from_check(self.doc.note_project_saved(payload, destination, stamp)),
        }
    }
}

/// An error dialog naming the file a write was for.
fn show_file_error(dialogs: &Rc<MessageWindows>, heading: SharedText, target: &Path, err: &anyhow::Error) {
    let about = ctr!("ErrorDialog", "File: {}", target.display()).to_string();
    dialogs.show_failure(heading.as_str(), Some(about), err.to_string(), error_with_causes(err));
}

fn show_save_error(dialogs: &Rc<MessageWindows>, target: &Path, err: &anyhow::Error) {
    show_file_error(dialogs, ctr!("ErrorDialog", "Failed to save"), target, err);
}

fn connect_mesh_export_action(ui: &MainWindow, ctrl: Rc<ViewController>) {
    forward!(ui, ctrl as ctrl {
        on_export_object_requested || {
            // The selected part alone, placed as in the scene, as an STL.
            let source = ctrl.scene_doc();
            let scene = source
                .read_scene(|s| {
                    let id = source.main_chosen_node()?;
                    let mesh = s.graph().leaf_mesh_arc(id)?.clone();
                    let placed = s.graph().world_transform_of(id).ok()?;
                    Some(sc_scene_edit::SceneFragment::from_leaf_snapshot(mesh, placed).into_scene())
                })
                .flatten();
            let Some(scene) = scene else {
                return;
            };
            let doc = SceneDocument::new();
            doc.take_spatial(sc_mesh_spatial::SceneSpatial::from_graph(scene), PathBuf::from("object.stl"), None);
            let ctrl = ctrl.clone();
            let _ = slint::spawn_local(async move {
                export_document_under_chosen_path(doc, ctrl.dialog_manager(), ctrl.export_panel(), Some("object".into()), "stl").await;
            });
        };
    });
    ui.on_export_model(move || {
        let doc = ctrl.scene_doc();
        let stem = Some(plate_file_stem(&doc, plate_position(&doc)));
        let extension = scene_export::suggested_export_suffix(&doc);
        let export =
            export_document_under_chosen_path(doc, ctrl.dialog_manager(), ctrl.export_panel(), stem, extension);
        if let Err(err) = slint::spawn_local(async move {
            let _ = export.await;
        }) {
            eprintln!("Failed to spawn export dialog task: {err}");
        }
    });
}

/// Export As: ask for a name, take the format from its extension, and
/// write a copy of the scene as it is now on a worker, with the export
/// progress panel up. `true` only when a file was written (so a caller may
/// go on to close the scene); refusals and failures show their dialog.
pub(crate) async fn export_document_under_chosen_path(
    doc: Rc<SceneDocument>,
    dialogs: Rc<MessageWindows>,
    export_progress: Rc<ExportProgressPanel>,
    default_stem: Option<String>,
    default_extension: &'static str,
) -> bool {
    // The dialog does not say which filter was chosen, so the name's
    // extension decides the format (`.stl` binary, `.stla` ASCII, ...).
    let (target, format) =
        match decide_export_dispatch(prompt_for_mesh_export_destination(default_stem, default_extension).await) {
            ExportDispatch::Cancelled => return false,
            ExportDispatch::UnsupportedExtension(target) => {
                let err = anyhow::anyhow!(
                    "Unable to infer file format from filename. Use one of: \
                 .stl (binary STL), .stla (ASCII STL), .obj, or .3mf."
                );
                report_mesh_export_failure(&dialogs, &target, &err);
                return false;
            }
            ExportDispatch::Ready { target, format } => (target, format),
        };
    // A copy taken now, so the file is what the screen shows at this
    // moment; none if a close raced the menu.
    let Some(scene) = scene_export::scene_copy_for_export(&doc) else {
        report_mesh_export_failure(&dialogs, &target, &anyhow::anyhow!("No scene loaded"));
        return false;
    };
    export_progress.begin();
    let sink = export_progress.sink();
    let worker_target = target.clone();
    let written = on_worker_caught(move || {
        scene_export::write_exported_scene(&scene, &worker_target, format, |phase, done, total| {
            sink.set(phase, done, total)
        })
    })
    .await;
    export_progress.end();
    match written {
        Some(Ok(warnings)) => {
            for warning in &warnings {
                eprintln!("WARNING [{}]: {}", target.display(), warning.user_message());
            }
            true
        }
        Some(Err(err)) => {
            report_mesh_export_failure(&dialogs, &target, &err);
            false
        }
        None => false,
    }
}

fn report_mesh_export_failure(dialogs: &Rc<MessageWindows>, target: &Path, err: &anyhow::Error) {
    show_file_error(
        dialogs,
        ctr!("ErrorDialog", "Failed to export"),
        target,
        err,
    );
}

/// The Export As dialog, opening on the suggested format: its filter
/// first (most dialogs make the first one active) and its extension on
/// the offered name.
async fn prompt_for_mesh_export_destination(
    default_stem: Option<String>,
    default_extension: &'static str,
) -> Option<PathBuf> {
    let stl = (ctr!("FileDialog", "STL (binary)"), "stl");
    let stl_ascii = (ctr!("FileDialog", "STL (ASCII)"), "stla");
    let obj = (ctr!("FileDialog", "OBJ files"), "obj");
    let three_mf = (ctr!("FileDialog", "3MF files"), "3mf");
    let ordered = match default_extension {
        "obj" => [obj, stl, stl_ascii, three_mf],
        "stla" => [stl_ascii, stl, obj, three_mf],
        "3mf" => [three_mf, stl, stl_ascii, obj],
        _ => [stl, stl_ascii, obj, three_mf],
    };
    let mut dialog = rfd::AsyncFileDialog::new();
    for (name, extension) in ordered {
        dialog = dialog.add_filter(name.to_string(), &[extension]);
    }
    if let Some(stem) = default_stem {
        dialog = dialog.set_file_name(format!("{stem}.{default_extension}"));
    }
    dialog.save_file().await.map(|h| h.path().to_path_buf())
}

/// The sliced file an export writes. All write the held slice; they differ
/// in the writer and in what the dialog asks for (a file, or a folder for
/// PNG layers).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlicedExportKind {
    GcodeZip,
    ThreeMf,
    /// One SVG, a group per layer.
    Svg,
    /// A folder of 8-bit grayscale PNGs, one per layer.
    PngLayers,
}

impl SlicedExportKind {
    fn format(self) -> sc_sla_formats::Format {
        use sc_sla_formats::Format;
        match self {
            Self::GcodeZip => Format::GcodeZip,
            Self::ThreeMf => Format::ThreeMf,
            Self::Svg => Format::SvgCombined,
            Self::PngLayers => Format::PngPerLayer,
        }
    }

    /// A new file (or folder) for this export in `dir`, never one already
    /// there.
    fn unused_target(self, dir: &Path, stem: &str) -> PathBuf {
        use crate::send_to_printer::unique_target;
        match self {
            Self::GcodeZip => unique_target(dir, stem, "zip"),
            Self::ThreeMf => unique_target(dir, stem, "3mf"),
            Self::Svg => unique_target(dir, stem, "svg"),
            Self::PngLayers => unique_target(dir, &format!("{stem}_layers"), ""),
        }
    }
}

/// The export menu's format list (and the Device tab's): 0 G-code ZIP,
/// 1 PNG layers, 2 3MF, 3 SVG.
pub(crate) fn export_kind_for_format(format: i32) -> SlicedExportKind {
    match format {
        1 => SlicedExportKind::PngLayers,
        2 => SlicedExportKind::ThreeMf,
        3 => SlicedExportKind::Svg,
        _ => SlicedExportKind::GcodeZip,
    }
}

fn wire_export_sliced(ui: &MainWindow, ctrl: &Rc<ViewController>) {
    forward!(ui, ctrl as ctrl {
        on_export_sliced_3mf_requested || spawn_slice_export(&ctrl, SlicedExportKind::ThreeMf);
        on_export_sliced_job_requested || spawn_slice_export(&ctrl, SlicedExportKind::GcodeZip);
        on_export_sliced_svg_requested || spawn_slice_export(&ctrl, SlicedExportKind::Svg);
        on_export_sliced_png_layers_requested || spawn_slice_export(&ctrl, SlicedExportKind::PngLayers);
    });
}

fn spawn_slice_export(ctrl: &Rc<ViewController>, kind: SlicedExportKind) {
    let export = run_export_sliced_flow(ctrl.scene_doc(), ctrl.dialog_manager(), kind, None);
    if let Err(err) = slint::spawn_local(export) {
        eprintln!("Failed to spawn slice export task: {err}");
    }
}

/// Export sliced file: the active plate's held slice, to a file the user
/// picks, or, with `drive` (Device tab, Export here), to a new name in
/// that folder. Then the after-export command, if one is set. A cancelled
/// dialog ends it quietly; problems show an error dialog.
pub(crate) async fn run_export_sliced_flow(
    doc: Rc<SceneDocument>,
    dialogs: Rc<MessageWindows>,
    kind: SlicedExportKind,
    drive: Option<PathBuf>,
) {
    // The plate as it was when the export was asked for.
    let position = plate_position(&doc);
    let stem = plate_file_stem(&doc, position);
    let target = match drive {
        Some(dir) => Some(kind.unused_target(&dir, &stem)),
        None => pick_sliced_target(kind, stem).await,
    };
    let Some(target) = target else {
        return;
    };
    let fail = |err: anyhow::Error| show_slice_export_error(&dialogs, &target, &err);
    let Some((stack, options)) = crate::slice_export::prepare_slice_export(&doc) else {
        return fail(anyhow::anyhow!(
            "No slice is held. Slice the plate first, then export it."
        ));
    };
    let active = doc.active_plate();
    let plate_scene = || {
        doc.read_scene(|s| s.graph().plate_scene(active))
            .transpose()
    };
    let preview_scene = plate_scene().ok().flatten();
    let calibration = plate_scene().map_err(|e| e.to_string()).and_then(|plate| {
        plate
            .map(|p| sc_print_glue::calibration::regions(&p))
            .transpose()
            .map_err(crate::calibration::error_message)
    });
    match calibration {
        Err(message) => return fail(anyhow::anyhow!(message)),
        Ok(Some(regions)) if !regions.is_empty() && kind != SlicedExportKind::GcodeZip => {
            return fail(anyhow::anyhow!(tr!(
                "Exposure calibration requires PNG + run.gcode (G-code ZIP). This format cannot express sequential tile exposures."
            )));
        }
        Ok(_) => {}
    }
    let job = match kind {
        SlicedExportKind::GcodeZip => match crate::slice_export::prepare_job_export(&doc) {
            Ok(settings) => Some(settings),
            Err(err) => return fail(err),
        },
        _ => None,
    };
    let project = (kind == SlicedExportKind::ThreeMf)
        .then(|| {
            doc.all_plate_slices()
                .into_iter()
                .find(|(index, _)| *index == position)
        })
        .flatten()
        .and_then(|(_, slice)| {
            scene_save::ProjectSnapshot::of_plate(&doc, position, &slice, target.clone())
        });
    let layer_count = stack.layers().len();
    let worker_target = target.clone();
    let written = on_worker_caught(move || {
        let mut options = options;
        if let Some(mesh) = preview_scene.and_then(|s| s.merge_meshes().ok().flatten()) {
            options = options.with_preview_mesh(&mesh);
        }
        if let Some(bundle) = &project {
            bundle.write(|_, _, _| {})
        } else if let Some(settings) = &job {
            let prepared = crate::slice_export::build_job_export(settings, &stack, &options)?;
            Ok(sc_sla_formats::write_gcode_zip(
                &stack,
                &worker_target,
                &options,
                &prepared,
            )?)
        } else {
            crate::slice_export::write_slice_export(&stack, &worker_target, kind.format(), &options)
        }
    })
    .await;
    match written {
        Some(Ok(())) => {
            crate::hub::record_export(&doc, &target, layer_count);
            let command = crate::send_to_printer::SendSettings::load().post_export_command;
            let after = if command.trim().is_empty() {
                None
            } else {
                let file = target.clone();
                on_worker(move || crate::send_to_printer::run_post_export(&command, &file))
                    .await
                    .flatten()
            };
            show_slice_export_success(&dialogs, &target, kind, layer_count, after);
        }
        Some(Err(err)) => fail(err),
        None => {}
    }
}

/// The dialog for a sliced export, offering `<stem>` with the kind's
/// extension.
async fn pick_sliced_target(kind: SlicedExportKind, stem: String) -> Option<PathBuf> {
    let (filter, name) = match kind {
        SlicedExportKind::GcodeZip => (
            Some((ctr!("FileDialog", "Printer job ZIP files"), "zip")),
            format!("{stem}.zip"),
        ),
        SlicedExportKind::ThreeMf => (
            Some((ctr!("FileDialog", "3MF project"), "3mf")),
            format!("{stem}.3mf"),
        ),
        SlicedExportKind::Svg => (
            Some((ctr!("FileDialog", "SVG files"), "svg")),
            format!("{stem}.svg"),
        ),
        // A save dialog, not a folder picker, so a new folder can be
        // named in one step: the PNG writer needs a folder that is new or
        // empty, and a folder picker leads to existing, full folders.
        SlicedExportKind::PngLayers => (None, format!("{stem}_layers")),
    };
    let mut dialog = rfd::AsyncFileDialog::new();
    if let Some((title, extension)) = filter {
        dialog = dialog.add_filter(title.to_string(), &[extension]);
    }
    dialog
        .set_file_name(name)
        .save_file()
        .await
        .map(|h| h.path().to_path_buf())
}

/// Says what was written. The SVG and PNG exports say plainly that they
/// are not files a printer can print: claiming more would be worse than
/// writing nothing.
fn show_slice_export_success(
    dialogs: &Rc<MessageWindows>,
    target: &Path,
    kind: SlicedExportKind,
    layer_count: usize,
    after: Option<Result<String, String>>,
) {
    let target = target.display();
    let message = match kind {
        SlicedExportKind::ThreeMf => {
            ctr!(
                "SliceExport",
                "Exported the scene and held layers to {}.",
                target
            )
        }
        SlicedExportKind::GcodeZip => {
            ctr!(
                "SliceExport",
                "Exported {} layers and printer job script to {}.",
                layer_count,
                target
            )
        }
        SlicedExportKind::Svg => ctr!(
            "SliceExport",
            "Exported {} layer(s) to {}.\n\nThis is a flattened preview file (SVG), not a \
             printer-ready file. Native printer formats (CTB, SL1, ...) are not implemented \
             in Sleptic Slicer yet.",
            layer_count,
            target
        ),
        SlicedExportKind::PngLayers => ctr!(
            "SliceExport",
            "Exported {} layer(s) as PNG images to {}.\n\nThese are the raw per-layer rasters, \
             not a printer-ready file. Native printer formats (CTB, SL1, ...) are not \
             implemented in Sleptic Slicer yet.",
            layer_count,
            target
        ),
    };
    // The after-export command's own report follows.
    let message = match after {
        Some(Ok(line) | Err(line)) => format!("{message}\n\n{line}"),
        None => message.to_string(),
    };
    dialogs.show_notice(
        &ctr!("SliceExport", "Sliced file exported"),
        message,
        String::new(),
    );
}

fn show_slice_export_error(dialogs: &Rc<MessageWindows>, target: &Path, err: &anyhow::Error) {
    show_file_error(
        dialogs,
        ctr!("ErrorDialog", "Failed to export sliced file"),
        target,
        err,
    );
}

fn wire_export_all(ui: &MainWindow, ctrl: &Rc<ViewController>) {
    let (doc, dialogs, weak) = (ctrl.scene_doc(), ctrl.dialog_manager(), ui.as_weak());
    ui.on_export_all_requested(move || {
        let format = weak.upgrade().map_or(0, |ui| ui.get_export_format());
        let _ = slint::spawn_local(export_all_plates(doc.clone(), dialogs.clone(), format));
    });
}

/// Export all: every plate's slice, in the export menu's `format`, into a
/// folder the user picks, as `<stem>_plate<n>.<extension>`.
async fn export_all_plates(doc: Rc<SceneDocument>, dialogs: Rc<MessageWindows>, format: i32) {
    let Some(folder) = rfd::AsyncFileDialog::new().pick_folder().await else {
        return;
    };
    let fail =
        |message: String| dialogs.show_failure(&tr!("Export all"), None, message, String::new());
    let slices = doc.all_plate_slices();
    if slices.len()
        != doc
            .read_scene(|s| s.graph().plates().order.len())
            .unwrap_or(0)
    {
        return fail(tr!("Slice every plate before exporting.").to_string());
    }
    let jobs: anyhow::Result<Vec<_>> = slices
        .iter()
        .map(|(index, slice)| plate_job(&doc, *index, slice, format))
        .collect();
    let jobs = match jobs {
        Ok(jobs) => jobs,
        Err(err) => return fail(err.to_string()),
    };
    let stem = scene_export::suggested_export_name(&doc).unwrap_or_else(|| "project".into());
    let folder = folder.path().to_path_buf();
    let extension = match format {
        0 => "zip",
        1 => "layers",
        2 => "3mf",
        _ => "svg",
    };
    let name = move |index: usize| folder.join(format!("{stem}_plate{index}.{extension}"));
    let bundles: Vec<_> = slices
        .iter()
        .map(|(index, slice)| {
            (format == 2)
                .then(|| scene_save::ProjectSnapshot::of_plate(&doc, *index, slice, name(*index)))
                .flatten()
        })
        .collect();
    let scene = doc.read_scene(|s| s.graph().clone());
    let written = on_worker(move || -> anyhow::Result<()> {
        for (((index, slice), job), bundle) in slices.into_iter().zip(jobs).zip(bundles) {
            let mut options =
                sc_sla_formats::WriterOptions::from_profile(&slice.profile, sc_sl1cer::VERSION);
            if let Some(scene) = &scene
                && let Some(plate) = scene.plates().order.get(index - 1)
                && let Some(mesh) = scene.plate_scene(plate.id)?.merge_meshes()?
            {
                options = options.with_preview_mesh(&mesh);
            }
            let target = name(index);
            if let Some(bundle) = bundle {
                bundle.write(|_, _, _| {})?;
            } else if let Some(settings) = job {
                let prepared =
                    crate::slice_export::build_job_export(&settings, &slice.merged, &options)?;
                sc_sla_formats::write_gcode_zip(&slice.merged, &target, &options, &prepared)?;
            } else {
                let kind = if format == 1 {
                    sc_sla_formats::Format::PngPerLayer
                } else {
                    sc_sla_formats::Format::SvgCombined
                };
                crate::slice_export::write_slice_export(&slice.merged, &target, kind, &options)?;
            }
        }
        Ok(())
    })
    .await;
    if let Some(Err(err)) = written {
        fail(err.to_string());
    }
}

/// The printer job of plate `index` (1-based) for Export all: job settings
/// with the plate's own process settings (else the scene's) and its
/// exposure test, for a G-code ZIP; nothing for the other formats, which
/// cannot hold an exposure test.
fn plate_job(
    doc: &SceneDocument,
    index: usize,
    slice: &crate::slice_result::SliceResult,
    format: i32,
) -> anyhow::Result<Option<crate::slice_export::JobExportSettings>> {
    let overrides = doc
        .read_scene(|s| {
            let settings = s
                .graph()
                .plates()
                .order
                .get(index - 1)?
                .settings
                .as_deref()?;
            Some(
                sc_print_profiles::scene_overrides::PlateSettings::parse(settings)
                    .ok()?
                    .process,
            )
        })
        .flatten()
        .unwrap_or(*doc.process_overrides.borrow());
    let regions = doc
        .read_scene(|s| -> anyhow::Result<_> {
            let plate = s
                .graph()
                .plates()
                .order
                .get(index - 1)
                .ok_or_else(|| anyhow::anyhow!("Missing plate"))?;
            let scene = s.graph().plate_scene(plate.id)?;
            sc_print_glue::calibration::regions(&scene)
                .map_err(crate::calibration::error_message)
                .map_err(|e| anyhow::anyhow!(e))
        })
        .transpose()?
        .unwrap_or_default();
    if format == 0 {
        let mut settings = crate::slice_export::prepare_job_export_for(&slice.params, &overrides)?;
        settings.regions = regions;
        Ok(Some(settings))
    } else if regions.is_empty() {
        Ok(None)
    } else {
        Err(anyhow::anyhow!(
            "Exposure calibration requires G-code ZIP with sequential PNG masks"
        ))
    }
}

#[cfg(test)]
#[path = "tests_wiring.rs"]
mod tests;

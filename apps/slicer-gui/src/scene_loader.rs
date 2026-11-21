// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Loading a scene in the background, one load at a time.
//!
//! [`SceneLoader::begin_load`] hands out a [`LoadTicket`], or nothing while
//! another load is under way. The ticket is the only way to deliver a
//! result, and dropping it ends the load: the "loading" flag clears when
//! the last ticket goes, however the load ended. So a load:
//!
//! 1. the UI asks for a ticket, and gives up if there is none;
//! 2. the ticket goes into the file dialog's closure; cancelling drops it;
//! 3. with a file picked it goes to the worker, which delivers the result
//!    with [`LoadTicket::deposit`] and wakes the UI thread;
//! 4. the UI takes it with [`SceneLoader::take_finished_load`], which drops
//!    results from loads since cancelled.
//!
//! [`SceneLoader::invalidate_and_clear`] is for closing the scene: loads
//! still running will be dropped when they deliver, and a result already
//! delivered is freed at once rather than kept until the UI gets to it.
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use sc_mesh_formats::messages::LoadMeshWarning;
use sc_mesh_spatial::SceneSpatial;
use sc_project_formats::{ProfileRefWarning, ProjectPayload, ReadProjectWarning};
use slint::ComponentHandle;
use slint::wgpu_29::wgpu;

use crate::MainWindow;
use crate::error_dialog::{MessageWindows, error_with_causes};
use crate::failure_gate::FailureStreak;
use crate::graphics_status::{GraphicsAbsence, NoGraphics};
use crate::i18n::ctr;
use crate::load_progress::{
    OpenStep as WorkerLoadPhase, LoadProgressSink, PROGRESS_PERMILLE_MAX, decode,
};
use crate::mesh_health::MeshHealth;
use crate::mesh_render::{DeviceLoss, DeviceStamp, UploadedScene, FrameResource, FrameRefusal};
use crate::reloaded_slices::SavedSlices;

/// How often the progress panel reads the worker's progress.
const SCENE_LOAD_POLL_PERIOD: Duration = Duration::from_millis(100);

/// The worker's phase as the window names it (`OpenStep` in
/// `main-window.slint`).
impl From<WorkerLoadPhase> for crate::OpenStep {
    fn from(phase: WorkerLoadPhase) -> Self {
        use WorkerLoadPhase as Worker;
        match phase {
            Worker::Parse => Self::Reading,
            Worker::Priming => Self::Meshing,
            Worker::GpuUpload => Self::Uploading,
        }
    }
}

/// The value behind `mutex`, also after a thread panicked holding it: every
/// value kept here (a delivery, a device) is whole at all times.
fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Puts the worker's progress on the panel.
fn show_progress(ui: &MainWindow, phase: WorkerLoadPhase, permille: u32) {
    ui.set_open_step(phase.into());
    ui.set_open_fraction(permille as f32 / PROGRESS_PERMILLE_MAX as f32);
}

/// What a worker hands the UI thread for a loaded file.
pub(crate) struct LoadedSceneBundle {
    pub placement_message: String,
    /// The scene with its spatial caches ready; kept so a new device can be
    /// given the scene again after a lost one.
    pub loaded_spatial_index: SceneSpatial,
    /// The scene on the GPU, for the device it was built on (its token).
    pub gpu_scene: UploadedScene,
    pub mesh_load_warnings: Vec<LoadMeshWarning>,
    /// The project data the file carried (profile names, embedded
    /// profiles); `None` for meshes and 3MF files without it. Saving writes
    /// it back.
    pub loaded_project_data: Option<ProjectPayload>,
    /// Warnings about the project data (another slicer's, an unknown
    /// version, an unrecognised profile part).
    pub loaded_project_warnings: Vec<ReadProjectWarning>,
    /// The file's named profiles that are not installed here.
    pub missing_profile_warnings: Vec<ProfileRefWarning>,
    /// The project's saved slices, if still valid for the scene: shown as
    /// the held slice without slicing again.
    pub restored_slice_stack: Option<SavedSlices>,
    /// Where the file was; for window titles and default export names.
    pub loaded_source_path: PathBuf,
    /// Whether the parts have problems, which picks the section the window
    /// opens on (Transform when none, else Prepare).
    pub loaded_mesh_health: MeshHealth,
}

/// A delivered result, with the guard that keeps its load counted.
type Delivery = (ActiveLoadGuard, PathBuf, anyhow::Result<LoadedSceneBundle>);
type LoadSlot = Arc<Mutex<Option<Delivery>>>;

/// Counts a load as under way until dropped; the last one clears the
/// window's loading flags. It holds nothing that points back at the slot it
/// is delivered into, so a slot never keeps itself alive.
struct ActiveLoadGuard {
    load_sequence: u64,
    active_load_count: Arc<AtomicU32>,
    load_window: slint::Weak<MainWindow>,
}

impl Drop for ActiveLoadGuard {
    fn drop(&mut self) {
        // A guard only exists while the count is at least 1, so the guard
        // that brings it to 0 is the last.
        let last = self.active_load_count.fetch_sub(1, Ordering::AcqRel) == 1;
        if !last {
            return;
        }
        let idle = |ui: MainWindow| {
            ui.set_busy_opening(false);
            ui.set_opening(false);
        };
        // On the UI thread with the window open (the usual case) clear the
        // flags now; from a worker, or with the window gone, through the
        // event loop.
        if let Some(ui) = self.load_window.upgrade() {
            idle(ui);
        } else {
            let _ = self.load_window.upgrade_in_event_loop(idle);
        }
    }
}

/// A load under way. Dropping it without delivering (dialog cancelled,
/// worker failed, scene closed) simply ends the load. The slot reference is
/// the ticket's, not the guard's, so it goes as soon as the ticket is used
/// or dropped and never into the slot.
pub(crate) struct SceneLoadTicket {
    load_guard: ActiveLoadGuard,
    delivery_slot: LoadSlot,
}

impl SceneLoadTicket {
    /// The load's number, which a delivery must still match when taken.
    #[cfg(test)]
    pub(crate) fn load_sequence(&self) -> u64 {
        self.load_guard.load_sequence
    }

    /// Delivers the result and wakes the UI thread. The load stays counted
    /// until the UI takes the result (or the loader goes, freeing it).
    pub(crate) fn deliver_loaded_scene(self, path: PathBuf, result: anyhow::Result<LoadedSceneBundle>) {
        let window = self.load_guard.load_window.clone();
        locked(&self.delivery_slot).replace((self.load_guard, path, result));
        let _ = window.upgrade_in_event_loop(|ui| ui.invoke_scene_opened());
    }
}

pub(crate) struct SceneLoadCoordinator {
    load_window: slint::Weak<MainWindow>,
    load_dialogs: Rc<MessageWindows>,
    /// The latest load's number; anything delivered with another is stale.
    latest_load_sequence: Cell<u64>,
    delivery_slot: LoadSlot,
    /// Loads under way: 0 or 1, the ticket's or its delivered guard's.
    active_load_count: Arc<AtomicU32>,
    /// One dialog per run of failed scene uploads, and per run of refused
    /// frames.
    upload_failure_gate: FailureStreak,
    render_failure_gate: FailureStreak,
    /// The graphics device, set when Slint's rendering starts and cleared
    /// when it ends. Workers take a copy, so reading and uploading both
    /// happen off the UI thread.
    gpu: Mutex<Option<(wgpu::Device, DeviceStamp, wgpu::Queue)>>,
    /// Why there is no device, when there is none. Only asked while `gpu`
    /// is empty, and moved by the same calls, so the two never disagree.
    unavailable_graphics_state: GraphicsAbsence,
    /// The process's lost-device flag, given to workers with the device.
    lost_device_state: DeviceLoss,
    /// The worker's progress, phase and permille packed together
    /// (`load_progress`).
    worker_load_progress: Arc<AtomicU32>,
    /// Reads `progress` into the window while a load runs; it stops itself
    /// once the load ends. Shared so its callback can reach it weakly.
    load_progress_timer: Rc<slint::Timer>,
}

impl SceneLoadCoordinator {
    pub(crate) fn new(
        ui: &MainWindow,
        dialogs: Rc<MessageWindows>,
        device_loss: DeviceLoss,
    ) -> Self {
        Self::with_window(ui.as_weak(), dialogs, device_loss)
    }

    /// A loader with no window, for tests: the window updates are skipped,
    /// so no Slint backend is needed. Avoid the paths that open a dialog
    /// (a failed load, the first failed upload), which do need one.
    #[cfg(test)]
    pub(crate) fn loader_for_test(dialogs: Rc<MessageWindows>) -> Self {
        Self::with_window(slint::Weak::default(), dialogs, DeviceLoss::default())
    }

    fn with_window(
        ui: slint::Weak<MainWindow>,
        dialogs: Rc<MessageWindows>,
        device_loss: DeviceLoss,
    ) -> Self {
        // Everything else starts empty: no load, no device, no failures.
        Self {
            load_window: ui,
            load_dialogs: dialogs,
            lost_device_state: device_loss,
            latest_load_sequence: Cell::default(),
            delivery_slot: LoadSlot::default(),
            active_load_count: Arc::default(),
            worker_load_progress: Arc::default(),
            load_progress_timer: Rc::default(),
            gpu: Mutex::default(),
            unavailable_graphics_state: GraphicsAbsence::default(),
            upload_failure_gate: FailureStreak::default(),
            render_failure_gate: FailureStreak::default(),
        }
    }

    /// Where the worker reports its progress.
    pub(crate) fn worker_progress_sink(&self) -> LoadProgressSink {
        LoadProgressSink::new(Arc::clone(&self.worker_load_progress))
    }

    /// Resets the progress and shows the progress panel, just before the
    /// worker starts: so never while the file dialog is open, nor for a
    /// load given up before. Without a window only the reset happens. The
    /// poll stops itself once the load ends (the guard clears
    /// `load-in-progress`).
    pub(crate) fn start_load_progress_display(&self) {
        self.worker_progress_sink().reset();
        let Some(ui) = self.load_window.upgrade() else {
            return;
        };
        show_progress(&ui, WorkerLoadPhase::Parse, 0);
        ui.set_opening(true);
        let packed = Arc::clone(&self.worker_load_progress);
        let window = self.load_window.clone();
        let poll = Rc::downgrade(&self.load_progress_timer);
        let tick = move || match window.upgrade().filter(|ui| ui.get_opening()) {
            Some(ui) => {
                let (phase, permille) = decode(packed.load(Ordering::Relaxed));
                show_progress(&ui, phase, permille);
            }
            // The load ended (or the window went): the poll ends too.
            None => {
                if let Some(timer) = poll.upgrade() {
                    timer.stop();
                }
            }
        };
        self.load_progress_timer
            .start(slint::TimerMode::Repeated, SCENE_LOAD_POLL_PERIOD, tick);
    }

    #[cfg(test)]
    pub(crate) fn load_progress_for_test(&self) -> u32 {
        self.worker_load_progress.load(Ordering::Relaxed)
    }

    /// A ticket for a new load, or `None` while one is under way.
    ///
    /// The device is not checked here but once a file is picked
    /// ([`SceneLoader::gpu_snapshot`]): the loader exists before rendering
    /// starts, and checking now would keep the file dialog from opening in
    /// tests and before the first frame.
    pub(crate) fn request_load_ticket(&self) -> Option<SceneLoadTicket> {
        // 0 to 1 only if no ticket is alive; acquiring on success sees
        // everything the previous load did before its release.
        let claimed = self
            .active_load_count
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
        if claimed.is_err() {
            return None;
        }
        let stamp = self.next_stamp();
        if let Some(ui) = self.load_window.upgrade() {
            ui.set_busy_opening(true);
        }
        let guard = ActiveLoadGuard {
            load_sequence: stamp,
            active_load_count: Arc::clone(&self.active_load_count),
            load_window: self.load_window.clone(),
        };
        let slot = Arc::clone(&self.delivery_slot);
        Some(SceneLoadTicket { load_guard: guard, delivery_slot: slot })
    }

    /// Moves on to a new load number, making every earlier one stale.
    fn next_stamp(&self) -> u64 {
        let stamp = self.latest_load_sequence.get().wrapping_add(1);
        self.latest_load_sequence.set(stamp);
        stamp
    }

    /// Rendering has started on this device.
    pub(crate) fn install_graphics_device(
        &self,
        device: wgpu::Device,
        device_token: DeviceStamp,
        queue: wgpu::Queue,
    ) {
        locked(&self.gpu).replace((device, device_token, queue));
        self.unavailable_graphics_state.device_arrived();
    }

    /// Rendering started with a graphics API this build cannot use; there
    /// is still no device, now with a reason to give.
    pub(crate) fn record_unsupported_graphics_api(&self, api: String) {
        self.unavailable_graphics_state.foreign_api(api);
    }

    /// The device refused the 3D view's renderer; there is no device.
    pub(crate) fn record_renderer_refusal(&self, reason: String) {
        self.unavailable_graphics_state.renderer_refused(reason);
    }

    /// The device is lost: no load starts on it again, and that is why.
    pub(crate) fn record_device_loss(&self) {
        self.remove_graphics_device();
        self.unavailable_graphics_state.device_gone();
    }

    /// The process's lost-device flag (shared, not a copy of its state).
    pub(crate) fn lost_device_state(&self) -> DeviceLoss {
        DeviceLoss::clone(&self.lost_device_state)
    }

    /// Why there is no device; `None` when there is one.
    pub(crate) fn unavailable_graphics_reason(&self) -> Option<NoGraphics> {
        let has_device = locked(&self.gpu).is_some();
        if has_device {
            return None;
        }
        Some(self.unavailable_graphics_state.why())
    }

    /// Rendering has ended. Workers already running keep their copy; a
    /// scene they build for the old device is refused on install and
    /// uploaded again by the recovery path.
    pub(crate) fn remove_graphics_device(&self) {
        locked(&self.gpu).take();
    }

    /// The device to load with, if rendering has started.
    pub(crate) fn gpu_snapshot(&self) -> Option<(wgpu::Device, DeviceStamp, wgpu::Queue)> {
        locked(&self.gpu).clone()
    }

    /// The scene was closed: loads still running become stale, a delivered
    /// result is freed now (which also ends its load), and the upload
    /// failure run is over.
    pub(crate) fn cancel_and_clear_loads(&self) {
        self.next_stamp();
        // Taken out under the lock, dropped after it: dropping a delivery
        // ends its load, which may touch the window.
        let delivered = locked(&self.delivery_slot).take();
        drop(delivered);
        self.clear_upload_failure_gate();
    }

    /// Starts a new run of upload failures (a fresh renderer).
    pub(crate) fn clear_upload_failure_gate(&self) {
        self.upload_failure_gate.reset();
    }

    #[cfg(test)]
    pub(crate) fn upload_failure_count_for_test(&self) -> u32 {
        self.upload_failure_gate.in_a_row()
    }

    /// An upload worked: the next failure shows a dialog again.
    pub(crate) fn record_upload_success(&self) {
        self.clear_upload_failure_gate();
    }

    /// An upload failed; only the first of a run shows a dialog.
    pub(crate) fn record_upload_failure(&self, err: &anyhow::Error) {
        let first_of_run = self.upload_failure_gate.count_failure();
        if first_of_run {
            let heading = ctr!("ErrorDialog", "Failed to prepare the 3D view");
            let consequence = ctr!(
                "ErrorDialog",
                "The scene could not be handed to the graphics device. The 3D view shows the build plate alone until the next successful update."
            );
            self.load_dialogs.show_failure_later(
                heading.to_string(),
                Some(consequence.to_string()),
                err.to_string(),
                error_with_causes(err),
            );
        }
    }

    /// A frame was refused; only the first of a run shows a dialog. A lost
    /// device is not counted: that is reported once for the whole process.
    pub(crate) fn record_render_failure(&self, err: &FrameRefusal) {
        // Only a refusal counts; a lost device is told elsewhere, once.
        let FrameRefusal::Refused { resource: what, source } = err else {
            return;
        };
        if !self.render_failure_gate.count_failure() {
            return;
        }
        // What the refusal costs depends on what could not be made.
        let consequence = if matches!(what, FrameResource::CutTexture) {
            ctr!("ErrorDialog", "The cut is drawn without its fill until another layer is shown.")
        } else {
            ctr!("ErrorDialog", "The 3D view keeps its last image until it is resized.")
        };
        let heading = ctr!("ErrorDialog", "Failed to draw the 3D view");
        self.load_dialogs.show_failure_later(
            heading.to_string(),
            Some(consequence.to_string()),
            err.to_string(),
            source.to_string(),
        );
    }

    /// A frame was drawn: the next refusal shows a dialog again.
    pub(crate) fn record_render_success(&self) {
        self.clear_render_failure_gate();
    }

    pub(crate) fn clear_render_failure_gate(&self) {
        self.render_failure_gate.reset();
    }

    #[cfg(test)]
    pub(crate) fn render_failure_count_for_test(&self) -> u32 {
        self.render_failure_gate.in_a_row()
    }

    /// The finished load's scene, if it is the current load and succeeded
    /// (which also ends the upload failure run). A stale or missing result
    /// gives `None`; a failed load shows its error and gives `None`. Its
    /// load ends here, on the UI thread.
    pub(crate) fn take_completed_scene_load(&self) -> Option<LoadedSceneBundle> {
        let (path, result) = self.take_current_delivery()?;
        let prepared = match result {
            Ok(prepared) => prepared,
            Err(err) => {
                let heading = ctr!("ErrorDialog", "Failed to open 3D model");
                let file = ctr!("ErrorDialog", "File: {}", path.display());
                let (message, chain) = (err.to_string(), error_with_causes(&err));
                self.load_dialogs.show_failure(heading.as_str(), Some(file.to_string()), message, chain);
                return None;
            }
        };
        let LoadedSceneBundle {
            mesh_load_warnings: warnings,
            loaded_project_warnings: project_warnings,
            missing_profile_warnings: profile_warnings,
            ..
        } = &prepared;
        crate::load_warnings::log_load_warnings(&path, warnings, project_warnings, profile_warnings);
        self.clear_upload_failure_gate();
        Some(prepared)
    }

    /// [`Self::take_finished_load`] without its dialog, so tests can look
    /// at a failed result without an event loop.
    #[cfg(test)]
    pub(crate) fn take_delivery_for_test(
        &self,
    ) -> Option<(PathBuf, anyhow::Result<LoadedSceneBundle>)> {
        self.take_current_delivery()
    }

    /// Takes the delivery, if any; one from a load since cancelled is
    /// dropped. Either way its load ends.
    fn take_current_delivery(&self) -> Option<(PathBuf, anyhow::Result<LoadedSceneBundle>)> {
        let (guard, path, result) = locked(&self.delivery_slot).take()?;
        let current = guard.load_sequence == self.latest_load_sequence.get();
        // The guard goes here either way, ending the load.
        drop(guard);
        current.then_some((path, result))
    }
}

#[cfg(test)]
#[path = "tests_scene_loader.rs"]
mod tests;

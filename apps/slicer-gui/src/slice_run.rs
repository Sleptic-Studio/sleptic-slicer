// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The Slice action of one window.
//!
//! A press reads the scene and the chosen printer, resin and settings on
//! the UI thread and slices on a worker thread, so the window keeps
//! responding. The worker counts layers into shared atomics that a timer
//! copies into the progress bar, leaves its result in a slot and wakes the
//! UI thread (`slice-finished`), which shows it. Cancel trips the run's
//! token, which the slicer checks between layers.
//!
//! `slicing-in-progress` is the one gate: Slice does nothing while it is
//! set, and every ending (done, cancelled, failed, the worker panicking)
//! clears it. A panic is caught on the worker and left in the slot as a
//! failure, so the progress window always closes.
//!
//! Slice all queues every plate and slices them one after another; an
//! export waiting on it (`batch-export-pending`) starts once the queue is
//! empty. A cancel or failure drops the rest of the queue.
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use sc_print_glue::{SlicedPlate, SlicingParams, slice_input_fingerprint};
use sc_print_primitives::{AaLevel, MaterialId, PositiveFiniteMm, PrinterId};
use sc_print_profiles::{ProfileLibraries, SceneOverrides, resolve};
use sc_scene::Scene;
use sc_sl1cer::contour::ContourStack;
use sc_sl1cer::profile::{LayerSchedule, SliceProfile};
use sc_sl1cer::{CancelToken, SliceProgress};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::MainWindow;
use crate::error_dialog::MessageWindows;
use crate::i18n::tr;
use crate::mesh_loader::panic_text;
use crate::preferences_bus::PreferenceHub;
use crate::print_estimate::{estimated_cost, format_cost, format_duration, format_volume_ml};
use crate::scene_document::SceneDocument;
use crate::slice_projection::{Reason, SliceFailure, load_chosen_profiles};
use crate::slice_result::{SliceAgain, SliceParams, SliceResult};
use crate::slice_state::SliceState;

/// How often the progress bar is refreshed during a run.
const SLICE_PROGRESS_REFRESH_INTERVAL: Duration = Duration::from_millis(100);

/// How long "already up to date" shows after a press that changed nothing.
const SLICE_CURRENT_HINT_DURATION: Duration = Duration::from_secs(10);

/// The smallest feature, in mm, flagged as a risk when the field holds no
/// usable number.
const DEFAULT_MINIMUM_FEATURE_MM: f64 = 0.3;

/// Layers sliced so far and in all, written by the worker and read by the
/// progress timer. `completed` only rises.
#[derive(Default)]
struct SliceLayerProgress {
    finished_layers: AtomicU32,
    scheduled_layers: AtomicU32,
}

impl SliceLayerProgress {
    fn reset(&self) {
        self.finished_layers.store(0, Ordering::Relaxed);
        self.scheduled_layers.store(0, Ordering::Relaxed);
    }

    fn record(&self, progress: SliceProgress) {
        self.finished_layers
            .fetch_max(progress.completed as u32, Ordering::Relaxed);
        self.scheduled_layers.store(progress.total as u32, Ordering::Relaxed);
    }

    fn read(&self) -> (u32, u32) {
        (
            self.finished_layers.load(Ordering::Relaxed),
            self.scheduled_layers.load(Ordering::Relaxed),
        )
    }
}

/// A finished slice, merged, fingerprinted and analysed on the worker so
/// the UI thread only shows it.
pub(crate) struct CompletedSlice {
    merged_contours: ContourStack,
    finished_layer_schedule: LayerSchedule,
    input_fingerprint: String,
    profile: SliceProfile,
    volume_mm3: f64,
    /// Parts sliced in tolerant mode, and the fragments it dropped.
    tolerant_leaves: usize,
    dropped_fragments: usize,
    /// The layer of every unsupported island, ascending; a layer repeats
    /// once per island on it.
    island_layers: Vec<usize>,
    risks: Vec<sc_print_job::PrintRisk>,
}

enum SliceWorkerOutcome {
    Completed(Box<CompletedSlice>),
    Aborted,
}

type SliceWorkerSlot = Arc<Mutex<Option<Result<SliceWorkerOutcome, SliceFailure>>>>;

pub(crate) struct SliceActionDriver {
    /// Plates Slice all has still to slice, and whether it is running.
    queue: RefCell<VecDeque<u32>>,
    all_running: Cell<bool>,
    window_handle: slint::Weak<MainWindow>,
    slice_preferences_bus: Rc<PreferenceHub>,
    slice_scene_document: Rc<SceneDocument>,
    slice_dialog_manager: Rc<MessageWindows>,
    layer_progress: Arc<SliceLayerProgress>,
    worker_result: SliceWorkerSlot,
    /// The running slice's token and inputs; `None` between runs.
    cancellation_token: RefCell<Option<CancelToken>>,
    pending_slice_inputs: RefCell<Option<SliceParams>>,
    /// Refreshes the progress bar; an `Rc` so its callback can stop it
    /// through a `Weak`.
    slice_progress_timer: Rc<slint::Timer>,
    /// Hides "already up to date"; restarted by every press that changed
    /// nothing, stopped by a real slice.
    current_hint_timer: Rc<slint::Timer>,
}

impl SliceActionDriver {
    /// Wires Slice, Slice all, Cancel and the worker's wake-up to a new
    /// driver, which the caller keeps for the window's life.
    pub(crate) fn attach_slice_actions(
        ui: &MainWindow,
        bus: Rc<PreferenceHub>,
        document: Rc<SceneDocument>,
        dialogs: Rc<MessageWindows>,
    ) -> Rc<Self> {
        *document.plate_ui.borrow_mut() = Some(ui.as_weak());
        let run = Rc::new(Self {
            queue: RefCell::default(),
            all_running: Cell::new(false),
            window_handle: ui.as_weak(),
            slice_preferences_bus: bus,
            slice_scene_document: document,
            slice_dialog_manager: dialogs,
            layer_progress: Arc::default(),
            worker_result: Arc::default(),
            cancellation_token: RefCell::default(),
            pending_slice_inputs: RefCell::default(),
            slice_progress_timer: Rc::default(),
            current_hint_timer: Rc::default(),
        });
        let to = |action: fn(&Rc<Self>)| {
            let run = Rc::clone(&run);
            move || action(&run)
        };
        ui.on_start_slice(to(Self::begin_slice_request));
        ui.on_cancel_slice(to(|run| run.cancel_slice_run()));
        ui.on_slice_done(to(Self::finish));
        ui.on_slice_all_requested(to(Self::start_all));
        run
    }

    /// Shows a slice made elsewhere (a recipe) as `plate`'s slice. Its
    /// geometry, first layer and anti-aliasing are recorded as the
    /// document's and the profile's now, not as `params` says.
    pub(crate) fn accept_recipe(
        self: &Rc<Self>,
        plate: u32,
        completed: CompletedSlice,
        mut params: SliceParams,
    ) {
        self.slice_scene_document.set_active_plate(plate);
        params.geometry = self.slice_scene_document.revision();
        params.first_layer_height = self.slice_scene_document.chosen_first_layer();
        params.aa_level = completed.profile.aa_level();
        *self.pending_slice_inputs.borrow_mut() = Some(params);
        self.deposit(Ok(SliceWorkerOutcome::Completed(Box::new(completed))));
        self.finish();
    }

    fn deposit(&self, result: Result<SliceWorkerOutcome, SliceFailure>) {
        *self.worker_result.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
    }

    fn slicing(&self) -> bool {
        self.window_handle
            .upgrade()
            .is_some_and(|ui| ui.get_slicing())
    }

    fn start_all(self: &Rc<Self>) {
        if self.slicing() {
            return;
        }
        let plates = self
            .slice_scene_document
            .read_scene(|s| s.graph().plates().order.iter().map(|p| p.id).collect());
        *self.queue.borrow_mut() = plates.unwrap_or_default();
        self.all_running.set(true);
        self.next_plate();
    }

    /// Slices the next queued plate, or, with none left, ends Slice all
    /// and starts the export waiting on it.
    fn next_plate(self: &Rc<Self>) {
        let next = self.queue.borrow_mut().pop_front();
        let ui = self.window_handle.upgrade();
        if let Some(plate) = next {
            self.slice_scene_document.set_active_plate(plate);
            if let Some(ui) = ui {
                ui.set_plate_action_message(tr!("Slicing plate {}", plate));
            }
            return self.begin_slice_request();
        }
        self.all_running.set(false);
        if let Some(ui) = ui {
            ui.set_plate_action_message("".into());
            if ui.get_batch_export_pending() {
                ui.set_batch_export_pending(false);
                ui.invoke_export_all_requested();
            }
        }
    }

    /// A Slice press. With a printer and resin chosen (the scene's, else
    /// the app's defaults, as the Slice panel shows them), slices when
    /// there is no slice yet or it is stale, asks first when the inputs
    /// changed, and only shows "already up to date" when nothing did. Does
    /// nothing while a slice runs.
    fn begin_slice_request(self: &Rc<Self>) {
        let Some(ui) = self.window_handle.upgrade() else {
            return;
        };
        if ui.get_slicing() {
            return;
        }
        let defaults = self.slice_preferences_bus.current_values();
        let printer = self
            .slice_scene_document
            .chosen_printer()
            .or(defaults.default_printer);
        let resin = self.slice_scene_document.chosen_resin().or(defaults.default_resin);
        let (Some(printer), Some(resin)) = (printer, resin) else {
            let message = "Choose a printer and a resin before slicing.".to_string();
            return self
                .slice_dialog_manager
                .show_failure("Cannot slice", None, message, String::new());
        };
        let current = SliceParams {
            printer,
            resin,
            first_layer_height: self.slice_scene_document.chosen_first_layer(),
            aa_level: self.slice_scene_document.scene_aa.get().unwrap_or(defaults.aa_level),
            geometry: self.slice_scene_document.revision(),
        };
        let stale = matches!(*self.slice_scene_document.slice_state.borrow(), SliceState::Stale);
        if stale || ui.get_slice_estimates_stale() {
            return self.launch_slice_worker(current);
        }
        let held = self.slice_scene_document.slicing_inputs();
        let moved = held
            .as_ref()
            .is_some_and(|held| self.slice_scene_document.slice_outdated_by_geometry(held.geometry));
        match SliceAgain::compare(held.as_ref(), &current, moved) {
            SliceAgain::FirstSlice => self.launch_slice_worker(current),
            SliceAgain::Unchanged if self.all_running.get() => self.next_plate(),
            SliceAgain::Unchanged => self.show_current_slice_hint(),
            SliceAgain::Changed if self.all_running.get() => self.launch_slice_worker(current),
            SliceAgain::Changed => {
                let run = Rc::clone(self);
                self.slice_dialog_manager
                    .ask_before_reslice(move || run.launch_slice_worker(current));
            }
        }
    }

    /// Starts the worker on a copy of the active plate. The copy and the
    /// geometry revision recorded with the result are taken together, so
    /// the revision is the one sliced.
    fn launch_slice_worker(self: &Rc<Self>, mut current: SliceParams) {
        let Some(ui) = self.window_handle.upgrade() else {
            return;
        };
        let plate = self.slice_scene_document.active_plate();
        let Some(scene) = self
            .slice_scene_document
            .read_scene(|s| s.graph().plate_scene(plate).ok())
            .flatten()
        else {
            return;
        };
        let (ordinal, count) = self
            .slice_scene_document
            .read_scene(|s| {
                let order = &s.graph().plates().order;
                (
                    order.iter().position(|p| p.id == plate).unwrap_or(0) + 1,
                    order.len(),
                )
            })
            .unwrap_or((1, 1));
        ui.set_slice_plate_label(tr!("Slicing plate {} of {}", ordinal, count));
        self.hide_current_slice_hint();
        current.geometry = self.slice_scene_document.revision();

        self.layer_progress.reset();
        ui.set_layers_done(0);
        ui.set_layers_planned(0);
        ui.set_slice_fraction(0.0);
        self.slice_scene_document.slice_state.borrow_mut().start();
        self.slice_scene_document.slice_state.borrow().apply(&ui);

        let cancel = CancelToken::new();
        *self.cancellation_token.borrow_mut() = Some(cancel.clone());
        *self.pending_slice_inputs.borrow_mut() = Some(current.clone());
        self.start_progress_updates();
        self.spawn(scene, current, cancel);
    }

    /// Slices on a new thread, leaves the result (a panic as a failure) in
    /// the slot and wakes the UI thread.
    fn spawn(&self, scene: Scene, params: SliceParams, cancel: CancelToken) {
        let (slot, progress, ui) = (self.worker_result.clone(), self.layer_progress.clone(), self.window_handle.clone());
        let overrides = *self.slice_scene_document.process_overrides.borrow();
        let minimum_feature_mm = self.window_handle.upgrade().map_or(DEFAULT_MINIMUM_FEATURE_MM, |ui| {
            minimum_feature_mm(&ui.get_minimum_feature_mm())
        });
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_slice_with_overrides(
                    &scene,
                    &params.printer,
                    &params.resin,
                    params.first_layer_height,
                    params.aa_level,
                    &cancel,
                    &progress,
                    &overrides,
                    minimum_feature_mm,
                )
            }))
            .unwrap_or_else(|payload| Err(SliceFailure::plain(Reason::Panic(panic_text(&payload)))));
            *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
            let _ = ui.upgrade_in_event_loop(|ui| ui.invoke_slice_done());
        });
    }

    /// Copies the worker's counts into the progress bar until the run ends
    /// or the window goes.
    fn start_progress_updates(&self) {
        let (progress, document, ui) = (
            self.layer_progress.clone(),
            self.slice_scene_document.clone(),
            self.window_handle.clone(),
        );
        let timer = Rc::downgrade(&self.slice_progress_timer);
        self.slice_progress_timer.start(
            slint::TimerMode::Repeated,
            SLICE_PROGRESS_REFRESH_INTERVAL,
            move || {
                let Some(ui) = ui.upgrade().filter(|ui| ui.get_slicing()) else {
                    if let Some(timer) = timer.upgrade() {
                        timer.stop();
                    }
                    return;
                };
                let (completed, total) = progress.read();
                ui.set_layers_done(completed as i32);
                ui.set_layers_planned(total as i32);
                let fraction = if total == 0 {
                    0.0
                } else {
                    completed as f32 / total as f32
                };
                document.slice_state.borrow_mut().progress(fraction);
                document.slice_state.borrow().apply(&ui);
            },
        );
    }

    fn cancel_slice_run(&self) {
        if let Some(token) = self.cancellation_token.borrow().as_ref() {
            token.cancel();
        }
    }

    /// Shows "already up to date" for the full delay from this press.
    fn show_current_slice_hint(&self) {
        let Some(ui) = self.window_handle.upgrade() else {
            return;
        };
        ui.set_slice_current(true);
        let ui = self.window_handle.clone();
        self.current_hint_timer.start(
            slint::TimerMode::SingleShot,
            SLICE_CURRENT_HINT_DURATION,
            move || {
                if let Some(ui) = ui.upgrade() {
                    ui.set_slice_current(false);
                }
            },
        );
    }

    fn hide_current_slice_hint(&self) {
        self.current_hint_timer.stop();
        if let Some(ui) = self.window_handle.upgrade() {
            ui.set_slice_current(false);
        }
    }

    /// Takes the worker's result on the UI thread: shows a finished slice,
    /// drops a cancelled one, reports a failure. Then Slice all goes on.
    fn finish(self: &Rc<Self>) {
        let taken = self
            .worker_result
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        *self.cancellation_token.borrow_mut() = None;
        let params = self.pending_slice_inputs.borrow_mut().take();
        let Some(result) = taken else {
            return;
        };
        match result {
            Ok(SliceWorkerOutcome::Completed(completed)) => {
                if let Some(params) = params {
                    self.land(*completed, params);
                }
            }
            Ok(SliceWorkerOutcome::Aborted) => {
                self.stop_all();
                let has_result = self.slice_scene_document.layer_total().is_some();
                self.slice_scene_document.slice_state.borrow_mut().cancel(has_result);
                self.apply_state();
            }
            Err(err) => {
                self.stop_all();
                self.slice_scene_document
                    .slice_state
                    .borrow_mut()
                    .finish(Some(err.to_string()));
                self.apply_state();
                self.slice_dialog_manager
                    .show_failure("Slicing failed", None, err.to_string(), String::new());
            }
        }
        if self.all_running.get() {
            self.next_plate();
        }
    }

    fn stop_all(&self) {
        self.queue.borrow_mut().clear();
        self.all_running.set(false);
    }

    fn apply_state(&self) {
        if let Some(ui) = self.window_handle.upgrade() {
            self.slice_scene_document.slice_state.borrow().apply(&ui);
        }
    }

    /// Makes a finished slice the document's and shows it: layer areas,
    /// risks, estimates, the layer slider on the top layer with content,
    /// and the Slice section.
    fn land(&self, completed: CompletedSlice, params: SliceParams) {
        let layer_count = completed.finished_layer_schedule.total_layers();
        let volume_mm3 = completed.volume_mm3;
        // The estimates use the printer and resin this run sliced with:
        // the document's own choice is `None` while it uses the defaults.
        let (printer, resin) = (params.printer.clone(), params.resin.clone());
        let areas = self.window_handle.upgrade().map(|ui| {
            let areas = layer_areas(&completed.merged_contours);
            ui.set_layer_areas(ModelRc::new(VecModel::from(areas.clone())));
            crate::print_risks::publish(&ui, &completed.risks, &completed.profile);
            let islands: Vec<i32> = completed
                .island_layers
                .iter()
                .map(|&layer| layer as i32)
                .collect();
            ui.set_island_layers(ModelRc::new(VecModel::from(islands)));
            self.show_estimates(&ui, &printer, &resin, &completed);
            areas
        });
        let (tolerant_leaves, dropped_fragments) =
            (completed.tolerant_leaves, completed.dropped_fragments);
        self.slice_scene_document.keep_slice(SliceResult {
            merged: completed.merged_contours,
            schedule: completed.finished_layer_schedule,
            token: completed.input_fingerprint,
            params,
            profile: completed.profile,
        });
        let (Some(ui), Some(areas)) = (self.window_handle.upgrade(), areas) else {
            return;
        };
        ui.set_layer_total(layer_count as i32);
        ui.set_layer_shown(top_layer_with_content(&areas, layer_count as usize) as f32);
        // The slice just recorded is the live geometry's.
        self.slice_scene_document.slice_state.borrow_mut().finish(None);
        self.slice_scene_document.slice_state.borrow().apply(&ui);
        ui.set_slice_recovery_message(if tolerant_leaves > 0 {
            tr!(
                "Tolerant slicing: {} objects, {} dropped fragments",
                tolerant_leaves,
                dropped_fragments
            )
        } else {
            "".into()
        });
        ui.invoke_section_picked(crate::WorkspaceSection::Slice);
        let currency = self.slice_preferences_bus.current_values().currency_symbol;
        let cost =
            estimate_cost_text(&resin, volume_mm3, &currency).filter(|text| !text.is_empty());
        ui.set_estimated_resin_volume_text(format_volume_ml(volume_mm3).into());
        ui.set_estimated_resin_cost_text(cost.map_or_else(|| tr!("Not priced"), Into::into));
        ui.invoke_layer_picked();
    }

    /// Weight, exposures and print time, from the profiles resolved with
    /// the scene's overrides; left as they were if the profiles fail.
    fn show_estimates(
        &self,
        ui: &MainWindow,
        printer: &PrinterId,
        resin: &MaterialId,
        completed: &CompletedSlice,
    ) {
        let Ok((printer_profile, material)) = load_chosen_profiles(printer, resin) else {
            return;
        };
        let Ok(config) = resolve(
            &printer_profile,
            &material,
            &self.slice_scene_document.process_overrides.borrow(),
        ) else {
            return;
        };
        let plate = self.slice_scene_document.active_plate();
        let regions = self
            .slice_scene_document
            .read_scene(|s| {
                let scene = s.graph().plate_scene(plate).ok()?;
                sc_print_glue::calibration::regions(&scene).ok()
            })
            .flatten()
            .unwrap_or_default();
        let resolved = &config.material_resolved;
        let (schedule, _) =
            sc_print_glue::calibration::schedule(resolved, &completed.finished_layer_schedule, &regions);
        let defaults = material.defaults();
        let estimate = sc_print_job::PrintEstimate::new(
            &schedule,
            completed.volume_mm3 / 1000.0,
            defaults.density_g_ml,
            defaults.price_per_liter.map(|p| p.value()),
        );
        ui.set_estimated_weight_text(format!("{:.1} g", estimate.weight_g).into());
        ui.set_exposure_summary(if regions.is_empty() {
            tr!(
                "Bottom       {} x {} s\nTransition  {}\nNormal       {} s",
                resolved.bottom_layer_count,
                resolved.bottom_exposure_sec.value(),
                resolved.transition_layers,
                resolved.normal_exposure_sec.value()
            )
        } else {
            let exposures: Vec<String> = regions.iter().map(|r| format!("{:.2}", r.exposure_s)).collect();
            tr!(
                "Exposure test: {} masks per layer\nNormal exposures: {} s\nOne motion cycle per layer",
                regions.len(),
                exposures.join(" / ")
            )
        });
        // A printer with a time calibration (Device tab) uses its fitted
        // model.
        let calibrated = crate::estimate_calibration::for_printer(printer.as_str()).samples > 0;
        let seconds = if calibrated {
            crate::hub::estimate_seconds(printer, &schedule)
        } else {
            estimate.total_time_s
        };
        ui.set_estimated_print_time_text(format_duration(seconds).into());
    }
}

/// Each layer's exposed area as a share of the largest (of 1 mm² at
/// least, so a near-empty slice is not blown up).
fn layer_areas(stack: &ContourStack) -> Vec<f32> {
    let areas: Vec<f32> = stack
        .layers()
        .iter()
        .map(|layer| {
            let area = |p: &sc_sl1cer::contour::Polygon| {
                p.outer().signed_area() + p.holes().iter().map(|h| h.signed_area()).sum::<f64>()
            };
            layer.polygons().iter().map(area).sum::<f64>() as f32
        })
        .collect();
    let max = areas.iter().copied().fold(0.0f32, f32::max).max(1.0);
    areas.into_iter().map(|area| area / max).collect()
}

/// The minimum feature field's value, if it is a positive number.
fn minimum_feature_mm(text: &str) -> f64 {
    text.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(DEFAULT_MINIMUM_FEATURE_MM)
}

/// What `volume_mm3` of `resin` costs, after `currency_symbol`; `None` when
/// the resin does not load or has no price. Only the resin is loaded: the
/// printer has nothing to do with the cost.
fn estimate_cost_text(
    resin: &MaterialId,
    volume_mm3: f64,
    currency_symbol: &str,
) -> Option<String> {
    let (material, issues) = ProfileLibraries::platform_default()
        .effective_material_by_id(resin)
        .ok()?;
    for issue in &issues {
        eprintln!("warning: {issue}");
    }
    let price = material.defaults().price_per_liter?;
    Some(format_cost(
        estimated_cost(price, volume_mm3),
        currency_symbol,
    ))
}

#[allow(clippy::too_many_arguments)]
fn run_slice_with_overrides(
    scene: &Scene,
    printer: &PrinterId,
    resin: &MaterialId,
    first_layer_height: Option<PositiveFiniteMm>,
    aa_level: AaLevel,
    cancel: &CancelToken,
    progress: &SliceLayerProgress,
    overrides: &SceneOverrides,
    minimum_feature_mm: f64,
) -> Result<SliceWorkerOutcome, SliceFailure> {
    let (printer_profile, resin_profile) = load_chosen_profiles(printer, resin)?;
    let repaired = repair_for_slice(scene);
    let params = SlicingParams {
        process_overrides: *overrides,
        layer_height_request: overrides.layer_height_mm,
        first_layer_height,
        aa_level,
    };
    let run = sc_print_glue::PlateJob::new(&repaired, &printer_profile, &resin_profile, &params).slice(cancel, |p: SliceProgress| progress.record(p), true)
    .map_err(SliceFailure::from)?;
    let plate = match run {
        sc_print_glue::SliceRun::Cancelled => return Ok(SliceWorkerOutcome::Aborted),
        sc_print_glue::SliceRun::Completed(plate) => plate,
    };
    // Risks are placed on the repaired parts that were sliced; the
    // fingerprint is the document's own scene.
    match complete(&repaired, scene, plate, minimum_feature_mm) {
        Ok(completed) => Ok(SliceWorkerOutcome::Completed(Box::new(completed))),
        Err(Incomplete::Placement(_) | Incomplete::Merge(_)) => Err(SliceFailure::plain(Reason::LayerGrids)),
        Err(Incomplete::Capture(failure)) => Err(failure),
    }
}

/// A recipe's sliced plate made ready to show, as a run's would be.
pub(crate) fn capture_recipe(
    scene: &Scene,
    plate: SlicedPlate,
    minimum: f64,
) -> Result<CompletedSlice, String> {
    complete(scene, scene, plate, minimum).map_err(|incomplete| match incomplete {
        Incomplete::Placement(message) | Incomplete::Merge(message) => message,
        Incomplete::Capture(failure) => failure.to_string(),
    })
}

/// Why a sliced plate could not be completed.
enum Incomplete {
    /// The placement risks failed; the error's message.
    Placement(String),
    /// The parts' layers did not merge; the error, debug-printed.
    Merge(String),
    Capture(SliceFailure),
}

/// Merges a sliced plate and analyses it: the placement risks on
/// `sliced_scene`, the fingerprint of `document_scene`, and the plate's
/// tolerant-slicing counts.
fn complete(
    sliced_scene: &Scene,
    document_scene: &Scene,
    plate: SlicedPlate,
    minimum_feature_mm: f64,
) -> Result<CompletedSlice, Incomplete> {
    let profile = plate.profile().clone();
    let schedule = *plate.schedule();
    let placement = sc_print_glue::scene_placement_risks(sliced_scene, plate.leaves(), &profile)
        .map_err(|err| Incomplete::Placement(err.to_string()))?;
    let tolerant_leaves = plate.leaves().iter().filter(|leaf| leaf.tolerant()).count();
    let dropped_fragments = plate
        .leaves()
        .iter()
        .map(|leaf| leaf.dropped_fragments())
        .sum();
    let merged = plate
        .merge()
        .map_err(|err| Incomplete::Merge(format!("{err:?}")))?;
    let mut completed = capture_with_minimum(
        document_scene,
        schedule,
        merged,
        profile,
        minimum_feature_mm,
    )
    .map_err(Incomplete::Capture)?;
    completed.risks.extend(placement);
    completed
        .risks
        .sort_by_key(|risk| (risk.kind, risk.first_layer));
    completed.tolerant_leaves = tolerant_leaves;
    completed.dropped_fragments = dropped_fragments;
    Ok(completed)
}

/// A repaired copy for the worker; the document's meshes, and so its
/// fingerprint, stay as they are.
fn repair_for_slice(scene: &Scene) -> Scene {
    let mut repaired = scene.clone();
    repaired.repair_meshes(sc_mesh_core::indexed_mesh::RepairPasses::default_repair());
    repaired
}

#[cfg(test)]
fn capture_completed_slice(
    scene: &Scene,
    schedule: LayerSchedule,
    merged: ContourStack,
    profile: SliceProfile,
) -> Result<CompletedSlice, SliceFailure> {
    capture_with_minimum(scene, schedule, merged, profile, DEFAULT_MINIMUM_FEATURE_MM)
}

/// Fingerprints the inputs and measures the merged stack (volume, islands
/// and risks in one rasterizing pass), off the UI thread.
fn capture_with_minimum(
    scene: &Scene,
    schedule: LayerSchedule,
    merged: ContourStack,
    profile: SliceProfile,
    minimum_feature_mm: f64,
) -> Result<CompletedSlice, SliceFailure> {
    let token = slice_input_fingerprint(scene, &schedule)
        .map_err(|e| SliceFailure::because(Reason::InputKey, e))?
        .to_token();
    let settings = sc_print_job::RiskSettings {
        minimum_feature_mm,
        ..Default::default()
    };
    let analysis = sc_print_job::analyze_layers_with_settings(&merged, &profile, settings);
    Ok(CompletedSlice {
        merged_contours: merged,
        finished_layer_schedule: schedule,
        input_fingerprint: token,
        profile,
        volume_mm3: analysis.volume_ml * 1000.0,
        tolerant_leaves: 0,
        dropped_fragments: 0,
        island_layers: analysis.islands.iter().map(|island| island.layer).collect(),
        risks: analysis.risks,
    })
}

/// The layer Preview opens on: the highest one with any exposed area. The
/// very last layer is often a near-empty sliver at the model's top, which
/// left the 2D view blank.
pub(crate) fn top_layer_with_content(normalized_areas: &[f32], layer_count: usize) -> usize {
    normalized_areas
        .iter()
        .rposition(|&a| a > 1e-4)
        .filter(|&i| i < layer_count)
        .unwrap_or_else(|| layer_count.saturating_sub(1))
}

#[cfg(test)]
#[path = "tests_slice_run.rs"]
mod tests;

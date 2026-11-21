// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
#[allow(unused_imports)]
use crate::panel_place::Panel;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Instant;

use i_slint_backend_testing::mock_elapsed_time;
use slint::Model as _;
use sc_mesh_core::transform::Transform3D;
use sc_mesh_fixtures::unit_cube_mesh;
use sc_scene::node::LeafPayload;
use sc_sl1cer::contour::{ClosedRing, ContourLayer, Polygon};
use sc_sl1cer::geom2d::Point2D;
use sc_sl1cer::profile::{BuildVolume, Mirror, PackingMode, PixelResolution};

use super::*;
use crate::build_volume::FALLBACK_VOLUME;
use crate::language::LanguagePreference;
use crate::preferences_bus::UserPreferences;
use crate::{MenuLayout, ThemePreference};

const PRINTER: &str = "sleptic-diy-msla";
const RESIN: &str = "standard-405";

fn cube_scene() -> Scene {
    let mut scene = Scene::new();
    let leaf = scene
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(leaf).unwrap();
    scene
}

fn two_layers() -> LayerSchedule {
    LayerSchedule::uniform(0.05, 0.05, 2).unwrap()
}

fn profile() -> SliceProfile {
    SliceProfile::new(
        BuildVolume::new(60.0, 60.0, 100.0).unwrap(),
        PixelResolution::new(1920, 1080).unwrap(),
        two_layers(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    )
}

/// A solid triangle layer under an empty one.
fn triangle_stack() -> ContourStack {
    let ring = ClosedRing::new(
        [(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)]
            .map(|(x, y)| Point2D::new(x, y))
            .to_vec(),
    );
    let solid = ContourLayer::new(0.025, vec![Polygon::new(ring, vec![]).unwrap()]);
    ContourStack::new(vec![solid, ContourLayer::new(0.075, vec![])]).unwrap()
}

fn token(scene: &Scene) -> String {
    slice_input_fingerprint(scene, &two_layers())
        .unwrap()
        .to_token()
}

#[test]
fn a_scene_layer_height_sets_the_layers_sliced() {
    let mut scene = cube_scene();
    scene.drop_at([0.0, 0.0], 0.0).unwrap();
    let overrides = SceneOverrides {
        layer_height_mm: Some(PositiveFiniteMm::new(0.1).unwrap()),
        ..Default::default()
    };
    let outcome = run_slice_with_overrides(
        &scene,
        &PrinterId::new(PRINTER).unwrap(),
        &MaterialId::new(RESIN).unwrap(),
        None,
        AaLevel::Off,
        &CancelToken::new(),
        &SliceLayerProgress::default(),
        &overrides,
        0.3,
    );
    let Ok(SliceWorkerOutcome::Completed(done)) = outcome else {
        panic!("the cube slices")
    };
    assert_eq!(done.finished_layer_schedule.total_layers(), 10);
    assert!((done.finished_layer_schedule.iter_slab_tops().next().unwrap() - 0.1).abs() < 1e-8);
}

#[test]
fn a_capture_keeps_the_stack_and_the_inputs_fingerprint() {
    let scene = cube_scene();
    let first = capture_completed_slice(&scene, two_layers(), triangle_stack(), profile()).unwrap();
    assert_eq!(
        (first.merged_contours.layers().len(), first.finished_layer_schedule),
        (2, two_layers())
    );
    assert_eq!(first.input_fingerprint, token(&scene));
    assert_eq!(
        capture_completed_slice(&scene, two_layers(), triangle_stack(), profile())
            .unwrap()
            .input_fingerprint,
        first.input_fingerprint
    );
    // An empty scene has nothing to fingerprint.
    let empty = capture_completed_slice(&Scene::new(), two_layers(), triangle_stack(), profile());
    assert!(matches!(&empty, Err(f) if *f.reason() == Reason::InputKey));
}

#[test]
fn a_broken_mesh_is_repaired_for_slicing_only() {
    let mut mesh = unit_cube_mesh();
    // A face using one corner twice.
    mesh.faces
        .push(sc_mesh_core::IndexedTriangle::from_raw_indices(
            sc_mesh_core::Normal::new([0.0, 0.0, 1.0]),
            [0, 0, 1],
        ));
    let mut scene = Scene::new();
    let leaf = scene
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(mesh)),
        )
        .unwrap();
    scene.add_root(leaf).unwrap();
    let before = token(&scene);
    let repaired = repair_for_slice(&scene);
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/profiles");
    let printer = sc_print_profiles::load_printer_profile_from_path(
        &assets.join(format!("printers/{PRINTER}.toml")),
    )
    .unwrap();
    let material = sc_print_profiles::EffectiveMaterial::from(
        sc_print_profiles::load_material_profile_from_path(
            &assets.join(format!("materials/{RESIN}.toml")),
        )
        .unwrap(),
    );
    let params = SlicingParams {
        process_overrides: Default::default(),
        layer_height_request: None,
        first_layer_height: None,
        aa_level: AaLevel::Off,
    };
    let slice = |scene: &Scene| {
        sc_print_glue::PlateJob::new(scene, &printer, &material, &params).slice(&CancelToken::new(), |_| {}, false)
    };
    assert!(slice(&scene).is_err());
    assert!(matches!(
        slice(&repaired),
        Ok(sc_print_glue::SliceRun::Completed(_))
    ));
    // The document's own copy and its fingerprint are untouched.
    assert_eq!(token(&scene), before);
    let captured = capture_completed_slice(&scene, two_layers(), triangle_stack(), profile()).unwrap();
    assert_eq!(captured.input_fingerprint, before);
    assert!(captured.volume_mm3 > 0.0);
}

/// A window with Slice wired to a document holding a cube on the plate.
struct Fixture {
    ui: MainWindow,
    bus: Rc<PreferenceHub>,
    document: Rc<SceneDocument>,
    dialogs: Rc<MessageWindows>,
    run: Rc<SliceActionDriver>,
}

fn fixture(printer_and_resin: bool) -> Fixture {
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    let (printer, resin) = if printer_and_resin {
        (
            Some(PrinterId::new(PRINTER).unwrap()),
            Some(MaterialId::new(RESIN).unwrap()),
        )
    } else {
        (None, None)
    };
    let bus = PreferenceHub::new(UserPreferences {
        theme: ThemePreference::System,
        menu_layout: MenuLayout::Bar,
        language: LanguagePreference::System,
        currency_symbol: "$".to_owned(),
        orient_preset: 0,
        perspective_view: false,
        default_printer: printer,
        default_resin: resin,
        favourite_printers: BTreeSet::new(),
        favourite_resins: BTreeSet::new(),
        aa_level: AaLevel::Off,
        build_volume: FALLBACK_VOLUME,
        object_list: Panel::ObjectList.start(),
        slice_panel: Panel::SliceControls.start(),
        slice_view: Panel::SliceView.start(),
        last_open_dir: None,
        recent_files: Vec::new(),
        setup_complete: true,
    });
    let mut scene = cube_scene();
    scene.drop_at([0.0, 0.0], 0.0).unwrap();
    let document = SceneDocument::new();
    document.take_mesh_scene(scene, PathBuf::from("/tmp/slice-run-cube.obj"));
    let dialogs = MessageWindows::new();
    let run = SliceActionDriver::attach_slice_actions(&ui, bus.clone(), document.clone(), dialogs.clone());
    Fixture {
        ui,
        bus,
        document,
        dialogs,
        run,
    }
}

impl Fixture {
    fn landed(&self) -> bool {
        self.run.worker_result.lock().unwrap().is_some()
    }

    /// Waits for the worker, then hands its result to the window as the
    /// event loop would.
    fn finish_run(&self) {
        let started = Instant::now();
        while !self.landed() {
            assert!(
                started.elapsed() < Duration::from_secs(120),
                "the slice never finished"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        self.ui.invoke_slice_done();
    }

    fn deposit(&self, result: Result<SliceWorkerOutcome, SliceFailure>) {
        *self.run.worker_result.lock().unwrap() = Some(result);
    }
}

#[test]
fn slicing_needs_a_printer_and_a_resin() {
    let f = fixture(false);
    f.ui.invoke_start_slice();
    assert_eq!(f.dialogs.error_headings(), ["Cannot slice"]);
    assert!(!f.ui.get_slicing());
    assert!(f.run.pending_slice_inputs.borrow().is_none());
}

#[test]
fn a_slice_runs_lands_and_is_then_up_to_date() {
    let f = fixture(true);
    f.ui.invoke_start_slice();
    assert!(f.ui.get_slicing());
    assert_eq!(f.ui.get_slice_plate_label(), "Slicing plate 1 of 1");
    // A second press while it runs does nothing.
    f.ui.invoke_start_slice();
    f.finish_run();
    assert!(!f.ui.get_slicing());
    assert!(f.ui.get_slice_export_ready());
    assert!(f.dialogs.error_headings().is_empty());
    let held = f.document.slicing_inputs().unwrap();
    assert_eq!(
        (held.printer.as_str(), held.resin.as_str()),
        (PRINTER, RESIN)
    );
    let layers = f.ui.get_layer_total();
    assert!(layers > 0);
    assert_eq!(f.document.layer_total(), Some(layers as u32));
    let areas: Vec<f32> = f.ui.get_layer_areas().iter().collect();
    assert_eq!(areas.len(), layers as usize);
    assert_eq!(areas.iter().copied().fold(0.0, f32::max), 1.0);
    assert_eq!(
        f.ui.get_layer_shown(),
        top_layer_with_content(&areas, layers as usize) as f32
    );
    assert!(
        f.ui.get_estimated_resin_volume_text().ends_with("mL"),
        "{}",
        f.ui.get_estimated_resin_volume_text()
    );
    assert!(!f.ui.get_estimated_resin_cost_text().is_empty());
    assert!(f.ui.get_estimated_weight_text().ends_with(" g"));
    assert!(f.ui.get_exposure_summary().starts_with("Bottom"));
    assert!(!f.ui.get_estimated_print_time_text().is_empty());
    assert_eq!(f.ui.get_slice_recovery_message(), "");
    // Nothing changed: no new run, a hint that fades after ten seconds.
    f.ui.invoke_start_slice();
    assert!(!f.ui.get_slicing());
    assert!(f.ui.get_slice_current());
    mock_elapsed_time(SLICE_CURRENT_HINT_DURATION - Duration::from_millis(100));
    assert!(f.ui.get_slice_current());
    mock_elapsed_time(Duration::from_millis(200));
    assert!(!f.ui.get_slice_current());
}

#[test]
fn a_stale_slice_is_redone_and_other_changes_ask_first() {
    let f = fixture(true);
    f.ui.invoke_start_slice();
    f.finish_run();
    // The scene's own settings mark the slice stale: no question.
    let first_layer = Some(PositiveFiniteMm::new(0.1).unwrap());
    f.document.choose_first_layer(first_layer);
    f.ui.invoke_start_slice();
    assert!(f.ui.get_slicing());
    assert!(f.dialogs.overwrite_windows().is_empty());
    f.finish_run();
    assert_eq!(
        f.document.slicing_inputs().unwrap().first_layer_height,
        first_layer
    );
    // Another default resin changes the inputs without staling the slice.
    f.bus.edit_preferences(|prefs| {
        prefs.default_resin = Some(MaterialId::new("elegoo-standard").unwrap());
        crate::preferences_bus::PreferenceEdit::DefaultResin
    });
    f.ui.invoke_start_slice();
    assert!(!f.ui.get_slicing(), "asks first");
    let asks = f.dialogs.overwrite_windows();
    assert_eq!(asks.len(), 1);
    asks[0].invoke_reslice_approved();
    assert!(f.ui.get_slicing());
    f.finish_run();
    assert_eq!(
        f.document.slicing_inputs().unwrap().resin.as_str(),
        "elegoo-standard"
    );
}

#[test]
fn the_poll_shows_the_workers_progress() {
    let f = fixture(true);
    f.ui.invoke_start_slice();
    // Stand-in for the worker's counters, written after launch reset them.
    f.run.layer_progress.finished_layers.store(3, Ordering::Relaxed);
    f.run.layer_progress.scheduled_layers.store(4, Ordering::Relaxed);
    mock_elapsed_time(SLICE_PROGRESS_REFRESH_INTERVAL);
    let shown = (f.ui.get_layers_done(), f.ui.get_layers_planned());
    // The real worker may have moved the counters on since.
    assert!(shown.1 > 0 && shown.0 <= shown.1, "{shown:?}");
    f.finish_run();
}

#[test]
fn cancel_trips_the_running_token() {
    let f = fixture(true);
    let token = CancelToken::new();
    *f.run.cancellation_token.borrow_mut() = Some(token.clone());
    f.ui.invoke_cancel_slice();
    assert!(token.is_cancelled());
    // With nothing running, Cancel does nothing.
    *f.run.cancellation_token.borrow_mut() = None;
    f.ui.invoke_cancel_slice();
}

#[test]
fn a_cancelled_or_failed_run_stops_the_batch() {
    let f = fixture(true);
    for outcome in [
        Ok(SliceWorkerOutcome::Aborted),
        Err(SliceFailure::plain(Reason::NoGeometry)),
    ] {
        let failed = outcome.is_err();
        f.run.queue.borrow_mut().extend([2, 3]);
        f.run.all_running.set(true);
        f.document.slice_state.borrow_mut().start();
        f.deposit(outcome);
        f.ui.invoke_slice_done();
        assert!(f.run.queue.borrow().is_empty() && !f.run.all_running.get());
        assert!(!f.ui.get_slicing());
        assert_eq!(f.ui.get_slice_failed(), failed);
        assert!(f.document.slicing_inputs().is_none());
    }
    assert_eq!(f.dialogs.error_headings(), ["Slicing failed"]);
    // Finishing with nothing deposited does nothing.
    f.ui.invoke_slice_done();
    assert_eq!(f.dialogs.error_headings().len(), 1);
}

#[test]
fn slicing_every_plate_ends_with_its_queued_export() {
    let f = fixture(true);
    let exports = Rc::new(Cell::new(0));
    let count = exports.clone();
    f.ui.on_export_all_requested(move || count.set(count.get() + 1));
    f.ui.set_batch_export_pending(true);
    f.ui.invoke_slice_all_requested();
    assert!(f.run.all_running.get());
    assert_eq!(f.ui.get_plate_action_message(), "Slicing plate 1");
    assert!(f.ui.get_slicing());
    f.finish_run();
    assert!(!f.run.all_running.get());
    assert_eq!(f.ui.get_plate_action_message(), "");
    assert_eq!(exports.get(), 1);
    assert!(!f.ui.get_batch_export_pending());
    // Again with nothing changed: every plate is up to date, export again.
    f.ui.set_batch_export_pending(true);
    f.ui.invoke_slice_all_requested();
    assert!(!f.ui.get_slicing() && !f.ui.get_slice_current());
    assert_eq!(exports.get(), 2);
}

#[test]
fn a_recipe_slice_is_taken_as_the_plates_slice() {
    let f = fixture(true);
    let scene = f.document.read_scene(|s| s.graph().clone()).unwrap();
    let completed = capture_completed_slice(&scene, two_layers(), triangle_stack(), profile()).unwrap();
    let params = SliceParams {
        printer: PrinterId::new(PRINTER).unwrap(),
        resin: MaterialId::new(RESIN).unwrap(),
        first_layer_height: Some(PositiveFiniteMm::new(0.2).unwrap()),
        aa_level: AaLevel::X4,
        geometry: f.document.revision(),
    };
    f.run.accept_recipe(1, completed, params);
    let held = f.document.slicing_inputs().unwrap();
    // The document's own first layer and the slice's real AA are recorded.
    assert_eq!(
        held.first_layer_height,
        f.document.chosen_first_layer()
    );
    assert_eq!(held.aa_level, AaLevel::Off);
    assert_eq!(held.geometry, f.document.revision());
    assert_eq!(f.ui.get_layer_total(), 2);
    assert_eq!(f.ui.get_layer_shown(), 0.0);
}

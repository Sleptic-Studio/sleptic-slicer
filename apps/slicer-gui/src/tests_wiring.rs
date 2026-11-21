// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
#[allow(unused_imports)]
use crate::panel_place::Panel;
use std::cell::Cell;
use std::collections::BTreeSet;
use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use sc_mesh_core::transform::Transform3D;
use sc_mesh_fixtures::unit_cube_mesh;
use sc_mesh_formats::filetypes::WriteFormat;
use sc_print_primitives::AaLevel;
use sc_scene::Scene;
use sc_scene::node::LeafPayload;
use slint::{ModelRc, VecModel};

use super::*;
use crate::build_volume::FALLBACK_VOLUME;
use crate::language::LanguagePreference;
use crate::mesh_render::DeviceLoss;
use crate::pending_load::WaitingFile;
use crate::preferences_bus::UserPreferences;
use crate::{MenuLayout, RiskRow, ThemePreference};

fn preferences(orient_preset: i32) -> UserPreferences {
    UserPreferences {
        theme: ThemePreference::System,
        menu_layout: MenuLayout::Bar,
        language: LanguagePreference::System,
        currency_symbol: "$".to_owned(),
        orient_preset,
        perspective_view: false,
        default_printer: None,
        default_resin: None,
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
    }
}

/// A wired window whose document holds a cube opened from `cube.obj`.
struct Wired {
    ui: MainWindow,
    ctrl: Rc<ViewController>,
    prefs: Rc<PreferenceHub>,
}

impl Wired {
    fn doc(&self) -> Rc<SceneDocument> {
        self.ctrl.scene_doc()
    }

    fn plate_ids(&self) -> Vec<u32> {
        self.doc()
            .read_scene(|s| s.graph().plates().order.iter().map(|p| p.id).collect())
            .unwrap()
    }
}

fn wired(orient_preset: i32) -> Wired {
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    let mut scene = Scene::new();
    let leaf = scene
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(leaf).unwrap();
    let document = SceneDocument::new();
    document.take_mesh_scene(scene, PathBuf::from("/models/cube.obj"));
    let ctrl = Rc::new(ViewController::new(
        &ui,
        document,
        Rc::new(WaitingFile::new()),
        DeviceLoss::default(),
    ));
    let prefs = PreferenceHub::new(preferences(orient_preset));
    // The testing backend has no renderer to notify; everything else is
    // wired before that last step.
    let _ = connect_main_window_actions(&ui, ctrl.clone(), prefs.clone());
    Wired { ui, ctrl, prefs }
}

/// Runs a flow that finishes without waiting on a dialog or a worker.
fn run_now<F: Future>(flow: F) -> F::Output {
    let mut flow = std::pin::pin!(flow);
    match flow.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("the flow waited"),
    }
}

#[test]
fn plates_move_one_place_and_stop_at_the_ends() {
    let w = wired(0);
    let doc = w.doc();
    doc.add_plate();
    doc.add_plate();
    doc.set_active_plate(2);
    w.ui.invoke_reorder_plate_requested(1);
    assert_eq!(w.plate_ids(), [1, 3, 2]);
    w.ui.invoke_reorder_plate_requested(1);
    assert_eq!(w.plate_ids(), [1, 3, 2]);
    // A long step goes only as far as the first place.
    w.ui.invoke_reorder_plate_requested(-5);
    assert_eq!(w.plate_ids(), [2, 3, 1]);
}

#[test]
fn a_plate_tile_action_works_on_that_plate() {
    let w = wired(0);
    let doc = w.doc();
    doc.add_plate();
    w.ui.invoke_plate_action(2, 26);
    assert_eq!(doc.active_plate(), 2);
    let locked = doc.read_scene(|s| {
        s.graph()
            .plates()
            .order
            .iter()
            .map(|p| p.locked)
            .collect::<Vec<_>>()
    });
    assert_eq!(locked.unwrap(), [false, true]);
    assert!(!w.ui.get_plate_editor_open());
    w.ui.invoke_plate_action(1, 22);
    assert_eq!(doc.active_plate(), 1);
    assert!(w.ui.get_plate_editor_open());
    assert_eq!(
        w.ui.get_plate_display_name(),
        doc.plate_name().unwrap_or_default()
    );
}

#[test]
fn the_projection_and_orient_choices_are_remembered() {
    let w = wired(1);
    let weights = sc_print_profiles::orient::Preset::FewestSupports.weights();
    let shown = [
        w.ui.get_orient_support_weight(),
        w.ui.get_orient_height_weight(),
        w.ui.get_orient_quality_weight(),
        w.ui.get_orient_footprint_weight(),
    ];
    assert_eq!(shown, weights.map(|weight| (weight * 25.0) as f32));
    assert_eq!(w.ui.get_orient_preset(), 1);
    w.ui.invoke_orient_preset_changed(3);
    assert_eq!(w.prefs.current_values().orient_preset, 3);
    assert!(!w.ui.get_perspective_view());
    w.ui.invoke_projection_toggled(true);
    assert!(w.prefs.current_values().perspective_view);
}

#[test]
fn an_unknown_orient_preset_shows_the_default_weights() {
    let w = wired(9);
    let weights = sc_print_profiles::orient::Preset::default().weights();
    assert_eq!(w.ui.get_orient_support_weight(), (weights[0] * 25.0) as f32);
}

#[test]
fn risk_rows_jump_to_their_layer_and_offer_their_fix() {
    let w = wired(0);
    let supports_asked = Rc::new(Cell::new(0));
    let asked = supports_asked.clone();
    w.ui.on_auto_supports_requested(move || asked.set(asked.get() + 1));
    let row = RiskRow {
        layer: 7,
        kind: 2,
        action: 1,
        ..Default::default()
    };
    w.ui.set_print_risks(ModelRc::new(VecModel::from(vec![row])));
    w.ui.set_preview_3d(true);
    w.ui.invoke_risk_jump_requested(0);
    assert_eq!(
        (w.ui.get_selected_risk(), w.ui.get_expanded_risk_kind()),
        (0, 2)
    );
    assert_eq!(w.ui.get_layer_shown(), 7.0);
    assert!(!w.ui.get_preview_3d());
    // No such row: nothing moves.
    w.ui.invoke_risk_jump_requested(4);
    assert_eq!(w.ui.get_selected_risk(), 0);
    // A stale analysis offers no fix.
    w.ui.set_slice_estimates_stale(true);
    w.ui.invoke_risk_fix_requested(0);
    assert_eq!(supports_asked.get(), 0);
    w.ui.set_slice_estimates_stale(false);
    w.ui.invoke_risk_fix_requested(0);
    assert!(w.ui.get_supports_enabled());
    assert_eq!(supports_asked.get(), 1);
    w.ui.invoke_risk_settings_changed();
    assert!(w.ui.get_slice_estimates_stale());
}

#[test]
fn next_island_steps_up_and_wraps_around() {
    let w = wired(0);
    w.ui.invoke_next_island_requested();
    assert_eq!(w.ui.get_layer_shown(), 0.0, "no islands, no jump");
    w.ui.set_island_layers(ModelRc::new(VecModel::from(vec![5, 2, 5])));
    w.ui.set_layer_shown(3.0);
    for expected in [5.0, 2.0, 5.0] {
        w.ui.invoke_next_island_requested();
        assert_eq!(w.ui.get_layer_shown(), expected);
    }
}

#[test]
fn the_next_island_is_the_lowest_above_or_else_the_lowest() {
    let islands = [463, 103, 121, 121, 357];
    for (current, next) in [
        (0, 103),
        (103, 121),
        (121, 357),
        (357, 463),
        (463, 103),
        (959, 103),
    ] {
        assert_eq!(
            next_island_layer(&islands, current),
            Some(next),
            "after {current}"
        );
    }
    assert_eq!(next_island_layer(&[], 10), None);
}

#[test]
fn opened_files_feed_recent_and_the_open_folder() {
    let w = wired(0);
    let folder = tempfile::tempdir().unwrap();
    let (first, second) = (folder.path().join("a.stl"), folder.path().join("b.stl"));
    // A file that does not exist is not recent.
    remember_recent_file(&w.prefs, &first);
    assert!(w.prefs.current_values().recent_files.is_empty());
    std::fs::write(&first, b"").unwrap();
    std::fs::write(&second, b"").unwrap();
    for path in [&first, &second, &first] {
        remember_recent_file(&w.prefs, path);
    }
    assert_eq!(
        w.prefs.current_values().recent_files,
        [first.clone(), second.clone()]
    );
    let folder_changes = Rc::new(Cell::new(0));
    let count = folder_changes.clone();
    let _listening = w.prefs.watch_preferences(move |_, change| {
        if *change == PreferenceEdit::LastOpenDir {
            count.set(count.get() + 1);
        }
    });
    retain_mesh_parent_directory(&w.prefs, &first);
    retain_mesh_parent_directory(&w.prefs, &second);
    assert_eq!(
        w.prefs.current_values().last_open_dir.as_deref(),
        Some(folder.path())
    );
    assert_eq!(
        folder_changes.get(),
        1,
        "the same folder is not written again"
    );
}

#[test]
fn recent_files_put_the_newest_first_once_and_keep_the_cap() {
    let cap = crate::app_settings::MAX_RECENT_FILES;
    let full: Vec<PathBuf> = (0..cap)
        .map(|i| PathBuf::from(format!("/m/{i}.stl")))
        .collect();
    let pushed = recent_files_with_pushed(full.clone(), "/m/new.stl".into());
    assert_eq!(pushed.len(), cap);
    assert_eq!(pushed[0], PathBuf::from("/m/new.stl"));
    assert_eq!(pushed[1..], full[..cap - 1]);
    let again = recent_files_with_pushed(vec!["/m/a".into(), "/m/b".into()], "/m/b".into());
    assert_eq!(again, [PathBuf::from("/m/b"), PathBuf::from("/m/a")]);
}

#[test]
fn the_open_dialog_starts_in_the_last_folder_that_still_exists() {
    let (last, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let gone = tempfile::tempdir().unwrap().path().to_path_buf();
    let pick = |last: Option<&Path>, home: Option<&Path>| {
        mesh_dialog_start_directory(last.map(Path::to_path_buf), home.map(Path::to_path_buf))
    };
    assert_eq!(
        pick(Some(last.path()), Some(home.path())).as_deref(),
        Some(last.path())
    );
    assert_eq!(
        pick(Some(&gone), Some(home.path())).as_deref(),
        Some(home.path())
    );
    assert_eq!(pick(None, Some(home.path())).as_deref(), Some(home.path()));
    assert_eq!(pick(None, Some(&gone)), None);
    assert_eq!(pick(None, None), None);
}

#[test]
fn an_export_name_picks_its_writer_or_is_refused() {
    assert_eq!(decide_export_dispatch(None), ExportDispatch::Cancelled);
    for refused in ["/out/model.dat", "/out/model", "model.ply", "model.amf"] {
        let path = PathBuf::from(refused);
        assert_eq!(
            decide_export_dispatch(Some(path.clone())),
            ExportDispatch::UnsupportedExtension(path)
        );
    }
    let writers = [
        ("/out/a.stl", WriteFormat::StlBin),
        ("/out/a.stlb", WriteFormat::StlBin),
        ("/out/a.stla", WriteFormat::StlAscii),
        ("/out/a.obj", WriteFormat::Obj),
        ("/out/a.3mf", WriteFormat::ThreeMf),
    ];
    for (name, format) in writers {
        let target = PathBuf::from(name);
        assert_eq!(
            decide_export_dispatch(Some(target.clone())),
            ExportDispatch::Ready { target, format }
        );
    }
}

#[test]
fn only_an_undisturbed_write_counts_as_saved() {
    assert_eq!(
        SaveResult::from_check(SaveCheck::Current),
        SaveResult::Saved
    );
    for moved in [SaveCheck::ChangedSince, SaveCheck::Superseded] {
        assert_eq!(SaveResult::from_check(moved), SaveResult::SavedButChanged);
    }
    // No scene: nothing to save, and no dialog.
    let w = wired(0);
    let empty = SceneDocument::new();
    let saver = ProjectSaver {
        doc: empty,
        dialogs: w.ctrl.dialog_manager(),
        progress: w.ctrl.save_panel(),
        printer: None,
        resin: None,
    };
    let outcome = run_now(saver.save());
    assert_eq!(outcome, SaveResult::NotSaved);
    assert!(w.ctrl.dialog_manager().error_headings().is_empty());
}

#[test]
fn sliced_export_formats_follow_the_menu_order() {
    use sc_sla_formats::Format;
    let menu = [
        (0, Format::GcodeZip),
        (1, Format::PngPerLayer),
        (2, Format::ThreeMf),
        (3, Format::SvgCombined),
        (7, Format::GcodeZip),
    ];
    for (index, format) in menu {
        assert!(
            export_kind_for_format(index).format() == format,
            "menu entry {index}"
        );
    }
}

#[test]
fn a_drive_export_names_the_plate_and_needs_a_slice() {
    let w = wired(0);
    let drive = tempfile::tempdir().unwrap();
    let dialogs = w.ctrl.dialog_manager();
    std::fs::write(drive.path().join("cube_plate1.zip"), b"earlier job").unwrap();
    let kinds = [
        (SlicedExportKind::GcodeZip, "cube_plate1 (2).zip"),
        (SlicedExportKind::ThreeMf, "cube_plate1.3mf"),
        (SlicedExportKind::Svg, "cube_plate1.svg"),
        (SlicedExportKind::PngLayers, "cube_plate1_layers"),
    ];
    for (kind, name) in kinds {
        run_now(run_export_sliced_flow(
            w.doc(),
            dialogs.clone(),
            kind,
            Some(drive.path().to_path_buf()),
        ));
        let (heading, context, message) = dialogs.errors().pop().unwrap();
        assert_eq!(heading, "Failed to export sliced file");
        assert_eq!(
            context,
            format!("File: {}", drive.path().join(name).display())
        );
        assert_eq!(
            message,
            "No slice is held. Slice the plate first, then export it."
        );
    }
}

#[test]
fn a_sliced_export_says_what_was_written() {
    let dialogs = MessageWindows::new();
    let target = Path::new("/out/job.zip");
    show_slice_export_success(&dialogs, target, SlicedExportKind::GcodeZip, 5, None);
    show_slice_export_success(
        &dialogs,
        target,
        SlicedExportKind::Svg,
        2,
        Some(Err("copy failed".into())),
    );
    show_slice_export_success(
        &dialogs,
        target,
        SlicedExportKind::ThreeMf,
        2,
        Some(Ok("sent".into())),
    );
    let mut messages = dialogs.info_messages();
    messages.sort();
    assert!(
        messages
            .iter()
            .all(|(heading, _)| heading == "Sliced file exported")
    );
    let texts: Vec<&str> = messages
        .iter()
        .map(|(_, message)| message.as_str())
        .collect();
    let shown = target.display();
    assert!(
        texts.contains(&format!("Exported 5 layers and printer job script to {shown}.").as_str())
    );
    assert!(
        texts.contains(&format!("Exported the scene and held layers to {shown}.\n\nsent").as_str())
    );
    let svg = texts
        .iter()
        .find(|text| text.starts_with("Exported 2 layer(s) to"))
        .unwrap();
    assert!(
        svg.contains("not a printer-ready file") && svg.ends_with("\n\ncopy failed"),
        "{svg}"
    );
}

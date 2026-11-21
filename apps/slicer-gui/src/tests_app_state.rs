// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Contracts for application routing and preference persistence.
#[allow(unused_imports)]
use crate::panel_place::Panel;
use super::*;
use slint::Model;

fn sample() -> UserPreferences {
    UserPreferences {
        orient_preset: 7,
        perspective_view: true,
        setup_complete: true,
        theme: crate::ThemePreference::Dark,
        menu_layout: crate::MenuLayout::Compact,
        language: crate::language::LanguagePreference::System,
        currency_symbol: "credits".into(),
        default_printer: Some(sc_print_primitives::PrinterId::new("sample-machine").unwrap()),
        default_resin: Some(sc_print_primitives::MaterialId::new("sample-resin").unwrap()),
        favourite_printers: [sc_print_primitives::PrinterId::new("favourite-machine").unwrap()]
            .into_iter()
            .collect(),
        favourite_resins: [sc_print_primitives::MaterialId::new("favourite-resin").unwrap()]
            .into_iter()
            .collect(),
        aa_level: sc_print_primitives::AaLevel::X8,
        build_volume: crate::build_volume::FALLBACK_VOLUME,
        object_list: Panel::ObjectList.start(),
        slice_panel: Panel::SliceControls.start(),
        slice_view: Panel::SliceView.start(),
        last_open_dir: Some(PathBuf::from("fixtures")),
        recent_files: vec![PathBuf::from("fixtures/part.obj")],
    }
}

#[test]
fn file_policy_obeys_precedence_for_every_state() {
    use FileRequest as A;
    let choose = classify_file;
    use crate::error_dialog::SaveChoice;
    for bits in 0..32 {
        let [supported, loaded, dirty, saved, project] =
            std::array::from_fn(|n| bits & (1 << n) != 0);
        let expected = match (supported, loaded, project, dirty, saved) {
            (false, ..) => A::Unsupported,
            (true, false, ..) => A::Load,
            (true, true, false, ..) => A::Append,
            (true, true, true, false, _) => A::Replace,
            (true, true, true, true, saved) => A::Ask(if saved {
                SaveChoice::OverwriteCurrent
            } else {
                SaveChoice::KeepAsCopy
            }),
        };
        assert_eq!(
            choose(supported, loaded, dirty, saved, project),
            expected,
            "state {bits}"
        );
    }
}

#[test]
fn menu_permissions_are_independent() {
    for picked in [true, false] {
        for copied in [true, false] {
            let actions = menu_access(picked, copied);
            assert_eq!(
                (actions.copy, actions.paste, actions.delete),
                (picked, copied, picked)
            );
            assert_eq!(actions.has_entries(), picked || copied);
        }
    }
}

#[test]
fn launch_prefers_live_focus_then_smallest_id() {
    for order in [[8, 3, 5], [5, 8, 3], [3, 5, 8]] {
        assert_eq!(launch_target(Some(8), order.into_iter()), Some(8));
        assert_eq!(launch_target(Some(99), order.into_iter()), Some(3));
        assert_eq!(launch_target(None, order.into_iter()), Some(3));
    }
    assert_eq!(launch_target(Some(8), std::iter::empty()), None);
}

#[test]
fn persistence_transfers_the_complete_stored_snapshot() {
    let input = sample();
    let mut stored = AppSettings::default();
    store_preferences(&mut stored, &input);
    assert_eq!(stored.orient_preset, input.orient_preset, "orient_preset");
    assert_eq!(
        stored.perspective_view, input.perspective_view,
        "perspective_view"
    );
    assert_eq!(
        stored.setup_complete, input.setup_complete,
        "setup_complete"
    );
    assert_eq!(stored.theme() as i32, input.theme as i32);
    assert_eq!(stored.menu_layout() as i32, input.menu_layout as i32);
    assert_eq!(stored.language(), input.language, "language");
    assert_eq!(
        stored.currency_symbol(),
        input.currency_symbol,
        "currency_symbol"
    );
    assert_eq!(
        stored.default_printer(),
        input.default_printer,
        "default_printer"
    );
    assert_eq!(stored.default_resin(), input.default_resin, "default_resin");
    assert_eq!(
        stored.favourite_printers(),
        input.favourite_printers,
        "favourite_printers"
    );
    assert_eq!(
        stored.favourite_resins(),
        input.favourite_resins,
        "favourite_resins"
    );
    assert_eq!(stored.aa_level(), input.aa_level, "aa_level");
    assert_eq!(
        stored.panel(Panel::ObjectList).visible,
        input.object_list.visible,
        "object_list_visible"
    );
    assert_eq!(
        stored.panel(Panel::ObjectList).anchor_right,
        input.object_list.anchor_right,
        "object_list_anchor_right"
    );
    assert_eq!(
        stored.panel(Panel::ObjectList).anchor_bottom,
        input.object_list.anchor_bottom,
        "object_list_anchor_bottom"
    );
    assert_eq!(
        stored.panel(Panel::ObjectList).offset_x,
        input.object_list.offset_x,
        "object_list_offset_x"
    );
    assert_eq!(
        stored.panel(Panel::ObjectList).offset_y,
        input.object_list.offset_y,
        "object_list_offset_y"
    );
    assert_eq!(
        stored.panel(Panel::ObjectList).width,
        input.object_list.width,
        "object_list_width"
    );
    assert_eq!(
        stored.panel(Panel::ObjectList).height,
        input.object_list.height,
        "object_list_height"
    );
    assert_eq!(
        stored.panel(Panel::ObjectList).minimized,
        input.object_list.minimized,
        "object_list_minimized"
    );
    assert_eq!(
        stored.panel(Panel::SliceControls).anchor_right,
        input.slice_panel.anchor_right,
        "slice_panel_anchor_right"
    );
    assert_eq!(
        stored.panel(Panel::SliceControls).anchor_bottom,
        input.slice_panel.anchor_bottom,
        "slice_panel_anchor_bottom"
    );
    assert_eq!(
        stored.panel(Panel::SliceControls).offset_x,
        input.slice_panel.offset_x,
        "slice_panel_offset_x"
    );
    assert_eq!(
        stored.panel(Panel::SliceControls).offset_y,
        input.slice_panel.offset_y,
        "slice_panel_offset_y"
    );
    assert_eq!(
        stored.panel(Panel::SliceControls).width,
        input.slice_panel.width,
        "slice_panel_width"
    );
    assert_eq!(
        stored.panel(Panel::SliceControls).height,
        input.slice_panel.height,
        "slice_panel_height"
    );
    assert_eq!(
        stored.panel(Panel::SliceView).visible,
        input.slice_view.visible,
        "slice_view_visible"
    );
    assert_eq!(
        stored.panel(Panel::SliceView).anchor_right,
        input.slice_view.anchor_right,
        "slice_view_anchor_right"
    );
    assert_eq!(
        stored.panel(Panel::SliceView).anchor_bottom,
        input.slice_view.anchor_bottom,
        "slice_view_anchor_bottom"
    );
    assert_eq!(
        stored.panel(Panel::SliceView).offset_x,
        input.slice_view.offset_x,
        "slice_view_offset_x"
    );
    assert_eq!(
        stored.panel(Panel::SliceView).offset_y,
        input.slice_view.offset_y,
        "slice_view_offset_y"
    );
    assert_eq!(
        stored.panel(Panel::SliceView).width,
        input.slice_view.width,
        "slice_view_width"
    );
    assert_eq!(
        stored.panel(Panel::SliceView).height,
        input.slice_view.height,
        "slice_view_height"
    );
    assert_eq!(
        stored.panel(Panel::SliceView).minimized,
        input.slice_view.minimized,
        "slice_view_minimized"
    );
    assert_eq!(stored.last_open_dir(), input.last_open_dir, "last_open_dir");
    assert_eq!(stored.recent_files(), input.recent_files, "recent_files");
}

#[test]
fn queued_preferences_collapse_to_the_last_value() {
    let (sender, receiver) = mpsc::channel();
    for value in 0..40 {
        let mut next = sample();
        next.orient_preset = value;
        sender.send(next).unwrap();
    }
    drop(sender);
    let mut written = Vec::new();
    consume_preferences(receiver, |value| written.push(value.orient_preset));
    assert_eq!(written, [39]);
}

#[test]
fn worker_accepts_another_wake_after_saving() {
    let (sender, receiver) = mpsc::channel();
    let (ack, acknowledgements) = mpsc::channel();
    let worker = thread::spawn(move || {
        consume_preferences(receiver, |value| ack.send(value.orient_preset).unwrap())
    });
    for value in [2, 9, 4] {
        let mut next = sample();
        next.orient_preset = value;
        sender.send(next).unwrap();
        assert_eq!(
            acknowledgements
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap(),
            value
        );
    }
    drop(sender);
    worker.join().unwrap();
}

#[test]
fn disconnected_worker_never_writes() {
    let (sender, receiver) = mpsc::channel();
    drop(sender);
    consume_preferences(receiver, |_| panic!("unexpected write"));
}

#[test]
fn saver_shutdown_joins_without_a_snapshot() {
    let (sender, lifetime) = start_preferences_worker(AppSettings::default()).unwrap();
    drop(sender);
    drop(lifetime);
}

fn row(header: bool, title: &str) -> crate::onboarding::Row {
    crate::onboarding::Row {
        header,
        title: title.into(),
        subtitle: "details".into(),
        badge: "badge".into(),
        selected: true,
    }
}

#[test]
fn cards_keep_flat_indices_and_leading_ungrouped_items() {
    let groups = setup_groups(
        vec![row(false, "Loose"), row(true, "Brand"), row(false, "Item")],
        vec![None; 3],
    );
    assert_eq!(groups.row_count(), 2);
    let loose = groups.row_data(0).unwrap();
    assert_eq!(loose.title, "");
    assert_eq!(loose.cards.row_data(0).unwrap().index, 0);
    let group = groups.row_data(1).unwrap();
    assert_eq!(group.title, "Brand");
    let card = group.cards.row_data(0).unwrap();
    assert_eq!(card.index, 2);
    assert_eq!(card.title, "Item");
    assert_eq!(card.subtitle, "details");
    assert_eq!(card.badge, "badge");
    assert!(card.selected);
}

#[test]
fn grouping_keeps_empty_headers_and_stops_at_shorter_input() {
    let groups = setup_groups(
        vec![row(true, "A"), row(true, "B"), row(false, "ignored")],
        vec![None; 2],
    );
    assert_eq!(groups.row_count(), 2);
    assert_eq!(groups.row_data(0).unwrap().cards.row_count(), 0);
    assert_eq!(groups.row_data(1).unwrap().cards.row_count(), 0);
    assert_eq!(setup_groups(Vec::new(), Vec::new()).row_count(), 0);
}

// No persistence listener is attached: callback tests must not write user settings.
fn application() -> Rc<AppState> {
    let bus = PreferenceHub::new(sample());
    let (sender, worker) = start_preferences_worker(AppSettings::default()).unwrap();
    drop(sender);
    Rc::new(AppState {
        about_dialog: RefCell::new(None),
        shortcuts_window: RefCell::new(None),
        preferences_window: RefCell::new(None),
        main_windows: RefCell::new(HashMap::new()),
        window_counter: Cell::new(0),
        focused_main_window: Cell::new(None),
        recovery_checked: Cell::new(false),
        setup_subscriptions: RefCell::new(Vec::new()),
        preference_bus: bus.clone(),
        profile_catalog_cell: Rc::new(RefCell::new(Rc::new(InstalledProfiles::scan_libraries(
            &ProfileLibraries::new(None, Vec::new()),
        )))),
        copied_parts: RefCell::new(None),
        dropped_paths: RefCell::new(HashMap::new()),
        gpu_loss: DeviceLoss::default(),
        _settings_feed: bus.watch_preferences(|_, _| {}),
        _translation_listener: bus.watch_preferences(|_, _| {}),
        _settings_saver: worker,
    })
}

#[test]
fn panel_callbacks_normalize_height_and_suppress_duplicate_writes() {
    i_slint_backend_testing::init_no_event_loop();
    let app = application();
    let window = MainWindow::new().unwrap();
    app.connect_application_actions(&window, SceneDocument::new());
    let changes = Rc::new(RefCell::new(Vec::new()));
    let received = changes.clone();
    let _listener = app
        .preference_bus
        .watch_preferences(move |_, event| received.borrow_mut().push(*event));
    for _ in 0..2 {
        window.invoke_parts_panel_moved(false, true, 11., 13., 250., 0., false);
        window.invoke_slice_box_moved(true, false, 17., 19., 260., -5.);
        window.invoke_layer_view_moved(false, false, 23., 29., 270., f32::NAN, true);
    }
    assert_eq!(
        &*changes.borrow(),
        &[
            PreferenceEdit::ObjectListPanel,
            PreferenceEdit::SliceControlPanel,
            PreferenceEdit::SliceViewPanel
        ]
    );
    let saved = app.preference_bus.current_values();
    assert_eq!(
        (
            saved.object_list.offset_x,
            saved.object_list.offset_y,
            saved.object_list.width,
            saved.object_list.height
        ),
        (Some(11.), Some(13.), Some(250.), None)
    );
    assert!(!saved.object_list.anchor_right);
    assert!(saved.object_list.anchor_bottom);
    assert!(!saved.object_list.minimized);
    assert_eq!(
        (
            saved.slice_panel.offset_x,
            saved.slice_panel.offset_y,
            saved.slice_panel.width,
            saved.slice_panel.height
        ),
        (Some(17.), Some(19.), Some(260.), None)
    );
    assert!(saved.slice_panel.anchor_right);
    assert!(!saved.slice_panel.anchor_bottom);
    assert_eq!(
        (
            saved.slice_view.offset_x,
            saved.slice_view.offset_y,
            saved.slice_view.width,
            saved.slice_view.height
        ),
        (Some(23.), Some(29.), Some(270.), None)
    );
    assert!(!saved.slice_view.anchor_right);
    assert!(!saved.slice_view.anchor_bottom);
    assert!(saved.slice_view.minimized);
    window.invoke_layer_view_moved(false, false, 23., 29., 270., 140., true);
    assert_eq!(app.preference_bus.current_values().slice_view.height, Some(140.));
    assert_eq!(changes.borrow().len(), 4);
}

#[test]
fn visibility_and_layout_callbacks_publish_even_repeated_values() {
    i_slint_backend_testing::init_no_event_loop();
    let app = application();
    let window = MainWindow::new().unwrap();
    app.connect_application_actions(&window, SceneDocument::new());
    let count = Rc::new(Cell::new(0));
    let received = count.clone();
    let _listener = app
        .preference_bus
        .watch_preferences(move |_, _| received.set(received.get() + 1));
    for _ in 0..2 {
        window.invoke_menu_style_picked(crate::MenuLayout::Bar);
        window.invoke_parts_panel_toggled(false);
        window.invoke_layer_view_toggled(false);
    }
    let saved = app.preference_bus.current_values();
    assert!(matches!(saved.menu_layout, crate::MenuLayout::Bar));
    assert!(!saved.object_list.visible);
    assert!(!saved.slice_view.visible);
    assert_eq!(count.get(), 6);
}

#[test]
fn setup_theme_previews_and_skip_finishes_without_a_profile_choice() {
    i_slint_backend_testing::init_no_event_loop();
    let app = application();
    app.preference_bus.edit_preferences(|snapshot| {
        snapshot.setup_complete = false;
        PreferenceEdit::Setup
    });
    assert!(app.needs_setup());
    let window = MainWindow::new().unwrap();
    app.connect_application_actions(&window, SceneDocument::new());
    window.invoke_show_setup_requested();
    assert!(window.get_setup_visible());
    assert_eq!(window.get_setup_step(), 0);
    assert_eq!(window.get_setup_theme_choice(), 1);
    for (choice, expected) in [
        (2, crate::ThemePreference::System),
        (-1, crate::ThemePreference::Light),
        (1, crate::ThemePreference::Dark),
    ] {
        window.invoke_setup_theme_picked(choice);
        assert_eq!(app.preference_bus.current_values().theme as i32, expected as i32);
        assert_eq!(window.get_setup_theme_choice(), choice);
    }
    window.invoke_setup_skip();
    assert!(!window.get_setup_visible());
    assert!(!app.needs_setup());
    assert_eq!(
        app.preference_bus.current_values().default_printer,
        sample().default_printer
    );
}

#[test]
fn requests_for_closed_windows_leave_application_data_alone() {
    let app = application();
    app.copy_selection(100);
    app.paste_at_cursor(100);
    app.paste_at_pixel(100, 4., 5.);
    app.duplicate_selection(100);
    app.show_view_menu(100, 4., 5.);
    app.load_path_into_window(100, PathBuf::from("missing.stl"));
    app.show_home(100);
    app.hide_recovery_prompt(100);
    app.show_setup(100);
    assert!(app.copied_parts.borrow().is_none());
    assert!(app.main_windows.borrow().is_empty());
    assert_eq!(app.preference_bus.current_values().recent_files, sample().recent_files);
}

#[test]
fn clearing_preferences_removes_old_optional_values() {
    let mut saved = AppSettings::default();
    let mut snapshot = sample();
    store_preferences(&mut saved, &snapshot);
    snapshot.default_printer = None;
    snapshot.default_resin = None;
    snapshot.object_list.offset_x = None;
    snapshot.object_list.offset_y = None;
    snapshot.object_list.width = None;
    snapshot.object_list.height = None;
    snapshot.slice_panel.offset_x = None;
    snapshot.slice_panel.offset_y = None;
    snapshot.slice_panel.width = None;
    snapshot.slice_panel.height = None;
    snapshot.slice_view.offset_x = None;
    snapshot.slice_view.offset_y = None;
    snapshot.slice_view.width = None;
    snapshot.slice_view.height = None;
    snapshot.last_open_dir = None;
    snapshot.currency_symbol.clear();
    snapshot.favourite_printers.clear();
    snapshot.favourite_resins.clear();
    snapshot.recent_files.clear();
    store_preferences(&mut saved, &snapshot);
    assert_eq!(saved.default_printer(), None, "default_printer");
    assert_eq!(saved.default_resin(), None, "default_resin");
    assert_eq!(saved.panel(Panel::ObjectList).offset_x, None, "object_list_offset_x");
    assert_eq!(saved.panel(Panel::ObjectList).offset_y, None, "object_list_offset_y");
    assert_eq!(saved.panel(Panel::ObjectList).width, None, "object_list_width");
    assert_eq!(saved.panel(Panel::ObjectList).height, None, "object_list_height");
    assert_eq!(saved.panel(Panel::SliceControls).offset_x, None, "slice_panel_offset_x");
    assert_eq!(saved.panel(Panel::SliceControls).offset_y, None, "slice_panel_offset_y");
    assert_eq!(saved.panel(Panel::SliceControls).width, None, "slice_panel_width");
    assert_eq!(saved.panel(Panel::SliceControls).height, None, "slice_panel_height");
    assert_eq!(saved.panel(Panel::SliceView).offset_x, None, "slice_view_offset_x");
    assert_eq!(saved.panel(Panel::SliceView).offset_y, None, "slice_view_offset_y");
    assert_eq!(saved.panel(Panel::SliceView).width, None, "slice_view_width");
    assert_eq!(saved.panel(Panel::SliceView).height, None, "slice_view_height");
    assert_eq!(saved.last_open_dir(), None, "last_open_dir");
    assert!(saved.favourite_printers().is_empty());
    assert!(saved.favourite_resins().is_empty());
    assert!(saved.recent_files().is_empty());
    assert_eq!(saved.currency_symbol(), "$");
}

#[test]
fn queued_load_keeps_its_slot_when_a_recent_file_listener_closes_the_window() {
    i_slint_backend_testing::init_no_event_loop();
    let app = application();
    let window = MainWindow::new().unwrap();
    let pending = Rc::new(WaitingFile::new());
    let view = Rc::new(ViewController::new(
        &window,
        SceneDocument::new(),
        pending.clone(),
        DeviceLoss::default(),
    ));
    let slice_run = SliceActionDriver::attach_slice_actions(&window, app.preference_bus.clone(), view.scene_doc(), view.dialog_manager());
    app.main_windows.borrow_mut().insert(
        5,
        WindowSession {
            drop_followup: None,
            autosave: None,
            recovery_source: None,
            main_window: window.clone_strong(),
            view_controller: view.clone(),
            load_waiting: pending.clone(),
            _prefs_watch: app.preference_bus.watch_preferences(|_, _| {}),
            _profile_picker: SliceProfileControls::attach_slice_profile_controls(
                &window,
                app.profile_catalog_cell.clone(),
                app.preference_bus.clone(),
                view.scene_doc(),
            ),
            _slice_driver: slice_run,
            _repair_action: RepairJob::attach(&window, view.scene_doc(), view.dialog_manager()),
            _auto_support_run: crate::auto_support_run::AutoSupportRun::install(
                &window,
                view.scene_doc(),
                app.preference_bus.clone(),
            ),
        },
    );
    let owner = Rc::downgrade(&app);
    let _close_on_notification = app.preference_bus.watch_preferences(move |_, _| {
        owner
            .upgrade()
            .unwrap()
            .main_windows
            .borrow_mut()
            .remove(&5);
    });
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("queued.obj");
    std::fs::write(&path, []).unwrap();
    app.open_in_window(5, path.clone());
    assert!(app.main_windows.borrow().is_empty());
    assert_eq!(pending.take(), Some(path));
}

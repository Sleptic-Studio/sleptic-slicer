// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Window lifetimes, application actions, and preference persistence.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use sc_print_profiles::ProfileLibraries;
use slint::{CloseRequestResponse, ComponentHandle};

use crate::MainWindow;
use crate::about_window::open_about_window;
use crate::app_settings::AppSettings;
use crate::clipboard::{Clipboard, copy_of_selection};
use crate::graphics_status::NoGraphics;
use crate::i18n::ctr;
use crate::mesh_loader::{
    OpenVerdict, OPENABLE_EXTENSIONS, open_verdict, can_open_extension,
    begin_ticketed_load,
};
use crate::mesh_render::DeviceLoss;
use crate::pending_load::WaitingFile;
use crate::panel_place::{Panel, PanelPlace};
use crate::preferences_bus::{
    PreferenceHub, PreferenceEdit, UserPreferences, PreferenceWatch,
};
use crate::preferences_window::{PreferencesWindowBinding, build_preferences_window};
use crate::profile_catalog::InstalledProfiles;
use crate::repair_run::RepairJob;
use crate::scene_document::SceneDocument;
use crate::selection::{LeafId, Selection};
use crate::shortcuts_window::build_shortcuts_window;
use crate::slice_panel::SliceProfileControls;
use crate::slice_run::SliceActionDriver;
use crate::viewport::ViewController;
use crate::window_prefs::TakesPreferences;
use crate::wiring::SaveResult;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileRequest {
    Unsupported,
    Load,
    Append,
    Replace,
    Ask(crate::error_dialog::SaveChoice),
}

fn classify_file(
    supported: bool,
    loaded: bool,
    dirty: bool,
    saved: bool,
    project: bool,
) -> FileRequest {
    use FileRequest::*;
    match (supported, loaded, project, dirty, saved) {
        (false, ..) => Unsupported,
        (true, false, ..) => Load,
        (true, true, false, ..) => Append,
        (true, true, true, false, _) => Replace,
        (true, true, true, true, true) => Ask(crate::error_dialog::SaveChoice::OverwriteCurrent),
        (true, true, true, true, false) => Ask(crate::error_dialog::SaveChoice::KeepAsCopy),
    }
}

struct MenuActions {
    copy: bool,
    paste: bool,
    delete: bool,
}
impl MenuActions {
    #[cfg(test)]
    fn has_entries(&self) -> bool {
        [self.copy, self.paste, self.delete]
            .into_iter()
            .any(|enabled| enabled)
    }
}
fn menu_access(object: bool, clipboard: bool) -> MenuActions {
    MenuActions {
        paste: clipboard,
        delete: object,
        copy: object,
    }
}

// UI callbacks must not keep the application (and hence the window) alive.
macro_rules! app_callbacks {
    ($app:expr, $window:expr; $($signal:ident => $action:ident ( $($arg:expr),* )),* $(,)?) => {
        $(let owner = Rc::downgrade($app);
        $window.$signal(move || { if let Some(state) = owner.upgrade() { state.$action($($arg),*); } });)*
    };
}

// A title-bar click must not cause a disk write; auto height stays unset.
macro_rules! preference_callback {
    ($bus:expr, $ui:expr; $($signal:ident => $field:ident, $event:ident);* $(;)?) => {
        $(let bus = Rc::clone($bus);
        $ui.$signal(move |value| {
            bus.edit_preferences(move |snapshot| { snapshot.$field = value; PreferenceEdit::$event });
        });)*
    };
}

thread_local! {

    static APPLICATION_ON_UI_THREAD: RefCell<Weak<AppState>> = const { RefCell::new(Weak::new()) };
}

struct WindowSession {
    drop_followup: Option<crate::scene_document::Watch>,
    autosave: Option<crate::recovery::Autosave>,
    recovery_source: Option<Rc<crate::recovery::RecoveryFile>>,
    main_window: MainWindow,
    view_controller: Rc<ViewController>,

    load_waiting: Rc<WaitingFile>,
    _prefs_watch: PreferenceWatch,

    _profile_picker: Rc<SliceProfileControls>,

    _slice_driver: Rc<SliceActionDriver>,

    _repair_action: Rc<RepairJob>,
    _auto_support_run: Rc<crate::auto_support_run::AutoSupportRun>,
}

impl WindowSession {
    fn clean_recovery(&self) {
        if let Some(a) = &self.autosave {
            a.clean_exit();
        }
        if let Some(file) = &self.recovery_source
            && self.view_controller.scene_doc().has_scene()
            && let Err(e) = file.remove()
        {
            eprintln!("Recovery cleanup: {e}");
        }
    }
}

pub(crate) struct AppState {
    about_dialog: RefCell<Option<crate::window_prefs::SideWindow<crate::AboutWindow>>>,
    shortcuts_window: RefCell<Option<crate::window_prefs::SideWindow<crate::ShortcutsDialog>>>,
    preferences_window: RefCell<Option<PreferencesWindowBinding>>,
    main_windows: RefCell<HashMap<u64, WindowSession>>,
    window_counter: Cell<u64>,
    focused_main_window: Cell<Option<u64>>,
    recovery_checked: Cell<bool>,

    setup_subscriptions: RefCell<Vec<PreferenceWatch>>,
    preference_bus: Rc<PreferenceHub>,

    profile_catalog_cell: Rc<RefCell<Rc<InstalledProfiles>>>,

    copied_parts: RefCell<Option<Clipboard>>,
    dropped_paths: RefCell<HashMap<u64, Vec<PathBuf>>>,

    gpu_loss: DeviceLoss,

    // Drop the sender before joining the worker, or shutdown blocks on recv.
    _settings_feed: PreferenceWatch,
    _translation_listener: PreferenceWatch,

    _settings_saver: SettingsWorker,
}

struct SettingsWorker(Option<JoinHandle<()>>);

impl Drop for SettingsWorker {
    fn drop(&mut self) {
        if let Some(task) = self.0.take() {
            let _ = task.join();
        }
    }
}

fn consume_preferences(
    receiver: mpsc::Receiver<UserPreferences>,
    mut persist: impl FnMut(&UserPreferences),
) {
    for first in &receiver {
        let newest = receiver.try_iter().last().unwrap_or(first);
        persist(&newest);
    }
}

#[derive(Default)]
struct SetupDraft {
    printer: Option<sc_print_primitives::PrinterId>,
    resin: Option<sc_print_primitives::MaterialId>,
    theme: i32,
    printer_query: String,
    resin_query: String,
    printer_ids: Vec<Option<sc_print_primitives::PrinterId>>,
    resin_ids: Vec<Option<sc_print_primitives::MaterialId>>,
}

fn launch_target(focused: Option<u64>, ids: impl Iterator<Item = u64>) -> Option<u64> {
    let mut smallest = None;
    let mut focus_exists = false;
    for candidate in ids {
        smallest = Some(smallest.map_or(candidate, |current: u64| current.min(candidate)));
        focus_exists |= focused == Some(candidate);
    }
    if focus_exists { focused } else { smallest }
}

// Setters retain their validation; cloning the values also leaves the live bus untouched.
fn store_preferences(destination: &mut AppSettings, source: &UserPreferences) {
    macro_rules! transfer {
        ($($field:ident => $setter:ident),* $(,)?) => { $(destination.$setter(source.$field);)* };
    }
    transfer! {
        aa_level => set_aa_level,
        language => set_language,
        menu_layout => set_menu_layout,
        theme => set_theme,
    }
    for panel in PANELS {
        destination.set_panel(panel, source.panel(panel));
    }
    destination.set_recent_files(source.recent_files.clone());
    destination.set_last_open_dir(source.last_open_dir.clone());
    destination.set_favourite_resins(source.favourite_resins.clone());
    destination.set_favourite_printers(source.favourite_printers.clone());
    destination.set_default_resin(source.default_resin.clone());
    destination.set_default_printer(source.default_printer.clone());
    destination.set_currency_symbol(source.currency_symbol.clone());
    destination.setup_complete = source.setup_complete;
    destination.perspective_view = source.perspective_view;
    destination.orient_preset = source.orient_preset;
}

fn start_preferences_worker(
    mut settings: AppSettings,
) -> std::io::Result<(Sender<UserPreferences>, SettingsWorker)> {
    let (sender, receiver) = mpsc::channel();
    let task = thread::Builder::new()
        .name("settings-saver".into())
        .spawn(move || {
            consume_preferences(receiver, |snapshot| {
                store_preferences(&mut settings, snapshot);
                if let Err(err) = settings.save() {
                    eprintln!("Failed to save settings: {err}");
                }
            });
        })?;
    Ok((sender, SettingsWorker(Some(task))))
}

fn read_preferences(
    saved: &AppSettings,
    volume: crate::build_volume::BuildVolume,
) -> UserPreferences {
    macro_rules! read {
        ($($field:ident),* $(,)?) => {
            UserPreferences {
                $($field: saved.$field(),)*
                orient_preset: saved.orient_preset,
                perspective_view: saved.perspective_view,
                setup_complete: saved.setup_complete,
                build_volume: volume,
                object_list: saved.panel(Panel::ObjectList),
                slice_panel: saved.panel(Panel::SliceControls),
                slice_view: saved.panel(Panel::SliceView),
            }
        };
    }
    read! {
        recent_files,
        last_open_dir,
        aa_level,
        favourite_resins,
        favourite_printers,
        default_resin,
        default_printer,
        currency_symbol,
        language,
        menu_layout,
        theme,
    }
}
const PANELS: [Panel; 3] = [Panel::ObjectList, Panel::SliceControls, Panel::SliceView];

/// The window properties one floating panel is placed through. The slice
/// controls cannot be hidden or folded, so they have no setters for that.
struct PanelSetters {
    shown: Option<fn(&MainWindow, bool)>,
    folded: Option<fn(&MainWindow, bool)>,
    corner: [fn(&MainWindow, bool); 2],
    /// x, y, width, height.
    extent: [fn(&MainWindow, f32); 4],
}

fn panel_setters(panel: Panel) -> PanelSetters {
    match panel {
        Panel::ObjectList => PanelSetters {
            shown: Some(MainWindow::set_parts_panel_shown),
            folded: Some(MainWindow::set_parts_panel_folded),
            corner: [MainWindow::set_parts_panel_at_right, MainWindow::set_parts_panel_at_bottom],
            extent: [MainWindow::set_parts_panel_x, MainWindow::set_parts_panel_y, MainWindow::set_parts_panel_w, MainWindow::set_parts_panel_h],
        },
        Panel::SliceControls => PanelSetters {
            shown: None,
            folded: None,
            corner: [MainWindow::set_slice_box_at_right, MainWindow::set_slice_box_at_bottom],
            extent: [MainWindow::set_slice_box_x, MainWindow::set_slice_box_y, MainWindow::set_slice_box_w, MainWindow::set_slice_box_h],
        },
        Panel::SliceView => PanelSetters {
            shown: Some(MainWindow::set_layer_view_shown),
            folded: Some(MainWindow::set_layer_view_folded),
            corner: [MainWindow::set_layer_view_at_right, MainWindow::set_layer_view_at_bottom],
            extent: [MainWindow::set_layer_view_x, MainWindow::set_layer_view_y, MainWindow::set_layer_view_w, MainWindow::set_layer_view_h],
        },
    }
}

/// Puts each panel where the preferences say. A size or position that was
/// never saved is left alone, so Slint's own layout decides it.
fn restore_panels(window: &MainWindow, saved: &UserPreferences) {
    for panel in PANELS {
        let place = saved.panel(panel);
        let setters = panel_setters(panel);
        setters.shown.inspect(|set| set(window, place.visible));
        setters.folded.inspect(|set| set(window, place.minimized));
        setters.corner[0](window, place.anchor_right);
        setters.corner[1](window, place.anchor_bottom);
        let extent = [place.offset_x, place.offset_y, place.width, place.height];
        for (set, value) in setters.extent.iter().zip(extent) {
            if let Some(value) = value {
                set(window, value);
            }
        }
    }
}

/// A panel was dragged, resized or folded in a window: its new place,
/// stored unless nothing actually changed. A zero height means the panel
/// sizes itself.
#[allow(clippy::too_many_arguments)]
fn record_panel_move(bus: &PreferenceHub, panel: Panel, edit: PreferenceEdit, corner: (bool, bool), at: (f32, f32), size: (f32, f32), folded: Option<bool>) {
    let before = bus.current_values().panel(panel);
    let after = PanelPlace {
        anchor_right: corner.0,
        anchor_bottom: corner.1,
        offset_x: Some(at.0),
        offset_y: Some(at.1),
        width: Some(size.0),
        height: (size.1 > 0.0).then_some(size.1),
        minimized: folded.unwrap_or(before.minimized),
        ..before
    };
    if after != before {
        bus.edit_preferences(move |values| {
            *values.panel_mut(panel) = after;
            edit
        });
    }
}

impl AppState {
    pub(crate) fn new(device_loss: DeviceLoss) -> Result<Rc<Self>, Box<dyn Error>> {
        let saved = AppSettings::load();
        let profiles = InstalledProfiles::scan_libraries(&ProfileLibraries::platform_default());
        let opening_printer = profiles.printer_in_effect(saved.default_printer().as_ref());
        let opening_volume = profiles.volume_for_printer(opening_printer.as_ref());
        let initial = read_preferences(&saved, opening_volume);
        initial.language.apply();
        let bus = PreferenceHub::new(initial);
        let (sender, worker) = start_preferences_worker(saved)?;
        let persistence = bus.watch_preferences(move |snapshot, _| {
            let _ = sender.send(snapshot.clone());
        });
        // Translation must change before any window rebuilds its labels.
        let translation = bus.watch_preferences(|snapshot, event| {
            if matches!(event, PreferenceEdit::Language) {
                snapshot.language.apply();
            }
        });
        let state = Rc::new(Self {
            preference_bus: bus,
            profile_catalog_cell: Rc::new(RefCell::new(Rc::new(profiles))),
            gpu_loss: device_loss,
            main_windows: RefCell::default(),
            about_dialog: RefCell::default(),
            shortcuts_window: RefCell::default(),
            preferences_window: RefCell::default(),
            window_counter: Cell::new(0),
            focused_main_window: Cell::new(None),
            recovery_checked: Cell::new(false),
            setup_subscriptions: RefCell::default(),
            copied_parts: RefCell::default(),
            dropped_paths: RefCell::default(),
            _settings_feed: persistence,
            _translation_listener: translation,
            _settings_saver: worker,
        });
        APPLICATION_ON_UI_THREAD.with(|slot| slot.replace(Rc::downgrade(&state)));
        Ok(state)
    }

    pub(crate) fn report_device_lost(reason: String) {
        if let Some(state) = APPLICATION_ON_UI_THREAD.with(|slot| slot.borrow().upgrade()) {
            state.show_device_lost(reason);
        }
    }

    fn show_device_lost(&self, reason: String) {
        let live = self.main_windows.borrow();
        live.values().for_each(|entry| {
            entry.view_controller.graphics_gone();
            entry.view_controller.scene_loader().record_device_loss();
        });
        if self.gpu_loss.claim_report()
            && let Some(entry) = live.values().next()
        {
            entry.view_controller.dialog_manager().show_failure(
                &ctr!("ErrorDialog", "The graphics device was lost"),
                None,
                NoGraphics::Gone.explain(),
                reason,
            );
        }
    }

    fn view_controller(&self, id: u64) -> Option<Rc<ViewController>> {
        self.main_windows
            .borrow()
            .get(&id)
            .map(|entry| Rc::clone(&entry.view_controller))
    }

    pub(crate) fn open_main_window(self: &Rc<Self>) -> Result<u64, Box<dyn Error>> {
        let ui = MainWindow::new()?;
        crate::theme_sync::install(&ui);
        ui.set_on_macos(cfg!(target_os = "macos"));
        let saved = self.preference_bus.current_values();
        ui.take_preferences(&saved);
        restore_panels(&ui, &saved);
        let pending_load = Rc::new(WaitingFile::new());
        let ctrl = Rc::new(ViewController::new(
            &ui,
            SceneDocument::new(),
            Rc::clone(&pending_load),
            self.gpu_loss.clone(),
        ));
        ctrl.listen_to_document();
        ctrl.set_build_volume(saved.build_volume);
        ctrl.set_plate_brand_from(saved.default_printer.as_ref());
        crate::wiring::connect_main_window_actions(&ui, Rc::clone(&ctrl), Rc::clone(&self.preference_bus))?;
        let slice_panel = SliceProfileControls::attach_slice_profile_controls(
            &ui,
            Rc::clone(&self.profile_catalog_cell),
            Rc::clone(&self.preference_bus),
            ctrl.scene_doc(),
        );
        let slice_run =
            SliceActionDriver::attach_slice_actions(&ui, Rc::clone(&self.preference_bus), ctrl.scene_doc(), ctrl.dialog_manager());
        crate::recipes::install(
            &ui,
            ctrl.scene_doc(),
            Rc::clone(&self.preference_bus),
            Rc::clone(&slice_run),
        );
        let auto_support_run = crate::auto_support_run::AutoSupportRun::install(
            &ui,
            ctrl.scene_doc(),
            Rc::clone(&self.preference_bus),
        );
        let repair_run = RepairJob::attach(&ui, ctrl.scene_doc(), ctrl.dialog_manager());
        self.connect_application_actions(&ui, ctrl.scene_doc());
        let id = self
            .window_counter
            .replace(self.window_counter.get().wrapping_add(1));

        app_callbacks!(self, ui;
            on_copy_key => copy_selection(id),
            on_paste_key => paste_at_cursor(id),
            on_duplicate_requested => duplicate_selection(id),
        );

        {
            let app_weak = Rc::downgrade(self);
            ui.on_add_model_requested(move || {
                let weak = app_weak.clone();
                let _ = slint::spawn_local(async move {
                    let files = rfd::AsyncFileDialog::new()
                        .add_filter(
                            ctr!("FileDialog", "Models").to_string(),
                            OPENABLE_EXTENSIONS,
                        )
                        .pick_files()
                        .await;
                    if let (Some(app), Some(files)) = (weak.upgrade(), files) {
                        for file in files {
                            app.open_dropped_path(id, file.path().to_path_buf());
                        }
                    }
                });
            });
        }

        {
            let weak = Rc::downgrade(self);
            ui.on_input_evidence(move |kind| {
                let Some(app) = weak.upgrade() else {
                    return;
                };
                if matches!(kind, 6..=12) {
                    let controller = app
                        .main_windows
                        .borrow()
                        .get(&id)
                        .map(|w| w.view_controller.clone());
                    if let Some(controller) = controller {
                        if kind == 6 {
                            controller.place_face_evidence();
                        } else if kind == 10 {
                            controller.layer_closeup_evidence();
                        } else if kind == 11 {
                            controller.support_closeup_evidence();
                        } else if kind == 12 {
                            controller.plate_label_closeup_evidence();
                        } else {
                            controller.cube_hover_evidence(kind - 7);
                        }
                    }
                    return;
                }
                if matches!(kind, 2 | 4 | 5) {
                    let controller = app
                        .main_windows
                        .borrow()
                        .get(&id)
                        .map(|w| w.view_controller.clone());
                    if let Some(controller) = controller {
                        if kind == 4 {
                            controller.orbit_by(crate::viewport::ScreenDelta::new(190.0, -85.0));
                        } else {
                            controller.zoom_model_evidence();
                        }
                        if kind == 5 {
                            controller.scene_doc().choose(Selection::None);
                            if let Some((x, y)) = controller.context_evidence_point(5) {
                                controller.on_pointer_down(
                                    crate::viewport::ScreenPoint::new(x, y),
                                    crate::PointerModifiers::default(),
                                );
                                controller.on_pointer_move(
                                    crate::viewport::ScreenPoint::new(x + 85.0, y - 25.0),
                                    crate::PointerModifiers {
                                        shift: true,
                                        ..Default::default()
                                    },
                                );
                            }
                        }
                    }
                    return;
                }
                let point = app
                    .main_windows
                    .borrow()
                    .get(&id)
                    .and_then(|w| w.view_controller.context_evidence_point(kind));
                if let Some((x, y)) = point {
                    let window = app
                        .main_windows
                        .borrow()
                        .get(&id)
                        .map(|w| w.main_window.clone_strong());
                    if let Some(window) = window {
                        window.invoke_secondary_pressed(x, y);
                        window.invoke_secondary_released(x, y);
                    }
                }
            });
        }

        let subscription = self.connect_window_actions(&ui, &ctrl, id);

        self.connect_native_events(&ui, id);

        let autosave =
            match crate::recovery::Autosave::start(ctrl.scene_doc(), self.preference_bus.clone(), id) {
                Ok(a) => Some(a),
                Err(e) => {
                    eprintln!("Autosave unavailable: {e}");
                    None
                }
            };
        if !self.recovery_checked.replace(true) {
            let files = crate::recovery::directory()
                .map(|d| crate::recovery::pending(&d))
                .unwrap_or_default();
            let files = Rc::new(RefCell::new(
                files.into_iter().map(Rc::new).collect::<Vec<_>>(),
            ));
            ui.set_recovery_prompt(!files.borrow().is_empty());
            ui.on_recovery_choice({
                let files = files.clone();
                let app = Rc::downgrade(self);
                let weak = ui.as_weak();
                move |restore| {
                    if let Some(w) = weak.upgrade() {
                        w.set_recovery_prompt(false);
                    }
                    if restore {
                        if let Some(app) = app.upgrade() {
                            for file in files.borrow().iter() {
                                if let Ok(window) = app.open_main_window() {
                                    if let Some(w) = app.main_windows.borrow_mut().get_mut(&window)
                                    {
                                        w.recovery_source = Some(file.clone());
                                    }
                                    app.load_path_into_window(window, file.path.clone());
                                }
                            }
                        }
                    } else {
                        for file in files.borrow().iter() {
                            if let Err(e) = file.remove() {
                                eprintln!("Recovery discard: {e}");
                            }
                        }
                    }
                }
            });
        }
        ui.show()?;
        crate::window_prefs::restore_geometry(&ui);

        crate::window_icon::apply(ui.window());
        self.main_windows.borrow_mut().insert(
            id,
            WindowSession {
                drop_followup: None,
                autosave,
                recovery_source: None,
                main_window: ui,
                view_controller: ctrl,
                load_waiting: pending_load,
                _prefs_watch: subscription,
                _profile_picker: slice_panel,
                _slice_driver: slice_run,
                _repair_action: repair_run,
                _auto_support_run: auto_support_run,
            },
        );
        Ok(id)
    }

    fn connect_native_events(self: &Rc<Self>, ui: &MainWindow, id: u64) {
        use slint::winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent};
        let owner = Rc::downgrade(self);
        let component = ui.as_weak();
        ui.window().on_winit_window_event(move |surface, event| {
            match event {
                WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                    surface.with_winit_window(crate::window_prefs::record_geometry);
                }
                WindowEvent::Focused(true) => {
                    if let Some(state) = owner.upgrade() {
                        state.focused_main_window.set(Some(id));
                    }
                }
                WindowEvent::HoveredFile(_)
                | WindowEvent::HoveredFileCancelled
                | WindowEvent::DroppedFile(_) => {
                    if let Some(window) = component.upgrade() {
                        window.set_file_over_window(matches!(event, WindowEvent::HoveredFile(_)));
                    }
                    if let WindowEvent::DroppedFile(path) = event
                        && let Some(state) = owner.upgrade()
                    {
                        state.open_dropped_path(id, path.clone());
                    }
                }
                _ => (),
            }
            EventResult::Propagate
        });
    }

    fn connect_window_actions(
        self: &Rc<Self>,
        ui: &MainWindow,
        view: &Rc<ViewController>,
        id: u64,
    ) -> PreferenceWatch {
        let owner = Rc::downgrade(self);
        let preferences = Rc::clone(&self.preference_bus);
        ui.on_open_recent_requested(move |index| {
            if let (Some(state), Ok(index)) = (owner.upgrade(), usize::try_from(index))
                && let Some(path) = preferences.current_values().recent_files.get(index)
            {
                state.load_path_into_window(id, path.clone());
            }
        });
        let pressed_view = Rc::clone(view);
        ui.on_secondary_pressed(move |x, y| pressed_view.right_down(x, y));
        let released_view = Rc::clone(view);
        let owner = Rc::downgrade(self);
        ui.on_secondary_released(move |x, y| {
            let position = released_view.right_up(x, y);
            if let (Some((x, y)), Some(state)) = (position, owner.upgrade()) {
                state.show_view_menu(id, x, y);
            }
        });
        app_callbacks!(self, ui; on_menu_copy_chosen => copy_selection(id));
        let owner = Rc::downgrade(self);
        ui.on_menu_paste_chosen(move |x, y| {
            if let Some(state) = owner.upgrade() {
                state.paste_at_pixel(id, x, y);
            }
        });
        let window = ui.as_weak();
        let observer = Rc::clone(view);
        let subscription = self.preference_bus.watch_preferences(move |snapshot, event| {
            if crate::window_prefs::changes_main_window(event)
                && let Some(window) = window.upgrade()
            {
                window.take_preferences(snapshot);
            }
            if *event == PreferenceEdit::DefaultPrinter {
                observer.set_build_volume(snapshot.build_volume);
                observer.set_plate_brand_from(snapshot.default_printer.as_ref());
            }
        });
        let owner = Rc::downgrade(self);
        ui.window().on_close_requested(move || {
            if let Some(state) = owner.upgrade() {
                let mut sessions = state.main_windows.borrow_mut();
                if let Some(closed) = sessions.remove(&id) {
                    closed.clean_recovery();
                }
            }
            CloseRequestResponse::HideWindow
        });
        subscription
    }

    pub(crate) fn clean_recovery(&self) {
        for w in self.main_windows.borrow().values() {
            w.clean_recovery();
        }
    }

    pub(crate) fn install_verification(
        &self,
        id: u64,
        options: crate::launch_options::LaunchOptions,
    ) -> Result<(), Box<dyn Error>> {
        let windows = self.main_windows.borrow();
        let wired = windows.get(&id).ok_or("Missing launch window")?;
        crate::verification::install(
            &wired.main_window,
            wired.view_controller.scene_doc(),
            options,
            self.preference_bus.current_values(),
        )
    }

    pub(crate) fn receive_launch(paths: Vec<PathBuf>) -> bool {
        APPLICATION_ON_UI_THREAD.with(|target| {
            let Some(app) = target.borrow().upgrade() else {
                return false;
            };
            let id = launch_target(
                app.focused_main_window.get(),
                app.main_windows.borrow().keys().copied(),
            );
            if let Some(id) = id {
                if let Some(wired) = app.main_windows.borrow().get(&id) {
                    use slint::winit_030::WinitWindowAccessor;
                    wired.main_window.window().with_winit_window(|window| {
                        window.set_minimized(false);
                        window.focus_window();
                        window.request_user_attention(Some(
                            slint::winit_030::winit::window::UserAttentionType::Informational,
                        ));
                    });
                }
                app.open_launch_paths(id, paths);
                true
            } else {
                false
            }
        })
    }

    pub(crate) fn show_home(&self, window_id: u64) {
        if let Some(w) = self.main_windows.borrow().get(&window_id) {
            crate::hub::show_home(&w.main_window);
        }
    }

    pub(crate) fn hide_recovery_prompt(&self, window_id: u64) {
        if let Some(w) = self.main_windows.borrow().get(&window_id) {
            w.main_window.set_recovery_prompt(false);
        }
    }

    pub(crate) fn open_launch_paths(self: &Rc<Self>, window_id: u64, paths: Vec<PathBuf>) {
        let (projects, models): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| {
            crate::drop_import::route(p) == crate::drop_import::DropRoute::ProjectPrompt
        });
        let has_models = !models.is_empty();
        if models.len() == 1 && projects.is_empty() {
            let empty = self
                .main_windows
                .borrow()
                .get(&window_id)
                .is_some_and(|w| !w.view_controller.scene_doc().has_scene());
            if empty {
                self.load_path_into_window(window_id, models.into_iter().next().unwrap());
                return;
            }
        }
        if has_models {
            for path in models {
                self.open_dropped_path(window_id, path);
            }
        }
        for (index, path) in projects.into_iter().enumerate() {
            let target = if !has_models && index == 0 {
                window_id
            } else {
                match self.open_main_window() {
                    Ok(id) => id,
                    Err(error) => {
                        eprintln!("Could not open {}: {error}", path.display());
                        continue;
                    }
                }
            };
            self.load_path_into_window(target, path);
        }
    }

    fn open_dropped_path(self: &Rc<Self>, window_id: u64, path: PathBuf) {
        if !can_open_extension(&path) {
            self.load_path_into_window(window_id, path);
            return;
        }
        let first = !self.dropped_paths.borrow().contains_key(&window_id);
        self.dropped_paths
            .borrow_mut()
            .entry(window_id)
            .or_default()
            .push(path);
        if first {
            let weak = Rc::downgrade(self);
            slint::Timer::single_shot(std::time::Duration::from_millis(200), move || {
                if let Some(app) = weak.upgrade() {
                    app.route_drop_batch(window_id);
                }
            });
        }
    }

    fn route_drop_batch(self: &Rc<Self>, window_id: u64) {
        let busy = self.main_windows.borrow().get(&window_id).is_some_and(|w| {
            w.load_waiting.blocks_import(
                w.main_window.get_busy_opening() || w.main_window.get_opening(),
                w.main_window.get_drop_project_prompt(),
            )
        });
        if busy {
            let weak = Rc::downgrade(self);
            slint::Timer::single_shot(std::time::Duration::from_millis(200), move || {
                if let Some(app) = weak.upgrade() {
                    app.route_drop_batch(window_id);
                }
            });
            return;
        }
        let paths = self
            .dropped_paths
            .borrow_mut()
            .remove(&window_id)
            .unwrap_or_default();
        if paths.is_empty() {
            return;
        }
        let windows = self.main_windows.borrow();
        let Some(wired) = windows.get(&window_id) else {
            return;
        };
        if paths
            .iter()
            .any(|p| crate::drop_import::route(p) == crate::drop_import::DropRoute::ProjectPrompt)
        {
            wired.main_window.set_drop_project_prompt(true);
            let weak = Rc::downgrade(self);
            wired.main_window.on_drop_project_choice(move |open| {
                let Some(app) = weak.upgrade() else {
                    return;
                };
                if let Some(wired) = app.main_windows.borrow().get(&window_id) {
                    wired.main_window.set_drop_project_prompt(false);
                }
                if open {
                    let mut rest = paths.clone();
                    let index = rest
                        .iter()
                        .position(|p| {
                            crate::drop_import::route(p)
                                == crate::drop_import::DropRoute::ProjectPrompt
                        })
                        .expect("project prompt has a 3MF");
                    let project = rest.remove(index);
                    if !rest.is_empty() {
                        let weak = Rc::downgrade(&app);
                        let expected = project.clone();
                        let document = app
                            .main_windows
                            .borrow()
                            .get(&window_id)
                            .map(|w| w.view_controller.scene_doc());
                        if let Some(document) = document {
                            let doc_weak = Rc::downgrade(&document);
                            let subscription = document.watch(move |event| {
                                if event != crate::scene_document::DocumentEvent::Opened
                                    || !doc_weak.upgrade().is_some_and(|d| {
                                        d.read_file_path(|p| p == expected).unwrap_or(false)
                                    })
                                {
                                    return;
                                }
                                let weak = weak.clone();
                                let rest = rest.clone();
                                slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                                    if let Some(app) = weak.upgrade() {
                                        if let Some(w) =
                                            app.main_windows.borrow_mut().get_mut(&window_id)
                                        {
                                            w.drop_followup = None;
                                        }
                                        app.dropped_paths
                                            .borrow_mut()
                                            .entry(window_id)
                                            .or_default()
                                            .extend(rest);
                                        app.route_drop_batch(window_id);
                                    }
                                });
                            });
                            if let Some(w) = app.main_windows.borrow_mut().get_mut(&window_id) {
                                w.drop_followup = Some(subscription);
                            }
                        }
                    }
                    app.load_path_into_window(window_id, project);
                } else {
                    app.import_paths(window_id, paths.clone());
                }
            });
        } else {
            drop(windows);
            self.import_paths(window_id, paths);
        }
    }

    fn import_paths(&self, window_id: u64, paths: Vec<PathBuf>) {
        let windows = self.main_windows.borrow();
        let Some(wired) = windows.get(&window_id) else {
            return;
        };
        crate::drop_import::start(wired.view_controller.clone(), paths);
    }

    pub(crate) fn load_path_into_window(self: &Rc<Self>, window_id: u64, path: PathBuf) {
        let Some(controller) = self.view_controller(window_id) else {
            return;
        };

        let action = classify_file(
            can_open_extension(&path),
            controller.scene_doc().has_scene(),
            controller.scene_doc().has_unsaved_changes(),
            controller.scene_doc().opened_from_project(),
            crate::drop_import::route(&path) == crate::drop_import::DropRoute::ProjectPrompt,
        );
        match action {
            FileRequest::Unsupported => {
                let title = ctr!("ErrorDialog", "Unsupported file type");
                let filename = ctr!("ErrorDialog", "File: {}", path.display()).to_string();
                let explanation = ctr!(
                    "ErrorDialog",
                    "Only the following file types can be opened: {}",
                    OPENABLE_EXTENSIONS.join(", ")
                )
                .to_string();
                controller.dialog_manager().show_failure(
                    title.as_str(),
                    Some(filename),
                    explanation,
                    String::new(),
                );
            }
            FileRequest::Load => {
                self.open_in_window(window_id, path);
            }
            FileRequest::Append => {
                crate::wiring::remember_recent_file(&self.preference_bus, &path);
                self.import_paths(window_id, vec![path]);
            }
            FileRequest::Replace => {
                self.drop_scene_and_open(window_id, path);
            }
            FileRequest::Ask(offer) => self.ask_to_replace(window_id, path, offer),
        }
    }

    fn ask_to_replace(
        self: &Rc<Self>,
        id: u64,
        incoming: PathBuf,
        offer: crate::error_dialog::SaveChoice,
    ) {
        let Some(view) = self.view_controller(id) else {
            return;
        };
        let window = self
            .main_windows
            .borrow()
            .get(&id)
            .map(|entry| entry.main_window.clone_strong());
        let Some(window) = window else { return };
        let previous = view
            .scene_doc()
            .file_path()
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| crate::i18n::tr!("this project").to_string());
        let name = incoming
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let heading = crate::i18n::tr!("Save changes to {}?", previous);
        let explanation = crate::i18n::tr!(
            "Opening {} closes the current project. Unsaved changes are lost if you don't save.",
            name
        );
        let save_label = match offer {
            crate::error_dialog::SaveChoice::KeepAsCopy => crate::i18n::tr!("Save as..."),
            _ => crate::i18n::tr!("Save"),
        };
        let owner = Rc::downgrade(self);
        crate::confirm::ask(
            &window,
            crate::confirm::Question {
                title: &heading,
                body: &explanation,
                primary: &save_label,
                secondary: &crate::i18n::tr!("Don't save"),
                destructive: false,
            },
            move |answer| {
                if let Some(state) = owner.upgrade() {
                    match answer {
                        crate::confirm::Choice::Cancel => (),
                        crate::confirm::Choice::Secondary => state.drop_scene_and_open(id, incoming),
                        crate::confirm::Choice::Primary => state.save_scene_and_open(id, incoming),
                    }
                }
            },
        );
    }

    pub(crate) fn copy_selection(&self, window_id: u64) {
        let Some(view) = self.view_controller(window_id) else {
            return;
        };
        if view.layout_tools_allowed() {
            let document = view.scene_doc();
            if let Some(content) = copy_of_selection(&document) {
                let anchor = crate::clipboard::selection_anchor(&document);
                self.copied_parts
                    .replace(Some(Clipboard::new(content).with_source(anchor)));
            }
        }
    }

    pub(crate) fn paste_at_cursor(&self, window_id: u64) {
        let Some(view) = self.view_controller(window_id) else {
            return;
        };
        if let Some(content) = &*self.copied_parts.borrow() {
            view.paste_at_cursor(content);
        }
    }

    pub(crate) fn duplicate_selection(&self, window_id: u64) {
        let selected = self
            .view_controller(window_id)
            .is_some_and(|view| view.scene_doc().main_chosen_node().is_some());
        if selected {
            self.copy_selection(window_id);
            if let (Some(view), Some(content)) =
                (self.view_controller(window_id), &*self.copied_parts.borrow())
            {
                view.duplicate_clipboard(content);
            }
        }
    }

    pub(crate) fn show_view_menu(&self, window_id: u64, x: f32, y: f32) {
        let sessions = self.main_windows.borrow();
        if let Some(session) = sessions.get(&window_id) {
            let view = &session.view_controller;
            view.activate_plate_at(x, y);
            let object = view.node_under(x, y);
            if let Some(id) = object {
                let document = view.scene_doc();
                let existing = document.chosen();
                let selection = if existing.ids().contains(&id) {
                    existing
                } else {
                    Selection::One(LeafId(id))
                };
                document.choose(selection);
            }
            let on_plate = crate::picking::menu_for_click(object.is_some(), view.hits_plate_at(x, y))
                == crate::picking::ClickMenu::OnPlate;
            session.main_window.set_context_on_plate(on_plate);
            let actions = menu_access(object.is_some(), self.copied_parts.borrow().is_some());
            session.main_window.invoke_open_context_menu(
                x,
                y,
                actions.copy,
                actions.paste,
                actions.delete,
            );
        }
    }

    pub(crate) fn paste_at_pixel(&self, window_id: u64, x: f32, y: f32) {
        let Some(view) = self.view_controller(window_id) else {
            return;
        };
        if let Some(content) = &*self.copied_parts.borrow() {
            view.paste_at_pixel(content, x, y);
        }
    }

    fn open_in_window(&self, window_id: u64, path: PathBuf) {
        let request = self.main_windows.borrow().get(&window_id).map(|session| {
            (
                session.view_controller.scene_loader(),
                Rc::clone(&session.load_waiting),
                session.view_controller.printer_volume(),
            )
        });
        let Some((loader, pending, volume)) = request else {
            return;
        };
        // Preference listeners may change or close the window; retain this request's target.
        match open_verdict(loader.unavailable_graphics_reason()) {
            OpenVerdict::HoldUntilGpu => {
                crate::wiring::remember_recent_file(&self.preference_bus, &path);
                pending.hold(path);
            }
            _ => {
                if let Some(ticket) = loader.request_load_ticket() {
                    crate::wiring::remember_recent_file(&self.preference_bus, &path);
                    begin_ticketed_load(ticket, path, &loader, volume);
                }
            }
        }
    }

    fn drop_scene_and_open(&self, window_id: u64, path: PathBuf) {
        if let Some(view) = self.view_controller(window_id) {
            view.close_scene();
            self.open_in_window(window_id, path);
        }
    }

    fn save_scene_and_open(self: &Rc<Self>, window_id: u64, path: PathBuf) {
        let Some(view) = self.view_controller(window_id) else {
            return;
        };
        let saver = crate::wiring::ProjectSaver::for_window(&view, &self.preference_bus);
        let messages = saver.dialogs.clone();
        let owner = Rc::downgrade(self);
        let scheduled = slint::spawn_local(async move {
            match saver.save().await {
                SaveResult::Saved => {
                    if let Some(state) = owner.upgrade() { state.drop_scene_and_open(window_id, path); }
                }
                SaveResult::SavedButChanged => messages.show_notice(
                    &ctr!("SaveDialog", "Scene not replaced"),
                    ctr!("SaveDialog", "The scene changed while saving, so the file does not hold its current state. Nothing was replaced.").to_string(),
                    String::new(),
                ),
                SaveResult::NotSaved => (),
            }
        });
        if let Err(err) = scheduled {
            eprintln!("Failed to spawn Save & Open task: {err}");
        }
    }

    fn request_new_window(self: &Rc<Self>) {
        match self.open_main_window() {
            Ok(_) => (),
            Err(err) => eprintln!("Failed to open new window: {err}"),
        }
    }

    fn connect_application_actions(self: &Rc<Self>, ui: &MainWindow, document: Rc<SceneDocument>) {
        self.wire_setup(ui);
        app_callbacks!(self, ui;
            on_show_about => show_about(),
            on_show_shortcuts_requested => show_shortcuts(),
        );
        {
            let app = Rc::downgrade(self);
            let document = document.clone();
            ui.on_profile_edit_requested(move |printer| {
                let Some(app) = app.upgrade() else {
                    return;
                };
                app.show_preferences();
                if printer {
                    let catalog = app.profile_catalog_cell.borrow().clone();
                    if let Some(index) = document
                        .chosen_printer()
                        .and_then(|id| catalog.printer_position(&id))
                        && let Some(prefs) = app.preferences_window.borrow().as_ref()
                    {
                        prefs.window.invoke_open_printer(index as i32, false);
                    }
                } else {
                    let catalog = app.profile_catalog_cell.borrow().clone();
                    if let Some(index) = document
                        .chosen_resin()
                        .and_then(|id| catalog.resin_position(&id))
                        && let Some(prefs) = app.preferences_window.borrow().as_ref()
                    {
                        prefs.window.invoke_fork_resin(index as i32);
                    }
                }
            });
        }
        app_callbacks!(self, ui; on_show_preferences => show_preferences());
        app_callbacks!(self, ui; on_open_new_window => request_new_window());
        preference_callback!(&self.preference_bus, ui;
            on_menu_style_picked => menu_layout, MenuLayout;
        );
        for (panel, edit) in [(Panel::ObjectList, PreferenceEdit::ObjectListPanel), (Panel::SliceView, PreferenceEdit::SliceViewPanel)] {
            let bus = Rc::clone(&self.preference_bus);
            let shown = move |visible: bool| {
                bus.edit_preferences(move |values| {
                    values.panel_mut(panel).visible = visible;
                    edit
                })
            };
            match panel {
                Panel::ObjectList => ui.on_parts_panel_toggled(shown),
                _ => ui.on_layer_view_toggled(shown),
            }
        }
        let bus = Rc::clone(&self.preference_bus);
        ui.on_parts_panel_moved(move |right, bottom, x, y, w, h, folded| {
            record_panel_move(&bus, Panel::ObjectList, PreferenceEdit::ObjectListPanel, (right, bottom), (x, y), (w, h), Some(folded));
        });
        let bus = Rc::clone(&self.preference_bus);
        ui.on_slice_box_moved(move |right, bottom, x, y, w, h| {
            record_panel_move(&bus, Panel::SliceControls, PreferenceEdit::SliceControlPanel, (right, bottom), (x, y), (w, h), None);
        });
        let bus = Rc::clone(&self.preference_bus);
        ui.on_layer_view_moved(move |right, bottom, x, y, w, h, folded| {
            record_panel_move(&bus, Panel::SliceView, PreferenceEdit::SliceViewPanel, (right, bottom), (x, y), (w, h), Some(folded));
        });
    }

    fn show_about(&self) {
        crate::window_prefs::show_kept(&self.about_dialog, "The About window", |w| w.show(), || open_about_window(&self.preference_bus));
    }

    fn show_shortcuts(&self) {
        crate::window_prefs::show_kept(&self.shortcuts_window, "The Keyboard Shortcuts window", |w| w.show(), || build_shortcuts_window(&self.preference_bus));
    }

    fn wire_setup(self: &Rc<Self>, ui: &MainWindow) {
        let state = Rc::new(RefCell::new(SetupDraft::default()));
        let weak_ui = ui.as_weak();
        let refresh = {
            let app = Rc::downgrade(self);
            let state = state.clone();
            let weak_ui = weak_ui.clone();
            Rc::new(move || {
                let (Some(app), Some(ui)) = (app.upgrade(), weak_ui.upgrade()) else {
                    return;
                };
                let catalog = app.profile_catalog_cell.borrow().clone();
                let mut st = state.borrow_mut();
                let tuned = crate::i18n::tr!("Tuned for your printer").to_string();
                let (printers, printer_ids) = crate::onboarding::printer_rows(
                    &catalog,
                    &st.printer_query,
                    st.printer.as_ref(),
                );
                let (resins, resin_ids) = crate::onboarding::resin_rows(
                    &catalog,
                    st.printer.as_ref(),
                    &st.resin_query,
                    st.resin.as_ref(),
                    &tuned,
                );
                use crate::product_pictures::{Kind, picture};
                let printer_pictures: Vec<_> = printer_ids
                    .iter()
                    .map(|id| {
                        id.as_ref()
                            .and_then(|id| picture(Kind::Printer, id.as_str()))
                    })
                    .collect();
                let resin_pictures: Vec<_> = resin_ids
                    .iter()
                    .map(|id| id.as_ref().and_then(|id| picture(Kind::Resin, id.as_str())))
                    .collect();
                ui.set_setup_printer_groups(setup_groups(printers, printer_pictures));
                ui.set_setup_resin_groups(setup_groups(resins, resin_pictures));
                ui.set_setup_printer_picture(
                    st.printer
                        .as_ref()
                        .and_then(|id| picture(Kind::Printer, id.as_str()))
                        .unwrap_or_default(),
                );
                ui.set_setup_resin_picture(
                    st.resin
                        .as_ref()
                        .and_then(|id| picture(Kind::Resin, id.as_str()))
                        .unwrap_or_default(),
                );
                let printer_name = st
                    .printer
                    .as_ref()
                    .and_then(|id| catalog.printers().iter().find(|p| &p.id == id))
                    .map(|p| format!("{} {}", p.printer_maker, p.name))
                    .unwrap_or_default();
                let resin_name = st
                    .resin
                    .as_ref()
                    .and_then(|id| catalog.resin_list().iter().find(|m| &m.id == id))
                    .map(|m| format!("{} {}", m.brand, m.name))
                    .unwrap_or_default();
                ui.set_setup_printer_name(printer_name.into());
                ui.set_setup_resin_name(resin_name.into());
                ui.set_setup_theme_choice(st.theme);
                st.printer_ids = printer_ids;
                st.resin_ids = resin_ids;
            })
        };
        {
            let app = Rc::downgrade(self);
            let state = state.clone();
            let refresh = refresh.clone();
            let weak_ui = weak_ui.clone();
            ui.on_show_setup_requested(move || {
                let (Some(app), Some(ui)) = (app.upgrade(), weak_ui.upgrade()) else {
                    return;
                };
                let snap = app.preference_bus.current_values();
                *state.borrow_mut() = SetupDraft {
                    printer: snap.default_printer.clone(),
                    resin: snap.default_resin.clone(),
                    theme: match snap.theme {
                        crate::ThemePreference::Dark => 1,
                        crate::ThemePreference::Light => 0,
                        _ => 2,
                    },
                    ..SetupDraft::default()
                };
                refresh();
                ui.set_setup_step(0);
                ui.set_setup_visible(true);
            });
        }
        {
            let app = Rc::downgrade(self);
            let state = state.clone();
            let refresh = refresh.clone();
            ui.on_setup_theme_picked(move |choice| {
                let Some(app) = app.upgrade() else {
                    return;
                };
                state.borrow_mut().theme = choice;

                let theme = match choice {
                    1 => crate::ThemePreference::Dark,
                    2 => crate::ThemePreference::System,
                    _ => crate::ThemePreference::Light,
                };
                app.preference_bus.edit_preferences(move |snap| {
                    snap.theme = theme;
                    PreferenceEdit::Theme
                });
                refresh();
            });
        }
        {
            let state = state.clone();
            let refresh = refresh.clone();
            ui.on_setup_printer_search(move |text| {
                state.borrow_mut().printer_query = text.to_string();
                refresh();
            });
        }
        {
            let state = state.clone();
            let refresh = refresh.clone();
            ui.on_setup_resin_search(move |text| {
                state.borrow_mut().resin_query = text.to_string();
                refresh();
            });
        }
        {
            let app = Rc::downgrade(self);
            let state = state.clone();
            let refresh = refresh.clone();
            ui.on_setup_printer_clicked(move |row| {
                let Some(app) = app.upgrade() else {
                    return;
                };
                {
                    let mut st = state.borrow_mut();
                    let Some(Some(id)) = st.printer_ids.get(row as usize).cloned() else {
                        return;
                    };

                    let catalog = app.profile_catalog_cell.borrow().clone();
                    st.resin = catalog.usual_resin_for(Some(&id)).or(st.resin.take());
                    st.printer = Some(id);
                }
                refresh();
            });
        }
        {
            let state = state.clone();
            let refresh = refresh.clone();
            ui.on_setup_resin_clicked(move |row| {
                {
                    let mut st = state.borrow_mut();
                    let Some(Some(id)) = st.resin_ids.get(row as usize).cloned() else {
                        return;
                    };
                    st.resin = Some(id);
                }
                refresh();
            });
        }
        {
            let app = Rc::downgrade(self);
            let state = state.clone();
            ui.on_setup_custom_printer(move || {
                let Some(app) = app.upgrade() else {
                    return;
                };
                app.show_preferences();
                let catalog = app.profile_catalog_cell.borrow().clone();
                let index = state
                    .borrow()
                    .printer
                    .as_ref()
                    .and_then(|id| catalog.printer_position(id))
                    .unwrap_or(0);
                if let Some(prefs) = app.preferences_window.borrow().as_ref() {
                    prefs.window.invoke_open_printer(index as i32, true);
                }
            });
        }
        {
            let state = state.clone();
            let refresh = refresh.clone();
            let weak_ui = weak_ui.clone();
            let subscription = self.preference_bus.watch_preferences(move |snap, change| {
                if *change != PreferenceEdit::DefaultPrinter
                    || !weak_ui.upgrade().is_some_and(|ui| ui.get_setup_visible())
                {
                    return;
                }
                state.borrow_mut().printer = snap.default_printer.clone();
                refresh();
            });
            self.setup_subscriptions.borrow_mut().push(subscription);
        }
        {
            let app = Rc::downgrade(self);
            let weak_ui = weak_ui.clone();
            ui.on_setup_skip(move || {
                let (Some(app), Some(ui)) = (app.upgrade(), weak_ui.upgrade()) else {
                    return;
                };
                app.preference_bus.edit_preferences(|snap| {
                    snap.setup_complete = true;
                    PreferenceEdit::Setup
                });
                ui.set_setup_visible(false);
            });
        }
        {
            let app = Rc::downgrade(self);
            let state = state.clone();
            let weak_ui = weak_ui.clone();
            ui.on_setup_finish(move |open| {
                let (Some(app), Some(ui)) = (app.upgrade(), weak_ui.upgrade()) else {
                    return;
                };
                let (printer, resin) = {
                    let st = state.borrow();
                    (st.printer.clone(), st.resin.clone())
                };
                if let Some(id) = printer {
                    let envelope = app.profile_catalog_cell.borrow().volume_for_printer(Some(&id));
                    app.preference_bus.edit_preferences(move |snap| {
                        snap.default_printer = Some(id);
                        snap.build_volume = envelope;
                        PreferenceEdit::DefaultPrinter
                    });
                }
                if let Some(id) = resin {
                    app.preference_bus.edit_preferences(move |snap| {
                        snap.default_resin = Some(id);
                        PreferenceEdit::DefaultResin
                    });
                }
                app.preference_bus.edit_preferences(|snap| {
                    snap.setup_complete = true;
                    PreferenceEdit::Setup
                });
                ui.set_setup_visible(false);
                if open {
                    ui.invoke_open_file();
                }
            });
        }
    }

    pub(crate) fn needs_setup(&self) -> bool {
        !self.preference_bus.current_values().setup_complete
    }

    pub(crate) fn show_setup(&self, window_id: u64) {
        if let Some(w) = self.main_windows.borrow().get(&window_id) {
            w.main_window.invoke_show_setup_requested();
        }
    }

    fn show_preferences(&self) {
        crate::window_prefs::show_kept(
            &self.preferences_window,
            "The Preferences window",
            |p| p.window.show(),
            || build_preferences_window(&self.preference_bus, self.profile_catalog_cell.clone()),
        );
    }
}

fn setup_groups(
    rows: Vec<crate::onboarding::Row>,
    pictures: Vec<Option<slint::Image>>,
) -> slint::ModelRc<crate::SetupGroup> {
    let mut groups: Vec<(String, Vec<crate::SetupCard>)> = Vec::new();
    for (index, (row, picture)) in rows.into_iter().zip(pictures).enumerate() {
        if row.header {
            groups.push((row.title, Vec::new()));
            continue;
        }
        if groups.is_empty() {
            groups.push((String::new(), Vec::new()));
        }
        if let Some((_, cards)) = groups.last_mut() {
            cards.push(crate::SetupCard {
                index: index as i32,
                title: row.title.into(),
                subtitle: row.subtitle.into(),
                badge: row.badge.into(),
                selected: row.selected,
                picture: picture.unwrap_or_default(),
            });
        }
    }
    slint::ModelRc::new(slint::VecModel::from(
        groups
            .into_iter()
            .map(|(title, cards)| crate::SetupGroup {
                title: title.into(),
                cards: slint::ModelRc::new(slint::VecModel::from(cards)),
            })
            .collect::<Vec<_>>(),
    ))
}

#[cfg(test)]
#[path = "tests_app_state.rs"]
mod tests;

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! How each window shows the user's preferences, and the main window's saved
//! size, position and sidebar groups.
//!
//! [`TakesPreferences`] is what a window does with a snapshot; when it gets
//! one is up to whoever opened the window, which subscribes to the
//! `PreferenceHub`. Language needs nothing here: Slint's `@tr` strings
//! follow the active translation by themselves.

use std::cell::RefCell;
use std::rc::Rc;

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::{
    AboutWindow, MainWindow, MenuLayout, PreferencesWindow, ShortcutsDialog,
    aa_level::aa_row_of,
    language::LanguagePreference,
    preferences_bus::{PreferenceEdit, PreferenceHub, PreferenceWatch, UserPreferences},
};

/// A window besides the main one (About, Keyboard Shortcuts) that keeps
/// up with the preferences for as long as this value lives; dropping it
/// ends the subscription, so nothing reaches a window that is gone.
pub(crate) struct SideWindow<W: ComponentHandle> {
    window: W,
    _watch: PreferenceWatch,
}

impl<W: ComponentHandle + TakesPreferences + 'static> SideWindow<W> {
    /// Shows the current preferences in `window` and every later change.
    /// `on_close` connects the window's own close callback to the given
    /// handler, which hides it (it is kept to be shown again).
    pub(crate) fn follow(bus: &Rc<PreferenceHub>, window: W, on_close: impl FnOnce(&W, Box<dyn Fn()>)) -> Self {
        window.take_preferences(&bus.current_values());
        let to_hide = window.as_weak();
        on_close(&window, Box::new(move || {
            to_hide.upgrade().map(|w| w.hide());
        }));
        let to_update = window.as_weak();
        let watch = bus.watch_preferences(move |values, _| {
            if let Some(w) = to_update.upgrade() {
                w.take_preferences(values);
            }
        });
        SideWindow { window, _watch: watch }
    }

    pub(crate) fn show(&self) -> Result<(), slint::PlatformError> {
        self.window.show()
    }
}

/// Shows the window kept in `slot`, creating it with `create` the first
/// time. A window that cannot be created or shown is reported on stderr
/// (named `what`) and not kept, so the next request tries again.
pub(crate) fn show_kept<T>(
    slot: &RefCell<Option<T>>,
    what: &str,
    show: impl Fn(&T) -> Result<(), slint::PlatformError>,
    create: impl FnOnce() -> Result<T, slint::PlatformError>,
) {
    let mut slot = slot.borrow_mut();
    if let Some(kept) = slot.as_ref() {
        let _ = show(kept);
        return;
    }
    match create().and_then(|made| show(&made).map(|()| made)) {
        Ok(made) => *slot = Some(made),
        Err(err) => eprintln!("{what} could not be opened: {err}"),
    }
}

/// Whether a preference change touches what a main window shows from the
/// snapshot (theme, menu layout, language, recent files). Panel layout,
/// folders, defaults and the rest do not (R117: hiding the object panel
/// re-applied everything, and the fresh recent-files model rebuilt the
/// native menu bar, which flashed the window).
pub(crate) fn changes_main_window(change: &PreferenceEdit) -> bool {
    matches!(
        change,
        PreferenceEdit::Theme
            | PreferenceEdit::MenuLayout
            | PreferenceEdit::Language
            | PreferenceEdit::RecentFiles
    )
}

/// Whether `shown` (the menu's recent-files model) differs from `files`.
fn recent_files_differ(shown: &ModelRc<SharedString>, files: &[std::path::PathBuf]) -> bool {
    shown.row_count() != files.len()
        || files
            .iter()
            .enumerate()
            .any(|(i, p)| shown.row_data(i).as_deref() != Some(p.display().to_string().as_str()))
}

/// A window that shows some of the preferences.
pub(crate) trait TakesPreferences {
    /// Shows `snapshot`'s values that this window uses.
    fn take_preferences(&self, snapshot: &UserPreferences);
}

impl TakesPreferences for MainWindow {
    fn take_preferences(&self, snapshot: &UserPreferences) {
        self.set_theme(snapshot.theme);
        // macOS always has its menus in the screen's top bar, so the
        // compact menu layout does not apply there.
        self.set_menu_style(match cfg!(target_os = "macos") {
            true => MenuLayout::Bar,
            false => snapshot.menu_layout,
        });
        // Only a changed list replaces the model, which rebuilds the native
        // menu bar (R117).
        if recent_files_differ(&self.get_recent_files(), &snapshot.recent_files) {
            let shown: Vec<SharedString> = snapshot
                .recent_files
                .iter()
                .map(|path| path.display().to_string().into())
                .collect();
            self.set_recent_files(ModelRc::new(VecModel::from(shown)));
        }
    }
}

impl TakesPreferences for PreferencesWindow {
    fn take_preferences(&self, snapshot: &UserPreferences) {
        self.set_theme(snapshot.theme);
        // The language list is built again because its "System" row is
        // itself translated.
        self.set_languages(LanguagePreference::menu_rows());
        self.set_language_row(snapshot.language.combo_index());
        self.set_aa_row(aa_row_of(snapshot.aa_level));
        self.set_currency(snapshot.currency_symbol.as_str().into());
        // The printer and resin lists come from the catalogue, not the
        // snapshot: `preferences_window::apply_printing` fills them.
    }
}

impl TakesPreferences for AboutWindow {
    fn take_preferences(&self, snapshot: &UserPreferences) {
        self.set_about_color_preference(snapshot.theme);
    }
}

impl TakesPreferences for ShortcutsDialog {
    fn take_preferences(&self, snapshot: &UserPreferences) {
        // The shortcut rows are fixed text built when the dialog opens.
        self.set_theme(snapshot.theme);
    }
}

/// Saved desktop geometry in logical pixels.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
struct WindowGeometry {
    width: f32,
    height: f32,
    x: i32,
    y: i32,
}

/// The folder the window and sidebar settings live in.
pub(crate) fn settings_dir() -> Option<std::path::PathBuf> {
    sc_app_paths::project_dirs().map(|p| p.config_dir().to_path_buf())
}

fn geometry_path() -> Option<std::path::PathBuf> {
    sc_app_paths::project_dirs().map(|p| p.config_dir().join("window-geometry.json"))
}

fn initial_size(screen: (f32, f32)) -> (f32, f32) {
    (
        (screen.0 * 0.85).clamp(1100.0, 1600.0),
        (screen.1 * 0.85).clamp(700.0, 1000.0),
    )
}

pub(crate) fn restore_geometry(ui: &MainWindow) {
    install_sidebar_preferences(ui);
    use slint::{ComponentHandle, winit_030::WinitWindowAccessor};
    let saved = geometry_path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice::<WindowGeometry>(&b).ok());
    ui.window().with_winit_window(|window| {
        let monitor = window.current_monitor();
        let screen = monitor
            .as_ref()
            .map(|m| m.size().to_logical::<f32>(m.scale_factor()))
            .map_or((1920.0, 1080.0), |s| (s.width, s.height));
        let (width, height) = saved.map_or_else(
            || initial_size(screen),
            |s| (s.width.max(1100.0), s.height.max(700.0)),
        );
        ui.window().set_size(slint::LogicalSize::new(width, height));
        if let Some(saved) = saved {
            // Restore only a position that intersects a connected monitor.
            let visible = window.available_monitors().any(|m| {
                let p = m.position();
                let s = m.size();
                saved.x < p.x + s.width as i32
                    && saved.x + width as i32 > p.x
                    && saved.y < p.y + s.height as i32
                    && saved.y + height as i32 > p.y
            });
            if visible {
                ui.window()
                    .set_position(slint::PhysicalPosition::new(saved.x, saved.y));
            }
        }
    });
}

pub(crate) fn record_geometry(window: &slint::winit_030::winit::window::Window) {
    // Automated verification must not overwrite the user's desktop arrangement.
    if std::env::args_os().any(|a| a == "--screenshot" || a == "--window") {
        return;
    }
    let size = window.inner_size().to_logical::<f32>(window.scale_factor());
    let Ok(position) = window.outer_position() else {
        return;
    };
    if size.width < 1100.0 || size.height < 700.0 {
        return;
    }
    let geometry = WindowGeometry {
        width: size.width,
        height: size.height,
        x: position.x,
        y: position.y,
    };
    if let Some(path) = geometry_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(bytes) = serde_json::to_vec(&geometry) {
            let _ = std::fs::write(path, bytes);
        }
    }
}

#[cfg(test)]
mod geometry_tests {
    use super::*;
    #[test]
    fn initial_size_respects_screen_fraction_cap_and_minimum() {
        assert_eq!(initial_size((3840.0, 2160.0)), (1600.0, 1000.0));
        assert_eq!(initial_size((1600.0, 1000.0)), (1360.0, 850.0));
        assert_eq!(initial_size((1000.0, 600.0)), (1100.0, 700.0));
    }
    #[test]
    fn saved_geometry_roundtrips() {
        let value = WindowGeometry {
            width: 1500.0,
            height: 900.0,
            x: -1500,
            y: 30,
        };
        assert_eq!(
            serde_json::from_slice::<WindowGeometry>(&serde_json::to_vec(&value).unwrap()).unwrap(),
            value
        );
    }
}

/// A separate additive document keeps group expansion independent of window geometry.
#[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq)]
struct SidebarPreferences {
    expanded: [bool; 3],
    #[serde(default)]
    export_format: i32,
    /// The Supports group (R97); files saved before it open it.
    #[serde(default = "expanded_by_default")]
    supports_expanded: bool,
    /// The sidebar tab (R98).
    #[serde(default)]
    settings_tab: i32,
}

fn expanded_by_default() -> bool {
    true
}

fn install_sidebar_preferences(ui: &MainWindow) {
    use slint::ComponentHandle;
    let Some(path) = geometry_path().map(|p| p.with_file_name("sidebar.json")) else {
        return;
    };
    let automated = std::env::args_os().any(|a| a == "--screenshot");
    if !automated
        && let Some(saved) = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<SidebarPreferences>(&b).ok())
    {
        ui.set_printer_expanded(saved.expanded[0]);
        ui.set_material_expanded(saved.expanded[1]);
        ui.set_process_expanded(saved.expanded[2]);
        ui.set_supports_expanded(saved.supports_expanded);
        ui.set_settings_tab(saved.settings_tab.clamp(0, 4));
        ui.set_export_format(saved.export_format.clamp(0, 3));
    }
    let weak = ui.as_weak();
    ui.on_sidebar_changed(move || {
        if automated {
            return;
        }
        if let Some(ui) = weak.upgrade() {
            let state = SidebarPreferences {
                export_format: ui.get_export_format(),
                supports_expanded: ui.get_supports_expanded(),
                settings_tab: ui.get_settings_tab(),
                expanded: [
                    ui.get_printer_expanded(),
                    ui.get_material_expanded(),
                    ui.get_process_expanded(),
                ],
            };
            if let Ok(bytes) = serde_json::to_vec(&state) {
                let _ = std::fs::write(&path, bytes);
            }
        }
    });
}

#[cfg(test)]
mod sidebar_preferences_tests {
    use super::*;
    #[test]
    fn collapsed_groups_round_trip_independently() {
        let state = SidebarPreferences {
            expanded: [false, true, false],
            export_format: 2,
            supports_expanded: false,
            settings_tab: 3,
        };
        assert_eq!(
            serde_json::from_slice::<SidebarPreferences>(&serde_json::to_vec(&state).unwrap())
                .unwrap(),
            state
        );
    }

    /// R97: sidebar files saved before the Supports group open it expanded.
    #[test]
    fn older_files_open_the_supports_group() {
        let old: SidebarPreferences =
            serde_json::from_str(r#"{"expanded":[true,true,true],"export_format":0}"#).unwrap();
        assert!(old.supports_expanded);
    }
}

#[cfg(test)]
mod r117_tests {
    use super::*;

    /// R117: panel and folder changes leave the main window alone; the
    /// recent-files list is compared before it replaces the menu's model.
    #[test]
    fn panel_changes_do_not_reapply_and_recent_files_compare() {
        for change in [
            PreferenceEdit::ObjectListPanel,
            PreferenceEdit::SliceControlPanel,
            PreferenceEdit::SliceViewPanel,
            PreferenceEdit::LastOpenDir,
        ] {
            assert!(!changes_main_window(&change), "{change:?}");
        }
        assert!(changes_main_window(&PreferenceEdit::Theme));
        assert!(changes_main_window(&PreferenceEdit::RecentFiles));
        let files = vec![std::path::PathBuf::from("a.stl"), std::path::PathBuf::from("b.stl")];
        let shown: ModelRc<SharedString> =
            ModelRc::new(VecModel::from(vec![SharedString::from("a.stl"), SharedString::from("b.stl")]));
        assert!(!recent_files_differ(&shown, &files));
        assert!(recent_files_differ(&shown, &files[..1]));
        assert!(recent_files_differ(&shown, &[std::path::PathBuf::from("c.stl"), files[1].clone()]));
    }
}

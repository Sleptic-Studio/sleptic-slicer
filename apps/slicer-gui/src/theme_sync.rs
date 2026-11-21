// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Keep independently instantiated window palettes in step with the main window.
use crate::{MainWindow, ThemePreference};
use slint::ComponentHandle;
use std::cell::{Cell, RefCell};
type ThemeListener = Box<dyn Fn(ThemePreference) -> bool>;
thread_local! {
    static CURRENT:Cell<ThemePreference>=const {Cell::new(ThemePreference::System)};
    static WINDOWS:RefCell<Vec<ThemeListener>>=const {RefCell::new(Vec::new())};
}
pub(crate) fn track<T: ComponentHandle + 'static>(window: &T, apply: fn(&T, ThemePreference)) {
    CURRENT.with(|theme| apply(window, theme.get()));
    let weak = window.as_weak();
    WINDOWS.with(|windows| {
        windows.borrow_mut().push(Box::new(move |theme| {
            if let Some(window) = weak.upgrade() {
                apply(&window, theme);
                true
            } else {
                false
            }
        }))
    });
}
pub(crate) fn install(ui: &MainWindow) {
    CURRENT.with(|theme| theme.set(ui.get_theme()));
    let weak = ui.as_weak();
    ui.on_shell_theme_changed(move || {
        if let Some(ui) = weak.upgrade() {
            let theme = ui.get_theme();
            CURRENT.with(|current| current.set(theme));
            WINDOWS.with(|windows| windows.borrow_mut().retain(|apply| apply(theme)));
        }
    });
}

#[cfg(test)]
mod tests {
    use slint::ComponentHandle;

    use crate::*;

    /// Every separate window follows the theme it is given, in its own
    /// copy of the shell colours, from the start and on every change.
    #[test]
    fn every_separate_window_follows_its_theme() {
        i_slint_backend_testing::init_no_event_loop();
        macro_rules! check {
            ($($window:ident : $setter:ident),*) => {$({
                let window = $window::new().unwrap();
                let light = || window.global::<ShellTheme>().get_light();
                assert!(!light(), "{} starts dark", stringify!($window));
                // `changed` handlers run on a later turn of the event loop.
                let settle = || i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(1));
                window.$setter(ThemePreference::Light);
                settle();
                assert!(light(), "{} turns light", stringify!($window));
                window.$setter(ThemePreference::Dark);
                settle();
                assert!(!light(), "{} turns dark again", stringify!($window));
            })*};
        }
        check!(
            PrinterEditor: set_theme, ProfileViewDialog: set_viewer_theme, ExtendDialog: set_extension_dialog_theme, ForkDialog: set_fork_dialog_theme, ErrorDialog: set_theme, InfoDialog: set_theme,
            SliceOverwriteDialog: set_theme, AboutWindow: set_about_color_preference, ShortcutsDialog: set_theme, PreferencesWindow: set_theme
        );
    }
}

#[cfg(test)]
mod size_tests {
    #[test]
    fn confirmation_windows_use_minimum_content_height_as_resize_limit() {
        for source in [
            include_str!("../ui/slice-overwrite-dialog.slint"),
        ] {
            assert!(source.contains("min-height: body.min-height;"));
            assert!(!source.contains("min-height: body.preferred-height;"));
        }
    }
}

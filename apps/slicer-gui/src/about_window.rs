// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Help > About: the version number, in the user's theme and language.
use std::rc::Rc;

use crate::AboutWindow;
use crate::preferences_bus::PreferenceHub;
use crate::window_prefs::SideWindow;

pub(crate) fn open_about_window(bus: &Rc<PreferenceHub>) -> Result<SideWindow<AboutWindow>, slint::PlatformError> {
    let about = AboutWindow::new()?;
    about.set_about_release_label(env!("CARGO_PKG_VERSION").into());
    Ok(SideWindow::follow(bus, about, |w, done| w.on_close_requested(done)))
}

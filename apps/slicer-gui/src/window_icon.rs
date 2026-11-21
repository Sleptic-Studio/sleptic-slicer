// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Use the embedded application mark for a live native window.

use slint::winit_030::{WinitWindowAccessor, winit::window::Icon};
use std::io::Cursor;

const ICON_BYTES: &[u8] = include_bytes!("../assets/icons/sleptic.ico");

pub(crate) fn apply(window: &slint::Window) {
    if let Some(icon) = app_icon() {
        window.with_winit_window(move |native| native.set_window_icon(Some(icon)));
    }
}

fn selected_image(bytes: &[u8]) -> Option<ico::IconImage> {
    let directory = ico::IconDir::read(Cursor::new(bytes)).ok()?;
    let entries = directory.entries();
    let selected = (0..entries.len()).max_by_key(|&index| {
        let entry = &entries[index];
        (u64::from(entry.width()) * u64::from(entry.height()), index)
    })?;
    entries[selected].decode().ok()
}

fn app_icon() -> Option<Icon> {
    let image = selected_image(ICON_BYTES)?;
    Icon::from_rgba(image.rgba_data().to_owned(), image.width(), image.height()).ok()
}

#[cfg(test)]
#[path = "tests_window_icon.rs"]
mod tests_window_icon;

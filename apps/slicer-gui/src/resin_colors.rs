// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The colour each resin is shown in (R121: the maintainer wanted the
//! models in the colour of the resin selected). A resin profile has no
//! colour, and one product comes in many, so the colour is guessed from the
//! resin's name (`slice_panel::resin_color`) unless the user picks the
//! colour of the bottle they actually use; picks are kept per resin in
//! `resin_colors.json` beside the window settings.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// The palette offered beside the Resin dropdown: the common resin colours.
pub(crate) const SWATCHES: [(&str, [u8; 3]); 14] = [
    ("Grey", [0x8c, 0x91, 0x96]),
    ("White", [0xf2, 0xf2, 0xee]),
    ("Black", [0x3a, 0x3a, 0x3e]),
    ("Clear", [0xcf, 0xe8, 0xf5]),
    ("Beige", [0xe3, 0xca, 0xa5]),
    ("Apricot", [0xf2, 0xb0, 0x7e]),
    ("Red", [0xd9, 0x44, 0x3b]),
    ("Orange", [0xf0, 0x8a, 0x2c]),
    ("Yellow", [0xf2, 0xc2, 0x30]),
    ("Green", [0x3f, 0xa3, 0x5b]),
    ("Cyan", [0x3c, 0xb8, 0xd6]),
    ("Blue", [0x3d, 0x7e, 0xe0]),
    ("Purple", [0x8a, 0x5c, 0xd6]),
    ("Pink", [0xec, 0x7f, 0xb0]),
];

fn path() -> Option<PathBuf> {
    crate::window_prefs::settings_dir().map(|d| d.join("resin_colors.json"))
}

fn load() -> BTreeMap<String, [u8; 3]> {
    path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Automated runs (screenshots, tests) never read or write the user's
/// picks, so they see the name-based colours.
fn automated() -> bool {
    cfg!(test) || std::env::args_os().any(|a| a == "--screenshot")
}

/// The colour the user picked for resin `id`, if any.
pub(crate) fn picked(id: &str) -> Option<[u8; 3]> {
    if automated() {
        return None;
    }
    load().get(id).copied()
}

/// Remembers `rgb` for resin `id`, or forgets the pick (`None`).
pub(crate) fn set(id: &str, rgb: Option<[u8; 3]>) {
    if automated() {
        return;
    }
    let mut all = load();
    match rgb {
        Some(rgb) => all.insert(id.to_string(), rgb),
        None => all.remove(id),
    };
    if let (Some(p), Ok(bytes)) = (path(), serde_json::to_vec_pretty(&all)) {
        let _ = std::fs::write(p, bytes);
    }
}

/// The colour models are drawn in: the pick, else the colour in the name.
pub(crate) fn body_color(id: &str, name: &str) -> slint::Color {
    match picked(id) {
        Some([r, g, b]) => slint::Color::from_rgb_u8(r, g, b),
        None => crate::slice_panel::resin_color(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R121: without a pick a resin is drawn in the colour in its name
    /// (grey when there is none), the shader takes the models' colour from
    /// the uniform, and the palette's 14 colours are distinct.
    #[test]
    fn models_take_the_resin_colour() {
        let green = body_color("phrozen-nylon-green", "Phrozen Nylon Green");
        assert!(green.green() > green.red() && green.green() > green.blue());
        let plain = body_color("anycubic-craftsman", "Anycubic Craftsman Resin");
        assert_eq!((plain.red(), plain.green(), plain.blue()), (0x9a, 0xa0, 0xa6));
        let clear = body_color("x", "Siraya Tech Craft Ultra Clear");
        assert!(clear.blue() > 0xe0);
        let mut seen: Vec<[u8; 3]> = SWATCHES.iter().map(|(_, c)| *c).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), SWATCHES.len());
        let shader = include_str!("mesh_render/shaders/mesh_render.wgsl");
        assert!(shader.contains("var own = scene.resin.rgb;"));
        let ui = include_str!("../ui/main-window.slint");
        assert!(ui.contains("background: root.resin-color;") && ui.contains("root.resin-color-picked(-1)"));
    }
}

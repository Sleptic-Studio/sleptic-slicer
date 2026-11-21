// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Pictures of the selected printer and resin beside their pickers, as Orca
//! shows the printer (R100). Each profile has its own render, embedded in the
//! program (`assets/pictures`, prepared by `scripts/dev/product_pictures.py`).
//! A user's own photo at `<data dir>/pictures/<printer|resin>/<id>.png` wins,
//! for a home-built printer above all.

use std::cell::RefCell;
use std::collections::HashMap;

/// Which picker a picture belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Kind {
    Printer,
    Resin,
}

impl Kind {
    fn folder(self) -> &'static str {
        match self {
            Kind::Printer => "printer",
            Kind::Resin => "resin",
        }
    }
}

/// `(kind folder, profile id, PNG bytes)` for every embedded picture.
const PICTURES: &[(&str, &str, &[u8])] = &include!("product_pictures_list.rs");

/// The embedded PNG for a profile, if it has one.
pub(crate) fn embedded(kind: Kind, id: &str) -> Option<&'static [u8]> {
    PICTURES
        .iter()
        .find(|(k, i, _)| *k == kind.folder() && *i == id)
        .map(|(_, _, bytes)| *bytes)
}

fn decode(bytes: &[u8]) -> Option<slint::Image> {
    let rgba = crate::safe_decode::decode_guarded(&"a built-in product picture", || {
        image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()
    })?
    .to_rgba8();
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        rgba.as_raw(),
        rgba.width(),
        rgba.height(),
    );
    Some(slint::Image::from_rgba8(buffer))
}

thread_local! {
    static CACHE: RefCell<HashMap<(Kind, String), Option<slint::Image>>> = RefCell::new(HashMap::new());
}

/// The picture for a printer or resin profile: the user's own photo, else the
/// embedded render, else none (the UI then draws its generic picture).
pub(crate) fn picture(kind: Kind, id: &str) -> Option<slint::Image> {
    CACHE.with(|cache| {
        cache
            .borrow_mut()
            .entry((kind, id.to_owned()))
            .or_insert_with(|| {
                let own = crate::print_history::data_dir()
                    .map(|d| d.join("pictures").join(kind.folder()).join(format!("{id}.png")))
                    .filter(|p| p.is_file())
                    .and_then(|p| std::fs::read(p).ok())
                    .and_then(|bytes| decode(&bytes));
                own.or_else(|| embedded(kind, id).and_then(decode))
            })
            .clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R100: every bundled printer and resin profile has its own picture, so
    /// the pickers look finished for all of them.
    #[test]
    fn every_bundled_profile_has_a_picture() {
        let profiles = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/profiles");
        let mut missing = Vec::new();
        for (kind, dir) in [(Kind::Printer, "printers"), (Kind::Resin, "materials")] {
            for entry in std::fs::read_dir(profiles.join(dir)).unwrap() {
                let name = entry.unwrap().file_name().to_string_lossy().to_string();
                if let Some(id) = name.strip_suffix(".toml")
                    && embedded(kind, id).is_none()
                {
                    missing.push(format!("{}/{id}", kind.folder()));
                }
            }
        }
        // Profiles added before their picture could be made are listed, and
        // only while they still have none.
        let awaiting: Vec<&str> = include_str!("../assets/pictures/awaiting.txt")
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect();
        let unlisted: Vec<&String> = missing.iter().filter(|m| !awaiting.contains(&m.as_str())).collect();
        assert!(unlisted.is_empty(), "profiles without a picture: {unlisted:?}");
        let stale: Vec<&&str> = awaiting.iter().filter(|a| !missing.iter().any(|m| m == **a)).collect();
        assert!(stale.is_empty(), "listed as awaiting a picture but have one (or no longer exist): {stale:?}");
    }

    /// Every embedded picture decodes to a square image with see-through
    /// corners (it sits on the sidebar's own background).
    #[test]
    fn embedded_pictures_decode_square_and_transparent() {
        assert!(!PICTURES.is_empty());
        for (kind, id, bytes) in PICTURES {
            let rgba = image::load_from_memory(bytes).unwrap().to_rgba8();
            assert_eq!(rgba.width(), rgba.height(), "{kind}/{id}");
            assert_eq!(rgba.get_pixel(0, 0)[3], 0, "{kind}/{id} corner is not transparent");
        }
        assert!(picture(Kind::Printer, "anycubic-photon-mono-m7-pro-pwsz").is_some());
        assert!(picture(Kind::Printer, "no-such-printer").is_none());
    }

    /// Where the printer's nearest base corner sits across its width, from -0.5
    /// (left edge) to 0.5 (right edge): the lowest stretch of its silhouette.
    /// Seen from the front-left the left side panel shows and that corner is
    /// left of centre; seen from the front-right, right of it.
    fn nearest_corner(rgba: &image::RgbaImage) -> f32 {
        let solid = |x: u32, y: u32| rgba.get_pixel(x, y)[3] > 160;
        let bottoms: Vec<(u32, u32)> = (0..rgba.width())
            .filter_map(|x| (0..rgba.height()).rev().find(|&y| solid(x, y)).map(|y| (x, y)))
            .collect();
        let (left, right) = (bottoms[0].0 as f32, bottoms[bottoms.len() - 1].0 as f32);
        let mut heights: Vec<u32> = bottoms.iter().map(|&(_, y)| y).collect();
        heights.sort_unstable();
        let low = heights[heights.len() * 95 / 100];
        let near: Vec<f32> = bottoms.iter().filter(|&&(_, y)| y >= low).map(|&(x, _)| x as f32).collect();
        (near.iter().sum::<f32>() / near.len() as f32 - 0.5 * (left + right)) / (right - left)
    }

    /// Printer pictures sit side by side in the chooser, so they are all seen
    /// from the same side: the front-left (R272; four came out mirrored).
    #[test]
    fn printer_pictures_are_all_seen_from_the_front_left() {
        let mut other_way = Vec::new();
        for (kind, id, bytes) in PICTURES.iter().filter(|(kind, ..)| *kind == "printer") {
            let rgba = image::load_from_memory(bytes).unwrap().to_rgba8();
            let corner = nearest_corner(&rgba);
            if corner >= 0.0 {
                other_way.push(format!("{kind}/{id} ({corner:.2})"));
            }
        }
        assert!(other_way.is_empty(), "seen from the front-right: {other_way:?}");
        // The measure tells the two views apart: a mirrored picture fails it.
        let mirrored = image::imageops::flip_horizontal(
            &image::load_from_memory(embedded(Kind::Printer, "elegoo-mars-2-ctb").unwrap()).unwrap().to_rgba8(),
        );
        assert!(nearest_corner(&mirrored) > 0.0);
    }
}

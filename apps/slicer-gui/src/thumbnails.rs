// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Small rendered previews of opened / saved files, shown on the Home tab's
//! recent-project cards. One PNG per source path in the data directory;
//! refreshed whenever the file is opened or saved.

use std::path::{Path, PathBuf};

use sc_scene::Scene;

pub(crate) const WIDTH: u32 = 320;
pub(crate) const HEIGHT: u32 = 180;
/// Neutral grey that reads as a "photo well" on both themes' cards.
const BACKGROUND: [u8; 3] = [46, 49, 56];

/// Stable cache file for `source` (FNV-1a of the case-folded path, since
/// Windows paths are case-insensitive).
pub(crate) fn path_for(source: &Path) -> Option<PathBuf> {
    let dir = crate::print_history::data_dir()?.join("thumbnails");
    Some(dir.join(format!("{:016x}.png", fnv1a(source))))
}

fn fnv1a(source: &Path) -> u64 {
    source
        .to_string_lossy()
        .to_lowercase()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        })
}

/// Render `scene` and write it as `source`'s thumbnail. Scenes with nothing
/// to draw (empty, or only CSG the merge refuses) leave any old file alone.
pub(crate) fn write(source: &Path, scene: &Scene) -> Result<(), String> {
    let Some(mesh) = scene.merge_meshes().map_err(|e| e.to_string())? else {
        return Ok(());
    };
    let pixels = sc_mesh_formats::preview::render_mesh(&mesh, WIDTH, HEIGHT, BACKGROUND);
    let target = path_for(source).ok_or("no data directory")?;
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = target.with_extension("png.tmp");
    image::save_buffer_with_format(
        &tmp,
        &pixels,
        WIDTH,
        HEIGHT,
        image::ExtendedColorType::Rgb8,
        image::ImageFormat::Png,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &target).map_err(|e| e.to_string())
}

/// Write the thumbnail off the UI thread; failures only cost the preview.
pub(crate) fn write_in_background(source: PathBuf, scene: Scene) {
    std::thread::spawn(move || {
        if let Err(e) = write(&source, &scene) {
            eprintln!("Thumbnail for {}: {e}", source.display());
        }
    });
}

/// The cached thumbnail for `source`, if one was written. Decoded here
/// rather than via `Image::load_from_path`, whose cache would keep showing
/// the old picture after a re-save.
pub(crate) fn load(source: &Path) -> Option<slint::Image> {
    let cached = path_for(source)?;
    // A damaged cache file must not take the window down with it.
    let rgb = crate::safe_decode::decode_guarded(&cached.display(), || image::open(&cached).ok())?.into_rgb8();
    let buffer = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::clone_from_slice(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
    );
    Some(slint::Image::from_rgb8(buffer))
}

#[cfg(test)]
mod tests {
    use super::*;

    // R31: one cache file per source path, insensitive to Windows path case.
    #[test]
    fn cache_name_is_stable_and_case_insensitive() {
        let a = fnv1a(Path::new(r"C:\Models\Benchy.stl"));
        assert_eq!(a, fnv1a(Path::new(r"c:\models\BENCHY.STL")));
        assert_ne!(a, fnv1a(Path::new(r"C:\Models\Benchy2.stl")));
    }
}

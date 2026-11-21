// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Render the packaging SVG as the seven Windows icon sizes.

use resvg::{tiny_skia, usvg};
use std::{error::Error, path::Path};

type Failure = Box<dyn Error>;
const SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

fn icon_entry(tree: &usvg::Tree, size: u32, extent: f32) -> Result<ico::IconDirEntry, Failure> {
    let mut canvas =
        tiny_skia::Pixmap::new(size, size).ok_or("failed to allocate the render pixmap")?;
    let ratio = size as f32 / extent;
    resvg::render(
        tree,
        tiny_skia::Transform::from_scale(ratio, ratio),
        &mut canvas.as_mut(),
    );
    let rgba = canvas
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let straight = pixel.demultiply();
            [
                straight.red(),
                straight.green(),
                straight.blue(),
                straight.alpha(),
            ]
        })
        .collect();
    let image = ico::IconImage::from_rgba_data(size, size, rgba);
    Ok(ico::IconDirEntry::encode(&image)?)
}

fn icon_directory(tree: &usvg::Tree) -> Result<ico::IconDir, Failure> {
    let extent = tree.size().width().max(tree.size().height());
    if extent <= 0.0 {
        return Err("the SVG reports a non-positive size".into());
    }
    let mut directory = ico::IconDir::new(ico::ResourceType::Icon);
    for size in SIZES {
        directory.add_entry(icon_entry(tree, size, extent)?);
    }
    Ok(directory)
}

fn run() -> Result<(), Failure> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .ok_or("cannot locate the workspace root from CARGO_MANIFEST_DIR")?;
    let source = root.join("packages/shared/com.sleptic.slicer.svg");
    let target = root.join("apps/slicer-gui/assets/icons/sleptic.ico");
    let bytes =
        std::fs::read(&source).map_err(|error| format!("reading {}: {error}", source.display()))?;
    let tree = usvg::Tree::from_data(&bytes, &usvg::Options::default())
        .map_err(|error| format!("parsing {}: {error}", source.display()))?;
    let directory = icon_directory(&tree)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("creating {}: {error}", parent.display()))?;
    }
    let output = std::fs::File::create(&target)
        .map_err(|error| format!("creating {}: {error}", target.display()))?;
    directory
        .write(output)
        .map_err(|error| format!("writing {}: {error}", target.display()))?;
    println!("wrote {} ({} sizes)", target.display(), SIZES.len());
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("icon-gen: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

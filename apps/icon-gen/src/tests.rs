// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

use super::*;

fn render(svg: &str) -> ico::IconDir {
    let tree = usvg::Tree::from_data(svg.as_bytes(), &usvg::Options::default()).unwrap();
    icon_directory(&tree).unwrap()
}

#[test]
fn every_required_size_is_present_and_decodes() {
    let directory = render(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><circle cx="8" cy="8" r="6" fill="blue"/></svg>"#,
    );
    assert_eq!(directory.entries().len(), 7);
    for (entry, size) in directory
        .entries()
        .iter()
        .zip([16, 24, 32, 48, 64, 128, 256])
    {
        let image = entry.decode().unwrap();
        assert_eq!((image.width(), image.height()), (size, size));
        assert_eq!(image.rgba_data().len(), (size * size * 4) as usize);
    }
}

#[test]
fn non_square_art_keeps_its_origin_and_transparent_unused_space() {
    let directory = render(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="1"><rect width="2" height="1" fill="red"/></svg>"#,
    );
    let image = directory.entries()[0].decode().unwrap();
    let pixel = |x: usize, y: usize| &image.rgba_data()[(y * 16 + x) * 4..(y * 16 + x + 1) * 4];
    assert_eq!(pixel(8, 2), [255, 0, 0, 255]);
    assert_eq!(pixel(8, 12), [0, 0, 0, 0]);
}

#[test]
fn icon_rgba_is_straight_alpha() {
    let directory = render(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><rect width="1" height="1" fill="red" opacity="0.5"/></svg>"#,
    );
    let image = directory.entries()[0].decode().unwrap();
    let middle = &image.rgba_data()[(8 * 16 + 8) * 4..(8 * 16 + 9) * 4];
    assert_eq!(middle, [255, 0, 0, 128]);
}

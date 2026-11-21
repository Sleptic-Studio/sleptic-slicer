// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

use super::*;

fn archive(images: &[(u32, u32, [u8; 4])]) -> Vec<u8> {
    let mut directory = ico::IconDir::new(ico::ResourceType::Icon);
    for (width, height, rgba) in images {
        let pixels = rgba.repeat((*width * *height) as usize);
        let image = ico::IconImage::from_rgba_data(*width, *height, pixels);
        directory.add_entry(ico::IconDirEntry::encode_as_png(&image).unwrap());
    }
    let mut bytes = Vec::new();
    directory.write(&mut bytes).unwrap();
    bytes
}

#[test]
fn the_embedded_frames_keep_the_application_mark_and_transparency() {
    let directory = ico::IconDir::read(Cursor::new(ICON_BYTES)).unwrap();
    assert_eq!(
        directory
            .entries()
            .iter()
            .map(|entry| entry.width())
            .collect::<Vec<_>>(),
        [16, 24, 32, 48, 64, 128, 256]
    );
    let image = selected_image(ICON_BYTES).unwrap();
    assert_eq!((image.width(), image.height()), (256, 256));
    let pixel = |x: usize, y: usize| &image.rgba_data()[(y * 256 + x) * 4..(y * 256 + x + 1) * 4];
    assert_eq!(pixel(0, 0)[3], 0);
    assert!(pixel(128, 40)[2] > pixel(128, 40)[0]);
    assert!(pixel(128, 40)[3] > 240);
    assert_eq!(pixel(128, 85)[3], 0);
    assert!(app_icon().is_some());
}

#[test]
fn image_area_wins_and_the_later_entry_breaks_ties() {
    let bytes = archive(&[
        (32, 16, [255, 0, 0, 255]),
        (24, 24, [0, 255, 0, 255]),
        (24, 24, [0, 0, 255, 255]),
    ]);
    let image = selected_image(&bytes).unwrap();
    assert_eq!((image.width(), image.height()), (24, 24));
    assert_eq!(&image.rgba_data()[..4], &[0, 0, 255, 255]);
}

#[test]
fn malformed_empty_and_corrupt_selected_entries_are_declined() {
    assert!(selected_image(b"not an icon").is_none());
    assert!(selected_image(&archive(&[])).is_none());
    let mut bytes = archive(&[(16, 16, [255, 0, 0, 255]), (32, 32, [0, 255, 0, 255])]);
    // ICO entry 1 begins at 6+16; its payload offset is the final u32.
    let offset = u32::from_le_bytes(bytes[34..38].try_into().unwrap()) as usize;
    bytes[offset..].fill(0);
    assert!(selected_image(&bytes).is_none());
}

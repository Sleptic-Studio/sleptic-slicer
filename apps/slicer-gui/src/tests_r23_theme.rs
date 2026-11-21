// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Contrast contracts against the actual Slint semantic tokens.

fn token(name: &str, light: bool) -> [f64; 3] {
    let source = include_str!("../ui/theme.slint");
    let declaration = source
        .split(&format!("out property <color> {name}: "))
        .nth(1)
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let colours: Vec<_> = declaration
        .split('#')
        .skip(1)
        .map(|part| &part[..6])
        .collect();
    let colour = colours[if !light && colours.len() > 1 { 1 } else { 0 }];
    std::array::from_fn(|i| {
        f64::from(u8::from_str_radix(&colour[2 * i..2 * i + 2], 16).unwrap()) / 255.0
    })
}

fn luminance(rgb: [f64; 3]) -> f64 {
    let linear = rgb.map(|c| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    });
    linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722
}

#[test]
fn semantic_text_has_normal_text_contrast_on_both_shell_surfaces() {
    for light in [true, false] {
        for foreground in ["text", "muted", "warning", "error", "caution"] {
            for background in ["panel", "background", "field"] {
                let fg = luminance(token(foreground, light));
                let bg = luminance(token(background, light));
                let contrast = (fg.max(bg) + 0.05) / (fg.min(bg) + 0.05);
                assert!(
                    contrast >= 4.5,
                    "{foreground} on {background}, light={light}: {contrast:.3}:1"
                );
            }
        }
    }
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_sla_formats::format::{Format, format_from_path};
use std::path::Path;

#[test]
fn format_names_and_path_inference_contract() {
    let cases = [
        (Format::GcodeZip, "gcode-zip"),
        (Format::SvgPerLayer, "svg-per-layer"),
        (Format::SvgCombined, "svg-combined"),
        (Format::PngPerLayer, "png-per-layer"),
        (Format::ThreeMf, "3mf"),
    ];
    for (format, name) in cases {
        assert_eq!(format.as_str(), name);
        assert_eq!(format.to_string(), name);
        assert_eq!(name.parse::<Format>(), Ok(format));
    }
    for name in ["SVG", "svg", "", "3MF", " svg-combined"] {
        let error = name.parse::<Format>().unwrap_err();
        assert_eq!(error.got, name);
        assert_eq!(
            error.to_string(),
            format!(
                "unknown format \"{}\"; expected one of: svg-per-layer, svg-combined, png-per-layer, 3mf, gcode-zip",
                name
            )
        );
    }
    for (path, expected) in [
        ("a.SVG", Some(Format::SvgCombined)),
        ("a.3MF", Some(Format::ThreeMf)),
        ("a.ZIP", Some(Format::GcodeZip)),
        ("a.png", None),
        ("a", None),
    ] {
        assert_eq!(format_from_path(Path::new(path)), expected);
    }
}

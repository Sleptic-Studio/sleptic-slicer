// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The output formats a sliced job can be written as, by the name the
//! command line and settings use.
use std::{error::Error, fmt, path::Path, str::FromStr};

/// An output format. Per-layer formats write a folder of files; the rest
/// write one file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    GcodeZip,
    SvgPerLayer,
    SvgCombined,
    PngPerLayer,
    ThreeMf,
}

/// Each format with its name and, where it writes one file, the extension
/// that picks it from a path.
const TABLE: [(Format, &str, Option<&str>); 5] = [
    (Format::SvgPerLayer, "svg-per-layer", None),
    (Format::SvgCombined, "svg-combined", Some("svg")),
    (Format::PngPerLayer, "png-per-layer", None),
    (Format::ThreeMf, "3mf", Some("3mf")),
    (Format::GcodeZip, "gcode-zip", Some("zip")),
];

impl Format {
    /// The name the command line and settings use.
    pub fn as_str(self) -> &'static str {
        TABLE.iter().find(|(format, ..)| *format == self).map_or("", |(_, name, _)| name)
    }

    /// Whether the output is a folder of per-layer files.
    pub fn is_directory_format(self) -> bool {
        self == Format::SvgPerLayer || self == Format::PngPerLayer
    }
}

impl fmt::Display for Format {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(self.as_str())
    }
}

/// A format name that is none of the known ones.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownFormat {
    pub got: String,
}

impl fmt::Display for UnknownFormat {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = TABLE.iter().map(|(_, name, _)| *name).collect();
        write!(out, "unknown format \"{}\"; expected one of: {}", self.got, names.join(", "))
    }
}

impl Error for UnknownFormat {}

impl FromStr for Format {
    type Err = UnknownFormat;

    fn from_str(name: &str) -> Result<Format, UnknownFormat> {
        match TABLE.iter().find(|(_, known, _)| *known == name) {
            Some((format, ..)) => Ok(*format),
            None => Err(UnknownFormat { got: name.to_owned() }),
        }
    }
}

/// The single-file format `path`'s extension picks, in any letter case.
pub fn format_from_path(path: &Path) -> Option<Format> {
    let extension = path.extension()?.to_str()?;
    TABLE.iter().find_map(|&(format, _, suffix)| {
        suffix.filter(|suffix| suffix.eq_ignore_ascii_case(extension)).map(|_| format)
    })
}

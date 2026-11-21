// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{Format, WriterError, WriterOptions};
use std::path::Path;
pub(crate) fn validate(p: &Path, f: Format) -> Result<(), WriterError> {
    if f.is_directory_format() {
        match std::fs::metadata(p) {
            Ok(m) => {
                if !m.is_dir() {
                    return Err(WriterError::OutputPathExistsAndIsNotADirectory { path: p.into() });
                }
                if std::fs::read_dir(p)?.next().transpose()?.is_some() {
                    return Err(WriterError::OutputPathExistsAndIsNotEmpty { path: p.into() });
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::fs::create_dir_all(p)?,
            Err(e) => return Err(e.into()),
        }
    } else {
        match std::fs::metadata(p) {
            Ok(m) if m.is_dir() => {
                return Err(WriterError::OutputPathInvalid {
                    reason: format!(
                        "format \"{f}\" writes a single file, but the output path \"{}\" is an existing directory",
                        p.display()
                    ),
                });
            }
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    Ok(())
}
pub fn write_slice(
    s: &sc_sl1cer::contour::ContourStack,
    p: &Path,
    f: Format,
    o: &WriterOptions,
) -> Result<(), WriterError> {
    validate(p, f)?;
    match f {
        Format::SvgPerLayer => crate::debug::svg::write_per_layer(s, p, o),
        Format::SvgCombined => crate::debug::svg::write_combined(s, p, o),
        Format::PngPerLayer => crate::debug::png::write_per_layer(s, p, o),
        Format::ThreeMf => Err(WriterError::SceneContextRequired { format: f }),
        Format::GcodeZip => Err(crate::error::encoding(
            f,
            "gcode-zip requires an explicit PreparedJob; use write_gcode_zip",
        )),
    }
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! JsonCatalog printer records to Sleptic printer profiles.
use sc_print_profiles::*;
use serde::Deserialize;

use super::problem::{Problem, RecordKind, RecordProblem};

/// A printer record as the file has it. The key names are JsonCatalog's.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PrinterRecord {
    // Who it is.
    preset_id: String,
    manufacturer: String,
    family: Option<String>,
    name: String,
    profile_version: u64,
    image_asset_path: Option<String>,
    // Its geometry: the volume (width and depth may be left to the pixel
    // pitch), the pitch, and the screen.
    build_volume_mm: Volume,
    pixel_size: Option<Pitch>,
    display: Screen,
    // What it can do.
    #[serde(default)]
    anti_aliasing: bool,
    #[serde(default)]
    has_camera: bool,
    network_support: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Volume {
    width: Option<f64>,
    depth: Option<f64>,
    height: f64,
}

#[derive(Debug, Deserialize)]
struct Pitch {
    x: f64,
    y: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Screen {
    resolution_x: u32,
    resolution_y: u32,
    #[serde(default)]
    mirror_x: bool,
    #[serde(default)]
    mirror_y: bool,
    output_format: String,
    format_version: Option<String>,
}

/// A problem in printer record `id`.
fn at(id: &str, field: &'static str, problem: Problem) -> RecordProblem {
    RecordProblem::new(RecordKind::Printer, id, field, problem)
}

/// Files a value the profile rules refuse under printer `id`'s `field`.
fn refused<'a, E: std::error::Error + Send + Sync + 'static>(id: &'a str, field: &'static str) -> impl FnOnce(E) -> RecordProblem + 'a {
    move |why| RecordProblem::refused(RecordKind::Printer, id, field, why)
}

/// One axis's length in mm and pixel pitch in microns, from whichever the
/// record gives. A given length wins and the pitch follows from it; a pitch
/// alone is kept exactly as written (working it back out of the length it
/// gives changes the last bit for some values).
fn axis_span(id: &str, axis: &'static str, pixels: u32, mm: Option<f64>, um: Option<f64>) -> Result<(f64, f64), RecordProblem> {
    let count = f64::from(pixels);
    match (mm, um) {
        (Some(mm), _) => Ok((mm, mm / count * 1000.0)),
        (None, Some(um)) => Ok((count * um / 1000.0, um)),
        (None, None) => Err(at(id, axis, Problem::NoGeometry)),
    }
}

/// An optional text that says something, or none.
fn said(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.is_empty())
}

/// The profile, and the record's picture path (relative to its file) if any.
pub(super) fn convert(record: PrinterRecord) -> Result<(PrinterProfile, Option<String>), RecordProblem> {
    let id_text = record.preset_id.as_str();
    let text = |field: &'static str, value: &str| NonEmptyString::new(value).map_err(|_| at(id_text, field, Problem::Empty));

    let id = PrinterId::new(id_text).map_err(|why| at(id_text, "presetId", Problem::BadId(why)))?;
    let name = text("name", &record.name)?;
    let manufacturer = text("manufacturer", &record.manufacturer)?;
    // A record without a family is its own family.
    let family = text("family", record.family.as_deref().filter(|f| !f.is_empty()).unwrap_or(&record.name))?;

    let screen = &record.display;
    let pixels = PixelResolution::new(screen.resolution_x, screen.resolution_y).map_err(refused(id_text, "display resolution"))?;
    let pitch = record.pixel_size.as_ref();
    let (width, x_um) = axis_span(id_text, "x", pixels.x(), record.build_volume_mm.width, pitch.map(|p| p.x))?;
    let (depth, y_um) = axis_span(id_text, "y", pixels.y(), record.build_volume_mm.depth, pitch.map(|p| p.y))?;
    let volume = BuildVolume::new(width, depth, record.build_volume_mm.height).map_err(refused(id_text, "build volume"))?;
    let pixel_size = PixelSizeMicrons::new(x_um, y_um).map_err(refused(id_text, "pixel pitch"))?;
    let mirror = Mirror { x: screen.mirror_x, y: screen.mirror_y };
    let hardware = PrinterHardware::new(volume, pixels, pixel_size, mirror).map_err(refused(id_text, "hardware"))?;

    let format_name = screen.output_format.trim().to_ascii_lowercase();
    if format_name.is_empty() {
        return Err(at(id_text, "outputFormat", Problem::Empty));
    }
    let version = said(screen.format_version.clone()).map(|v| text("formatVersion", &v)).transpose()?;
    let capabilities = PrinterCapabilities {
        output_formats: OutputFormatBindings::new(OutputFormatBinding { format: FormatKind::from_canonical(&format_name), version }),
        supports_grayscale: record.anti_aliasing,
        has_camera: record.has_camera,
        network_protocol: said(record.network_support.clone()).map(|p| NetworkProtocol::from_canonical(&p)),
        two_stage_lift: false,
        transition_layers: None,
    };
    let identity = PrinterIdentity {
        id,
        manufacturer,
        family,
        name,
        profile_version: semver::Version::new(record.profile_version, 0, 0),
        image_asset_path: None,
    };
    let profile = PrinterProfile {
        identity,
        hardware,
        capabilities,
        constraints: PrinterConstraints::default(),
        motion: None,
        job: None,
    };
    Ok((profile, said(record.image_asset_path)))
}

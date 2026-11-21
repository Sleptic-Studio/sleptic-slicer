// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! A printer's physical description: its build volume, the screen's pixel
//! grid and pitch, mirroring, and the calibration a user measures.
use crate::{LengthOutOfRange, PositiveFiniteMicrons, PositiveFiniteMm};
use sc_mesh_core::primitives::CoordAxis;
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

/// The printable box, centred on the plate in x and y and rising from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "VolumeSides")]
pub struct BuildVolume {
    x_mm: PositiveFiniteMm,
    y_mm: PositiveFiniteMm,
    z_mm: PositiveFiniteMm,
}

/// The sides as a profile has them, before they are checked.
#[derive(Deserialize)]
struct VolumeSides {
    x_mm: f64,
    y_mm: f64,
    z_mm: f64,
}

impl TryFrom<VolumeSides> for BuildVolume {
    type Error = AxisOutOfRange;

    fn try_from(sides: VolumeSides) -> Result<BuildVolume, AxisOutOfRange> {
        BuildVolume::new(sides.x_mm, sides.y_mm, sides.z_mm)
    }
}

impl BuildVolume {
    /// The box, if every side is a finite positive length (x checked first).
    pub fn new(x_mm: f64, y_mm: f64, z_mm: f64) -> Result<BuildVolume, AxisOutOfRange> {
        let side = |axis: CoordAxis, mm: f64| {
            PositiveFiniteMm::new(mm).map_err(|source| AxisOutOfRange { axis, source })
        };
        Ok(BuildVolume {
            x_mm: side(CoordAxis::X, x_mm)?,
            y_mm: side(CoordAxis::Y, y_mm)?,
            z_mm: side(CoordAxis::Z, z_mm)?,
        })
    }

    pub fn x(&self) -> PositiveFiniteMm {
        self.x_mm
    }

    pub fn y(&self) -> PositiveFiniteMm {
        self.y_mm
    }

    pub fn z(&self) -> PositiveFiniteMm {
        self.z_mm
    }

    pub fn x_mm(&self) -> f64 {
        self.x().value()
    }

    pub fn y_mm(&self) -> f64 {
        self.y().value()
    }

    pub fn z_mm(&self) -> f64 {
        self.z().value()
    }

    /// The box's lowest corner: half the width and depth either side of
    /// the plate's centre, at the plate.
    pub fn envelope_min_mm(&self) -> [f64; 3] {
        let [x, y, _] = self.envelope_max_mm();
        [-x, -y, 0.0]
    }

    /// The box's highest corner.
    pub fn envelope_max_mm(&self) -> [f64; 3] {
        [0.5 * self.x_mm(), 0.5 * self.y_mm(), self.z_mm()]
    }
}

/// A build volume side that is not a usable length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisOutOfRange {
    pub axis: CoordAxis,
    pub source: LengthOutOfRange,
}

impl fmt::Display for AxisOutOfRange {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (axis, got) = (self.axis, self.source.value);
        write!(out, "build volume {axis} axis must be a finite positive length in millimetres, got {got}")
    }
}

impl Error for AxisOutOfRange {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// The screen's size in pixels; neither side is zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PixelCounts")]
pub struct PixelResolution {
    x: u32,
    y: u32,
}

#[derive(Deserialize)]
struct PixelCounts {
    x: u32,
    y: u32,
}

impl TryFrom<PixelCounts> for PixelResolution {
    type Error = ZeroPixelExtentError;

    fn try_from(counts: PixelCounts) -> Result<PixelResolution, ZeroPixelExtentError> {
        PixelResolution::new(counts.x, counts.y)
    }
}

impl PixelResolution {
    pub fn new(x: u32, y: u32) -> Result<PixelResolution, ZeroPixelExtentError> {
        let empty = [(CoordAxis::X, x), (CoordAxis::Y, y)].into_iter().find(|&(_, count)| count == 0);
        match empty {
            Some((axis, _)) => Err(ZeroPixelExtentError { axis }),
            None => Ok(PixelResolution { x, y }),
        }
    }

    pub fn x(&self) -> u32 {
        self.x
    }

    pub fn y(&self) -> u32 {
        self.y
    }
}

/// A screen side of no pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZeroPixelExtentError {
    pub axis: CoordAxis,
}

impl fmt::Display for ZeroPixelExtentError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "pixel resolution {} axis must be non-zero", self.axis)
    }
}

impl Error for ZeroPixelExtentError {}

/// One of the screen's two axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelAxis {
    X,
    Y,
}

impl fmt::Display for PixelAxis {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(if *self == PixelAxis::X { "x" } else { "y" })
    }
}

/// The size of one pixel on the screen, per axis, in microns.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PixelSizeMicrons {
    pub x: PositiveFiniteMicrons,
    pub y: PositiveFiniteMicrons,
}

impl PixelSizeMicrons {
    pub fn new(x_um: f64, y_um: f64) -> Result<PixelSizeMicrons, PixelAxisOutOfRange> {
        let pitch = |axis: PixelAxis, um: f64| {
            PositiveFiniteMicrons::new(um).map_err(|source| PixelAxisOutOfRange { axis, source })
        };
        Ok(PixelSizeMicrons { x: pitch(PixelAxis::X, x_um)?, y: pitch(PixelAxis::Y, y_um)? })
    }
}

/// A pixel pitch that is not a usable length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelAxisOutOfRange {
    pub axis: PixelAxis,
    pub source: LengthOutOfRange,
}

impl fmt::Display for PixelAxisOutOfRange {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (axis, got) = (self.axis, self.source.value);
        write!(out, "pixel size {axis} axis must be a finite positive length in microns, got {got}")
    }
}

impl Error for PixelAxisOutOfRange {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// Whether the screen shows the image flipped along each axis.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mirror {
    pub x: bool,
    pub y: bool,
}

/// How the screen is turned relative to the plate, in quarter turns.
/// Profiles write it as degrees.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub enum LcdRotation {
    #[default]
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

const ROTATIONS: [(LcdRotation, u16); 4] =
    [(LcdRotation::Deg0, 0), (LcdRotation::Deg90, 90), (LcdRotation::Deg180, 180), (LcdRotation::Deg270, 270)];

impl LcdRotation {
    /// Whether a quarter turn either way swaps the screen's width and height.
    pub fn swaps_axes(self) -> bool {
        u16::from(self) % 180 == 90
    }
}

impl TryFrom<u16> for LcdRotation {
    type Error = &'static str;

    fn try_from(degrees: u16) -> Result<LcdRotation, &'static str> {
        ROTATIONS
            .iter()
            .find(|(_, listed)| *listed == degrees)
            .map(|(rotation, _)| *rotation)
            .ok_or("LCD rotation must be 0, 90, 180 or 270 degrees")
    }
}

impl From<LcdRotation> for u16 {
    fn from(rotation: LcdRotation) -> u16 {
        ROTATIONS.iter().find(|(listed, _)| *listed == rotation).map_or(0, |(_, degrees)| *degrees)
    }
}

/// What a user has measured about their machine: how the screen is turned
/// and how far printed x/y sizes are off (as multipliers, 0.5 to 2).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "CalibrationEntry")]
pub struct MachineCalibration {
    pub rotation: LcdRotation,
    xy_scale: [f64; 2],
}

const NO_SCALING: [f64; 2] = [1.0, 1.0];
const SCALE_LIMITS: std::ops::RangeInclusive<f64> = 0.5..=2.0;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CalibrationEntry {
    #[serde(default)]
    rotation: LcdRotation,
    #[serde(default = "no_scaling", deserialize_with = "two_scales")]
    xy_scale: [f64; 2],
}

fn no_scaling() -> [f64; 2] {
    NO_SCALING
}

/// Exactly two multipliers. Read as a list and counted, since some formats
/// let a fixed-size tuple take extra trailing items.
fn two_scales<'de, D: serde::Deserializer<'de>>(input: D) -> Result<[f64; 2], D::Error> {
    let listed = Vec::<f64>::deserialize(input)?;
    let count = listed.len();
    <[f64; 2]>::try_from(listed)
        .map_err(|_| serde::de::Error::invalid_length(count, &"exactly two XY scale multipliers"))
}

impl TryFrom<CalibrationEntry> for MachineCalibration {
    type Error = &'static str;

    fn try_from(entry: CalibrationEntry) -> Result<MachineCalibration, &'static str> {
        MachineCalibration::new(entry.rotation, entry.xy_scale)
    }
}

impl Default for MachineCalibration {
    fn default() -> MachineCalibration {
        MachineCalibration { rotation: LcdRotation::Deg0, xy_scale: NO_SCALING }
    }
}

impl MachineCalibration {
    pub fn new(rotation: LcdRotation, xy_scale: [f64; 2]) -> Result<MachineCalibration, &'static str> {
        if xy_scale.iter().any(|scale| !scale.is_finite() || !SCALE_LIMITS.contains(scale)) {
            return Err("XY scale correction must be between 0.5 and 2");
        }
        Ok(MachineCalibration { rotation, xy_scale })
    }

    pub fn xy_scale(self) -> [f64; 2] {
        self.xy_scale
    }

    /// Whether nothing has been measured (no turn, no scaling).
    pub fn is_default(&self) -> bool {
        *self == MachineCalibration::default()
    }

    /// The calibration after a ruler print: each axis's scale is corrected
    /// by how far the `measured` length is off the `nominal` one.
    pub fn measured(self, nominal: [f64; 2], measured: [f64; 2]) -> Result<MachineCalibration, &'static str> {
        let lengths = nominal.into_iter().chain(measured);
        if lengths.into_iter().any(|length| !(length.is_finite() && length > 0.0)) {
            return Err("Ruler lengths must be finite and positive");
        }
        let corrected = [0, 1].map(|axis| self.xy_scale[axis] * nominal[axis] / measured[axis]);
        MachineCalibration::new(self.rotation, corrected)
    }
}

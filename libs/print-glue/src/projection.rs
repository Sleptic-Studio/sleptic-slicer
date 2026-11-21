// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! From checked printer and material data to the slicer's profile: how
//! many layers a scene needs, and the profile those layers make on a
//! printer.
use sc_print_primitives::{AaLevel, PositiveFiniteMm};
use sc_print_profiles::{PrinterHardware, ResolvedPrintConfig};
use sc_sl1cer::profile::{LayerSchedule, PackingMode, SliceProfile, UniformSchedule};
use sc_sl1cer::raster::accuracy::Accuracy;
use std::{error::Error, fmt, num::NonZeroU32};

/// Uniform layers after a first layer of its own height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerPlan {
    /// Every layer after the first.
    pub layer_height: PositiveFiniteMm,
    /// The first layer.
    pub first_layer_height: PositiveFiniteMm,
    /// All layers, the first included.
    pub layers: NonZeroU32,
}

/// The profile a printer gives on its own: its volume, pixel grid,
/// mirroring and calibration over the `plan`'s layers. Nothing from
/// exposure or motion, and no accuracy settings.
pub fn hardware_profile(
    hardware: &PrinterHardware,
    plan: LayerPlan,
    aa_level: AaLevel,
    packing_mode: PackingMode,
) -> SliceProfile {
    let schedule = UniformSchedule::new(plan.layer_height, plan.first_layer_height, plan.layers);
    SliceProfile::new(
        hardware.build_volume(),
        hardware.pixel_resolution(),
        LayerSchedule::Uniform(schedule),
        hardware.mirror(),
        aa_level,
        packing_mode,
    )
    .with_calibration(hardware.calibration())
}

/// The profile for a resolved print: [`hardware_profile`] at the
/// material's layer height, with the material's accuracy settings
/// (compensations, anti-aliasing levels, bottom and transition layers).
pub fn print_profile(
    config: &ResolvedPrintConfig,
    first_layer_height: PositiveFiniteMm,
    layers: NonZeroU32,
    aa_level: AaLevel,
    packing_mode: PackingMode,
) -> SliceProfile {
    let resin = &config.material_resolved;
    let plan = LayerPlan { layer_height: resin.layer_height_mm, first_layer_height, layers };
    hardware_profile(&config.printer.hardware, plan, aa_level, packing_mode).with_accuracy(Accuracy {
        xy_compensation_mm: resin.xy_compensation_mm,
        bottom_compensation_mm: resin.bottom_compensation_mm,
        bottom_layers: resin.bottom_layer_count,
        transition_layers: resin.transition_layers,
        aa_levels: resin.aa_levels,
    })
}

/// How many layers reach `z_max`: the first layer, then as many layers of
/// `layer_height` as it takes to reach or pass the top. Compared exactly,
/// with no tolerance, so a top a hair above a layer boundary takes one
/// more layer. A top at or under the first layer is one layer.
pub fn total_layers_for_height(
    z_max: f64,
    layer_height: f64,
    first_layer_height: f64,
) -> Result<NonZeroU32, UnschedulableHeight> {
    if !z_max.is_finite() {
        return Err(UnschedulableHeight::NonFiniteHeight { z_max_mm: z_max });
    }
    let above_first = z_max - first_layer_height;
    if above_first <= 0.0 {
        return Ok(NonZeroU32::MIN);
    }
    // NaN or infinite for a zero layer height or a NaN first layer; tested
    // before the count is formed, since clamping would hide a NaN.
    let more = (above_first / layer_height).ceil();
    let overflow = UnschedulableHeight::LayerCountOverflow { z_max_mm: z_max };
    if !more.is_finite() || more.max(0.0) + 1.0 > f64::from(u32::MAX) {
        return Err(overflow);
    }
    NonZeroU32::new(more.max(0.0) as u32 + 1).ok_or(overflow)
}

/// A scene top no whole number of layers (up to `u32::MAX`) can reach.
#[derive(Debug, Clone, PartialEq)]
pub enum UnschedulableHeight {
    /// The top is NaN or infinite.
    NonFiniteHeight {
        /// The scene top that was asked for, in mm.
        z_max_mm: f64,
    },
    /// The layer count is not a number or passes `u32::MAX`.
    LayerCountOverflow {
        /// The scene top that was asked for, in mm.
        z_max_mm: f64,
    },
}

impl fmt::Display for UnschedulableHeight {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnschedulableHeight::NonFiniteHeight { z_max_mm } => {
                write!(out, "Scene top must be finite, got {z_max_mm} mm")
            }
            UnschedulableHeight::LayerCountOverflow { z_max_mm } => {
                write!(out, "Scene top {z_max_mm} mm cannot fit the layer-count range")
            }
        }
    }
}

impl Error for UnschedulableHeight {}

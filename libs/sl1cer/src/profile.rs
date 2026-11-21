// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::raster::{accuracy::Accuracy, pixel_map::PixelMap};
pub use sc_print_primitives::*;
use std::{fmt, num::NonZeroU32};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackingMode {
    None,
}
#[derive(Debug, Clone, PartialEq)]
pub enum InvalidLayerSchedule {
    LayerHeightOutOfRange(LengthOutOfRange),
    FirstLayerHeightOutOfRange(LengthOutOfRange),
    ZeroTotalLayers,
}
impl fmt::Display for InvalidLayerSchedule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LayerHeightOutOfRange(e) => write!(
                f,
                "layer_height_mm must be a finite positive length in millimetres, got {}",
                e.value
            ),
            Self::FirstLayerHeightOutOfRange(e) => write!(
                f,
                "first_layer_height_mm must be a finite positive length in millimetres, got {}",
                e.value
            ),
            Self::ZeroTotalLayers => f.write_str("total_layers must be non-zero"),
        }
    }
}
impl std::error::Error for InvalidLayerSchedule {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::LayerHeightOutOfRange(e) | Self::FirstLayerHeightOutOfRange(e) => Some(e),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UniformSchedule {
    layer_height: PositiveFiniteMm,
    first_layer_height: PositiveFiniteMm,
    total_layers: NonZeroU32,
}
impl UniformSchedule {
    pub fn new(
        layer_height: PositiveFiniteMm,
        first_layer_height: PositiveFiniteMm,
        total_layers: NonZeroU32,
    ) -> Self {
        Self {
            layer_height,
            first_layer_height,
            total_layers,
        }
    }
    pub fn checked(h: f64, h0: f64, n: u32) -> Result<Self, InvalidLayerSchedule> {
        let height = PositiveFiniteMm::new(h)
            .map_err(InvalidLayerSchedule::LayerHeightOutOfRange)?;
        let first = PositiveFiniteMm::new(h0)
            .map_err(InvalidLayerSchedule::FirstLayerHeightOutOfRange)?;
        let count = NonZeroU32::new(n).ok_or(InvalidLayerSchedule::ZeroTotalLayers)?;
        Ok(Self::new(height, first, count))
    }
    pub fn layer_height_mm(&self) -> PositiveFiniteMm {
        self.layer_height
    }
    pub fn first_layer_height_mm(&self) -> PositiveFiniteMm {
        self.first_layer_height
    }
    pub fn layer_height(&self) -> PositiveFiniteMm {
        self.layer_height
    }
    pub fn first_layer_height(&self) -> PositiveFiniteMm {
        self.first_layer_height
    }
    pub fn total_layers(&self) -> NonZeroU32 {
        self.total_layers
    }
    pub fn iter_z(&self) -> impl ExactSizeIterator<Item = f64> + use<> {
        let first = self.first_layer_height.value();
        let height = self.layer_height.value();
        (0..self.total_layers.get()).map(move |index| match index {
            0 => first / 2.0,
            _ => (index as f64 - 0.5).mul_add(height, first),
        })
    }
    pub fn iter_slab_tops(&self) -> impl ExactSizeIterator<Item = f64> + use<> {
        let first = self.first_layer_height.value();
        let height = self.layer_height.value();
        (0..self.total_layers.get()).map(move |index| (index as f64).mul_add(height, first))
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LayerSchedule {
    Uniform(UniformSchedule),
}
impl LayerSchedule {
    pub fn uniform(h: f64, h0: f64, n: u32) -> Result<Self, InvalidLayerSchedule> {
        UniformSchedule::checked(h, h0, n).map(Self::Uniform)
    }
    pub fn total_layers(&self) -> u32 {
        match self {
            Self::Uniform(s) => s.total_layers.get(),
        }
    }
    pub fn iter_z(&self) -> impl ExactSizeIterator<Item = f64> + use<> {
        match self {
            Self::Uniform(s) => s.iter_z(),
        }
    }
    pub fn iter_slab_tops(&self) -> impl ExactSizeIterator<Item = f64> + use<> {
        match self {
            Self::Uniform(s) => s.iter_slab_tops(),
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct SliceProfile {
    build_volume: BuildVolume,
    resolution: PixelResolution,
    schedule: LayerSchedule,
    mirror: Mirror,
    aa: AaLevel,
    packing: PackingMode,
    accuracy: Accuracy,
    calibration: MachineCalibration,
    support_overhang_angle: f64,
}
impl SliceProfile {
    pub fn new(
        build_volume: BuildVolume,
        resolution: PixelResolution,
        schedule: LayerSchedule,
        mirror: Mirror,
        aa: AaLevel,
        packing: PackingMode,
    ) -> Self {
        Self {
            build_volume,
            resolution,
            schedule,
            mirror,
            aa,
            packing,
            accuracy: Accuracy::default(),
            calibration: MachineCalibration::default(),
            support_overhang_angle: 45.,
        }
    }
    pub fn with_accuracy(mut self, v: Accuracy) -> Self {
        self.accuracy = v;
        self
    }
    pub fn with_calibration(mut self, v: MachineCalibration) -> Self {
        self.calibration = v;
        self
    }
    pub fn with_support_overhang_angle(mut self, v: f64) -> Self {
        if v.is_finite() && (0.0..90.0).contains(&v) && v != 0.0 {
            self.support_overhang_angle = v
        }
        self
    }
    pub fn build_volume(&self) -> &BuildVolume {
        &self.build_volume
    }
    pub fn resolution(&self) -> PixelResolution {
        self.resolution
    }
    pub fn schedule(&self) -> &LayerSchedule {
        &self.schedule
    }
    pub fn mirror(&self) -> Mirror {
        self.mirror
    }
    pub fn aa(&self) -> AaLevel {
        self.aa
    }
    pub fn packing_mode(&self) -> PackingMode {
        self.packing
    }
    pub fn packing(&self) -> PackingMode {
        self.packing
    }
    pub fn accuracy(&self) -> Accuracy {
        self.accuracy
    }
    pub fn pixel_resolution(&self) -> &PixelResolution {
        &self.resolution
    }
    pub fn aa_level(&self) -> AaLevel {
        self.aa
    }
    pub fn layer_schedule(&self) -> &LayerSchedule {
        &self.schedule
    }
    pub fn calibration(&self) -> MachineCalibration {
        self.calibration
    }
    pub fn support_overhang_angle_deg(&self) -> f64 {
        self.support_overhang_angle
    }
    pub fn support_overhang_angle(&self) -> f64 {
        self.support_overhang_angle
    }
    pub fn pixel_map(&self) -> PixelMap {
        PixelMap::new(
            self.build_volume.x(),
            self.build_volume.y(),
            self.resolution,
            self.mirror,
        )
        .with_calibration(self.calibration)
    }
}

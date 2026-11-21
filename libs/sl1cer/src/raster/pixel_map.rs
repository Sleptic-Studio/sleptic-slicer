// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{
    geom2d::Point2D,
    profile::{
        BuildVolume, LcdRotation, MachineCalibration, Mirror, PixelResolution, PositiveFiniteMm,
    },
};
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelMap {
    plate: BuildVolume,
    resolution: PixelResolution,
    mirror: Mirror,
    calibration: MachineCalibration,
}
impl PixelMap {
    pub fn new(
        x: PositiveFiniteMm,
        y: PositiveFiniteMm,
        resolution: PixelResolution,
        mirror: Mirror,
    ) -> Self {
        Self {
            plate: BuildVolume::new(x.value(), y.value(), 1.).unwrap(),
            resolution,
            mirror,
            calibration: MachineCalibration::default(),
        }
    }
    pub fn pixel_resolution(&self) -> PixelResolution {
        PixelResolution::new(self.width(), self.height()).unwrap()
    }
    pub fn with_calibration(mut self, c: MachineCalibration) -> Self {
        self.calibration = c;
        self
    }
    pub fn width(&self) -> u32 {
        if self.calibration.rotation.swaps_axes() {
            self.resolution.y()
        } else {
            self.resolution.x()
        }
    }
    pub fn height(&self) -> u32 {
        if self.calibration.rotation.swaps_axes() {
            self.resolution.x()
        } else {
            self.resolution.y()
        }
    }
    pub fn plate_x_mm(&self) -> f64 {
        self.plate.x_mm()
    }
    pub fn plate_y_mm(&self) -> f64 {
        self.plate.y_mm()
    }
    pub fn mm_to_pixel(&self, p: Point2D) -> (f64, f64) {
        let [sx, sy] = self.calibration.xy_scale();
        let rx = self.resolution.x() as f64;
        let ry = self.resolution.y() as f64;
        let mut u = (sx * p.x + self.plate.x_mm() / 2.) * rx / self.plate.x_mm();
        let mut v = (sy * p.y + self.plate.y_mm() / 2.) * ry / self.plate.y_mm();
        if self.mirror.x {
            u = rx - u
        }
        if self.mirror.y {
            v = ry - v
        }
        let (x, y) = match self.calibration.rotation {
            LcdRotation::Deg0 => (u, v),
            LcdRotation::Deg90 => (v, rx - u),
            LcdRotation::Deg180 => (rx - u, ry - v),
            LcdRotation::Deg270 => (ry - v, u),
        };
        (x, y)
    }
    pub fn pixel_to_mm(&self, p: (f64, f64)) -> Point2D {
        let rx = self.resolution.x() as f64;
        let ry = self.resolution.y() as f64;
        let (mut u, mut v) = match self.calibration.rotation {
            LcdRotation::Deg0 => (p.0, p.1),
            LcdRotation::Deg90 => (rx - p.1, p.0),
            LcdRotation::Deg180 => (rx - p.0, ry - p.1),
            LcdRotation::Deg270 => (p.1, ry - p.0),
        };
        if self.mirror.x {
            u = rx - u
        }
        if self.mirror.y {
            v = ry - v
        }
        let [sx, sy] = self.calibration.xy_scale();
        Point2D::new(
            (u * self.plate.x_mm() / rx - self.plate.x_mm() / 2.) / sx,
            (v * self.plate.y_mm() / ry - self.plate.y_mm() / 2.) / sy,
        )
    }
}

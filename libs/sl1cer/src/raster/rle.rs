// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Run-length layer images: a layer's pixels row by row, top to bottom,
//! as runs of one grey value. Resin layers are mostly empty or mostly
//! solid, so runs are far smaller than the pixels.

/// `length` pixels in a row (possibly spanning rows) of grey `value`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RleRun {
    pub value: u8,
    pub length: u32,
}

/// One layer's image at height `z_mm`, `width` x `height` pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerRaster {
    runs: Vec<RleRun>,
    size: (u32, u32),
    z_mm: f64,
}

impl LayerRaster {
    pub fn new(z_mm: f64, width: u32, height: u32, runs: Vec<RleRun>) -> LayerRaster {
        LayerRaster { runs, size: (width, height), z_mm }
    }

    /// The layer's height above the plate, in mm.
    pub fn z_mm(&self) -> f64 {
        self.z_mm
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.size.0
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.size.1
    }

    pub fn runs(&self) -> &[RleRun] {
        &self.runs
    }

    pub fn into_runs(self) -> Vec<RleRun> {
        self.runs
    }
}

/// Builds a run list pixel stretch by stretch, joining a stretch to the
/// last run when the grey matches. A run never passes `u32::MAX`; a
/// longer stretch goes on in a new run of the same grey.
#[derive(Debug, Default)]
pub struct RleRunAccumulator {
    runs: Vec<RleRun>,
}

impl RleRunAccumulator {
    pub fn new() -> RleRunAccumulator {
        RleRunAccumulator::default()
    }

    /// Adds `length` pixels of grey `value`.
    pub fn push(&mut self, value: u8, length: u32) {
        let mut left = length;
        while left > 0 {
            match self.runs.last_mut() {
                Some(last) if last.value == value && last.length < u32::MAX => {
                    let taken = left.min(u32::MAX - last.length);
                    last.length += taken;
                    left -= taken;
                }
                _ => {
                    self.runs.push(RleRun { value, length: left });
                    left = 0;
                }
            }
        }
    }

    /// How many runs so far.
    pub fn len(&self) -> usize {
        self.runs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    pub fn into_runs(self) -> Vec<RleRun> {
        self.runs
    }
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_sl1cer::{
    profile::*,
    raster::{accuracy::Accuracy, pixel_map::PixelMap},
};
#[derive(Clone, Debug, PartialEq)]
pub struct WriterOptions {
    profile: SliceProfile,
    version: &'static str,
    min_alpha: f32,
    pub(crate) preview: Option<Vec<u8>>,
}
impl WriterOptions {
    pub fn from_profile(p: &SliceProfile, version: &'static str) -> Self {
        Self {
            profile: p.clone(),
            version,
            min_alpha: 0.0,
            preview: None,
        }
    }
    pub fn with_preview_mesh(mut self, m: &sc_mesh_core::IndexedMesh) -> Self {
        self.preview = Some(sc_mesh_formats::preview::render_mesh(m, 256, 256, [242; 3]));
        self
    }
    pub fn with_min_alpha_pct(mut self, v: f32) -> Self {
        self.min_alpha = v;
        self
    }
    pub fn min_alpha_pct(&self) -> f32 {
        self.min_alpha
    }
    pub fn build_volume(&self) -> &BuildVolume {
        self.profile.build_volume()
    }
    pub fn pixel_resolution(&self) -> &PixelResolution {
        self.profile.pixel_resolution()
    }
    pub fn mirror(&self) -> Mirror {
        self.profile.mirror()
    }
    pub fn calibration(&self) -> MachineCalibration {
        self.profile.calibration()
    }
    pub fn aa(&self) -> AaLevel {
        self.profile.aa_level()
    }
    pub fn accuracy(&self) -> Accuracy {
        self.profile.accuracy()
    }
    pub fn layer_schedule(&self) -> &LayerSchedule {
        self.profile.layer_schedule()
    }
    pub fn slicer_version(&self) -> &'static str {
        self.version
    }
    pub fn pixel_map(&self) -> PixelMap {
        self.profile.pixel_map()
    }
}

pub(crate) fn aa_text(a: AaLevel) -> &'static str {
    match a {
        AaLevel::Off => "off",
        AaLevel::X2 => "x2",
        AaLevel::X4 => "x4",
        AaLevel::X8 => "x8",
    }
}

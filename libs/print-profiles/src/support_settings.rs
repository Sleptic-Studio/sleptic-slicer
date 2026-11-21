// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SupportSettings {
    pub overhang_angle_deg: f64,
    pub spacing_mm: f64,
    pub tip_contact_diameter_mm: f64,
    pub tip_length_mm: f64,
    pub shaft_diameter_mm: f64,
    pub base_diameter_mm: f64,
    pub base_height_mm: f64,
    pub raft: bool,
    pub raft_thickness_mm: f64,
    pub raft_margin_mm: f64,
    pub lift_mm: f64,
    pub contact_depth_mm: f64,
    pub lattice_raft: bool,
    pub skate_raft: bool,
    pub min_column_diameter_mm: f64,
    pub max_column_diameter_mm: f64,
    pub symmetric_supports: bool,
    pub tree_supports: bool,
    pub branch_angle_deg: f64,
    #[serde(default, skip_serializing, rename = "min_island_area_mm2")]
    pub retired_min_island_area: Retired,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Retired;
impl<'de> Deserialize<'de> for Retired {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let _ = serde::de::IgnoredAny::deserialize(d)?;
        Ok(Self)
    }
}
impl Default for SupportSettings {
    fn default() -> Self {
        Self {
            overhang_angle_deg: 45.0,
            spacing_mm: 4.0,
            tip_contact_diameter_mm: 0.4,
            tip_length_mm: 2.0,
            shaft_diameter_mm: 1.2,
            base_diameter_mm: 4.0,
            base_height_mm: 0.6,
            raft: true,
            raft_thickness_mm: 1.0,
            raft_margin_mm: 3.0,
            lift_mm: 5.0,
            contact_depth_mm: 0.1,
            lattice_raft: false,
            skate_raft: false,
            min_column_diameter_mm: 0.8,
            max_column_diameter_mm: 2.0,
            symmetric_supports: false,
            tree_supports: true,
            branch_angle_deg: 40.0,
            retired_min_island_area: Retired,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SupportPreset {
    Light,
    Medium,
    Heavy,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidSupportSettings(pub &'static str);
impl std::fmt::Display for InvalidSupportSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid support setting: {}", self.0)
    }
}
impl std::error::Error for InvalidSupportSettings {}
impl SupportSettings {
    pub fn validate(&self) -> Result<(), InvalidSupportSettings> {
        if !self.spacing_mm.is_finite() || self.spacing_mm <= 0.0 {
            return Err(InvalidSupportSettings("spacing_mm"));
        }
        if !self.min_column_diameter_mm.is_finite() || self.min_column_diameter_mm <= 0.0 {
            return Err(InvalidSupportSettings("min_column_diameter_mm"));
        }
        if !self.max_column_diameter_mm.is_finite() || self.max_column_diameter_mm <= 0.0 {
            return Err(InvalidSupportSettings("max_column_diameter_mm"));
        }
        if !self.tip_contact_diameter_mm.is_finite() || self.tip_contact_diameter_mm <= 0.0 {
            return Err(InvalidSupportSettings("tip_contact_diameter_mm"));
        }
        if !self.tip_length_mm.is_finite() || self.tip_length_mm <= 0.0 {
            return Err(InvalidSupportSettings("tip_length_mm"));
        }
        if !self.shaft_diameter_mm.is_finite() || self.shaft_diameter_mm <= 0.0 {
            return Err(InvalidSupportSettings("shaft_diameter_mm"));
        }
        if !self.base_diameter_mm.is_finite() || self.base_diameter_mm <= 0.0 {
            return Err(InvalidSupportSettings("base_diameter_mm"));
        }
        if !self.base_height_mm.is_finite() || self.base_height_mm <= 0.0 {
            return Err(InvalidSupportSettings("base_height_mm"));
        }
        if !self.raft_thickness_mm.is_finite() || self.raft_thickness_mm <= 0.0 {
            return Err(InvalidSupportSettings("raft_thickness_mm"));
        }
        if !self.lift_mm.is_finite() || self.lift_mm <= 0.0 {
            return Err(InvalidSupportSettings("lift_mm"));
        }
        if !self.raft_margin_mm.is_finite() || self.raft_margin_mm < 0.0 {
            return Err(InvalidSupportSettings("raft_margin_mm"));
        }
        if !self.contact_depth_mm.is_finite() || self.contact_depth_mm < 0.0 {
            return Err(InvalidSupportSettings("contact_depth_mm"));
        }
        if !(1.0..80.0).contains(&self.branch_angle_deg) {
            return Err(InvalidSupportSettings("branch_angle_deg"));
        }
        if !(0.0..90.0).contains(&self.overhang_angle_deg) {
            return Err(InvalidSupportSettings("overhang_angle_deg"));
        }
        if self.tip_contact_diameter_mm > self.shaft_diameter_mm {
            return Err(InvalidSupportSettings("tip diameter exceeds shaft"));
        }
        if self.shaft_diameter_mm > self.base_diameter_mm {
            return Err(InvalidSupportSettings("shaft diameter exceeds base"));
        }
        if self.tip_contact_diameter_mm > self.min_column_diameter_mm
            || self.min_column_diameter_mm > self.max_column_diameter_mm
            || self.max_column_diameter_mm > self.base_diameter_mm
        {
            return Err(InvalidSupportSettings("column diameter bounds"));
        }
        if self.lift_mm
            <= self.base_height_mm
                + self.tip_length_mm
                + if self.raft {
                    self.raft_thickness_mm
                } else {
                    0.0
                }
        {
            return Err(InvalidSupportSettings("lift leaves no shaft clearance"));
        }
        Ok(())
    }
    pub fn lowest_contact_mm(&self) -> f64 {
        (self.base_height_mm + self.tip_length_mm).min(1.8)
    }
    pub fn apply_preset(&mut self, preset: SupportPreset) {
        let (t, l, s, c) = match preset {
            SupportPreset::Light => (0.25, 1.5, 0.8, 0.05),
            SupportPreset::Medium => (0.4, 2., 1.2, 0.1),
            SupportPreset::Heavy => (0.7, 2.5, 2., 0.2),
        };
        self.tip_contact_diameter_mm = t;
        self.tip_length_mm = l;
        self.shaft_diameter_mm = s;
        self.contact_depth_mm = c;
        self.min_column_diameter_mm = s;
        self.max_column_diameter_mm = (1.6 * s).min(self.base_diameter_mm);
    }
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::*;
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "SceneOverridesDocument")]
pub struct SceneOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition_layers: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom_compensation_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xy_compensation_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aa_levels: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_off_delay_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom_layer_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normal_exposure_sec: Option<PositiveFiniteSec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom_exposure_sec: Option<PositiveFiniteSec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lift_distance_mm: Option<PositiveFiniteMm>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lift_speed_mm_min: Option<PositiveFiniteMmPerMin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retract_speed_mm_min: Option<PositiveFiniteMmPerMin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer_height_mm: Option<PositiveFiniteMm>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale_compensation_pct: Option<ScaleCompensation>,
}
#[derive(Deserialize)]
struct SceneOverridesDocument {
    transition_layers: Option<u32>,
    bottom_compensation_mm: Option<f64>,
    xy_compensation_mm: Option<f64>,
    aa_levels: Option<u16>,
    light_off_delay_s: Option<f64>,
    bottom_layer_count: Option<u32>,
    normal_exposure_sec: Option<PositiveFiniteSec>,
    bottom_exposure_sec: Option<PositiveFiniteSec>,
    lift_distance_mm: Option<PositiveFiniteMm>,
    lift_speed_mm_min: Option<PositiveFiniteMmPerMin>,
    retract_speed_mm_min: Option<PositiveFiniteMmPerMin>,
    layer_height_mm: Option<PositiveFiniteMm>,
    scale_compensation_pct: Option<ScaleCompensation>,
}
impl TryFrom<SceneOverridesDocument> for SceneOverrides {
    type Error = &'static str;
    fn try_from(v: SceneOverridesDocument) -> Result<Self, Self::Error> {
        if let Some(x) = v.xy_compensation_mm
            && !x.is_finite()
        {
            return Err("XY compensation must be finite");
        }
        if let Some(x) = v.aa_levels
            && !(2..=256).contains(&x)
        {
            return Err("aa_levels must be 2..=256");
        }
        if let Some(x) = v.bottom_compensation_mm
            && (!x.is_finite() || x < 0.0)
        {
            return Err("bottom_compensation_mm must be finite and non-negative");
        }
        Ok(Self {
            transition_layers: v.transition_layers,
            bottom_compensation_mm: v.bottom_compensation_mm,
            xy_compensation_mm: v.xy_compensation_mm,
            aa_levels: v.aa_levels,
            light_off_delay_s: v.light_off_delay_s,
            bottom_layer_count: v.bottom_layer_count,
            normal_exposure_sec: v.normal_exposure_sec,
            bottom_exposure_sec: v.bottom_exposure_sec,
            lift_distance_mm: v.lift_distance_mm,
            lift_speed_mm_min: v.lift_speed_mm_min,
            retract_speed_mm_min: v.retract_speed_mm_min,
            layer_height_mm: v.layer_height_mm,
            scale_compensation_pct: v.scale_compensation_pct,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlateSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub process: SceneOverrides,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aa: Option<AaLevel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_layer_height: Option<PositiveFiniteMm>,
}
impl PlateSettings {
    pub fn parse(s: &str) -> Result<Self, toml_edit::de::Error> {
        toml_edit::de::from_str(s)
    }
    pub fn serialize(&self) -> Result<String, toml_edit::ser::Error> {
        toml_edit::ser::to_string_pretty(self)
    }
}

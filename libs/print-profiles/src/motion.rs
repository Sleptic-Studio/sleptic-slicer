// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "MotionFields")]
pub struct Motion {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lift_slow_distance_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lift_fast_distance_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retract_slow_distance_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retract_fast_distance_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lift_slow_speed_mm_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lift_fast_speed_mm_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retract_slow_speed_mm_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retract_fast_speed_mm_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_before_cure_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_after_lift_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_after_retract_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_off_delay_s: Option<f64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MotionFields {
    pub lift_slow_distance_mm: Option<f64>,
    pub lift_fast_distance_mm: Option<f64>,
    pub retract_slow_distance_mm: Option<f64>,
    pub retract_fast_distance_mm: Option<f64>,
    pub lift_slow_speed_mm_min: Option<f64>,
    pub lift_fast_speed_mm_min: Option<f64>,
    pub retract_slow_speed_mm_min: Option<f64>,
    pub retract_fast_speed_mm_min: Option<f64>,
    pub wait_before_cure_s: Option<f64>,
    pub wait_after_lift_s: Option<f64>,
    pub wait_after_retract_s: Option<f64>,
    pub light_off_delay_s: Option<f64>,
}
impl TryFrom<MotionFields> for Motion {
    type Error = &'static str;
    fn try_from(v: MotionFields) -> Result<Self, Self::Error> {
        let m = Self {
            lift_slow_distance_mm: v.lift_slow_distance_mm,
            lift_fast_distance_mm: v.lift_fast_distance_mm,
            retract_slow_distance_mm: v.retract_slow_distance_mm,
            retract_fast_distance_mm: v.retract_fast_distance_mm,
            lift_slow_speed_mm_min: v.lift_slow_speed_mm_min,
            lift_fast_speed_mm_min: v.lift_fast_speed_mm_min,
            retract_slow_speed_mm_min: v.retract_slow_speed_mm_min,
            retract_fast_speed_mm_min: v.retract_fast_speed_mm_min,
            wait_before_cure_s: v.wait_before_cure_s,
            wait_after_lift_s: v.wait_after_lift_s,
            wait_after_retract_s: v.wait_after_retract_s,
            light_off_delay_s: v.light_off_delay_s,
        };
        m.validate_values()?;
        Ok(m)
    }
}
impl Motion {
    pub fn validate_values(&self) -> Result<(), &'static str> {
        if [
            self.lift_slow_distance_mm,
            self.lift_fast_distance_mm,
            self.retract_slow_distance_mm,
            self.retract_fast_distance_mm,
            self.wait_before_cure_s,
            self.wait_after_lift_s,
            self.wait_after_retract_s,
            self.light_off_delay_s,
        ]
        .into_iter()
        .flatten()
        .any(|v| !v.is_finite() || v < 0.0)
        {
            return Err("Motion distances and waits must be finite and nonnegative");
        }
        if [
            self.lift_slow_speed_mm_min,
            self.lift_fast_speed_mm_min,
            self.retract_slow_speed_mm_min,
            self.retract_fast_speed_mm_min,
        ]
        .into_iter()
        .flatten()
        .any(|v| !v.is_finite() || v <= 0.0)
        {
            return Err("Motion speeds must be positive and finite");
        }
        Ok(())
    }
    pub fn total_lift_distance_mm(&self) -> f64 {
        self.lift_slow_distance_mm.unwrap_or(0.0) + self.lift_fast_distance_mm.unwrap_or(0.0)
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        self.validate_values()?;
        if (self.total_lift_distance_mm()
            - self.retract_slow_distance_mm.unwrap_or(0.0)
            - self.retract_fast_distance_mm.unwrap_or(0.0))
        .abs()
            > 1e-6
        {
            return Err("Total lift and retract distances must match");
        }
        Ok(())
    }
    pub fn overlay(mut self, other: Self) -> Self {
        if other.lift_slow_distance_mm.is_some() {
            self.lift_slow_distance_mm = other.lift_slow_distance_mm;
        }
        if other.lift_fast_distance_mm.is_some() {
            self.lift_fast_distance_mm = other.lift_fast_distance_mm;
        }
        if other.retract_slow_distance_mm.is_some() {
            self.retract_slow_distance_mm = other.retract_slow_distance_mm;
        }
        if other.retract_fast_distance_mm.is_some() {
            self.retract_fast_distance_mm = other.retract_fast_distance_mm;
        }
        if other.lift_slow_speed_mm_min.is_some() {
            self.lift_slow_speed_mm_min = other.lift_slow_speed_mm_min;
        }
        if other.lift_fast_speed_mm_min.is_some() {
            self.lift_fast_speed_mm_min = other.lift_fast_speed_mm_min;
        }
        if other.retract_slow_speed_mm_min.is_some() {
            self.retract_slow_speed_mm_min = other.retract_slow_speed_mm_min;
        }
        if other.retract_fast_speed_mm_min.is_some() {
            self.retract_fast_speed_mm_min = other.retract_fast_speed_mm_min;
        }
        if other.wait_before_cure_s.is_some() {
            self.wait_before_cure_s = other.wait_before_cure_s;
        }
        if other.wait_after_lift_s.is_some() {
            self.wait_after_lift_s = other.wait_after_lift_s;
        }
        if other.wait_after_retract_s.is_some() {
            self.wait_after_retract_s = other.wait_after_retract_s;
        }
        if other.light_off_delay_s.is_some() {
            self.light_off_delay_s = other.light_off_delay_s;
        }
        self
    }
    pub fn interpolate(self, other: Self, fraction: f64) -> Self {
        let t = fraction.clamp(0.0, 1.0);
        Self {
            lift_slow_distance_mm: match (self.lift_slow_distance_mm, other.lift_slow_distance_mm) {
                (Some(a), Some(b)) => Some(a + (b - a) * t),
                (a, b) => a.or(b),
            },
            lift_fast_distance_mm: match (self.lift_fast_distance_mm, other.lift_fast_distance_mm) {
                (Some(a), Some(b)) => Some(a + (b - a) * t),
                (a, b) => a.or(b),
            },
            retract_slow_distance_mm: match (
                self.retract_slow_distance_mm,
                other.retract_slow_distance_mm,
            ) {
                (Some(a), Some(b)) => Some(a + (b - a) * t),
                (a, b) => a.or(b),
            },
            retract_fast_distance_mm: match (
                self.retract_fast_distance_mm,
                other.retract_fast_distance_mm,
            ) {
                (Some(a), Some(b)) => Some(a + (b - a) * t),
                (a, b) => a.or(b),
            },
            lift_slow_speed_mm_min: match (
                self.lift_slow_speed_mm_min,
                other.lift_slow_speed_mm_min,
            ) {
                (Some(a), Some(b)) => Some(a + (b - a) * t),
                (a, b) => a.or(b),
            },
            lift_fast_speed_mm_min: match (
                self.lift_fast_speed_mm_min,
                other.lift_fast_speed_mm_min,
            ) {
                (Some(a), Some(b)) => Some(a + (b - a) * t),
                (a, b) => a.or(b),
            },
            retract_slow_speed_mm_min: match (
                self.retract_slow_speed_mm_min,
                other.retract_slow_speed_mm_min,
            ) {
                (Some(a), Some(b)) => Some(a + (b - a) * t),
                (a, b) => a.or(b),
            },
            retract_fast_speed_mm_min: match (
                self.retract_fast_speed_mm_min,
                other.retract_fast_speed_mm_min,
            ) {
                (Some(a), Some(b)) => Some(a + (b - a) * t),
                (a, b) => a.or(b),
            },
            wait_before_cure_s: match (self.wait_before_cure_s, other.wait_before_cure_s) {
                (Some(a), Some(b)) => Some(a + (b - a) * t),
                (a, b) => a.or(b),
            },
            wait_after_lift_s: match (self.wait_after_lift_s, other.wait_after_lift_s) {
                (Some(a), Some(b)) => Some(a + (b - a) * t),
                (a, b) => a.or(b),
            },
            wait_after_retract_s: match (self.wait_after_retract_s, other.wait_after_retract_s) {
                (Some(a), Some(b)) => Some(a + (b - a) * t),
                (a, b) => a.or(b),
            },
            light_off_delay_s: match (self.light_off_delay_s, other.light_off_delay_s) {
                (Some(a), Some(b)) => Some(a + (b - a) * t),
                (a, b) => a.or(b),
            },
        }
    }
    pub fn duration_s(&self) -> f64 {
        self.lift_slow_distance_mm.unwrap_or(0.0) * 60.0
            / self.lift_slow_speed_mm_min.unwrap_or(1.0)
            + self.lift_fast_distance_mm.unwrap_or(0.0) * 60.0
                / self.lift_fast_speed_mm_min.unwrap_or(1.0)
            + self.retract_slow_distance_mm.unwrap_or(0.0) * 60.0
                / self.retract_slow_speed_mm_min.unwrap_or(1.0)
            + self.retract_fast_distance_mm.unwrap_or(0.0) * 60.0
                / self.retract_fast_speed_mm_min.unwrap_or(1.0)
            + self.wait_before_cure_s.unwrap_or(0.0)
            + self.wait_after_lift_s.unwrap_or(0.0)
            + self.wait_after_retract_s.unwrap_or(0.0)
            + self.light_off_delay_s.unwrap_or(0.0)
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MotionProfile {
    pub normal: Motion,
    pub bottom: Motion,
    pub transition: Motion,
}
impl MotionProfile {
    pub fn overlay(mut self, other: Self) -> Self {
        self.normal = self.normal.overlay(other.normal);
        self.bottom = self.bottom.overlay(other.bottom);
        self.transition = self.transition.overlay(other.transition);
        self
    }
}

impl Motion {
    pub fn legacy(m: &crate::MaterialResolved) -> Self {
        Self {
            lift_slow_distance_mm: Some(m.lift_distance_mm.value()),
            lift_fast_distance_mm: Some(0.0),
            retract_slow_distance_mm: Some(m.lift_distance_mm.value()),
            retract_fast_distance_mm: Some(0.0),
            lift_slow_speed_mm_min: Some(m.lift_speed_mm_min.value()),
            lift_fast_speed_mm_min: Some(m.lift_speed_mm_min.value()),
            retract_slow_speed_mm_min: Some(m.retract_speed_mm_min.value()),
            retract_fast_speed_mm_min: Some(m.retract_speed_mm_min.value()),
            wait_before_cure_s: Some(0.0),
            wait_after_lift_s: Some(0.0),
            wait_after_retract_s: Some(0.0),
            light_off_delay_s: Some(m.light_off_delay_s),
        }
    }
}

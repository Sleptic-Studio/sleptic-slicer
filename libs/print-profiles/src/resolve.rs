// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::motion::{Motion, MotionProfile};
use crate::*;
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialResolved {
    pub identity: MaterialIdentity,
    pub transition_layers: u32,
    pub light_off_delay_s: f64,
    pub layer_height_mm: PositiveFiniteMm,
    pub normal_exposure_sec: PositiveFiniteSec,
    pub bottom_exposure_sec: PositiveFiniteSec,
    pub bottom_layer_count: u32,
    pub lift_distance_mm: PositiveFiniteMm,
    pub lift_speed_mm_min: PositiveFiniteMmPerMin,
    pub retract_speed_mm_min: PositiveFiniteMmPerMin,
    pub scale_compensation_pct: ScaleCompensation,
    pub bottom_compensation_mm: f64,
    pub xy_compensation_mm: f64,
    pub aa_levels: u16,
    pub motion: MotionProfile,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPrintConfig {
    pub printer: PrinterProfile,
    pub material_resolved: MaterialResolved,
    pub overrides_applied: Vec<OverrideRecord>,
    pub warnings: Vec<ResolveWarning>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResolvedField {
    TransitionLayers,
    LightOffDelayS,
    LayerHeightMm,
    NormalExposureSec,
    BottomExposureSec,
    BottomLayerCount,
    LiftDistanceMm,
    LiftSpeedMmMin,
    RetractSpeedMmMin,
    ScaleCompensationPct,
}
impl fmt::Display for ResolvedField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TransitionLayers => "transition_layers",
            Self::LightOffDelayS => "light_off_delay_s",
            Self::LayerHeightMm => "layer_height_mm",
            Self::NormalExposureSec => "normal_exposure_sec",
            Self::BottomExposureSec => "bottom_exposure_sec",
            Self::BottomLayerCount => "bottom_layer_count",
            Self::LiftDistanceMm => "lift_distance_mm",
            Self::LiftSpeedMmMin => "lift_speed_mm_min",
            Self::RetractSpeedMmMin => "retract_speed_mm_min",
            Self::ScaleCompensationPct => "scale_compensation_pct",
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueSource {
    Calibrated,
    Default,
    Derived,
    Override,
}
impl fmt::Display for ValueSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Calibrated => "calibrated",
            Self::Default => "default",
            Self::Derived => "derived",
            Self::Override => "override",
        })
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct OverrideRecord {
    pub field: ResolvedField,
    pub source: ValueSource,
    pub prior_value: Option<f64>,
    pub final_value: f64,
}
impl fmt::Display for OverrideRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {} ({}", self.field, self.final_value, self.source)?;
        if let Some(v) = self.prior_value {
            write!(f, ", was {v}")?;
        }
        f.write_str(")")
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentSource {
    TargetEnergyDensityMjPerCm2,
    TargetLayerHeightMm,
}
impl fmt::Display for IntentSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TargetEnergyDensityMjPerCm2 => "target_energy_density_mj_per_cm2",
            Self::TargetLayerHeightMm => "target_layer_height_mm",
        })
    }
}
#[derive(Debug, Clone, PartialEq)]
pub enum SnapReason {
    Quantised { step: f64 },
    Clamped { min: f64, max: f64 },
    ClampedAtMaximum { max: f64 },
}
impl fmt::Display for SnapReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Quantised { step } => write!(f, "quantised to multiple of {step}"),
            Self::Clamped { min, max } => write!(f, "clamped into [{min}, {max}]"),
            Self::ClampedAtMaximum { max } => write!(f, "clamped at maximum {max}"),
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub enum ResolveWarning {
    Autosnap {
        field: ResolvedField,
        requested: f64,
        snapped: f64,
        reason: SnapReason,
    },
    Derived {
        field: ResolvedField,
        source: IntentSource,
    },
    DefaultUsed {
        field: ResolvedField,
    },
    GenericCalibrationFallback,
}
impl fmt::Display for ResolveWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Autosnap {
                field,
                requested,
                snapped,
                reason,
            } => write!(
                f,
                "field '{field}' snapped from {requested} to {snapped} ({reason})"
            ),
            Self::Derived { field, source } => {
                write!(f, "field '{field}' derived from intent {source}")
            }
            Self::DefaultUsed { field } => {
                write!(f, "field '{field}' fell back to material default")
            }
            Self::GenericCalibrationFallback => f.write_str(
                "no calibration block for this printer; the generic machine block was used",
            ),
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub enum ResolveError {
    InvalidMotion(&'static str),
    RejectedBelowMinimum {
        field: ResolvedField,
        value: f64,
        minimum: f64,
    },
    MissingRequiredField {
        field: ResolvedField,
    },
}
impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMotion(s) => f.write_str(s),
            Self::RejectedBelowMinimum {
                field,
                value,
                minimum,
            } => write!(
                f,
                "field '{field}' value {value} is below the printer minimum {minimum}"
            ),
            Self::MissingRequiredField { field } => write!(
                f,
                "required field '{field}' has no value after all resolution layers"
            ),
        }
    }
}
impl std::error::Error for ResolveError {}
fn quantise(value: f64, step: f64) -> f64 {
    let ratio = value / step;
    if !ratio.is_finite() {
        return value;
    }
    let result = ratio.round().max(1.0) * step;
    if result.is_finite() { result } else { value }
}
pub fn snap_to_z_step(
    constraints: &PrinterConstraints,
    value: PositiveFiniteMm,
) -> PositiveFiniteMm {
    let snapped = constraints
        .z_step_mm
        .map_or(value.value(), |s| quantise(value.value(), s.value()));
    PositiveFiniteMm::new(snapped).unwrap_or(value)
}
fn adjustment(
    field: ResolvedField,
    requested: f64,
    snapped: f64,
    reason: SnapReason,
    warnings: &mut Vec<ResolveWarning>,
) -> f64 {
    if ((snapped - requested) / requested).abs() > 0.05 {
        warnings.push(ResolveWarning::Autosnap {
            field,
            requested,
            snapped,
            reason,
        });
    }
    snapped
}
fn constrain(
    field: ResolvedField,
    mut value: f64,
    c: &PrinterConstraints,
    warnings: &mut Vec<ResolveWarning>,
) -> Result<f64, ResolveError> {
    match field {
        ResolvedField::LayerHeightMm => {
            if let Some(range) = c.layer_height_range_mm {
                let min = range.min().value();
                let max = range.max().value();
                value = adjustment(
                    field,
                    value,
                    value.clamp(min, max),
                    SnapReason::Clamped { min, max },
                    warnings,
                );
            }
            if let Some(step) = c.z_step_mm {
                value = adjustment(
                    field,
                    value,
                    quantise(value, step.value()),
                    SnapReason::Quantised { step: step.value() },
                    warnings,
                );
            }
        }
        ResolvedField::LiftDistanceMm => {
            if let Some(max) = c.lift_distance_max_mm {
                value = adjustment(
                    field,
                    value,
                    value.min(max.value()),
                    SnapReason::ClampedAtMaximum { max: max.value() },
                    warnings,
                );
            }
        }
        ResolvedField::LiftSpeedMmMin | ResolvedField::RetractSpeedMmMin => {
            let range = if field == ResolvedField::LiftSpeedMmMin {
                c.lift_speed_range_mm_min
            } else {
                c.retract_speed_range_mm_min
            };
            if let Some(range) = range {
                let min = range.min().value();
                let max = range.max().value();
                if value < min {
                    return Err(ResolveError::RejectedBelowMinimum {
                        field,
                        value,
                        minimum: min,
                    });
                }
                value = adjustment(
                    field,
                    value,
                    value.min(max),
                    SnapReason::ClampedAtMaximum { max },
                    warnings,
                );
            }
        }
        _ => {}
    }
    Ok(value)
}
struct Selection {
    values: BTreeMap<ResolvedField, f64>,
    scale: Option<ScaleCompensation>,
    records: Vec<OverrideRecord>,
    warnings: Vec<ResolveWarning>,
}
fn select(
    printer: &PrinterProfile,
    material: &EffectiveMaterial,
) -> Result<Selection, ResolveError> {
    let mut s = Selection {
        values: BTreeMap::new(),
        scale: material.defaults.scale_compensation_pct,
        records: Vec::new(),
        warnings: Vec::new(),
    };
    let cal = material
        .calibration_for(&printer.identity.id)
        .map(|(_, c)| c);
    if cal.is_some()
        && !material
            .per_printer
            .contains_key(&printer.identity.id.clone().into())
    {
        s.warnings.push(ResolveWarning::GenericCalibrationFallback);
    }
    if let Some(c) = cal {
        if let Some(v) = c.transition_layers {
            let value = f64::from(v);
            s.values.insert(ResolvedField::TransitionLayers, value);
            s.records.push(OverrideRecord {
                field: ResolvedField::TransitionLayers,
                source: ValueSource::Calibrated,
                prior_value: None,
                final_value: value,
            });
        }
        if let Some(v) = c.light_off_delay_s {
            let value = v;
            s.values.insert(ResolvedField::LightOffDelayS, value);
            s.records.push(OverrideRecord {
                field: ResolvedField::LightOffDelayS,
                source: ValueSource::Calibrated,
                prior_value: None,
                final_value: value,
            });
        }
        if let Some(v) = c.normal_exposure_sec {
            let value = v.value();
            s.values.insert(ResolvedField::NormalExposureSec, value);
            s.records.push(OverrideRecord {
                field: ResolvedField::NormalExposureSec,
                source: ValueSource::Calibrated,
                prior_value: None,
                final_value: value,
            });
        }
        if let Some(v) = c.bottom_exposure_sec {
            let value = v.value();
            s.values.insert(ResolvedField::BottomExposureSec, value);
            s.records.push(OverrideRecord {
                field: ResolvedField::BottomExposureSec,
                source: ValueSource::Calibrated,
                prior_value: None,
                final_value: value,
            });
        }
        if let Some(v) = c.bottom_layer_count {
            let value = f64::from(v);
            s.values.insert(ResolvedField::BottomLayerCount, value);
            s.records.push(OverrideRecord {
                field: ResolvedField::BottomLayerCount,
                source: ValueSource::Calibrated,
                prior_value: None,
                final_value: value,
            });
        }
        if let Some(v) = c.lift_distance_mm {
            let value = v.value();
            s.values.insert(ResolvedField::LiftDistanceMm, value);
            s.records.push(OverrideRecord {
                field: ResolvedField::LiftDistanceMm,
                source: ValueSource::Calibrated,
                prior_value: None,
                final_value: value,
            });
        }
        if let Some(v) = c.lift_speed_mm_min {
            let value = v.value();
            s.values.insert(ResolvedField::LiftSpeedMmMin, value);
            s.records.push(OverrideRecord {
                field: ResolvedField::LiftSpeedMmMin,
                source: ValueSource::Calibrated,
                prior_value: None,
                final_value: value,
            });
        }
        if let Some(v) = c.retract_speed_mm_min {
            let value = v.value();
            s.values.insert(ResolvedField::RetractSpeedMmMin, value);
            s.records.push(OverrideRecord {
                field: ResolvedField::RetractSpeedMmMin,
                source: ValueSource::Calibrated,
                prior_value: None,
                final_value: value,
            });
        }
    }
    if !s.values.contains_key(&ResolvedField::LightOffDelayS)
        && let Some(v) = material.defaults.light_off_delay_s
    {
        let value = v;
        s.values.insert(ResolvedField::LightOffDelayS, value);
        s.records.push(OverrideRecord {
            field: ResolvedField::LightOffDelayS,
            source: ValueSource::Default,
            prior_value: None,
            final_value: value,
        });
        s.warnings.push(ResolveWarning::DefaultUsed {
            field: ResolvedField::LightOffDelayS,
        });
    }
    if !s.values.contains_key(&ResolvedField::TransitionLayers)
        && let Some(v) = material.defaults.transition_layers
    {
        let value = f64::from(v);
        s.values.insert(ResolvedField::TransitionLayers, value);
        s.records.push(OverrideRecord {
            field: ResolvedField::TransitionLayers,
            source: ValueSource::Default,
            prior_value: None,
            final_value: value,
        });
        s.warnings.push(ResolveWarning::DefaultUsed {
            field: ResolvedField::TransitionLayers,
        });
    }
    if !s.values.contains_key(&ResolvedField::BottomLayerCount)
        && let Some(v) = material.defaults.bottom_layer_count
    {
        let value = f64::from(v);
        s.values.insert(ResolvedField::BottomLayerCount, value);
        s.records.push(OverrideRecord {
            field: ResolvedField::BottomLayerCount,
            source: ValueSource::Default,
            prior_value: None,
            final_value: value,
        });
        s.warnings.push(ResolveWarning::DefaultUsed {
            field: ResolvedField::BottomLayerCount,
        });
    }
    if s.scale.is_some() {
        s.warnings.push(ResolveWarning::DefaultUsed {
            field: ResolvedField::ScaleCompensationPct,
        });
    }
    if let Some(v) = material.intent.target_layer_height_mm {
        let value = v.value();
        s.values.insert(ResolvedField::LayerHeightMm, value);
        s.records.push(OverrideRecord {
            field: ResolvedField::LayerHeightMm,
            source: ValueSource::Derived,
            prior_value: None,
            final_value: value,
        });
        s.warnings.push(ResolveWarning::Derived {
            field: ResolvedField::LayerHeightMm,
            source: IntentSource::TargetLayerHeightMm,
        });
    }
    if !s.values.contains_key(&ResolvedField::NormalExposureSec)
        && let (Some(energy), Some(light)) = (
            material.intent.target_energy_density_mj_per_cm2,
            printer.constraints.light_intensity_mw_per_cm2,
        )
    {
        let value = energy.value() / light.value();
        if value.is_finite() && value > 0.0 {
            s.values.insert(ResolvedField::NormalExposureSec, value);
            s.records.push(OverrideRecord {
                field: ResolvedField::NormalExposureSec,
                source: ValueSource::Derived,
                prior_value: None,
                final_value: value,
            });
            s.warnings.push(ResolveWarning::Derived {
                field: ResolvedField::NormalExposureSec,
                source: IntentSource::TargetEnergyDensityMjPerCm2,
            });
        }
    }
    for (field, value) in &mut s.values {
        *value = constrain(*field, *value, &printer.constraints, &mut s.warnings)?;
    }
    Ok(s)
}
fn overrides(
    s: &mut Selection,
    printer: &PrinterProfile,
    o: &SceneOverrides,
) -> Result<(), ResolveError> {
    if let Some(v) = o.transition_layers {
        let value = f64::from(v);
        let field = ResolvedField::TransitionLayers;
        let prior_value = s.values.get(&field).copied();
        s.records.push(OverrideRecord {
            field,
            source: ValueSource::Override,
            prior_value,
            final_value: value,
        });
        s.values.insert(
            field,
            constrain(field, value, &printer.constraints, &mut s.warnings)?,
        );
    }
    if let Some(v) = o.light_off_delay_s
        && v.is_finite()
        && v >= 0.0
    {
        let value = v;
        let field = ResolvedField::LightOffDelayS;
        let prior_value = s.values.get(&field).copied();
        s.records.push(OverrideRecord {
            field,
            source: ValueSource::Override,
            prior_value,
            final_value: value,
        });
        s.values.insert(
            field,
            constrain(field, value, &printer.constraints, &mut s.warnings)?,
        );
    }
    if let Some(v) = o.layer_height_mm {
        let value = v.value();
        let field = ResolvedField::LayerHeightMm;
        let prior_value = s.values.get(&field).copied();
        s.records.push(OverrideRecord {
            field,
            source: ValueSource::Override,
            prior_value,
            final_value: value,
        });
        s.values.insert(
            field,
            constrain(field, value, &printer.constraints, &mut s.warnings)?,
        );
    }
    if let Some(v) = o.normal_exposure_sec {
        let value = v.value();
        let field = ResolvedField::NormalExposureSec;
        let prior_value = s.values.get(&field).copied();
        s.records.push(OverrideRecord {
            field,
            source: ValueSource::Override,
            prior_value,
            final_value: value,
        });
        s.values.insert(
            field,
            constrain(field, value, &printer.constraints, &mut s.warnings)?,
        );
    }
    if let Some(v) = o.bottom_exposure_sec {
        let value = v.value();
        let field = ResolvedField::BottomExposureSec;
        let prior_value = s.values.get(&field).copied();
        s.records.push(OverrideRecord {
            field,
            source: ValueSource::Override,
            prior_value,
            final_value: value,
        });
        s.values.insert(
            field,
            constrain(field, value, &printer.constraints, &mut s.warnings)?,
        );
    }
    if let Some(v) = o.bottom_layer_count {
        let value = f64::from(v);
        let field = ResolvedField::BottomLayerCount;
        let prior_value = s.values.get(&field).copied();
        s.records.push(OverrideRecord {
            field,
            source: ValueSource::Override,
            prior_value,
            final_value: value,
        });
        s.values.insert(
            field,
            constrain(field, value, &printer.constraints, &mut s.warnings)?,
        );
    }
    if let Some(v) = o.lift_distance_mm {
        let value = v.value();
        let field = ResolvedField::LiftDistanceMm;
        let prior_value = s.values.get(&field).copied();
        s.records.push(OverrideRecord {
            field,
            source: ValueSource::Override,
            prior_value,
            final_value: value,
        });
        s.values.insert(
            field,
            constrain(field, value, &printer.constraints, &mut s.warnings)?,
        );
    }
    if let Some(v) = o.lift_speed_mm_min {
        let value = v.value();
        let field = ResolvedField::LiftSpeedMmMin;
        let prior_value = s.values.get(&field).copied();
        s.records.push(OverrideRecord {
            field,
            source: ValueSource::Override,
            prior_value,
            final_value: value,
        });
        s.values.insert(
            field,
            constrain(field, value, &printer.constraints, &mut s.warnings)?,
        );
    }
    if let Some(v) = o.retract_speed_mm_min {
        let value = v.value();
        let field = ResolvedField::RetractSpeedMmMin;
        let prior_value = s.values.get(&field).copied();
        s.records.push(OverrideRecord {
            field,
            source: ValueSource::Override,
            prior_value,
            final_value: value,
        });
        s.values.insert(
            field,
            constrain(field, value, &printer.constraints, &mut s.warnings)?,
        );
    }
    if o.scale_compensation_pct.is_some() {
        s.scale = o.scale_compensation_pct;
    }
    Ok(())
}
fn required(s: &Selection, field: ResolvedField) -> Result<f64, ResolveError> {
    s.values
        .get(&field)
        .copied()
        .ok_or(ResolveError::MissingRequiredField { field })
}
pub fn resolve_accuracy(
    printer: &PrinterProfile,
    material: &EffectiveMaterial,
    o: &SceneOverrides,
) -> (f64, f64, u16) {
    let c = material
        .calibration_for(&printer.identity.id)
        .map(|(_, c)| c);
    (
        o.bottom_compensation_mm
            .or_else(|| c.and_then(|c| c.bottom_compensation_mm))
            .or(material.defaults.bottom_compensation_mm)
            .unwrap_or(0.0),
        o.xy_compensation_mm
            .or_else(|| c.and_then(|c| c.xy_compensation_mm))
            .or(material.defaults.xy_compensation_mm)
            .unwrap_or(0.0),
        o.aa_levels
            .or_else(|| c.and_then(|c| c.aa_levels))
            .or(material.defaults.aa_levels)
            .unwrap_or(256),
    )
}
pub fn resolve_layer_height(
    printer: &PrinterProfile,
    material: &EffectiveMaterial,
    requested: Option<PositiveFiniteMm>,
) -> Result<PositiveFiniteMm, ResolveError> {
    let mut s = select(printer, material)?;
    overrides(
        &mut s,
        printer,
        &SceneOverrides {
            layer_height_mm: requested,
            ..Default::default()
        },
    )?;
    PositiveFiniteMm::new(required(&s, ResolvedField::LayerHeightMm)?).map_err(|_| {
        ResolveError::MissingRequiredField {
            field: ResolvedField::LayerHeightMm,
        }
    })
}
pub fn resolve(
    printer: &PrinterProfile,
    material: &EffectiveMaterial,
    o: &SceneOverrides,
) -> Result<ResolvedPrintConfig, ResolveError> {
    let mut s = select(printer, material)?;
    overrides(&mut s, printer, o)?;
    let layer_height_mm = PositiveFiniteMm::new(required(&s, ResolvedField::LayerHeightMm)?)
        .map_err(|_| ResolveError::MissingRequiredField {
            field: ResolvedField::LayerHeightMm,
        })?;
    let normal_exposure_sec =
        PositiveFiniteSec::new(required(&s, ResolvedField::NormalExposureSec)?).map_err(|_| {
            ResolveError::MissingRequiredField {
                field: ResolvedField::NormalExposureSec,
            }
        })?;
    let bottom_exposure_sec =
        PositiveFiniteSec::new(required(&s, ResolvedField::BottomExposureSec)?).map_err(|_| {
            ResolveError::MissingRequiredField {
                field: ResolvedField::BottomExposureSec,
            }
        })?;
    let bottom_layer_count = required(&s, ResolvedField::BottomLayerCount)? as u32;
    let lift_distance_mm = PositiveFiniteMm::new(required(&s, ResolvedField::LiftDistanceMm)?)
        .map_err(|_| ResolveError::MissingRequiredField {
            field: ResolvedField::LiftDistanceMm,
        })?;
    let lift_speed_mm_min =
        PositiveFiniteMmPerMin::new(required(&s, ResolvedField::LiftSpeedMmMin)?).map_err(
            |_| ResolveError::MissingRequiredField {
                field: ResolvedField::LiftSpeedMmMin,
            },
        )?;
    let retract_speed_mm_min =
        PositiveFiniteMmPerMin::new(required(&s, ResolvedField::RetractSpeedMmMin)?).map_err(
            |_| ResolveError::MissingRequiredField {
                field: ResolvedField::RetractSpeedMmMin,
            },
        )?;
    let scale_compensation_pct = s.scale.ok_or(ResolveError::MissingRequiredField {
        field: ResolvedField::ScaleCompensationPct,
    })?;
    let transition_layers = s
        .values
        .get(&ResolvedField::TransitionLayers)
        .copied()
        .unwrap_or(0.0) as u32;
    let light_off_delay_s = s
        .values
        .get(&ResolvedField::LightOffDelayS)
        .copied()
        .unwrap_or(0.0);
    let (bottom_compensation_mm, xy_compensation_mm, aa_levels) =
        resolve_accuracy(printer, material, o);
    let mut motion = printer.motion.unwrap_or_default();
    if let Some(m) = &material.defaults.motion {
        motion = motion.overlay(*m);
    }
    if let Some(m) = material
        .calibration_for(&printer.identity.id)
        .and_then(|(_, c)| c.motion.as_ref())
    {
        motion = motion.overlay(*m);
    }
    let mut normal = Motion {
        lift_slow_distance_mm: Some(lift_distance_mm.value()),
        lift_fast_distance_mm: Some(0.0),
        retract_slow_distance_mm: Some(lift_distance_mm.value()),
        retract_fast_distance_mm: Some(0.0),
        lift_slow_speed_mm_min: Some(lift_speed_mm_min.value()),
        lift_fast_speed_mm_min: Some(lift_speed_mm_min.value()),
        retract_slow_speed_mm_min: Some(retract_speed_mm_min.value()),
        retract_fast_speed_mm_min: Some(retract_speed_mm_min.value()),
        wait_before_cure_s: Some(0.0),
        wait_after_lift_s: Some(0.0),
        wait_after_retract_s: Some(0.0),
        light_off_delay_s: Some(light_off_delay_s),
    };
    normal = normal.overlay(motion.normal);
    let mut bottom = normal;
    bottom = bottom.overlay(motion.bottom);
    let mut transition = bottom;
    transition = transition.overlay(motion.transition);
    for cycle in [normal, bottom, transition] {
        cycle.validate().map_err(ResolveError::InvalidMotion)?;
        if printer
            .constraints
            .lift_distance_max_mm
            .is_some_and(|m| cycle.total_lift_distance_mm() > m.value())
        {
            return Err(ResolveError::InvalidMotion(
                "Motion lift exceeds the printer's maximum lift distance",
            ));
        }
    }
    Ok(ResolvedPrintConfig {
        printer: printer.clone(),
        material_resolved: MaterialResolved {
            identity: material.identity.clone(),
            transition_layers,
            light_off_delay_s,
            layer_height_mm,
            normal_exposure_sec,
            bottom_exposure_sec,
            bottom_layer_count,
            lift_distance_mm,
            lift_speed_mm_min,
            retract_speed_mm_min,
            scale_compensation_pct,
            bottom_compensation_mm,
            xy_compensation_mm,
            aa_levels,
            motion,
        },
        overrides_applied: s.records,
        warnings: s.warnings,
    })
}

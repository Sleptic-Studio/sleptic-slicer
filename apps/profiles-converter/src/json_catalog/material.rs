// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! JsonCatalog resin records to Sleptic resin profiles.
use std::collections::BTreeMap;

use sc_print_profiles::*;
use serde::Deserialize;

use super::problem::{Problem, RecordKind, RecordProblem};

/// A resin record as the file has it. The key names are JsonCatalog's.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MaterialRecord {
    // Who it is.
    template_id: String,
    brand: String,
    name: String,
    resin_family: String,
    profile_version: u64,
    // The printers it is calibrated for (all with the values below).
    #[serde(default)]
    valid_for_presets: Vec<String>,
    // Exposure.
    layer_height_mm: Option<f64>,
    normal_exposure_sec: Option<f64>,
    bottom_exposure_sec: Option<f64>,
    bottom_layer_count: Option<u32>,
    // Motion.
    lift_distance_mm: Option<f64>,
    lift_speed_mm_min: Option<f64>,
    retract_speed_mm_min: Option<f64>,
    // Shrinkage, per axis.
    scale_compensation_pct: Option<PerAxis>,
}

#[derive(Debug, Deserialize)]
struct PerAxis {
    x: f64,
    y: f64,
    z: f64,
}

/// Turns the values of one record into profile values, filing every refusal
/// under the record's id and the field it came from.
struct Mapping<'a> {
    id: &'a str,
}

impl Mapping<'_> {
    fn refused(&self, field: &'static str) -> impl FnOnce(Box<dyn std::error::Error + Send + Sync>) -> RecordProblem + '_ {
        move |why| RecordProblem::new(RecordKind::Resin, self.id, field, Problem::Refused(why))
    }

    fn text(&self, field: &'static str, value: &str) -> Result<NonEmptyString, RecordProblem> {
        NonEmptyString::new(value).map_err(|_| RecordProblem::new(RecordKind::Resin, self.id, field, Problem::Empty))
    }

    /// An optional number the profile type checks with `make`.
    fn value<T, E: std::error::Error + Send + Sync + 'static>(
        &self,
        field: &'static str,
        number: Option<f64>,
        make: impl FnOnce(f64) -> Result<T, E>,
    ) -> Result<Option<T>, RecordProblem> {
        number.map(|n| make(n).map_err(|e| self.refused(field)(Box::new(e)))).transpose()
    }
}

pub(super) fn convert(record: MaterialRecord) -> Result<MaterialProfile, RecordProblem> {
    let at = Mapping { id: &record.template_id };
    let id = MaterialId::new(&record.template_id)
        .map_err(|why| RecordProblem::new(RecordKind::Resin, at.id, "templateId", Problem::BadId(why)))?;
    let identity = MaterialIdentity {
        id,
        brand: at.text("brand", &record.brand)?,
        family: at.text("resinFamily", &record.resin_family)?,
        name: at.text("name", &record.name)?,
        profile_version: semver::Version::new(record.profile_version, 0, 0),
    };
    let intent = MaterialIntent {
        target_energy_density_mj_per_cm2: None,
        target_layer_height_mm: at.value("layerHeightMm", record.layer_height_mm, PositiveFiniteMm::new)?,
    };
    let calibration = MaterialPerPrinter {
        normal_exposure_sec: at.value("normalExposureSec", record.normal_exposure_sec, PositiveFiniteSec::new)?,
        bottom_exposure_sec: at.value("bottomExposureSec", record.bottom_exposure_sec, PositiveFiniteSec::new)?,
        bottom_layer_count: record.bottom_layer_count,
        lift_distance_mm: at.value("liftDistanceMm", record.lift_distance_mm, PositiveFiniteMm::new)?,
        lift_speed_mm_min: at.value("liftSpeedMmMin", record.lift_speed_mm_min, PositiveFiniteMmPerMin::new)?,
        retract_speed_mm_min: at.value("retractSpeedMmMin", record.retract_speed_mm_min, PositiveFiniteMmPerMin::new)?,
        ..MaterialPerPrinter::default()
    };
    let scale_compensation_pct = match &record.scale_compensation_pct {
        None => None,
        Some(axes) => {
            let axis = |field: &'static str, value: f64| -> Result<_, RecordProblem> {
                ScaleCompensationPercent::new(value).map_err(|e| at.refused(field)(Box::new(e)))
            };
            Some(ScaleCompensation { x: axis("x", axes.x)?, y: axis("y", axes.y)?, z: axis("z", axes.z)? })
        }
    };
    // The same calibration for every printer the record lists.
    let mut per_printer = BTreeMap::new();
    for target in &record.valid_for_presets {
        let printer = PrinterId::new(target).map_err(|why| {
            RecordProblem::new(RecordKind::Resin, at.id, "validForPresets", Problem::BadTarget { target: target.clone(), why })
        })?;
        per_printer.insert(CalibrationTarget::from(printer), calibration);
    }
    let defaults = MaterialDefaults {
        bottom_layer_count: record.bottom_layer_count,
        scale_compensation_pct,
        ..MaterialDefaults::default()
    };
    Ok(MaterialProfile { extension: false, identity, intent, defaults, per_printer })
}

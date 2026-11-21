// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! What the read-only resin profile window shows: the resin (with what its
//! extension adds) as sections of label and value. Identity, intent and
//! defaults come first, then one section per calibration. Every optional
//! field is listed, unset ones as "not set", so a partial profile shows
//! what it lacks.
use std::collections::BTreeMap;

use sc_print_primitives::PrinterId;
use sc_print_profiles::{
    CalibrationTarget, EffectiveMaterial, MaterialDefaults, MaterialIdentity, MaterialIntent,
    MaterialPerPrinter, MaterialProfile,
};
use slint::{ModelRc, SharedString, VecModel};

use crate::i18n::ctr;
use crate::profile_catalog::{CatalogPrinter, ProfileSource};
use crate::{ResinDisplayRow, ResinDisplayCard};

/// The window's heading: brand, then name.
pub(crate) fn resin_title(identity: &MaterialIdentity) -> String {
    format!("{} {}", identity.brand.as_str(), identity.name.as_str())
}

/// All the sections for a resin. The calibration the default printer would
/// print with (its own, or else the generic one) is highlighted.
pub(crate) fn profile_view_rows(
    material: &EffectiveMaterial,
    printers: &[CatalogPrinter],
    default_printer: Option<&PrinterId>,
    source: ProfileSource,
) -> Vec<ResinDisplayCard> {
    let blocks = material.per_printer();
    let wanted = default_printer.map(CalibrationTarget::from_printer);
    let used = match wanted {
        Some(own) if blocks.contains_key(&own) => Some(own),
        Some(_) if blocks.contains_key(&CalibrationTarget::Generic) => {
            Some(CalibrationTarget::Generic)
        }
        _ => None,
    };
    let calibrations = in_display_order(blocks, printers, default_printer)
        .into_iter()
        .map(|(target, block)| {
            let title = match target {
                CalibrationTarget::Generic => ctr!("ProfileViewDialog", "Generic calibration"),
                CalibrationTarget::Printer(id) => {
                    ctr!(
                        "ProfileViewDialog",
                        "Calibration: {}",
                        printer_name(printers, id.as_printer_id())
                    )
                }
            };
            section(
                title,
                used.as_ref() == Some(target),
                calibration_fields(block),
            )
        });
    let mut sections = vec![
        identity_section(material.identity(), source),
        intent_section(material.intent()),
        defaults_section(material.defaults()),
    ];
    sections.extend(calibrations);
    sections
}

/// The profile's calibrations and their labels, in display order.
pub(crate) fn ordered_calibration_targets(
    profile: &MaterialProfile,
    printers: &[CatalogPrinter],
    default_printer: Option<&PrinterId>,
) -> Vec<(CalibrationTarget, SharedString)> {
    let ordered = in_display_order(&profile.per_printer, printers, default_printer);
    ordered
        .into_iter()
        .map(|(target, _)| {
            let label = match target {
                CalibrationTarget::Generic => ctr!("ProfileViewDialog", "Generic"),
                CalibrationTarget::Printer(id) => printer_name(printers, id.as_printer_id()).into(),
            };
            (target.clone(), label)
        })
        .collect()
}

/// The printer's name in the catalog, or its id when it is not there.
fn printer_name(printers: &[CatalogPrinter], id: &PrinterId) -> String {
    match printers.iter().find(|printer| &printer.id == id) {
        Some(printer) => printer.name.clone(),
        None => id.as_str().to_owned(),
    }
}

/// The default printer's calibration first, the generic one next, the rest
/// by printer name.
fn in_display_order<'a, V>(
    blocks: &'a BTreeMap<CalibrationTarget, V>,
    printers: &[CatalogPrinter],
    default_printer: Option<&PrinterId>,
) -> Vec<(&'a CalibrationTarget, &'a V)> {
    let wanted = default_printer.map(CalibrationTarget::from_printer);
    let group = |target: &CalibrationTarget| match target {
        _ if wanted.as_ref() == Some(target) => 0,
        CalibrationTarget::Generic => 1,
        CalibrationTarget::Printer(_) => 2,
    };
    let mut ordered: Vec<_> = blocks.iter().collect();
    ordered.sort_by(|(a, _), (b, _)| {
        group(a).cmp(&group(b)).then_with(|| {
            printer_name(printers, a.printer_id()).cmp(&printer_name(printers, b.printer_id()))
        })
    });
    ordered
}

fn section(
    title: SharedString,
    highlighted: bool,
    fields: Vec<ResinDisplayRow>,
) -> ResinDisplayCard {
    ResinDisplayCard {
        card_heading: title,
        is_emphasised: highlighted,
        display_rows: ModelRc::new(VecModel::from(fields)),
    }
}

/// One row: a translated label and its value, or "not set".
fn row(label: SharedString, value: Option<String>) -> ResinDisplayRow {
    let value = value.map_or_else(|| ctr!("ProfileViewDialog", "not set"), SharedString::from);
    ResinDisplayRow { caption_text: label, display_text: value }
}

/// A number as the profile file would write it (30.0 shows as 30).
fn number(value: impl std::fmt::Display) -> String {
    value.to_string()
}

pub(crate) fn identity_section(
    identity: &MaterialIdentity,
    source: ProfileSource,
) -> ResinDisplayCard {
    let origin = match source {
        ProfileSource::Bundled => ctr!("ProfileViewDialog", "Bundled"),
        ProfileSource::User => ctr!("ProfileViewDialog", "Custom"),
    };
    let text = |s: &str| Some(s.to_owned());
    let rows = vec![
        row(
            ctr!("ProfileViewDialog", "Brand"),
            text(identity.brand.as_str()),
        ),
        row(
            ctr!("ProfileViewDialog", "Name"),
            text(identity.name.as_str()),
        ),
        row(
            ctr!("ProfileViewDialog", "Family"),
            text(identity.family.as_str()),
        ),
        row(
            ctr!("ProfileViewDialog", "Profile version"),
            Some(identity.profile_version.to_string()),
        ),
        ResinDisplayRow {
            caption_text: ctr!("ProfileViewDialog", "Origin"),
            display_text: origin,
        },
    ];
    section(ctr!("ProfileViewDialog", "Identity"), false, rows)
}

pub(crate) fn intent_section(intent: &MaterialIntent) -> ResinDisplayCard {
    let rows = vec![
        row(
            ctr!("ProfileViewDialog", "Target energy density (mJ/cm2)"),
            intent
                .target_energy_density_mj_per_cm2
                .map(|v| number(v.value())),
        ),
        row(
            ctr!("ProfileViewDialog", "Target layer height (mm)"),
            intent.target_layer_height_mm.map(|v| number(v.value())),
        ),
    ];
    section(ctr!("ProfileViewDialog", "Intent"), false, rows)
}

fn defaults_section(defaults: &MaterialDefaults) -> ResinDisplayCard {
    let scale = defaults
        .scale_compensation_pct
        .map(|s| format!("{}% / {}% / {}%", s.x.value(), s.y.value(), s.z.value()));
    let rows = vec![
        row(
            ctr!("ProfileViewDialog", "Scale compensation (x/y/z)"),
            scale,
        ),
        row(
            ctr!("ProfileViewDialog", "Bottom layer count"),
            defaults.bottom_layer_count.map(number),
        ),
        row(
            ctr!("ProfileViewDialog", "Price per liter"),
            defaults.price_per_liter.map(|v| number(v.value())),
        ),
    ];
    section(ctr!("ProfileViewDialog", "Defaults"), false, rows)
}

fn calibration_fields(block: &MaterialPerPrinter) -> Vec<ResinDisplayRow> {
    vec![
        row(
            ctr!("ProfileViewDialog", "Normal exposure (s)"),
            block.normal_exposure_sec.map(|v| number(v.value())),
        ),
        row(
            ctr!("ProfileViewDialog", "Bottom exposure (s)"),
            block.bottom_exposure_sec.map(|v| number(v.value())),
        ),
        row(
            ctr!("ProfileViewDialog", "Bottom layer count"),
            block.bottom_layer_count.map(number),
        ),
        row(
            ctr!("ProfileViewDialog", "Lift distance (mm)"),
            block.lift_distance_mm.map(|v| number(v.value())),
        ),
        row(
            ctr!("ProfileViewDialog", "Lift speed (mm/min)"),
            block.lift_speed_mm_min.map(|v| number(v.value())),
        ),
        row(
            ctr!("ProfileViewDialog", "Retract speed (mm/min)"),
            block.retract_speed_mm_min.map(|v| number(v.value())),
        ),
    ]
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "tests_profile_view_dialog.rs"]
mod tests_profile_view_dialog;

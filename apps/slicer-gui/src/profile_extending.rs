// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Extending a resin: adding a calibration for a printer it has none for.
//! The calibration goes into the resin's extension file, which has the
//! resin's id. The first extension copies the resin's identity, is marked
//! as an extension and leaves intent and defaults empty (the resin's own
//! apply); later ones add to it. The dialog itself is in the preferences
//! window.
use sc_print_primitives::PrinterId;
use sc_print_profiles::{CalibrationTarget, EffectiveMaterial, MaterialPerPrinter, MaterialProfile};
use slint::SharedString;

use crate::i18n::ctr;
use crate::profile_catalog::CatalogPrinter;

/// The printers in the catalog (the generic machine too) that the resin,
/// with its extension, has no calibration for: exactly what the extend
/// dialog offers. The default printer comes first, then the generic
/// machine, then the rest in catalog order.
pub(crate) fn printers_without_calibration(
    resin: &EffectiveMaterial,
    catalog: &[CatalogPrinter],
    usual_printer: Option<&PrinterId>,
) -> Vec<(CalibrationTarget, SharedString)> {
    let usual = usual_printer.map(CalibrationTarget::from_printer);
    let calibrated = resin.per_printer();
    let mut rows = Vec::new();
    for printer in catalog {
        let target = CalibrationTarget::from_printer(&printer.id);
        if calibrated.contains_key(&target) {
            continue;
        }
        let shown: SharedString = if target == CalibrationTarget::Generic {
            ctr!("ExtendDialog", "Generic")
        } else {
            printer.name.as_str().into()
        };
        rows.push((target, shown));
    }
    // The usual printer, then the generic machine, then the rest; a stable
    // sort keeps catalog order inside each.
    let rank = |target: &CalibrationTarget| {
        if usual.as_ref() == Some(target) {
            0
        } else if *target == CalibrationTarget::Generic {
            1
        } else {
            2
        }
    };
    rows.sort_by_key(|(target, _)| rank(target));
    rows
}

/// The extension file to save once `values` are added for `target`: the
/// resin's existing extension with them added, or a new one. A resin row
/// that is itself an extension (one whose resin is missing) is added to
/// directly, keeping its calibrations.
pub(crate) fn extension_with_calibration(
    resin: &MaterialProfile,
    saved_extension: Option<&MaterialProfile>,
    printer: &CalibrationTarget,
    calibration: MaterialPerPrinter,
) -> MaterialProfile {
    let mut file = if let Some(saved) = saved_extension {
        saved.clone()
    } else if resin.extension {
        resin.clone()
    } else {
        // Only the identity is copied; empty intent and defaults leave the
        // resin's own in force.
        MaterialProfile {
            extension: true,
            identity: resin.identity.clone(),
            intent: Default::default(),
            defaults: Default::default(),
            per_printer: Default::default(),
        }
    };
    file.per_printer.insert(printer.clone(), calibration);
    file
}

#[cfg(test)]
mod tests;

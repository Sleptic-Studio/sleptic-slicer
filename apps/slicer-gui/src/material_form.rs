// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The resin profile editor's fields, apart from any window: the texts typed
//! into them turned into profile values, and values turned back into texts.
//! A field left empty means unset and is saved as no key at all, never as
//! zero. Scale compensation is set on all three axes or on none.
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use sc_print_primitives::{
    PositiveFiniteCurrencyPerLiter, PositiveFiniteMm, PositiveFiniteMmPerMin, PositiveFiniteSec,
    ScaleCompensation, ScaleCompensationPercent,
};
use sc_print_profiles::{
    MaterialDefaults, MaterialPerPrinter, MaterialProfile, save_material_extension_to_library,
    save_material_profile_to_library,
};
use slint::SharedString;

use crate::i18n::ctr;

/// One field of the editor. The calibration block uses the first six, the
/// defaults block `BottomLayers` and the last five.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ResinField {
    Exposure,
    BottomLayersExposure,
    BottomLayers,
    LiftHeight,
    LiftRate,
    RetractRate,
    ScaleX,
    ScaleY,
    ScaleZ,
    Price,
    OverhangAngle,
}

impl ResinField {
    /// The field's name in English for logs, and in the user's language for
    /// dialogs (the second built where it is needed, as translating needs
    /// the literal text at the call).
    fn names(self) -> (&'static str, fn() -> SharedString) {
        match self {
            Self::Exposure => ("Normal exposure", || ctr!("MaterialForm", "Normal exposure")),
            Self::BottomLayersExposure => ("Bottom exposure", || ctr!("MaterialForm", "Bottom exposure")),
            Self::BottomLayers => ("Bottom layer count", || ctr!("MaterialForm", "Bottom layer count")),
            Self::LiftHeight => ("Lift distance", || ctr!("MaterialForm", "Lift distance")),
            Self::LiftRate => ("Lift speed", || ctr!("MaterialForm", "Lift speed")),
            Self::RetractRate => ("Retract speed", || ctr!("MaterialForm", "Retract speed")),
            Self::ScaleX => ("Scale compensation x", || ctr!("MaterialForm", "Scale compensation x")),
            Self::ScaleY => ("Scale compensation y", || ctr!("MaterialForm", "Scale compensation y")),
            Self::ScaleZ => ("Scale compensation z", || ctr!("MaterialForm", "Scale compensation z")),
            Self::Price => ("Price per liter", || ctr!("MaterialForm", "Price per liter")),
            Self::OverhangAngle => ("Support overhang angle", || ctr!("MaterialForm", "Support overhang angle")),
        }
    }
}

/// What the user typed, by field; a field never filled in is empty.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct FieldTexts(BTreeMap<ResinField, String>);

impl FieldTexts {
    pub(crate) fn text(&self, field: ResinField) -> &str {
        self.0.get(&field).map_or("", String::as_str)
    }

    /// Fills `field` in; an empty text is the same as never filled in.
    pub(crate) fn set(&mut self, field: ResinField, text: impl Into<String>) {
        let text = text.into();
        match text.is_empty() {
            true => self.0.remove(&field),
            false => self.0.insert(field, text),
        };
    }

    /// `self` with `field` holding `text`.
    pub(crate) fn with(mut self, field: ResinField, text: impl Into<String>) -> Self {
        self.set(field, text);
        self
    }

    /// The text of `field` with the spaces around it gone.
    fn trimmed(&self, field: ResinField) -> &str {
        self.text(field).trim()
    }
}

impl FromIterator<(ResinField, String)> for FieldTexts {
    fn from_iter<I: IntoIterator<Item = (ResinField, String)>>(pairs: I) -> Self {
        let mut texts = FieldTexts::default();
        pairs.into_iter().for_each(|(field, text)| texts.set(field, text));
        texts
    }
}

/// Why a field's text was refused. Displayed in English for logs; the
/// editor shows [`Self::rejection_line`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FieldRejected {
    Unreadable { field: ResinField },
    NotAllowed { field: ResinField },
    /// Some scale compensation axes filled in and others empty.
    ScaleAxesIncomplete,
}

impl FieldRejected {
    /// The editor's error line, in the user's language.
    pub(crate) fn rejection_line(&self) -> SharedString {
        match self {
            Self::Unreadable { field } => ctr!("MaterialForm", "{} must be a number", (field.names().1)()),
            Self::NotAllowed { field } => ctr!("MaterialForm", "{} is out of range", (field.names().1)()),
            Self::ScaleAxesIncomplete => {
                ctr!("MaterialForm", "Fill all three scale compensation axes, or leave all three empty")
            }
        }
    }
}

impl fmt::Display for FieldRejected {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { field } => write!(out, "{} must be a number", field.names().0),
            Self::NotAllowed { field } => write!(out, "{} is out of range", field.names().0),
            Self::ScaleAxesIncomplete => out.write_str("Fill all three scale compensation axes, or leave all three empty"),
        }
    }
}

impl std::error::Error for FieldRejected {}

/// Reads one optional field of a form: empty is unset; otherwise the text
/// must parse, and then pass `check`.
struct Reader<'a> {
    texts: &'a FieldTexts,
}

impl Reader<'_> {
    fn number(&self, field: ResinField) -> Result<Option<f64>, FieldRejected> {
        match self.texts.trimmed(field) {
            "" => Ok(None),
            typed => typed.parse().map(Some).map_err(|_| FieldRejected::Unreadable { field }),
        }
    }

    fn checked<T, E>(&self, field: ResinField, check: impl FnOnce(f64) -> Result<T, E>) -> Result<Option<T>, FieldRejected> {
        self.number(field)?.map(check).transpose().map_err(|_| FieldRejected::NotAllowed { field })
    }

    /// A count: anything but a whole number is unreadable.
    fn count(&self, field: ResinField) -> Result<Option<u32>, FieldRejected> {
        match self.texts.trimmed(field) {
            "" => Ok(None),
            typed => typed.parse().map(Some).map_err(|_| FieldRejected::Unreadable { field }),
        }
    }
}

/// The calibration fields as a per-printer block. A block with every field
/// empty is still valid: having it at all marks the printer calibrated.
pub(crate) fn calibration_from_texts(texts: &FieldTexts) -> Result<MaterialPerPrinter, FieldRejected> {
    use ResinField as F;
    let read = Reader { texts };
    Ok(MaterialPerPrinter {
        normal_exposure_sec: read.checked(F::Exposure, PositiveFiniteSec::new)?,
        bottom_exposure_sec: read.checked(F::BottomLayersExposure, PositiveFiniteSec::new)?,
        bottom_layer_count: read.count(F::BottomLayers)?,
        lift_distance_mm: read.checked(F::LiftHeight, PositiveFiniteMm::new)?,
        lift_speed_mm_min: read.checked(F::LiftRate, PositiveFiniteMmPerMin::new)?,
        retract_speed_mm_min: read.checked(F::RetractRate, PositiveFiniteMmPerMin::new)?,
        ..MaterialPerPrinter::default()
    })
}

/// The defaults fields. The scale axes come as a set (all or none); the
/// overhang angle lies strictly between 0 and 90 degrees.
pub(crate) fn defaults_from_texts(texts: &FieldTexts) -> Result<MaterialDefaults, FieldRejected> {
    use ResinField as F;
    let read = Reader { texts };
    let axes = [F::ScaleX, F::ScaleY, F::ScaleZ];
    let filled = axes.iter().filter(|axis| !texts.trimmed(**axis).is_empty()).count();
    let scale_compensation_pct = match filled {
        0 => None,
        3 => {
            let [x, y, z] = axes.map(|axis| read.checked(axis, ScaleCompensationPercent::new));
            Some(ScaleCompensation { x: x?.expect("filled"), y: y?.expect("filled"), z: z?.expect("filled") })
        }
        _ => return Err(FieldRejected::ScaleAxesIncomplete),
    };
    let inside_right_angle = |degrees: f64| (degrees > 0.0 && degrees < 90.0).then_some(degrees).ok_or(());
    Ok(MaterialDefaults {
        scale_compensation_pct,
        bottom_layer_count: read.count(F::BottomLayers)?,
        price_per_liter: read.checked(F::Price, PositiveFiniteCurrencyPerLiter::new)?,
        support_overhang_angle_deg: read.checked(F::OverhangAngle, inside_right_angle)?,
        ..MaterialDefaults::default()
    })
}

/// A stored value as a field shows it: the number as written, or nothing.
fn shown(value: Option<impl fmt::Display>) -> String {
    value.map_or_else(String::new, |v| v.to_string())
}

/// The calibration fields for a loaded block.
pub(crate) fn calibration_texts(block: &MaterialPerPrinter) -> FieldTexts {
    use ResinField as F;
    [
        (F::Exposure, shown(block.normal_exposure_sec.map(|v| v.value()))),
        (F::BottomLayersExposure, shown(block.bottom_exposure_sec.map(|v| v.value()))),
        (F::BottomLayers, shown(block.bottom_layer_count)),
        (F::LiftHeight, shown(block.lift_distance_mm.map(|v| v.value()))),
        (F::LiftRate, shown(block.lift_speed_mm_min.map(|v| v.value()))),
        (F::RetractRate, shown(block.retract_speed_mm_min.map(|v| v.value()))),
    ]
    .into_iter()
    .collect()
}

/// The defaults fields for loaded defaults.
pub(crate) fn defaults_texts(defaults: &MaterialDefaults) -> FieldTexts {
    use ResinField as F;
    let scale = defaults.scale_compensation_pct;
    [
        (F::ScaleX, shown(scale.map(|s| s.x.value()))),
        (F::ScaleY, shown(scale.map(|s| s.y.value()))),
        (F::ScaleZ, shown(scale.map(|s| s.z.value()))),
        (F::BottomLayers, shown(defaults.bottom_layer_count)),
        (F::Price, shown(defaults.price_per_liter.map(|v| v.value()))),
        (F::OverhangAngle, shown(defaults.support_overhang_angle_deg)),
    ]
    .into_iter()
    .collect()
}

/// Which file an edited profile goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResinFileKind {
    /// A whole profile: `<library>/materials/<id>.toml`.
    WholeProfile,
    /// Only what one printer changes in a shared profile, in the library's
    /// `materials/extensions/<id>.toml`.
    ExtensionOnly,
}

/// Saves an edited profile with the writer for its kind; the error, as
/// text, is the editor's error line.
pub(crate) fn write_edited_resin(folder: &Path, resin: &MaterialProfile, kind: ResinFileKind) -> Result<(), String> {
    match kind {
        ResinFileKind::WholeProfile => save_material_profile_to_library(folder, resin).map_err(|e| e.to_string()),
        ResinFileKind::ExtensionOnly => save_material_extension_to_library(folder, resin).map_err(|e| e.to_string()),
    }
}

#[cfg(test)]
mod tests;

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Build a separately identified resin profile from selected source sections.

use std::collections::BTreeSet;
use std::fmt;

use sc_print_primitives::{MaterialId, NonEmptyString};
use sc_print_profiles::{CalibrationTarget, MaterialProfile};
use slint::SharedString;

use crate::i18n::ctr;

/// Which parts of the source resin the copy takes; the rest start empty.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CopiedSections {
    /// The intent section (what the resin is for).
    pub with_intent: bool,
    /// The defaults section (scale compensation, price and so on).
    pub with_defaults: bool,
    /// The printers whose calibration comes along.
    pub printer_blocks: Vec<CalibrationTarget>,
}

/// Why the copy cannot be made with what the dialog holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ForkRefusal {
    NameMissing,
    BrandMissing,
    /// No id made from the name (with a number after it) is free.
    NoFreeId,
}

impl ForkRefusal {
    /// The dialog's error line, translated.
    pub(crate) fn dialog_line(&self) -> SharedString {
        match self {
            Self::NameMissing => ctr!("ForkDialog", "Enter a name for the forked profile"),
            Self::BrandMissing => ctr!("ForkDialog", "Enter a brand for the forked profile"),
            Self::NoFreeId => ctr!("ForkDialog", "Could not derive a free profile id from that name"),
        }
    }

    /// The same line in English, for logs.
    fn log_text(&self) -> &'static str {
        match self {
            Self::NameMissing => "Enter a name for the forked profile",
            Self::BrandMissing => "Enter a brand for the forked profile",
            Self::NoFreeId => "Could not derive a free profile id from that name",
        }
    }
}

impl fmt::Display for ForkRefusal {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(self.log_text())
    }
}

impl std::error::Error for ForkRefusal {}

/// A new resin profile copied from `source`: its own `name`, `brand` and an
/// id made from the name that none of `taken` uses, with the `sections`
/// chosen. Never an extension, even when copied from one.
pub(crate) fn forked_resin(
    source: &MaterialProfile,
    sections: &CopiedSections,
    name: &str,
    brand: &str,
    taken: &BTreeSet<MaterialId>,
) -> Result<MaterialProfile, ForkRefusal> {
    let mut identity = source.identity.clone();
    identity.name = NonEmptyString::new(name.trim()).map_err(|_| ForkRefusal::NameMissing)?;
    identity.brand = NonEmptyString::new(brand.trim()).map_err(|_| ForkRefusal::BrandMissing)?;
    identity.id = free_id_for(identity.name.as_str(), taken).ok_or(ForkRefusal::NoFreeId)?;
    let mut copy_blocks = source.per_printer.clone();
    copy_blocks.retain(|printer, _| sections.printer_blocks.contains(printer));
    let intent = if sections.with_intent { source.intent } else { Default::default() };
    let defaults = if sections.with_defaults { source.defaults } else { Default::default() };
    Ok(MaterialProfile { extension: false, identity, intent, defaults, per_printer: copy_blocks })
}

/// The name in lowercase words joined by `-` (`Tough Grey 2` gives
/// `tough-grey-2`), with `-2`, `-3` ... added until the id is not `taken`.
fn free_id_for(name: &str, taken: &BTreeSet<MaterialId>) -> Option<MaterialId> {
    let words: Vec<String> = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    if words.is_empty() {
        return None;
    }
    let stem = words.join("-");
    let attempt = |n: u32| match n {
        1 => MaterialId::new(&stem).ok(),
        n => MaterialId::new(&format!("{stem}-{n}")).ok(),
    };
    (1..10_000).filter_map(attempt).find(|id| !taken.contains(id))
}

#[cfg(test)]
mod tests;

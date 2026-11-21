// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Checked values that printer and resin profiles, the slicer and the
//! output writers all share. Each is checked when made or read, so a value
//! of one of these types is always usable.
//!
//! - `quantities`: single numbers with a unit (lengths, times, doses).
//! - `ranges`: ordered spans of those.
//! - `machine`: a printer's volume, screen and calibration.
//! - `names`: ids, keywords and anti-aliasing levels.
mod machine;
mod names;
mod quantities;
mod ranges;

pub use machine::*;
pub use names::*;
pub use quantities::*;
pub use ranges::*;

#[cfg(test)]
mod tests_spec;

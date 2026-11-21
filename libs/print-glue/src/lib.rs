#![deny(missing_docs)]
// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Where the scene meets the printer: turning profiles into slicing
//! settings, slicing a plate, and the model tools (orient, supports,
//! hollowing, calibration prints) that run before slicing.

// From profiles and scene to sliced layers. The common entry points are
// re-exported at the crate root.
pub mod fingerprint;
pub mod prepare;
pub mod projection;
/// Placement checks shared by preview and export.
pub mod risks;
pub mod slice_plate;
pub use fingerprint::*;
pub use prepare::*;
pub use projection::*;
pub use risks::*;
pub use slice_plate::*;

// Model tools, used by path (`orient::`, `supports::` and so on).
/// Geometry for exposure calibration prints.
pub mod calibration;
pub mod hollow;
mod label_font;
pub mod model_tools;
/// Build-volume-aware orientation selection.
pub mod orient;
pub mod supports;

#[cfg(test)]
mod tests_hollow;
#[cfg(test)]
mod tests_pipeline;
#[cfg(test)]
mod tests_supports;

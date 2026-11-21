// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Slices meshes into horizontal layers and rasterizes them for a resin
//! printer's screen.
//!
//! The entry points (`api`) take a scene and a [`profile::SliceProfile`]
//! and return each part's layer outlines (`contour`); `raster` turns
//! outlines into run-length grey images.

/// The version written into output files.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod api;
pub use api::*;

pub mod error;
pub mod profile;

pub mod contour;
pub mod geom2d;
pub mod grouping;
mod section;

pub mod raster;

#[cfg(test)]
mod tests_spec;

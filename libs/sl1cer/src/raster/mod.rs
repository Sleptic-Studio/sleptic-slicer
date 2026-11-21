// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! From a layer's contours to its pixels: scanline filling into run-length
//! images (`rle`), the millimetre-to-pixel mapping, and the accuracy
//! passes (compensation, anti-aliasing) applied to the result.
pub mod rle;
pub mod scanline;

pub mod accuracy;
pub mod pixel_map;

pub use scanline::rasterize_layer;

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The one 2D type the slicer needs: a point in a layer's plane, in mm.

/// A point in the layer plane, in mm (x right, y away from the viewer).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point2D {
    pub x: f64,
    pub y: f64,
}

impl Point2D {
    pub fn new(x: f64, y: f64) -> Point2D {
        Point2D { x, y }
    }
}

impl From<(f64, f64)> for Point2D {
    fn from((x, y): (f64, f64)) -> Point2D {
        Point2D { x, y }
    }
}

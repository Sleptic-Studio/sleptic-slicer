// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The specification's small values: keywords, ids and positions, and
//! the transform matrix.

spec_keywords! {
    /// The unit the model's coordinates are in.
    Unit {
        Micron = "micron",
        #[default]
        Millimeter = "millimeter",
        Centimeter = "centimeter",
        Inch = "inch",
        Foot = "foot",
        Meter = "meter",
    }
}

impl Unit {
    /// Millimetres in one of this unit.
    pub fn scale_to_mm(self) -> f64 {
        match self {
            Unit::Micron => 0.001,
            Unit::Millimeter => 1.0,
            Unit::Centimeter => 10.0,
            Unit::Inch => 25.4,
            Unit::Foot => 304.8,
            Unit::Meter => 1000.0,
        }
    }
}

spec_keywords! {
    /// What an object is for.
    ObjectType {
        #[default]
        Model = "model",
        SolidSupport = "solidsupport",
        Support = "support",
        Surface = "surface",
        Other = "other",
    }
}

spec_keywords! {
    /// Whether an object's mesh is its real geometry or a low-resolution
    /// stand-in for its slices (slice extension).
    MeshResolution {
        #[default]
        Fullres = "fullres",
        Lowres = "lowres",
    }
}

spec_number!(
    /// A resource's id, from 1.
    ResourceId from 1
);

impl ResourceId {
    pub const MIN: ResourceId = ResourceId(1);
}

spec_number!(
    /// A position in one of a resource's lists (vertices, triangles,
    /// materials), from 0.
    ResourceIndex from 0
);

impl ResourceIndex {
    pub const ZERO: ResourceIndex = ResourceIndex(0);
}

/// An affine transform as the specification writes it: twelve numbers,
/// three rows of the linear part and then the move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix3D(pub [f64; 12]);

impl Matrix3D {
    pub const IDENTITY: Matrix3D = Matrix3D([1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0.]);

    /// Exactly twelve finite numbers between white space, or nothing.
    pub fn parse(text: &str) -> Option<Matrix3D> {
        let numbers = text
            .split_whitespace()
            .map(|word| word.parse::<f64>().ok().filter(|n| n.is_finite()))
            .collect::<Option<Vec<f64>>>()?;
        <[f64; 12]>::try_from(numbers).ok().map(Matrix3D)
    }

    pub fn to_attribute(self) -> String {
        self.0.map(|n| n.to_string()).join(" ")
    }
}

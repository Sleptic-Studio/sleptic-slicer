// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{Vertex, transform::AffineTransform3D};
use std::ops::{Add, Mul, Sub};
mod sealed {
    pub trait Sealed {}
    impl Sealed for f32 {}
    impl Sealed for f64 {}
}
pub trait ArvoFloat:
    sealed::Sealed + Copy + Add<Output = Self> + Sub<Output = Self> + Mul<Output = Self>
{
    fn abs(self) -> Self;
    fn mul_add(self, a: Self, b: Self) -> Self;
    fn half() -> Self;
}
macro_rules! scalar {
    ($f:ty) => {
        impl ArvoFloat for $f {
            fn abs(self) -> Self {
                self.abs()
            }
            fn mul_add(self, a: Self, b: Self) -> Self {
                self.mul_add(a, b)
            }
            fn half() -> Self {
                0.5
            }
        }
    };
}
scalar!(f32);
scalar!(f64);
pub fn arvo_transformed_aabb<F: ArvoFloat>(
    min: [F; 3],
    max: [F; 3],
    linear: &[[F; 3]; 3],
    translation: &[F; 3],
) -> ([F; 3], [F; 3]) {
    let center = std::array::from_fn::<_, 3, _>(|i| (min[i] + max[i]) * F::half());
    let radius = std::array::from_fn::<_, 3, _>(|i| (max[i] - min[i]) * F::half());
    let c: [F; 3] = std::array::from_fn(|i| {
        linear[i][0].mul_add(
            center[0],
            linear[i][1].mul_add(center[1], linear[i][2].mul_add(center[2], translation[i])),
        )
    });
    let r: [F; 3] = std::array::from_fn(|i| {
        linear[i][0].abs().mul_add(
            radius[0],
            linear[i][1]
                .abs()
                .mul_add(radius[1], linear[i][2].abs() * radius[2]),
        )
    });
    (
        std::array::from_fn(|i| c[i] - r[i]),
        std::array::from_fn(|i| c[i] + r[i]),
    )
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    min: Vertex,
    max: Vertex,
}
impl Aabb {
    pub const fn empty() -> Self {
        Self {
            min: Vertex::new([f32::INFINITY, f32::INFINITY, f32::INFINITY]),
            max: Vertex::new([f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY]),
        }
    }
    pub fn from_min_max(min: Vertex, max: Vertex) -> Self {
        debug_assert!(
            (0..3).all(|i| !min[i].is_finite() || !max[i].is_finite() || min[i] <= max[i])
        );
        Self { min, max }
    }
    pub fn is_empty(&self) -> bool {
        self.min
            .coords()
            .into_iter()
            .zip(self.max.coords())
            .any(|(low, high)| low > high)
    }
    pub const fn min(&self) -> Vertex {
        self.min
    }
    pub const fn max(&self) -> Vertex {
        self.max
    }
    pub fn extend(&mut self, v: Vertex) {
        for i in 0..3 {
            self.min.as_array_mut()[i] = self.min[i].min(v[i]);
            self.max.as_array_mut()[i] = self.max[i].max(v[i]);
        }
    }
    pub fn union(&mut self, b: &Self) {
        for i in 0..3 {
            self.min.as_array_mut()[i] = self.min[i].min(b.min[i]);
            self.max.as_array_mut()[i] = self.max[i].max(b.max[i]);
        }
    }
    pub fn transformed_arvo(&self, t: &AffineTransform3D) -> Self {
        if self.is_empty() {
            return Self::empty();
        }
        let (a, b) = arvo_transformed_aabb(
            self.min.coords().map(f64::from),
            self.max.coords().map(f64::from),
            t.linear(),
            t.translation(),
        );
        Self {
            min: Vertex::new([a[0] as f32, a[1] as f32, a[2] as f32]),
            max: Vertex::new([b[0] as f32, b[1] as f32, b[2] as f32]),
        }
    }
}

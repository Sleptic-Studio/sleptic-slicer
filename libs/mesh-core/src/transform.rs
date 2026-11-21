// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{Normal, Vector3D, Vertex};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformError {
    SingularLinear,
    ZeroAxis,
    ZeroMagnitudeNormal,
}
impl std::fmt::Display for TransformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::SingularLinear => "linear part of the transform is singular",
            Self::ZeroAxis => "rotation axis must be non-zero",
            Self::ZeroMagnitudeNormal => "transformed normal has zero magnitude",
        })
    }
}
impl std::error::Error for TransformError {}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AffineTransform3D {
    linear: [[f64; 3]; 3],
    translation: [f64; 3],
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0].mul_add(b[0], a[1].mul_add(b[1], a[2] * b[2]))
}
fn greater(a: f64, b: f64) -> bool {
    a.partial_cmp(&b) == Some(std::cmp::Ordering::Greater)
}
impl Default for AffineTransform3D {
    fn default() -> Self {
        Self::identity()
    }
}
impl AffineTransform3D {
    pub const fn identity() -> Self {
        Self::from_translation([0., 0., 0.])
    }
    pub const fn from_translation(translation: [f64; 3]) -> Self {
        Self {
            linear: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            translation,
        }
    }
    pub const fn from_uniform_scale(s: f64) -> Self {
        Self::from_non_uniform_scale([s, s, s])
    }
    pub const fn from_non_uniform_scale([x, y, z]: [f64; 3]) -> Self {
        Self {
            linear: [[x, 0., 0.], [0., y, 0.], [0., 0., z]],
            translation: [0.; 3],
        }
    }
    pub const fn from_linear_and_translation(linear: [[f64; 3]; 3], translation: [f64; 3]) -> Self {
        Self {
            linear,
            translation,
        }
    }
    pub const fn linear(&self) -> &[[f64; 3]; 3] {
        &self.linear
    }
    pub const fn translation(&self) -> &[f64; 3] {
        &self.translation
    }
    /// The transform as four columns: where it sends the X, Y and Z unit
    /// vectors (without the move), then the move itself. Column-major
    /// formats and matrix libraries take it in this order.
    pub fn columns(&self) -> [[f64; 3]; 4] {
        let l = &self.linear;
        let axis = |c: usize| [l[0][c], l[1][c], l[2][c]];
        [axis(0), axis(1), axis(2), self.translation]
    }
    pub fn from_rotation_x(a: f64) -> Self {
        let (s, c) = a.sin_cos();
        Self::from_linear_and_translation([[1., 0., 0.], [0., c, -s], [0., s, c]], [0.; 3])
    }
    pub fn from_rotation_y(a: f64) -> Self {
        let (s, c) = a.sin_cos();
        Self::from_linear_and_translation([[c, 0., s], [0., 1., 0.], [-s, 0., c]], [0.; 3])
    }
    pub fn from_rotation_z(a: f64) -> Self {
        let (s, c) = a.sin_cos();
        Self::from_linear_and_translation([[c, -s, 0.], [s, c, 0.], [0., 0., 1.]], [0.; 3])
    }
    pub fn from_axis_angle(axis: [f64; 3], angle: f64) -> Result<Self, TransformError> {
        let n = dot(axis, axis);
        if !greater(n, f64::EPSILON) {
            return Err(TransformError::ZeroAxis);
        }
        let [x, y, z] = axis.map(|v| v / n.sqrt());
        let (s, c) = angle.sin_cos();
        let t = 1. - c;
        Ok(Self::from_linear_and_translation(
            [
                [
                    (t * x).mul_add(x, c),
                    (t * x).mul_add(y, -s * z),
                    (t * x).mul_add(z, s * y),
                ],
                [
                    (t * y).mul_add(x, s * z),
                    (t * y).mul_add(y, c),
                    (t * y).mul_add(z, -s * x),
                ],
                [
                    (t * z).mul_add(x, -s * y),
                    (t * z).mul_add(y, s * x),
                    (t * z).mul_add(z, c),
                ],
            ],
            [0.; 3],
        ))
    }
    pub fn apply_to_vertex(&self, v: Vertex) -> Vertex {
        let p = v.coords().map(f64::from);
        let r: [f32; 3] = std::array::from_fn(|i| {
            self.linear[i][0].mul_add(
                p[0],
                self.linear[i][1]
                    .mul_add(p[1], self.linear[i][2].mul_add(p[2], self.translation[i])),
            ) as f32
        });
        Vertex::new([r[0], r[1], r[2]])
    }
    pub fn apply_to_vector(&self, v: Vector3D<f32>) -> Vector3D<f32> {
        let p = v.coords().map(f64::from);
        Vector3D::new([
            dot(self.linear[0], p) as f32,
            dot(self.linear[1], p) as f32,
            dot(self.linear[2], p) as f32,
        ])
    }
    fn cofactor(&self) -> [[f64; 3]; 3] {
        std::array::from_fn(|i| {
            std::array::from_fn(|j| {
                let a = (i + 1) % 3;
                let b = (i + 2) % 3;
                let c = (j + 1) % 3;
                let d = (j + 2) % 3;
                self.linear[a][c].mul_add(self.linear[b][d], -self.linear[a][d] * self.linear[b][c])
            })
        })
    }
    pub fn determinant(&self) -> f64 {
        dot(self.linear[0], self.cofactor()[0])
    }
    pub fn compose(&self, other: &Self) -> Self {
        let linear = std::array::from_fn(|i| {
            std::array::from_fn(|j| {
                dot(
                    self.linear[i],
                    [other.linear[0][j], other.linear[1][j], other.linear[2][j]],
                )
            })
        });
        let translation = std::array::from_fn(|i| {
            self.linear[i][0].mul_add(
                other.translation[0],
                self.linear[i][1].mul_add(
                    other.translation[1],
                    self.linear[i][2].mul_add(other.translation[2], self.translation[i]),
                ),
            )
        });
        Self {
            linear,
            translation,
        }
    }
    pub fn inverse(&self) -> Result<Self, TransformError> {
        let c = self.cofactor();
        let det = dot(self.linear[0], c[0]);
        if !greater(det.abs(), f64::EPSILON) {
            return Err(TransformError::SingularLinear);
        }
        let linear = std::array::from_fn(|i| std::array::from_fn(|j| c[j][i] / det));
        let translation = std::array::from_fn(|i| -dot(linear[i], self.translation));
        Ok(Self {
            linear,
            translation,
        })
    }
    pub fn apply_to_normal(&self, n: Normal) -> Result<Normal, TransformError> {
        let c = self.cofactor();
        let det = dot(self.linear[0], c[0]);
        if !greater(det.abs(), f64::EPSILON) {
            return Err(TransformError::SingularLinear);
        }
        let p = n.coords().map(f64::from);
        let r = c.map(|row| dot(row, p));
        let sq = dot(r, r);
        if !greater(sq, f64::EPSILON) {
            return Err(TransformError::ZeroMagnitudeNormal);
        }
        let k = det.signum() / sq.sqrt();
        Ok(Normal::new([
            (r[0] * k) as f32,
            (r[1] * k) as f32,
            (r[2] * k) as f32,
        ]))
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Transform3D {
    #[default]
    Identity,
    Affine(AffineTransform3D),
}
impl Transform3D {
    pub fn apply_to_vertex(&self, v: Vertex) -> Vertex {
        if let Self::Affine(transform) = self {
            return transform.apply_to_vertex(v);
        }
        v
    }
    pub fn apply_to_normal(&self, n: Normal) -> Result<Normal, TransformError> {
        if let Self::Affine(transform) = self {
            return transform.apply_to_normal(n);
        }
        Ok(n)
    }
    pub fn compose(&self, b: &Self) -> Self {
        match (self, b) {
            (Self::Identity, _) => *b,
            (_, Self::Identity) => *self,
            (Self::Affine(a), Self::Affine(b)) => Self::Affine(a.compose(b)),
        }
    }
    pub fn inverse(&self) -> Result<Self, TransformError> {
        if let Self::Affine(transform) = self {
            return transform.inverse().map(Self::Affine);
        }
        Ok(*self)
    }
    pub fn determinant(&self) -> f64 {
        if let Self::Affine(transform) = self {
            return transform.determinant();
        }
        1.
    }
    pub fn is_identity(&self) -> bool {
        matches!(self, Self::Identity)
    }
}

#[cfg(test)]
mod column_tests {
    use super::AffineTransform3D;

    #[test]
    fn columns_are_the_images_of_the_axes_then_the_move() {
        let rows = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]];
        let moved = AffineTransform3D::from_linear_and_translation(rows, [10.0, 11.0, 12.0]);
        assert_eq!(moved.columns(), [[1.0, 4.0, 7.0], [2.0, 5.0, 8.0], [3.0, 6.0, 9.0], [10.0, 11.0, 12.0]]);
    }
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Local XYZ Euler and scale editors. Sheared transforms are left intact.
use glam::{DMat3, DQuat, DVec3, EulerRot};
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};

pub(crate) fn components(t: Transform3D) -> Option<([f64; 3], [f64; 3])> {
    let a = match t {
        Transform3D::Identity => AffineTransform3D::identity(),
        Transform3D::Affine(a) => a,
    };
    let rows = a.linear();
    let matrix = DMat3::from_cols_array(&[
        rows[0][0], rows[1][0], rows[2][0], rows[0][1], rows[1][1], rows[2][1], rows[0][2],
        rows[1][2], rows[2][2],
    ]);
    let mut scale = DVec3::new(
        matrix.x_axis.length(),
        matrix.y_axis.length(),
        matrix.z_axis.length(),
    );
    if scale.min_element() < 1e-12 {
        return None;
    }
    if matrix.determinant() < 0.0 {
        scale.x = -scale.x;
    }
    let rotation = matrix * DMat3::from_diagonal(scale.recip());
    let unit = rotation.transpose() * rotation;
    if (unit - DMat3::IDENTITY)
        .to_cols_array()
        .iter()
        .any(|v| v.abs() > 1e-7)
    {
        return None;
    }
    let (x, y, z) = DQuat::from_mat3(&rotation).to_euler(EulerRot::XYZ);
    Some((
        [x.to_degrees(), y.to_degrees(), z.to_degrees()],
        scale.to_array().map(|v| v * 100.0),
    ))
}

pub(crate) fn edit(
    t: Transform3D,
    scale_edit: bool,
    axis: usize,
    value: f64,
) -> Option<Transform3D> {
    if axis > 2 || !value.is_finite() || (scale_edit && value.abs() < 0.001) {
        return None;
    }
    let (mut angles, mut scales) = components(t)?;
    if scale_edit {
        scales[axis] = value;
    } else {
        angles[axis] = value;
    }
    let rotation = DQuat::from_euler(
        EulerRot::XYZ,
        angles[0].to_radians(),
        angles[1].to_radians(),
        angles[2].to_radians(),
    );
    let matrix = DMat3::from_quat(rotation)
        * DMat3::from_diagonal(DVec3::from_array(scales.map(|v| v / 100.0)));
    let cols = matrix.to_cols_array_2d();
    let rows = std::array::from_fn(|i| std::array::from_fn(|j| cols[j][i]));
    let translation = match t {
        Transform3D::Identity => [0.0; 3],
        Transform3D::Affine(a) => *a.translation(),
    };
    Some(Transform3D::Affine(
        AffineTransform3D::from_linear_and_translation(rows, translation),
    ))
}

#[cfg(test)]
#[path = "tests_object_transform.rs"]
mod tests;

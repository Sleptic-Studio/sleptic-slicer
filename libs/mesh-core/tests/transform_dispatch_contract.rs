// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_mesh_core::{transform::{AffineTransform3D, Transform3D, TransformError}, Normal, Vertex};

#[test]
fn transform_dispatch_contract() {
    let id = Transform3D::Identity;
    let affine = Transform3D::Affine(AffineTransform3D::from_non_uniform_scale([2.0, 3.0, 4.0]));
    let point = Vertex::new([1.0, 2.0, 3.0]);
    let normal = Normal::new([1.0, 0.0, 0.0]);
    assert_eq!(id.apply_to_vertex(point), point);
    assert_eq!(affine.apply_to_vertex(point).coords(), [2.0, 6.0, 12.0]);
    assert_eq!(id.apply_to_normal(normal), Ok(normal));
    assert_eq!(affine.apply_to_normal(normal), Ok(normal));
    assert_eq!(id.compose(&affine), affine);
    assert_eq!(affine.compose(&id), affine);
    assert_eq!(affine.compose(&affine).determinant(), 576.0);
    assert_eq!(id.inverse(), Ok(id));
    assert_eq!(affine.inverse().unwrap().determinant(), 1.0 / 24.0);
    assert!(id.is_identity());
    assert!(!Transform3D::Affine(AffineTransform3D::identity()).is_identity());
    let singular = Transform3D::Affine(AffineTransform3D::from_uniform_scale(0.0));
    assert_eq!(singular.inverse(), Err(TransformError::SingularLinear));
    assert_eq!(singular.apply_to_normal(normal), Err(TransformError::SingularLinear));
}

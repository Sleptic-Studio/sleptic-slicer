// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
#[test]
fn rotation_scale_edits_preserve_translation_and_other_components() {
    let t = Transform3D::Affine(AffineTransform3D::from_translation([2.0, 3.0, 4.0]));
    let t = edit(t, false, 1, 35.0).unwrap();
    let t = edit(t, true, 2, 150.0).unwrap();
    let (angles, scales) = components(t).unwrap();
    assert!((angles[1] - 35.0).abs() < 1e-8);
    assert!((scales[2] - 150.0).abs() < 1e-8);
    if let Transform3D::Affine(a) = t {
        assert_eq!(*a.translation(), [2.0, 3.0, 4.0]);
    }
    assert!(edit(t, true, 0, 0.0).is_none());
}
#[test]
fn shear_is_never_silently_decomposed() {
    let t = Transform3D::Affine(AffineTransform3D::from_linear_and_translation(
        [[1., 0.2, 0.], [0., 1., 0.], [0., 0., 1.]],
        [0.; 3],
    ));
    assert!(components(t).is_none());
    assert!(edit(t, false, 0, 10.0).is_none());
}

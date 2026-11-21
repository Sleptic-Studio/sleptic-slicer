// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_sl1cer::{contour::ClosedRing, geom2d::Point2D, grouping::point_in_ring};

fn ring(points: &[(f64, f64)]) -> ClosedRing {
    ClosedRing::new(points.iter().map(|&(x, y)| Point2D::new(x, y)).collect())
}

#[test]
fn short_rings_never_contain_a_point() {
    for points in [&[][..], &[(0., 0.)][..], &[(0., 0.), (4., 4.)][..]] {
        assert!(!point_in_ring(Point2D::new(1., 1.), &ring(points)));
    }
}

#[test]
fn containment_uses_crossings_and_preserves_boundary_convention() {
    let square = ring(&[(0., 0.), (4., 0.), (4., 4.), (0., 4.)]);
    for (x, y, expected) in [
        (2., 2., true),
        (5., 2., false),
        (-1., 2., false),
        (0., 2., true),
        (4., 2., false),
        (2., 0., true),
        (2., 4., false),
    ] {
        assert_eq!(
            point_in_ring(Point2D::new(x, y), &square),
            expected,
            "({x}, {y})"
        );
    }
    let concave = ring(&[(0., 0.), (4., 0.), (4., 4.), (2., 2.), (0., 4.)]);
    assert!(point_in_ring(Point2D::new(1., 2.), &concave));
    assert!(!point_in_ring(Point2D::new(2., 3.), &concave));
}

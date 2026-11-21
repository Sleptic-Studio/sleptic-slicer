// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_sl1cer::contour::{
    ClosedRing, ContourLayer, ContourStack, ContourStackError, LayerAlignmentError, Polygon,
    PolygonWindingError, Winding, merge_stacks,
};
use sc_sl1cer::geom2d::Point2D;

fn square(x: f64) -> ClosedRing {
    ClosedRing::new(vec![
        Point2D { x, y: 0. },
        Point2D { x: x + 2., y: 0. },
        Point2D { x: x + 2., y: 2. },
        Point2D { x, y: 2. },
    ])
}
fn marked_polygon(x: f64) -> Polygon {
    let mut hole = square(x + 0.25);
    hole.reverse();
    Polygon::new(square(x), vec![hole]).unwrap()
}
fn stack(z: &[f64], polygons: Vec<Polygon>) -> ContourStack {
    ContourStack::new_unchecked(
        z.iter()
            .map(|z| ContourLayer::new(*z, polygons.clone()))
            .collect(),
    )
}

#[test]
fn rings_and_checked_polygons_keep_order_winding_and_unordered_comparisons() {
    let mut ring = square(3.);
    let original = ring.vertices().to_vec();
    assert_eq!(ring.len(), 4);
    assert!(!ring.is_empty());
    assert_eq!(ring.signed_area(), 4.);
    assert_eq!(ring.winding(), Some(Winding::Ccw));
    ring.reverse();
    assert_eq!(
        ring.vertices(),
        original.into_iter().rev().collect::<Vec<_>>()
    );
    assert_eq!(ring.signed_area(), -4.);
    assert_eq!(ring.winding(), Some(Winding::Cw));
    let empty = ClosedRing::new(vec![]);
    assert!(empty.is_empty());
    assert_eq!(empty.signed_area(), 0.);
    assert_eq!(empty.winding(), None);
    assert_eq!(
        Polygon::new(empty.clone(), vec![square(0.)]).unwrap_err(),
        PolygonWindingError::OuterNotCcw
    );
    assert_eq!(
        Polygon::new(square(0.), vec![ring, square(1.)]).unwrap_err(),
        PolygonWindingError::HoleNotCw { hole_idx: 1 }
    );
    let unordered = ClosedRing::new(vec![
        Point2D { x: f64::NAN, y: 0. },
        Point2D { x: 1., y: 0. },
        Point2D { x: 0., y: 1. },
    ]);
    assert!(unordered.signed_area().is_nan());
    assert_eq!(unordered.winding(), None);
    assert!(Polygon::new(unordered.clone(), vec![unordered]).is_ok());
    let raw = Polygon::new_unchecked(empty.clone(), vec![empty]);
    assert!(raw.outer().is_empty());
    assert_eq!(raw.holes().len(), 1);
}

#[test]
fn stack_validation_prioritizes_nonfinite_z_and_reports_the_first_pair() {
    let layers = |values: &[f64]| {
        values
            .iter()
            .map(|z| ContourLayer::new(*z, vec![]))
            .collect()
    };
    let failure = ContourStack::new(layers(&[1., 0., f64::NAN])).unwrap_err();
    assert!(matches!(failure, ContourStackError::NonFiniteZ { idx: 2, z_mm } if z_mm.is_nan()));
    assert!(
        matches!(ContourStack::new(layers(&[f64::INFINITY, f64::NAN])).unwrap_err(),
        ContourStackError::NonFiniteZ { idx: 0, z_mm } if z_mm==f64::INFINITY)
    );
    assert_eq!(
        ContourStack::new(layers(&[0.1, 0.2, 0.2, 0.1])).unwrap_err(),
        ContourStackError::NonMonotoneZ {
            prev_idx: 1,
            prev_z_mm: 0.2,
            next_z_mm: 0.2
        }
    );
    let valid = ContourStack::new(layers(&[-2., -0., 0.25])).unwrap();
    assert_eq!(valid.layers()[1].z_mm().to_bits(), (-0.0f64).to_bits());
    assert!(ContourStack::new(vec![]).unwrap().layers().is_empty());
    let raw = ContourStack::new_unchecked(layers(&[9., 3.]));
    assert_eq!(
        raw.layers()
            .iter()
            .map(ContourLayer::z_mm)
            .collect::<Vec<_>>(),
        [9., 3.]
    );
}

#[test]
fn merge_preserves_polygons_and_holes_and_checks_exact_alignment_in_order() {
    let first = vec![marked_polygon(10.), marked_polygon(20.)];
    let second = vec![marked_polygon(30.), marked_polygon(40.)];
    let merged = merge_stacks(vec![
        stack(&[-0., 1.], first.clone()),
        stack(&[0., 1.], second.clone()),
    ])
    .unwrap();
    let expected = [first, second].concat();
    assert_eq!(merged.layers().len(), 2);
    assert_eq!(merged.layers()[0].z_mm().to_bits(), (-0.0f64).to_bits());
    for layer in merged.layers() {
        assert_eq!(layer.polygons(), expected);
    }
    let identical = marked_polygon(0.);
    let duplicates = merge_stacks(vec![
        stack(&[1.], vec![identical.clone()]),
        stack(&[1.], vec![identical]),
    ])
    .unwrap();
    assert_eq!(duplicates.layers()[0].polygons().len(), 2);
    assert!(merge_stacks(vec![]).unwrap().layers().is_empty());
    let single = stack(&[2., 4.], vec![marked_polygon(5.)]);
    assert_eq!(merge_stacks(vec![single.clone()]).unwrap(), single);
    assert_eq!(
        merge_stacks(vec![stack(&[0., 1.], vec![]), stack(&[99.], vec![])]).unwrap_err(),
        LayerAlignmentError::LayerCountMismatch {
            stack_idx: 1,
            expected: 2,
            found: 1
        }
    );
    let neighbor = f64::from_bits(1.0f64.to_bits() + 1);
    assert_eq!(
        merge_stacks(vec![
            stack(&[0., 1.], vec![]),
            stack(&[0., neighbor], vec![])
        ])
        .unwrap_err(),
        LayerAlignmentError::LayerZMismatch {
            stack_idx: 1,
            layer_idx: 1,
            expected_z_mm: 1.,
            found_z_mm: neighbor
        }
    );
}

#[test]
fn contour_boundaries_and_later_stack_errors() {
    let short = ClosedRing::new(vec![Point2D::new(1., 2.), Point2D::new(3., 4.)]);
    assert_eq!(short.signed_area(), 0.);
    let layers = |zs: &[f64]| zs.iter().map(|&z| ContourLayer::new(z, vec![])).collect();
    assert!(matches!(ContourStack::new(layers(&[0., 1., 0.])), Err(ContourStackError::NonMonotoneZ { prev_idx: 1, .. })));
    assert!(matches!(merge_stacks(vec![stack(&[0.], vec![]), stack(&[0.], vec![]), stack(&[1.], vec![])]), Err(LayerAlignmentError::LayerZMismatch { stack_idx: 2, layer_idx: 0, .. })));
}

#[test]
fn contour_rewrite_contract() {
    assert_eq!(square(0.).signed_area(), 4.);
    let layers = |zs: &[f64]| zs.iter().map(|&z| ContourLayer::new(z, vec![])).collect();
    assert!(matches!(ContourStack::new(layers(&[0., f64::INFINITY])), Err(ContourStackError::NonFiniteZ { idx: 1, .. })));
    assert!(matches!(ContourStack::new(layers(&[0., 0.])), Err(ContourStackError::NonMonotoneZ { prev_idx: 0, .. })));
}

#[test]
fn merge_rewrite_contract() {
    let a = marked_polygon(0.);
    let b = marked_polygon(3.);
    let result = merge_stacks(vec![stack(&[1.], vec![a.clone()]), stack(&[1.], vec![b.clone()])]).unwrap();
    assert_eq!(result.layers()[0].polygons(), &[a, b]);
    assert!(matches!(merge_stacks(vec![stack(&[1.], vec![]), stack(&[], vec![])]), Err(LayerAlignmentError::LayerCountMismatch { stack_idx: 1, expected: 1, found: 0 })));
    assert!(matches!(merge_stacks(vec![stack(&[1.], vec![]), stack(&[2.], vec![])]), Err(LayerAlignmentError::LayerZMismatch { stack_idx: 1, layer_idx: 0, expected_z_mm: 1., found_z_mm: 2. })));
}

#[test]
fn polygon_validation_contract() {
    let mut clockwise = square(0.);
    clockwise.reverse();
    assert_eq!(clockwise.winding(), Some(Winding::Cw));
    assert_eq!(Polygon::new(clockwise.clone(), vec![square(0.)]).unwrap_err(), PolygonWindingError::OuterNotCcw);
    assert_eq!(Polygon::new(square(0.), vec![clockwise, square(0.)]).unwrap_err(), PolygonWindingError::HoleNotCw { hole_idx: 1 });
    let nan = ClosedRing::new(vec![Point2D::new(f64::NAN, 0.), Point2D::new(1., 0.), Point2D::new(0., 1.)]);
    assert_eq!(nan.winding(), None);
    assert!(Polygon::new(nan.clone(), vec![nan]).is_ok());
}

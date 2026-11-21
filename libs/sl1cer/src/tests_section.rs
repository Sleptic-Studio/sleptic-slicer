// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
fn segment(a: (f64, f64), b: (f64, f64)) -> Passage {
    Passage {
        ends: [
            End {
                point: Point2D::new(a.0, a.1),
                key: Key(0, 0, 0),
                order: 0,
            },
            End {
                point: Point2D::new(b.0, b.1),
                key: Key(0, 0, 0),
                order: 0,
            },
        ],
    }
}
#[test]
fn tolerant_gap_limit_and_fragment_accounting() {
    for (gap, closed) in [(0.005, true), (0.01, true), (0.02, false)] {
        let s = vec![
            segment((0., 0.), (1., 0.)),
            segment((1., 0.), (1., 1.)),
            segment((1., 1.), (0., 1.)),
            segment((0., 1.), (0., gap)),
        ];
        let (r, d) = tolerant_rings(s).unwrap();
        assert_eq!(r.len(), usize::from(closed));
        assert_eq!(d, usize::from(!closed));
    }
}
#[test]
fn fixed_grid_rounds_away_and_checks_limit() {
    assert_eq!(fixed(Point2D::new(0.000005, -0.000005)).unwrap(), (1, -1));
    assert!(fixed(Point2D::new((1u64 << 61) as f64 / 100000., 0.)).is_err());
}
#[test]
fn direction_is_not_reversed_to_close_a_fragment() {
    let (r, d) = tolerant_rings(vec![
        segment((0., 0.), (1., 0.)),
        segment((1., 1.), (1., 0.)),
        segment((1., 1.), (0., 0.)),
    ])
    .unwrap();
    assert!(r.is_empty());
    assert!(d > 0);
}

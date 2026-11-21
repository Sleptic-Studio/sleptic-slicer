// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
#[test]
fn distances_use_world_coordinates_and_third_click_starts_again() {
    let mut m = Measure::default();
    assert_eq!(m.click([10., 20., 30.]), None);
    assert_eq!(m.click([13., 16., 42.]), Some([13., 3., -4., 12.]));
    assert_eq!(m.click([0., 0., 0.]), None);
    assert_eq!(m.points.len(), 1);
    m.clear();
    assert!(m.points.is_empty());
    assert_eq!(m.click([1., 2., 3.]), None);
}

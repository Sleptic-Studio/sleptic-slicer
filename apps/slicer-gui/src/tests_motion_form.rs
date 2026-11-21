// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;

#[test]
fn live_normal_example_keeps_inherited_stages_and_local_overrides() {
    let legacy = Motion {
        lift_slow_distance_mm: Some(6.0),
        lift_slow_speed_mm_min: Some(60.0),
        retract_slow_distance_mm: Some(6.0),
        retract_slow_speed_mm_min: Some(150.0),
        ..Default::default()
    };
    let inherited = Motion {
        lift_slow_distance_mm: Some(2.0),
        lift_fast_distance_mm: Some(4.0),
        lift_fast_speed_mm_min: Some(240.0),
        wait_before_cure_s: Some(0.25),
        ..Default::default()
    };
    let edited = Motion {
        lift_fast_speed_mm_min: Some(300.0),
        ..Default::default()
    };
    let m = normal_example(legacy, inherited, edited);
    assert_eq!(m.lift_slow_distance_mm, Some(2.0));
    assert_eq!(m.lift_fast_distance_mm, Some(4.0));
    assert_eq!(m.lift_fast_speed_mm_min, Some(300.0));
    assert_eq!(m.retract_slow_distance_mm, Some(6.0));
    assert_eq!(m.wait_before_cure_s, Some(0.25));
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

use super::FailureStreak;

#[test]
fn each_run_announces_once_and_counts_every_failure() {
    let gate = FailureStreak::default();
    assert_eq!(gate.in_a_row(), 0);
    for run in 0..4 {
        gate.reset();
        gate.reset();
        assert_eq!(gate.in_a_row(), 0);
        for count in 1..=37 {
            assert_eq!(
                gate.count_failure(),
                count == 1,
                "run {run}, failure {count}"
            );
            assert_eq!(gate.in_a_row(), count);
        }
    }
}

#[test]
fn the_counter_saturates_without_announcing_again() {
    let gate = FailureStreak::seeded_for_test(u32::MAX - 1);
    for _ in 0..3 {
        assert!(!gate.count_failure());
        assert_eq!(gate.in_a_row(), u32::MAX);
    }
    gate.reset();
    assert!(gate.count_failure());
    assert_eq!(gate.in_a_row(), 1);
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Anti-aliasing dropdowns list the levels in one fixed order (Off, 2x, 4x,
//! 8x, see `ui/preferences/slicing.slint` and the plate settings); this is
//! the only Rust code that knows that order.
use sc_print_primitives::AaLevel;

const ORDER: [AaLevel; 4] = [AaLevel::Off, AaLevel::X2, AaLevel::X4, AaLevel::X8];

/// The dropdown row showing `level`.
pub(crate) fn aa_row_of(level: AaLevel) -> i32 {
    ORDER.iter().position(|l| *l == level).unwrap_or(0) as i32
}

/// The level of a dropdown row; anything outside the list reads as Off.
pub(crate) fn aa_level_at_row(index: i32) -> AaLevel {
    usize::try_from(index)
        .ok()
        .and_then(|at| ORDER.get(at).copied())
        .unwrap_or(AaLevel::Off)
}

/// A plate's own setting: the reset row (-1) removes the override, while
/// Off is a choice like any other.
pub(crate) fn scene_aa_from_combo_index(index: i32) -> Option<AaLevel> {
    (index >= 0).then(|| aa_level_at_row(index))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_follow_the_dropdown_order_and_round_trip() {
        for (row, level) in ORDER.into_iter().enumerate() {
            assert_eq!(aa_row_of(level), row as i32);
            assert_eq!(aa_level_at_row(row as i32), level);
        }
        assert_eq!(aa_row_of(AaLevel::X8), 3);
    }

    #[test]
    fn rows_outside_the_list_read_as_off() {
        for row in [-7, -1, 4, 99] {
            assert_eq!(aa_level_at_row(row), AaLevel::Off);
        }
    }

    #[test]
    fn reset_clears_a_plate_override_but_off_sets_one() {
        assert_eq!(scene_aa_from_combo_index(-1), None);
        assert_eq!(scene_aa_from_combo_index(0), Some(AaLevel::Off));
        assert_eq!(scene_aa_from_combo_index(2), Some(AaLevel::X4));
        assert_eq!(scene_aa_from_combo_index(9), Some(AaLevel::Off));
    }
}

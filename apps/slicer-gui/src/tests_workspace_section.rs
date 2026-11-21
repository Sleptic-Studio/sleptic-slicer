// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

use super::{layout_tools_allowed_in, repair_allowed_in, undo_allowed_in, cut_height_for};
use crate::WorkspaceSection::{Prepare, Slice, Transform};

#[test]
fn every_section_has_the_expected_editing_permissions() {
    for (section, expected) in [
        (Prepare, [false, true, true]),
        (Transform, [true, true, false]),
        (Slice, [false, false, false]),
    ] {
        assert_eq!(
            [
                layout_tools_allowed_in(section),
                undo_allowed_in(section),
                repair_allowed_in(section)
            ],
            expected
        );
    }
}

#[test]
fn clipping_is_slice_only_and_keeps_the_unvalidated_float_cast() {
    for section in [Prepare, Transform, Slice] {
        assert_eq!(cut_height_for(section, None), None);
        for height in [
            0.0,
            -0.0,
            -3.5,
            1.23456789012,
            f64::MAX,
            f64::NEG_INFINITY,
            f64::NAN,
        ] {
            let result = cut_height_for(section, Some(height));
            if section == Slice {
                let actual = result.expect("Slice carries a selected height");
                if height.is_nan() {
                    assert!(actual.is_nan());
                } else {
                    assert_eq!(actual.to_bits(), (height as f32).to_bits());
                }
            } else {
                assert_eq!(result, None);
            }
        }
    }
}

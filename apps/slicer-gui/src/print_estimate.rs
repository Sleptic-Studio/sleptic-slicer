// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Shared print-job estimate presentation.
pub(crate) use sc_print_job::{estimated_cost, format_cost, format_duration, format_volume_ml};

/// A physical area cannot be negative, including IEEE negative zero.
pub(crate) fn lit_area(area: f64) -> f64 {
    area.max(0.0)
}
#[cfg(test)]
mod tests {
    #[test]
    fn empty_layer_area_formats_as_positive_zero() {
        for area in [-0.0, -1e-12, 0.0] {
            assert_eq!(format!("{:.2}", super::lit_area(area)), "0.00");
        }
    }
}

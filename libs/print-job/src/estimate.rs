// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Estimated print duration and resin volume for a held slice: pure
//! arithmetic over data the slice pipeline already computes, with no
//! new geometry algorithms of its own.
//!
//! Time includes exposure, transitions, light-off delay and configured travel.
//! It remains a lower bound because acceleration and firmware overhead are not modeled.
//!
//! The cost estimate ([`estimated_cost`]) is a separate, optional
//! calculation: a resin's `price_per_liter` lives on
//! [`sc_print_profiles::MaterialDefaults`], never on
//! `MaterialResolved` (the resolver's required-field vocabulary), so
//! it is read straight off the loaded profile rather than threaded
//! through resolution. `None` when the resin has no price set, so an
//! unpriced resin shows a volume estimate with no cost line rather
//! than a fabricated zero.

use sc_print_primitives::PositiveFiniteCurrencyPerLiter;
use sc_print_profiles::material_resolved::MaterialResolved;
use sc_sl1cer::contour::ContourStack;
use sc_sl1cer::profile::LayerSchedule;

/// Total estimated print duration, in seconds, for `total_layers`
/// layers sliced with `material`'s resolved settings. Each layer
/// contributes one exposure (bottom or normal, whichever applies)
/// plus one lift and one retract move at the profile's configured
/// distance and speeds.
pub fn estimated_seconds(material: &MaterialResolved, total_layers: u32) -> f64 {
    crate::ExposureSchedule::uniform(
        material,
        total_layers,
        material.layer_height_mm.value(),
        material.layer_height_mm.value(),
    )
    .total_time_s()
}

/// Normal-layer exposure plus resolved lift/retract/waits per millimetre.
/// Uses the existing estimator with startup phases removed, so even profiles
/// with more than 1000 bottom/transition layers return the normal cycle.
pub fn seconds_per_mm(material: &MaterialResolved) -> f64 {
    let mut normal = material.clone();
    normal.bottom_layer_count = 0;
    normal.transition_layers = 0;
    estimated_seconds(&normal, 1) / normal.layer_height_mm.value()
}

/// Formats a duration in seconds as `"Nh Mm"` (or `"Mm"` under an
/// hour, or `"<1m"` under a minute). Truncates rather than rounds up,
/// matching how a countdown timer reads once a print is underway.
pub fn format_duration(total_seconds: f64) -> String {
    let total_minutes = (total_seconds / 60.0).floor() as u64;
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if total_minutes > 0 {
        format!("{minutes}m")
    } else {
        "<1m".to_string()
    }
}

/// Total resin volume, in cubic millimetres, of everything held in
/// `stack` (the whole plate's merged contours — the model and any
/// generated supports actually on it, whatever the slice ran with)
/// sliced under `schedule`.
///
/// Sums each layer's net cross-sectional area times that layer's
/// thickness. The area is the sum of every polygon's signed area
/// (outer ring plus holes); `ContourStack`'s own construction
/// invariants already guarantee holes carry negative area, so a hole
/// subtracts itself out with no separate case needed. Layer 0 uses
/// the schedule's first-layer height; every other layer uses the
/// uniform layer height, matching how this schema represents a
/// (non-adaptive) layer schedule.
pub fn estimated_volume_mm3(stack: &ContourStack, schedule: &LayerSchedule) -> f64 {
    // `Uniform` is `LayerSchedule`'s only variant today; the per-layer
    // heights live on its `UniformSchedule` payload, not on the enum
    // itself (only `total_layers` is exposed at that level).
    let LayerSchedule::Uniform(uniform) = schedule;
    let first_layer_height = uniform.first_layer_height().value();
    let layer_height = uniform.layer_height().value();
    stack
        .layers()
        .iter()
        .enumerate()
        .map(|(i, layer)| {
            let area_mm2: f64 = layer
                .polygons()
                .iter()
                .map(|polygon| {
                    let holes_area: f64 = polygon.holes().iter().map(|h| h.signed_area()).sum();
                    polygon.outer().signed_area() + holes_area
                })
                .sum();
            let height_mm = if i == 0 {
                first_layer_height
            } else {
                layer_height
            };
            area_mm2.max(0.0) * height_mm
        })
        .sum::<f64>()
        .max(0.0)
}

/// Formats a volume in cubic millimetres as millilitres (1 mL = 1000
/// mm³) to one decimal place, e.g. `"12.3 mL"`.
pub fn format_volume_ml(volume_mm3: f64) -> String {
    format!("{:.1} mL", volume_mm3.max(0.0) / 1000.0)
}

/// Estimated resin cost for `volume_mm3` of a resin priced at
/// `price_per_liter`. Pure unit conversion (1 litre = 1,000,000 mm³);
/// carries no currency of its own, since the price itself is
/// whatever currency the user entered it in.
pub fn estimated_cost(price_per_liter: PositiveFiniteCurrencyPerLiter, volume_mm3: f64) -> f64 {
    (volume_mm3 / 1_000_000.0) * price_per_liter.value()
}

/// Formats an estimated cost to two decimal places with `symbol` as a
/// prefix, e.g. `format_cost(1.5, "$")` -> `"$1.50"`. `symbol` is a
/// user-configured display preference (Preferences > Appearance), not
/// a real currency code, so it is placed as a plain prefix rather than
/// following any locale-specific formatting rule.
pub fn format_cost(amount: f64, symbol: &str) -> String {
    format!("{symbol}{:.2}", amount.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_print_primitives::{
        MaterialId, NonEmptyString, PositiveFiniteMm, PositiveFiniteMmPerMin, PositiveFiniteSec,
        ScaleCompensation, ScaleCompensationPercent,
    };
    use sc_print_profiles::MaterialIdentity;
    use semver::Version;

    fn material(
        normal_exposure_sec: f64,
        bottom_exposure_sec: f64,
        bottom_layer_count: u32,
        lift_distance_mm: f64,
        lift_speed_mm_min: f64,
        retract_speed_mm_min: f64,
    ) -> MaterialResolved {
        MaterialResolved {
            motion: Default::default(),
            transition_layers: 0,
            bottom_compensation_mm: 0.0,
            xy_compensation_mm: 0.0,
            aa_levels: 256,
            light_off_delay_s: 0.0,
            identity: MaterialIdentity {
                id: MaterialId::new("test-resin").unwrap(),
                brand: NonEmptyString::new("Test Brand").unwrap(),
                family: NonEmptyString::new("Test Family").unwrap(),
                name: NonEmptyString::new("Test Resin").unwrap(),
                profile_version: Version::new(1, 0, 0),
            },
            layer_height_mm: PositiveFiniteMm::new(0.05).unwrap(),
            normal_exposure_sec: PositiveFiniteSec::new(normal_exposure_sec).unwrap(),
            bottom_exposure_sec: PositiveFiniteSec::new(bottom_exposure_sec).unwrap(),
            bottom_layer_count,
            lift_distance_mm: PositiveFiniteMm::new(lift_distance_mm).unwrap(),
            lift_speed_mm_min: PositiveFiniteMmPerMin::new(lift_speed_mm_min).unwrap(),
            retract_speed_mm_min: PositiveFiniteMmPerMin::new(retract_speed_mm_min).unwrap(),
            scale_compensation_pct: ScaleCompensation {
                x: ScaleCompensationPercent::new(0.0).unwrap(),
                y: ScaleCompensationPercent::new(0.0).unwrap(),
                z: ScaleCompensationPercent::new(0.0).unwrap(),
            },
        }
    }

    #[test]
    fn all_layers_normal_when_bottom_count_is_zero() {
        let m = material(2.0, 30.0, 0, 5.0, 60.0, 150.0);
        // 10 layers x 2s exposure = 20s; lift 5mm@60mm/min = 5s,
        // retract 5mm@150mm/min = 2s, x10 layers = 70s movement.
        let seconds = estimated_seconds(&m, 10);
        assert!((seconds - 90.0).abs() < 1e-9, "got {seconds}");
    }

    #[test]
    fn bottom_layers_use_bottom_exposure() {
        let m = material(2.0, 30.0, 3, 5.0, 60.0, 150.0);
        // 3 bottom layers x 30s + 7 normal x 2s = 90 + 14 = 104s
        // exposure; movement unchanged from the test above at 70s.
        let seconds = estimated_seconds(&m, 10);
        assert!((seconds - 174.0).abs() < 1e-9, "got {seconds}");
    }

    #[test]
    fn bottom_layer_count_never_exceeds_total_layers() {
        // A resin profile with more bottom layers than the whole
        // print has (e.g. a very short print) must not go negative
        // computing the normal-layer count.
        let m = material(2.0, 30.0, 8, 5.0, 60.0, 150.0);
        let seconds = estimated_seconds(&m, 3);
        // All 3 layers at bottom exposure: 3 x 30 = 90s exposure;
        // movement: 3 x (5s lift + 2s retract) = 21s.
        assert!((seconds - 111.0).abs() < 1e-9, "got {seconds}");
    }

    #[test]
    fn marginal_seconds_per_mm_uses_exposure_and_both_moves() {
        let mut m = material(2.0, 30.0, 4, 5.0, 60.0, 150.0);
        // 2 s cure + 5 s lift + 2 s retract at 0.05 mm.
        assert!((seconds_per_mm(&m) - 180.0).abs() < 1e-8);
        assert!(
            (estimated_seconds(&m, 200)
                - estimated_seconds(&m, 100)
                - seconds_per_mm(&m) * 100.0 * m.layer_height_mm.value())
            .abs()
                < 1e-8
        );
        m.motion.normal.wait_after_lift_s = Some(3.0);
        assert!((seconds_per_mm(&m) - 240.0).abs() < 1e-8);
        m.bottom_layer_count = 2000;
        m.transition_layers = 2000;
        assert!((seconds_per_mm(&m) - 240.0).abs() < 1e-8);
    }

    #[test]
    fn zero_layers_is_zero_seconds() {
        let m = material(2.0, 30.0, 4, 5.0, 60.0, 150.0);
        assert_eq!(estimated_seconds(&m, 0), 0.0);
    }

    #[test]
    fn format_duration_under_a_minute() {
        assert_eq!(format_duration(30.0), "<1m");
        assert_eq!(format_duration(0.0), "<1m");
    }

    #[test]
    fn format_duration_minutes_only() {
        assert_eq!(format_duration(125.0), "2m");
        assert_eq!(format_duration(3599.0), "59m");
    }

    #[test]
    fn format_duration_hours_and_minutes() {
        assert_eq!(format_duration(3600.0), "1h 0m");
        assert_eq!(format_duration(3661.0), "1h 1m");
        assert_eq!(format_duration(9000.0), "2h 30m");
    }

    use sc_sl1cer::contour::{ClosedRing, ContourLayer, Polygon};
    use sc_sl1cer::geom2d::Point2D;

    /// A CCW square ring of the given half-width, centred on the
    /// origin, area `(2 * half_width)^2`.
    fn square_ring(half_width: f64) -> ClosedRing {
        ClosedRing::new(vec![
            Point2D::new(-half_width, -half_width),
            Point2D::new(half_width, -half_width),
            Point2D::new(half_width, half_width),
            Point2D::new(-half_width, half_width),
        ])
    }

    #[test]
    fn volume_is_area_times_height_times_layer_count() {
        // A 2x2mm square (area 4mm^2), 3 layers at a uniform 0.1mm
        // height (first-layer height equal to the rest, so every
        // layer contributes the same amount): 4 x 0.1 x 3 = 1.2mm^3.
        let square = Polygon::new(square_ring(1.0), vec![]).unwrap();
        let layers = vec![
            ContourLayer::new(0.05, vec![square.clone()]),
            ContourLayer::new(0.15, vec![square.clone()]),
            ContourLayer::new(0.25, vec![square]),
        ];
        let stack = ContourStack::new_unchecked(layers);
        let schedule = LayerSchedule::uniform(0.1, 0.1, 3).unwrap();

        let volume = estimated_volume_mm3(&stack, &schedule);
        assert!((volume - 1.2).abs() < 1e-9, "got {volume}");
    }

    #[test]
    fn first_layer_uses_the_schedules_first_layer_height() {
        // Same 2x2mm square, 2 layers, but a first-layer height
        // triple the normal one: 4 x 0.3 (layer 0) + 4 x 0.1 (layer
        // 1) = 1.2 + 0.4 = 1.6mm^3.
        let square = Polygon::new(square_ring(1.0), vec![]).unwrap();
        let layers = vec![
            ContourLayer::new(0.15, vec![square.clone()]),
            ContourLayer::new(0.35, vec![square]),
        ];
        let stack = ContourStack::new_unchecked(layers);
        let schedule = LayerSchedule::uniform(0.1, 0.3, 2).unwrap();

        let volume = estimated_volume_mm3(&stack, &schedule);
        assert!((volume - 1.6).abs() < 1e-9, "got {volume}");
    }

    #[test]
    fn a_hole_subtracts_its_own_area() {
        // A 4x4mm outer square (area 16mm^2) with a 2x2mm hole (area
        // 4mm^2, CW-wound as `Polygon::new` requires) cut out of its
        // centre: net area 12mm^2, x1 layer x0.1mm height = 1.2mm^3.
        let mut hole = square_ring(1.0);
        hole.reverse(); // CCW square -> CW, satisfying the hole invariant.
        let with_hole = Polygon::new(square_ring(2.0), vec![hole]).unwrap();
        let stack = ContourStack::new_unchecked(vec![ContourLayer::new(0.05, vec![with_hole])]);
        let schedule = LayerSchedule::uniform(0.1, 0.1, 1).unwrap();

        let volume = estimated_volume_mm3(&stack, &schedule);
        assert!((volume - 1.2).abs() < 1e-9, "got {volume}");
    }

    #[test]
    fn empty_layer_contributes_no_volume() {
        let stack = ContourStack::new_unchecked(vec![ContourLayer::new(0.05, vec![])]);
        let schedule = LayerSchedule::uniform(0.1, 0.1, 1).unwrap();
        assert_eq!(estimated_volume_mm3(&stack, &schedule), 0.0);
    }

    #[test]
    fn statistics_never_format_negative_zero() {
        for value in [-0.0, -1e-10] {
            assert_eq!(format_volume_ml(value), "0.0 mL");
            assert_eq!(format_cost(value, "$"), "$0.00");
        }
    }

    #[test]
    fn format_volume_ml_converts_from_cubic_millimetres() {
        assert_eq!(format_volume_ml(1000.0), "1.0 mL");
        assert_eq!(format_volume_ml(51234.0), "51.2 mL");
        assert_eq!(format_volume_ml(0.0), "0.0 mL");
    }

    #[test]
    fn estimated_cost_scales_by_liters_used() {
        // 500,000 mm^3 = 0.5 L, at $40/L = $20.
        let price = PositiveFiniteCurrencyPerLiter::new(40.0).unwrap();
        let cost = estimated_cost(price, 500_000.0);
        assert!((cost - 20.0).abs() < 1e-9, "got {cost}");
    }

    #[test]
    fn estimated_cost_of_zero_volume_is_zero() {
        let price = PositiveFiniteCurrencyPerLiter::new(40.0).unwrap();
        assert_eq!(estimated_cost(price, 0.0), 0.0);
    }

    #[test]
    fn format_cost_prefixes_the_symbol_and_rounds_to_cents() {
        assert_eq!(format_cost(12.3, "$"), "$12.30");
        assert_eq!(format_cost(0.0, "$"), "$0.00");
        assert_eq!(format_cost(1.006, "€"), "€1.01");
    }
}

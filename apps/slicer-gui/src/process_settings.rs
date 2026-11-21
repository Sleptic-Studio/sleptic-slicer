// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Scene settings adapt the existing resolver overrides; printer geometry stays read-only.
use crate::{SettingRow, i18n::tr};
use sc_print_primitives::{PositiveFiniteMm, PositiveFiniteMmPerMin, PositiveFiniteSec};
use sc_print_profiles::SceneOverrides;
use slint::{ModelRc, VecModel};

pub(crate) fn edit(overrides: &mut SceneOverrides, id: i32, text: &str) -> bool {
    let reset = text.trim().is_empty();
    let value = text.trim().parse::<f64>().ok();
    macro_rules! set {
        ($field:ident, $ty:ty) => {{
            let next = if reset {
                None
            } else {
                let Some(value) = value.and_then(|v| <$ty>::new(v).ok()) else {
                    return false;
                };
                Some(value)
            };
            let changed = overrides.$field != next;
            overrides.$field = next;
            changed
        }};
    }
    match id {
        0 => set!(layer_height_mm, PositiveFiniteMm),
        1 => set!(normal_exposure_sec, PositiveFiniteSec),
        2 => set!(bottom_exposure_sec, PositiveFiniteSec),
        3 => {
            let next = if reset {
                None
            } else {
                let Ok(v) = text.parse::<u32>() else {
                    return false;
                };
                Some(v)
            };
            let changed = overrides.bottom_layer_count != next;
            overrides.bottom_layer_count = next;
            changed
        }
        4 => set!(lift_distance_mm, PositiveFiniteMm),
        5 => set!(lift_speed_mm_min, PositiveFiniteMmPerMin),
        6 => set!(retract_speed_mm_min, PositiveFiniteMmPerMin),
        7 => {
            let next = if reset {
                None
            } else {
                let Ok(v) = text.parse::<u32>() else {
                    return false;
                };
                Some(v)
            };
            let changed = overrides.transition_layers != next;
            overrides.transition_layers = next;
            changed
        }
        8 => {
            let next = if reset {
                None
            } else {
                let Some(v) = value.filter(|v| v.is_finite() && *v >= 0.0) else {
                    return false;
                };
                Some(v)
            };
            let changed = overrides.light_off_delay_s != next;
            overrides.light_off_delay_s = next;
            changed
        }
        9..=11 => {
            let Some(v) = value
                .filter(|v| v.is_finite())
                .or(if reset { Some(0.0) } else { None })
            else {
                return false;
            };
            if id == 11 {
                if !reset && (!(2.0..=256.0).contains(&v) || v.fract() != 0.0) {
                    return false;
                }
                let next = if reset { None } else { Some(v as u16) };
                let changed = overrides.aa_levels != next;
                overrides.aa_levels = next;
                changed
            } else {
                if id == 9 && v < 0.0 {
                    return false;
                }
                let field = if id == 9 {
                    &mut overrides.bottom_compensation_mm
                } else {
                    &mut overrides.xy_compensation_mm
                };
                let next = if reset { None } else { Some(v) };
                let changed = *field != next;
                *field = next;
                changed
            }
        }
        _ => false,
    }
}

pub(crate) fn rows(
    config: &sc_print_profiles::ResolvedPrintConfig,
    overrides: &SceneOverrides,
) -> ModelRc<SettingRow> {
    let m = &config.material_resolved;
    let entries = [
        (
            tr!("Layer height"),
            "mm",
            m.layer_height_mm.value(),
            overrides.layer_height_mm.is_some(),
            0,
            tr!("Height of each normal layer, constrained by the printer Z step and layer range."),
        ),
        (
            tr!("Normal exposure"),
            "s",
            m.normal_exposure_sec.value(),
            overrides.normal_exposure_sec.is_some(),
            0,
            tr!("UV exposure duration for normal layers."),
        ),
        (
            tr!("Bottom exposure"),
            "s",
            m.bottom_exposure_sec.value(),
            overrides.bottom_exposure_sec.is_some(),
            0,
            tr!("UV exposure duration for adhesion layers."),
        ),
        (
            tr!("Bottom layers"),
            "",
            f64::from(m.bottom_layer_count),
            overrides.bottom_layer_count.is_some(),
            0,
            tr!("Number of adhesion layers before the transition ramp."),
        ),
        (
            tr!("Lift distance"),
            "mm",
            m.lift_distance_mm.value(),
            overrides.lift_distance_mm.is_some(),
            1,
            tr!("Distance raised during the peel move."),
        ),
        (
            tr!("Lift speed"),
            "mm/min",
            m.lift_speed_mm_min.value(),
            overrides.lift_speed_mm_min.is_some(),
            1,
            tr!("Speed of the peel lift, in millimetres per minute."),
        ),
        (
            tr!("Retract speed"),
            "mm/min",
            m.retract_speed_mm_min.value(),
            overrides.retract_speed_mm_min.is_some(),
            1,
            tr!("Speed returning the plate after peeling."),
        ),
        (
            tr!("Transition layers"),
            "",
            f64::from(m.transition_layers),
            overrides.transition_layers.is_some(),
            1,
            tr!("Number of layers in the linear exposure ramp from bottom to normal."),
        ),
        (
            tr!("Light-off delay"),
            "s",
            m.light_off_delay_s,
            overrides.light_off_delay_s.is_some(),
            2,
            tr!("Settling delay after motion and before UV exposure."),
        ),
        (
            tr!("Bottom compensation"),
            "mm",
            m.bottom_compensation_mm,
            overrides.bottom_compensation_mm.is_some(),
            1,
            tr!("Erode adhesion layers in XY; fades linearly through transition layers."),
        ),
        (
            tr!("XY compensation"),
            "mm",
            m.xy_compensation_mm,
            overrides.xy_compensation_mm.is_some(),
            1,
            tr!("Outline offset on every layer. Positive grows the part and shrinks holes."),
        ),
        (
            tr!("AA gray levels"),
            "",
            f64::from(m.aa_levels),
            overrides.aa_levels.is_some(),
            1,
            tr!("Number of evenly spaced grayscale values, including black and white (2 to 256)."),
        ),
    ];
    ModelRc::new(VecModel::from(
        entries
            .into_iter()
            .enumerate()
            .map(
                |(id, (label, units, value, modified, level, help))| SettingRow {
                    minimum: limits(&config.printer.constraints, id).0 as f32,
                    maximum: limits(&config.printer.constraints, id).1 as f32,
                    step: limits(&config.printer.constraints, id).2 as f32,
                    id: id as i32,
                    label,
                    units: units.into(),
                    value: if units.is_empty() {
                        format!("{value:.0}").into()
                    } else {
                        trimmed_decimal(value).into()
                    },
                    modified,
                    level,
                    help,
                },
            )
            .collect::<Vec<_>>(),
    ))
}

/// Up to three decimals without trailing zeros: 2.5, 150, 0.05.
pub(crate) fn trimmed_decimal(value: f64) -> String {
    let text = format!("{value:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0".into() } else { text.into() }
}

#[cfg(test)]
#[path = "tests_process_settings.rs"]
mod tests;

/// Editor bounds derive from the same printer constraints as resolution.
pub(crate) fn limits(c: &sc_print_profiles::PrinterConstraints, id: usize) -> (f64, f64, f64) {
    match id {
        0 => (
            c.layer_height_range_mm.map_or(0.001, |r| r.min().value()),
            c.layer_height_range_mm.map_or(1.0, |r| r.max().value()),
            c.z_step_mm.map_or(0.01, |s| s.value()),
        ),
        3 | 7 => (0.0, u32::MAX as f64, 1.0),
        4 => (
            0.001,
            c.lift_distance_max_mm.map_or(1000.0, |v| v.value()),
            0.5,
        ),
        5 => (
            c.lift_speed_range_mm_min.map_or(0.001, |r| r.min().value()),
            c.lift_speed_range_mm_min
                .map_or(10000.0, |r| r.max().value()),
            5.0,
        ),
        6 => (
            c.retract_speed_range_mm_min
                .map_or(0.001, |r| r.min().value()),
            c.retract_speed_range_mm_min
                .map_or(10000.0, |r| r.max().value()),
            5.0,
        ),
        8 => (0.0, 86400.0, 0.1),
        9 => (0.0, 10.0, 0.01),
        10 => (-10.0, 10.0, 0.01),
        11 => (2.0, 256.0, 1.0),
        _ => (0.001, 86400.0, 0.1),
    }
}

pub(crate) fn edit_constrained(
    overrides: &mut SceneOverrides,
    c: &sc_print_profiles::PrinterConstraints,
    id: i32,
    text: &str,
) -> bool {
    if !text.trim().is_empty() {
        let (min, max, _) = limits(c, id as usize);
        if !text.parse::<f64>().is_ok_and(|v| v >= min && v <= max) {
            return false;
        }
    }
    edit(overrides, id, text)
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Motion profile form conversion and live cycle summary.
use crate::{MotionField, ProfileViewDialog};
use sc_print_profiles::motion::{Motion, MotionProfile};
use slint::{Model, ModelRc, VecModel};
const KEYS: [&str; 12] = [
    "lift_slow_distance_mm",
    "lift_slow_speed_mm_min",
    "lift_fast_distance_mm",
    "lift_fast_speed_mm_min",
    "retract_slow_distance_mm",
    "retract_slow_speed_mm_min",
    "retract_fast_distance_mm",
    "retract_fast_speed_mm_min",
    "wait_before_cure_s",
    "wait_after_lift_s",
    "wait_after_retract_s",
    "light_off_delay_s",
];
pub(crate) fn seed(dialog: &ProfileViewDialog, profile: MotionProfile) {
    let labels = [
        crate::i18n::tr!("Peel distance (mm)"),
        crate::i18n::tr!("Peel speed (mm/min)"),
        crate::i18n::tr!("Fast lift distance (mm)"),
        crate::i18n::tr!("Fast lift speed (mm/min)"),
        crate::i18n::tr!("Slow retract distance (mm)"),
        crate::i18n::tr!("Slow retract speed (mm/min)"),
        crate::i18n::tr!("Fast retract distance (mm)"),
        crate::i18n::tr!("Fast retract speed (mm/min)"),
        crate::i18n::tr!("Wait before cure (s)"),
        crate::i18n::tr!("Wait after lift (s)"),
        crate::i18n::tr!("Wait after retract (s)"),
        crate::i18n::tr!("Light-off delay (s)"),
    ];
    let mut rows = Vec::new();
    for (group, motion) in [profile.normal, profile.bottom, profile.transition]
        .into_iter()
        .enumerate()
    {
        let value = toml::Value::try_from(motion).expect("finite motion profile");
        for (i, key) in KEYS.iter().enumerate() {
            rows.push(MotionField {
                label: format!(
                    "{}{}",
                    match group {
                        1 => crate::i18n::tr!("Bottom: "),
                        2 => crate::i18n::tr!("Transition: "),
                        _ => "".into(),
                    },
                    labels[i]
                )
                .into(),
                value: value
                    .get(*key)
                    .and_then(toml::Value::as_float)
                    .map(|v| v.to_string())
                    .unwrap_or_default()
                    .into(),
                group: group as i32,
                help: crate::i18n::tr!("Enter a motion value in the displayed units. Blank inherits the printer or material value. Bottom overrides normal; transition ramps from its overridden bottom endpoint to normal. Lift and retract total travel must match."),
            });
        }
    }
    dialog.set_motion_fields(ModelRc::new(VecModel::from(rows)));
    refresh(dialog);
}
pub(crate) fn parse(dialog: &ProfileViewDialog) -> Result<MotionProfile, String> {
    let rows = dialog.get_motion_fields();
    let mut groups = [Motion::default(); 3];
    for (group, motion) in groups.iter_mut().enumerate() {
        let mut table = toml::map::Map::new();
        for (i, key) in KEYS.iter().enumerate() {
            let text = rows
                .row_data(group * 12 + i)
                .map(|r| r.value.to_string())
                .unwrap_or_default();
            if text.trim().is_empty() {
                continue;
            }
            let value = text
                .trim()
                .parse::<f64>()
                .map_err(|_| crate::i18n::tr!("Motion values must be numbers.").to_string())?;
            table.insert((*key).into(), toml::Value::Float(value));
        }
        *motion = toml::Value::Table(table)
            .try_into()
            .map_err(|e| format!("{e}"))?;
    }
    Ok(MotionProfile {
        normal: groups[0],
        bottom: groups[1],
        transition: groups[2],
    })
}
pub(crate) fn refresh(dialog: &ProfileViewDialog) {
    let text = match parse(dialog) {
        Ok(profile) => {
            let lift = dialog
                .get_lift_distance_text()
                .parse::<f64>()
                .unwrap_or(6.0);
            let speed = dialog
                .get_lift_speed_text()
                .parse::<f64>()
                .unwrap_or(60.0)
                .max(0.001);
            let retract = dialog
                .get_retract_speed_text()
                .parse::<f64>()
                .unwrap_or(150.0)
                .max(0.001);
            let fallback = Motion {
                lift_slow_distance_mm: Some(lift),
                lift_slow_speed_mm_min: Some(speed),
                retract_slow_distance_mm: Some(lift),
                retract_slow_speed_mm_min: Some(retract),
                ..Default::default()
            };
            let inherited = toml::from_str(&dialog.get_motion_inherited()).unwrap_or_default();
            let m = normal_example(fallback, inherited, profile.normal);
            let durations = [
                60.0 * m.lift_slow_distance_mm.unwrap_or(0.0)
                    / m.lift_slow_speed_mm_min.unwrap_or(speed),
                60.0 * m.lift_fast_distance_mm.unwrap_or(0.0)
                    / m.lift_fast_speed_mm_min.unwrap_or(speed),
                m.wait_after_lift_s.unwrap_or(0.0),
                60.0 * m.retract_fast_distance_mm.unwrap_or(0.0)
                    / m.retract_fast_speed_mm_min.unwrap_or(retract),
                60.0 * m.retract_slow_distance_mm.unwrap_or(0.0)
                    / m.retract_slow_speed_mm_min.unwrap_or(retract),
                m.wait_after_retract_s.unwrap_or(0.0)
                    + m.wait_before_cure_s.unwrap_or(0.0)
                    + m.light_off_delay_s.unwrap_or(0.0),
                dialog
                    .get_normal_exposure_input()
                    .parse::<f64>()
                    .unwrap_or(2.5),
            ];
            dialog.set_motion_phases(ModelRc::new(VecModel::from(
                durations.map(|v| v as f32).to_vec(),
            )));
            format!(
                "{} {:.2}s / {:.2}s / {:.2}s / {:.2}s / {:.2}s / {:.2}s / {:.2}s",
                crate::i18n::tr!(
                    "Peel / fast lift / wait / fast retract / slow retract / settle / cure:"
                ),
                durations[0],
                durations[1],
                durations[2],
                durations[3],
                durations[4],
                durations[5],
                durations[6]
            )
        }
        Err(e) => e,
    };
    dialog.set_motion_timeline(text.into());
}

pub(crate) fn open(
    profile: MotionProfile,
    saved: std::rc::Rc<dyn Fn(MotionProfile)>,
) -> Result<ProfileViewDialog, slint::PlatformError> {
    use slint::ComponentHandle;
    let dialog = ProfileViewDialog::new()?;
    crate::theme_sync::track(&dialog, ProfileViewDialog::set_viewer_theme);
    dialog.set_resin_heading(crate::i18n::tr!("Motion"));
    dialog.set_allow_profile_edit(true);
    dialog.set_motion_only(true);
    dialog.set_calibration_target_captions(ModelRc::new(VecModel::from(vec![crate::i18n::tr!(
        "Printer motion"
    )])));
    dialog.set_selected_calibration_target(0);
    seed(&dialog, profile);
    dialog.on_motion_field_changed({
        let weak = dialog.as_weak();
        move |i, value| {
            if let Some(d) = weak.upgrade() {
                let rows = d.get_motion_fields();
                if let Some(mut r) = rows.row_data(i as usize) {
                    r.value = value;
                    rows.set_row_data(i as usize, r);
                    refresh(&d);
                }
            }
        }
    });
    dialog.on_profile_save_requested({
        let weak = dialog.as_weak();
        move || {
            if let Some(d) = weak.upgrade() {
                match parse(&d) {
                    Ok(m) => {
                        saved(m);
                        let _ = d.hide();
                    }
                    Err(e) => d.set_editor_error(e.into()),
                }
            }
        }
    });
    dialog.on_viewer_close_requested({
        let weak = dialog.as_weak();
        move || {
            if let Some(d) = weak.upgrade() {
                let _ = d.hide();
            }
        }
    });
    dialog.show()?;
    Ok(dialog)
}

fn normal_example(legacy: Motion, inherited: Motion, edited: Motion) -> Motion {
    legacy.overlay(inherited).overlay(edited)
}

#[cfg(test)]
#[path = "tests_motion_form.rs"]
mod tests;

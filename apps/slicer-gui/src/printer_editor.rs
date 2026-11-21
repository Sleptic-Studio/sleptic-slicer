// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Printer authoring through the same typed schema and renderer as export.
use crate::i18n::ctr;
use crate::{PrinterEditor, PrinterField};
use sc_print_profiles::{PrinterProfile, ProfileLibraries};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

const PATHS: &[&[&str]] = &[
    &["identity", "name"],
    &["hardware", "pixel_resolution", "x"],
    &["hardware", "pixel_resolution", "y"],
    &["hardware", "pixel_size_microns", "x"],
    &["hardware", "pixel_size_microns", "y"],
    &["hardware", "build_volume", "x_mm"],
    &["hardware", "build_volume", "y_mm"],
    &["hardware", "build_volume", "z_mm"],
];
const KEYS: &[&str] = &[
    "layers_total",
    "total_time_s",
    "volume_ml",
    "end_lift_z",
    "layer",
    "layer_index0",
    "z",
    "layer_height",
    "exposure_s",
    "exposure_ms",
    "light_off_ms",
    "image",
    "lift_distance",
    "lift_speed",
    "retract_speed",
    "is_bottom",
    "motion_cycle",
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

fn fields(profile: &PrinterProfile) -> Vec<String> {
    let value = toml::Value::try_from(profile).expect("serializable printer");
    PATHS
        .iter()
        .map(|path| {
            let mut v = &value;
            for key in *path {
                v = &v[*key];
            }
            v.as_str().map_or_else(|| v.to_string(), str::to_owned)
        })
        .collect()
}

fn edited(
    base: &PrinterProfile,
    values: &[String],
    mirror: [bool; 2],
    templates: [String; 3],
) -> anyhow::Result<PrinterProfile> {
    let mut value = toml::Value::try_from(base)?;
    for (i, path) in PATHS.iter().enumerate() {
        let mut v = &mut value;
        for key in &path[..path.len() - 1] {
            v = &mut v[*key];
        }
        v[path[path.len() - 1]] = if i == 0 {
            toml::Value::String(values[i].trim().into())
        } else if i <= 2 {
            toml::Value::Integer(values[i].parse::<i64>()?)
        } else {
            toml::Value::Float(values[i].parse::<f64>()?)
        };
    }
    value["hardware"]["mirror"]["x"] = toml::Value::Boolean(mirror[0]);
    value["hardware"]["mirror"]["y"] = toml::Value::Boolean(mirror[1]);
    let mut profile: PrinterProfile = value.try_into()?;
    let job = profile
        .job
        .get_or_insert_with(|| sc_print_profiles::job_profile::JobProfile {
            kind: sc_print_profiles::job_profile::JobKind::GcodeZip,
            templates: sc_print_job::default_job_templates(),
            custom: BTreeMap::new(),
        });
    [job.templates.start, job.templates.layer, job.templates.end] = templates;
    let mut substitutions: BTreeMap<String, String> =
        KEYS.iter().map(|k| ((*k).into(), "0".into())).collect();
    for (key, value) in &job.custom {
        substitutions.insert(format!("custom.{key}"), value.to_string());
    }
    for (name, text) in [
        ("start", &job.templates.start),
        ("layer", &job.templates.layer),
        ("end", &job.templates.end),
    ] {
        sc_print_job::render_template(name, text, &substitutions)?;
    }
    Ok(profile)
}

pub(crate) fn duplicate_profile(
    mut profile: PrinterProfile,
    libraries: &ProfileLibraries,
) -> PrinterProfile {
    for n in 1.. {
        let id = sc_print_primitives::PrinterId::new(&format!("{}-copy-{n}", profile.identity.id))
            .expect("valid copied id");
        if !libraries
            .roots()
            .any(|(root, _)| sc_print_profiles::printer_profile_path_in_library(root, &id).exists())
        {
            profile.identity.id = id;
            profile.identity.name = sc_print_primitives::NonEmptyString::new(&format!(
                "{} ({})",
                profile.identity.name,
                ctr!("PrinterEditor", "copy")
            ))
            .expect("nonempty copy name");
            return profile;
        }
    }
    unreachable!("printer copy id space exhausted")
}

pub(crate) fn open(
    base: PrinterProfile,
    duplicate: bool,
    saved: Rc<dyn Fn(&PrinterProfile)>,
) -> Result<PrinterEditor, slint::PlatformError> {
    let dialog = PrinterEditor::new()?;
    crate::theme_sync::track(&dialog, PrinterEditor::set_theme);
    let base = if duplicate {
        duplicate_profile(base, &ProfileLibraries::platform_default())
    } else {
        base
    };
    let labels = [
        ctr!("PrinterEditor", "Name"),
        ctr!("PrinterEditor", "Resolution X"),
        ctr!("PrinterEditor", "Resolution Y"),
        ctr!("PrinterEditor", "Pixel X (microns)"),
        ctr!("PrinterEditor", "Pixel Y (microns)"),
        ctr!("PrinterEditor", "Build X (mm)"),
        ctr!("PrinterEditor", "Build Y (mm)"),
        ctr!("PrinterEditor", "Build Z (mm)"),
    ];
    let values = Rc::new(RefCell::new(fields(&base)));
    dialog.set_fields(ModelRc::new(VecModel::from(
        labels
            .iter()
            .zip(values.borrow().iter())
            .map(|(label, value)| PrinterField {
                label: label.clone(),
                value: value.as_str().into(),
            })
            .collect::<Vec<_>>(),
    )));
    dialog.set_rotation(i32::from(u16::from(base.hardware.calibration().rotation)) / 90);
    dialog.set_scale_x(base.hardware.calibration().xy_scale()[0].to_string().into());
    dialog.set_scale_y(base.hardware.calibration().xy_scale()[1].to_string().into());
    dialog.set_mirror_x(base.hardware.mirror().x);
    dialog.set_mirror_y(base.hardware.mirror().y);
    let templates = base
        .job
        .as_ref()
        .map(|j| j.templates.clone())
        .unwrap_or_else(sc_print_job::default_job_templates);
    dialog.set_start(templates.start.as_str().into());
    dialog.set_layer(templates.layer.as_str().into());
    dialog.set_end(templates.end.as_str().into());
    let mut placeholders = KEYS.iter().map(|k| format!("{{{k}}}")).collect::<Vec<_>>();
    if let Some(job) = &base.job {
        placeholders.extend(job.custom.keys().map(|k| format!("{{custom.{k}}}")));
    }
    dialog.set_placeholders(placeholders.join("\n").into());
    let base = Rc::new(base);
    let motion = Rc::new(std::cell::Cell::new(base.motion));
    let motion_window = Rc::new(RefCell::new(None));
    dialog.on_motion_requested({
        let parent = dialog.as_weak();
        let motion = motion.clone();
        move || {
            let state = motion.clone();
            match crate::motion_form::open(
                motion.get().unwrap_or_default(),
                Rc::new(move |m| state.set(Some(m))),
            ) {
                Ok(d) => {
                    if let Some(parent) = parent.upgrade() {
                        d.set_viewer_theme(parent.get_theme());
                    }
                    *motion_window.borrow_mut() = Some(d);
                }
                Err(e) => eprintln!("Motion editor: {e}"),
            }
        }
    });
    let validate: Rc<dyn Fn(bool)> = {
        let motion = motion.clone();
        let weak = dialog.as_weak();
        let values = values.clone();
        let base = base.clone();
        Rc::new(move |save| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let result = edited(
                &base,
                &values.borrow(),
                [w.get_mirror_x(), w.get_mirror_y()],
                [
                    w.get_start().to_string(),
                    w.get_layer().to_string(),
                    w.get_end().to_string(),
                ],
            );
            let result = result.and_then(|mut profile| {
                let calibration = sc_print_primitives::MachineCalibration::new(
                    sc_print_primitives::LcdRotation::try_from((w.get_rotation() * 90) as u16)
                        .map_err(anyhow::Error::msg)?,
                    [w.get_scale_x().parse()?, w.get_scale_y().parse()?],
                )
                .map_err(anyhow::Error::msg)?;
                profile.hardware = profile.hardware.with_calibration(calibration);
                Ok(profile)
            });
            match result {
                Ok(mut profile) => {
                    profile.motion = motion.get();
                    w.set_error("".into());
                    if save {
                        let result = sc_print_profiles::user_library_dir()
                            .map_err(anyhow::Error::from)
                            .and_then(|dir| {
                                sc_print_profiles::save_printer_profile_to_library(&dir, &profile)
                                    .map_err(Into::into)
                            });
                        match result {
                            Ok(()) => {
                                saved(&profile);
                                let _ = w.hide();
                            }
                            Err(e) => w.set_error(e.to_string().into()),
                        }
                    }
                }
                Err(e) => w.set_error(e.to_string().into()),
            }
        })
    };
    dialog.on_field_edited({
        let values = values.clone();
        let validate = validate.clone();
        let weak = dialog.as_weak();
        move |i, text| {
            if let Some(value) = values.borrow_mut().get_mut(i as usize) {
                *value = text.to_string();
            }
            if let Some(w) = weak.upgrade() {
                let model = w.get_fields();
                if let Some(mut row) = model.row_data(i as usize) {
                    row.value = text;
                    model.set_row_data(i as usize, row);
                }
            }
            validate(false);
        }
    });
    dialog.on_validate({
        let validate = validate.clone();
        move || validate(false)
    });
    validate(false);
    dialog.on_save(move || validate(true));
    dialog.on_cancel({
        let weak = dialog.as_weak();
        move || {
            if let Some(w) = weak.upgrade() {
                let _ = w.hide();
            }
        }
    });
    dialog.show()?;
    Ok(dialog)
}

#[cfg(test)]
#[path = "tests_printer_editor.rs"]
mod tests;

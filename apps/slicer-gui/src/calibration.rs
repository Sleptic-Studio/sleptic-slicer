// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Exposure calibration UI and material-profile recording.
use crate::{MainWindow, preferences_bus::PreferenceHub, viewport::ViewController};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};
pub(crate) fn wire(ui: &MainWindow, ctrl: Rc<ViewController>, prefs: Rc<PreferenceHub>) {
    wire_machine(ui, ctrl.clone(), prefs.clone());
    wire_overhang(ui, ctrl.clone(), prefs.clone());
    let values = Rc::new(RefCell::new(Vec::<f64>::new()));
    ui.on_calibration_opened({
        let weak = ui.as_weak();
        let ctrl = ctrl.clone();
        let values = values.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                let doc = ctrl.scene_doc();
                if let Some(Ok(regions)) = doc.read_scene(|s| {
                    s.graph()
                        .plate_scene(doc.active_plate())
                        .map_err(|e| e.to_string())
                        .and_then(|s| {
                            sc_print_glue::calibration::regions(&s).map_err(error_message)
                        })
                }) && !regions.is_empty()
                {
                    let exposures: Vec<_> = regions.iter().map(|r| r.exposure_s).collect();
                    ui.set_calibration_tiles(ModelRc::new(VecModel::from(
                        exposures
                            .iter()
                            .enumerate()
                            .map(|(i, v)| {
                                format!(
                                    "{}: {}s",
                                    i + 1,
                                    sc_print_glue::calibration::exposure_text(*v)
                                )
                                .into()
                            })
                            .collect::<Vec<slint::SharedString>>(),
                    )));
                    *values.borrow_mut() = exposures;
                }
            }
        }
    });
    ui.on_calibration_create({
        let weak = ui.as_weak();
        let ctrl = ctrl.clone();
        let values = values.clone();
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let result = (|| -> Result<(), String> {
                let exposures = if !ui.get_calibration_list().trim().is_empty() {
                    sc_print_glue::calibration::exposures(
                        2,
                        1.0,
                        1.0,
                        ui.get_calibration_list().as_str(),
                    )
                    .map_err(error_message)?
                } else {
                    sc_print_glue::calibration::exposures(
                        ui.get_calibration_count()
                            .parse()
                            .map_err(|_| crate::i18n::tr!("Enter a tile count.").to_string())?,
                        ui.get_calibration_start().parse().map_err(|_| {
                            crate::i18n::tr!("Enter start exposure seconds.").to_string()
                        })?,
                        ui.get_calibration_step().parse().map_err(|_| {
                            crate::i18n::tr!("Enter exposure step seconds.").to_string()
                        })?,
                        ui.get_calibration_list().as_str(),
                    )
                    .map_err(error_message)?
                };
                let doc = ctrl.scene_doc();
                let base = doc.read_scene(|s| s.graph().clone()).unwrap_or_default();
                let plate = doc.active_plate();
                if base
                    .reachable_leaves(
                        sc_scene::CsgPolicy::Descend,
                        sc_scene::ReferencePolicy::Skip,
                    )
                    .map_err(|e| e.to_string())?
                    .iter()
                    .any(|&id| base.plate_of(id) == plate)
                {
                    return Err(crate::i18n::tr!(
                        "Use an empty active plate for exposure calibration."
                    )
                    .to_string());
                }
                let sources =
                    sc_print_glue::calibration::scenes(&exposures).map_err(error_message)?;
                let mut staged = base;
                let (edit, _) = crate::layout_tools::import_geometry(
                    &mut staged,
                    &sources,
                    ctrl.printer_volume(),
                    plate,
                )?;
                doc.apply_batch(edit, staged.nodes_len())?;
                ui.set_calibration_tiles(ModelRc::new(VecModel::from(
                    exposures
                        .iter()
                        .enumerate()
                        .map(|(i, v)| {
                            format!(
                                "{}: {}s",
                                i + 1,
                                sc_print_glue::calibration::exposure_text(*v)
                            )
                            .into()
                        })
                        .collect::<Vec<slint::SharedString>>(),
                )));
                *values.borrow_mut() = exposures;
                Ok(())
            })();
            ui.set_calibration_message(match result {
                Ok(()) => crate::i18n::tr!(
                    "Tiles placed. Close this dialog, slice and export G-code ZIP."
                ),
                Err(e) => e.into(),
            });
        }
    });
    ui.on_calibration_record({
        let weak = ui.as_weak();
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let result = (|| -> Result<String, String> {
                let exposure = *values
                    .borrow()
                    .get(ui.get_calibration_best() as usize)
                    .ok_or_else(|| crate::i18n::tr!("Choose a tile first").to_string())?;
                let snapshot = prefs.current_values();
                let printer = ctrl
                    .scene_doc()
                    .chosen_printer()
                    .or(snapshot.default_printer)
                    .ok_or_else(|| crate::i18n::tr!("Select a printer first").to_string())?;
                let resin = ctrl
                    .scene_doc()
                    .chosen_resin()
                    .or(snapshot.default_resin)
                    .ok_or_else(|| crate::i18n::tr!("Select a material first").to_string())?;
                let libraries = sc_print_profiles::ProfileLibraries::platform_default();
                let (effective, _) = libraries
                    .effective_material_by_id(&resin)
                    .map_err(|e| e.to_string())?;
                let source = sc_print_profiles::MaterialProfile {
                    extension: false,
                    identity: effective.identity().clone(),
                    intent: *effective.intent(),
                    defaults: *effective.defaults(),
                    per_printer: effective.per_printer().clone(),
                };
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| e.to_string())?
                    .as_secs();
                let name = format!(
                    "{} - exposure {}s - {stamp}",
                    source.identity.name,
                    sc_print_glue::calibration::exposure_text(exposure)
                );
                let mut existing = BTreeSet::new();
                let fork = loop {
                    let fork = result_profile(&source, &printer, exposure, &name, &existing)?;
                    if !libraries.roots().any(|(root, _)| {
                        sc_print_profiles::material_profile_path_in_library(root, &fork.identity.id)
                            .exists()
                    }) {
                        break fork;
                    }
                    existing.insert(fork.identity.id);
                };
                let library = sc_print_profiles::user_library_dir().map_err(|e| e.to_string())?;
                crate::material_form::write_edited_resin(
                    &library,
                    &fork,
                    crate::material_form::ResinFileKind::WholeProfile,
                )?;
                prefs.edit_preferences(|_| crate::preferences_bus::PreferenceEdit::Profiles);
                Ok(format!("{} {name}", crate::i18n::tr!("Saved material:")))
            })();
            ui.set_calibration_message(result.unwrap_or_else(|e| e).into());
        }
    });
}

/// Overhang test (R80): place it, then save the flattest ledge that
/// printed clean as the selected resin's support angle.
fn wire_overhang(ui: &MainWindow, ctrl: Rc<ViewController>, prefs: Rc<PreferenceHub>) {
    ui.on_overhang_test_create({
        let weak = ui.as_weak();
        let ctrl = ctrl.clone();
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let result = (|| -> Result<(), String> {
                let doc = ctrl.scene_doc();
                let mut staged = doc.read_scene(|s| s.graph().clone()).unwrap_or_default();
                let source = sc_print_glue::calibration::overhang_test_scene()
                    .map_err(error_message)?;
                let (edit, _) = crate::layout_tools::import_geometry(
                    &mut staged,
                    &[source],
                    ctrl.printer_volume(),
                    doc.active_plate(),
                )?;
                doc.apply_batch(edit, staged.nodes_len())?;
                Ok(())
            })();
            ui.set_overhang_test_message(match result {
                Ok(()) => crate::i18n::tr!(
                    "Placed. Slice and print it WITHOUT supports in the resin you want to measure."
                ),
                Err(e) => e.into(),
            });
        }
    });
    ui.on_overhang_test_record({
        let weak = ui.as_weak();
        move |index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let result = (|| -> Result<String, String> {
                let angle = *sc_print_glue::calibration::OVERHANG_TEST_ANGLES
                    .get(usize::try_from(index).unwrap_or(usize::MAX))
                    .ok_or_else(|| crate::i18n::tr!("Choose a ledge first").to_string())?;
                let snapshot = prefs.current_values();
                let resin = ctrl
                    .scene_doc()
                    .chosen_resin()
                    .or(snapshot.default_resin)
                    .ok_or_else(|| crate::i18n::tr!("Select a material first").to_string())?;
                let libraries = sc_print_profiles::ProfileLibraries::platform_default();
                let (effective, _) = libraries
                    .effective_material_by_id(&resin)
                    .map_err(|e| e.to_string())?;
                let source = sc_print_profiles::MaterialProfile {
                    extension: false,
                    identity: effective.identity().clone(),
                    intent: *effective.intent(),
                    defaults: *effective.defaults(),
                    per_printer: effective.per_printer().clone(),
                };
                let name = format!("{} - overhang {angle:.0}", source.identity.name);
                let mut existing = BTreeSet::new();
                let fork = loop {
                    let fork = overhang_result_profile(&source, angle, &name, &existing)?;
                    if !libraries.roots().any(|(root, _)| {
                        sc_print_profiles::material_profile_path_in_library(root, &fork.identity.id)
                            .exists()
                    }) {
                        break fork;
                    }
                    existing.insert(fork.identity.id);
                };
                let library = sc_print_profiles::user_library_dir().map_err(|e| e.to_string())?;
                crate::material_form::write_edited_resin(
                    &library,
                    &fork,
                    crate::material_form::ResinFileKind::WholeProfile,
                )?;
                // Use it right away: the resin picker and the Supports
                // panel's overhang angle follow.
                ctrl.scene_doc().choose_resin(Some(fork.identity.id.clone()));
                prefs.edit_preferences(|_| crate::preferences_bus::PreferenceEdit::Profiles);
                Ok(crate::i18n::tr!(
                    "Saved and selected resin \"{}\": supports overhangs under {} deg.",
                    name,
                    angle
                )
                .to_string())
            })();
            ui.set_overhang_test_message(result.unwrap_or_else(|e| e).into());
        }
    });
}

/// A copy of `source` (all its calibration) whose overhangs flatter than
/// `angle` degrees need supports.
pub(crate) fn overhang_result_profile(
    source: &sc_print_profiles::MaterialProfile,
    angle: f64,
    name: &str,
    existing: &BTreeSet<sc_print_primitives::MaterialId>,
) -> Result<sc_print_profiles::MaterialProfile, String> {
    let mut fork = crate::profile_forking::forked_resin(
        source,
        &crate::profile_forking::CopiedSections {
            with_intent: true,
            with_defaults: true,
            printer_blocks: source.per_printer.keys().cloned().collect(),
        },
        name,
        source.identity.brand.as_str(),
        existing,
    )
    .map_err(|e| e.to_string())?;
    fork.defaults.support_overhang_angle_deg = Some(angle);
    Ok(fork)
}

fn result_profile(
    source: &sc_print_profiles::MaterialProfile,
    printer: &sc_print_primitives::PrinterId,
    exposure: f64,
    name: &str,
    existing: &BTreeSet<sc_print_primitives::MaterialId>,
) -> Result<sc_print_profiles::MaterialProfile, String> {
    let mut fork = crate::profile_forking::forked_resin(
        source,
        &crate::profile_forking::CopiedSections {
            with_intent: true,
            with_defaults: true,
            printer_blocks: source.per_printer.keys().cloned().collect(),
        },
        name,
        source.identity.brand.as_str(),
        existing,
    )
    .map_err(|e| e.to_string())?;
    let target = sc_print_profiles::CalibrationTarget::from_printer(printer);
    let block = source
        .per_printer
        .get(&target)
        .or_else(|| {
            source
                .per_printer
                .get(&sc_print_profiles::CalibrationTarget::Generic)
        })
        .copied()
        .ok_or_else(|| crate::i18n::tr!("Material needs a calibrated printer block").to_string())?;
    fork.per_printer.insert(
        target,
        sc_print_profiles::MaterialPerPrinter {
            normal_exposure_sec: Some(
                sc_print_primitives::PositiveFiniteSec::new(exposure).map_err(|e| e.to_string())?,
            ),
            ..block
        },
    );
    Ok(fork)
}

pub(crate) fn error_message(error: sc_print_glue::calibration::CalibrationError) -> String {
    use sc_print_glue::calibration::CalibrationError;
    match error {
        CalibrationError::MachineSize => crate::i18n::tr!(
            "Printer is too small for this standard machine test, or dimensions are invalid"
        )
        .to_string(),
        CalibrationError::ExposureList => {
            crate::i18n::tr!("Enter comma-separated exposure seconds").to_string()
        }
        CalibrationError::ExposureSet => crate::i18n::tr!(
            "Choose 2 to 8 distinct exposures from 0.001 to 600 seconds in 0.001-second steps"
        )
        .to_string(),
        CalibrationError::Exposure => {
            crate::i18n::tr!("Exposure must be from 0.001 to 600 seconds in 0.001-second steps")
                .to_string()
        }
        CalibrationError::Geometry(feature, error) => crate::i18n::tr!(
            "Could not build calibration geometry: {} ({})",
            error,
            feature
        )
        .to_string(),
        CalibrationError::Scene(error) => {
            crate::i18n::tr!("Could not prepare the calibration scene: {}", error).to_string()
        }
        CalibrationError::MixedPlate => {
            crate::i18n::tr!("Calibration plate must contain exposure tiles only").to_string()
        }
        CalibrationError::InvalidTag => {
            crate::i18n::tr!("Invalid calibration exposure metadata").to_string()
        }
        CalibrationError::Overlap => {
            crate::i18n::tr!("Calibration tile masks overlap; arrange the tiles before export")
                .to_string()
        }
    }
}

#[cfg(test)]
#[path = "tests_calibration.rs"]
mod tests;

fn selected_machine(
    ctrl: &ViewController,
    prefs: &PreferenceHub,
) -> Result<sc_print_profiles::PrinterProfile, String> {
    let id = ctrl
        .scene_doc()
        .chosen_printer()
        .or(prefs.current_values().default_printer)
        .ok_or_else(|| crate::i18n::tr!("Select a printer first").to_string())?;
    sc_print_profiles::ProfileLibraries::platform_default()
        .load_printer_by_id(&id)
        .map_err(|e| e.to_string())
}
fn wire_machine(ui: &MainWindow, ctrl: Rc<ViewController>, prefs: Rc<PreferenceHub>) {
    ui.on_machine_opened({
        let weak = ui.as_weak();
        let ctrl = ctrl.clone();
        let prefs = prefs.clone();
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            ui.set_machine_message("".into());
            match selected_machine(&ctrl, &prefs) {
                Ok(p) => {
                    ui.set_machine_mirror_x(p.hardware.mirror().x);
                    ui.set_machine_mirror_y(p.hardware.mirror().y);
                    ui.set_machine_rotation(
                        i32::from(u16::from(p.hardware.calibration().rotation)) / 90,
                    );
                }
                Err(e) => ui.set_machine_message(e.into()),
            }
        }
    });
    ui.on_machine_create({
        let weak = ui.as_weak();
        let ctrl = ctrl.clone();
        let prefs = prefs.clone();
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let result = (|| -> Result<(), String> {
                let printer = selected_machine(&ctrl, &prefs)?;
                let doc = ctrl.scene_doc();
                let base = doc.read_scene(|s| s.graph().clone()).unwrap_or_default();
                let plate = doc.active_plate();
                if base
                    .reachable_leaves(
                        sc_scene::CsgPolicy::Descend,
                        sc_scene::ReferencePolicy::Skip,
                    )
                    .map_err(|e| e.to_string())?
                    .iter()
                    .any(|&id| base.plate_of(id) == plate)
                {
                    return Err(crate::i18n::tr!(
                        "Use an empty active plate for machine calibration."
                    )
                    .to_string());
                }
                use sc_print_glue::calibration::MachineTool;
                let tool = match ui.get_machine_tool() {
                    0 => MachineTool::Orientation,
                    1 => MachineTool::XyRuler,
                    _ => MachineTool::ZTower,
                };
                let b = printer.hardware.build_volume();
                let source = sc_print_glue::calibration::machine_scene(
                    tool,
                    [b.x_mm(), b.y_mm(), b.z_mm()],
                    machine_layer_height(&ctrl, &prefs, &printer)?,
                )
                .map_err(error_message)?;
                // Keep the generated machine coordinates: placement must not move the
                // single-layer pattern away from the actual plate centre.
                let mut staged = base;
                let (edit, _) = crate::layout_tools::import_machine_geometry(
                    &mut staged,
                    &[source],
                    ctrl.printer_volume(),
                    plate,
                )?;
                doc.apply_batch(edit, staged.nodes_len())?;
                Ok(())
            })();
            ui.set_machine_message(match result {
                Ok(()) => {
                    crate::i18n::tr!("Test placed. Close this dialog, then slice and export.")
                }
                Err(e) => e.into(),
            });
        }
    });
    ui.on_machine_record({let weak=ui.as_weak();move || {
        let Some(ui)=weak.upgrade() else {return;};
        let result=(||->Result<String,String> {
            let source=selected_machine(&ctrl,&prefs)?;
            let calibration=if ui.get_machine_tool()==0 {
                sc_print_primitives::MachineCalibration::new(sc_print_primitives::LcdRotation::try_from((ui.get_machine_rotation()*90) as u16).map_err(str::to_string)?,source.hardware.calibration().xy_scale()).map_err(str::to_string)?
            } else {
                let measured=[ui.get_machine_measured_x().parse().map_err(|_|crate::i18n::tr!("Enter measured X millimetres").to_string())?,ui.get_machine_measured_y().parse().map_err(|_|crate::i18n::tr!("Enter measured Y millimetres").to_string())?];
                source.hardware.calibration().measured([100.0;2],measured).map_err(|_|crate::i18n::tr!("Enter positive ruler measurements giving corrections between 0.5 and 2").to_string())?
            };
            let libraries=sc_print_profiles::ProfileLibraries::platform_default();
            let mut fork=crate::printer_editor::duplicate_profile(source.clone(),&libraries);
            let label=if ui.get_machine_tool()==0 {crate::i18n::tr!("LCD orientation calibrated")} else {crate::i18n::tr!("XY scale calibrated")};
            fork.identity.name=sc_print_primitives::NonEmptyString::new(&format!("{} - {}",source.identity.name,label)).map_err(|e|e.to_string())?;
            fork=machine_result_profile(fork,calibration,if ui.get_machine_tool()==0 {sc_print_primitives::Mirror {x:ui.get_machine_mirror_x(),y:ui.get_machine_mirror_y()}} else {source.hardware.mirror()})?;
            let library=sc_print_profiles::user_library_dir().map_err(|e|e.to_string())?;
            sc_print_profiles::save_printer_profile_to_library(&library,&fork).map_err(|e|e.to_string())?;
            Ok(format!("{} {} ({})",crate::i18n::tr!("Saved printer; select it in printer settings:"),fork.identity.name,fork.identity.id))
        })();ui.set_machine_message(result.unwrap_or_else(|e|e).into());
    }});
}
fn machine_result_profile(
    mut fork: sc_print_profiles::PrinterProfile,
    calibration: sc_print_primitives::MachineCalibration,
    mirror: sc_print_primitives::Mirror,
) -> Result<sc_print_profiles::PrinterProfile, String> {
    let h = fork.hardware;
    fork.hardware = sc_print_profiles::PrinterHardware::new(
        h.build_volume(),
        h.pixel_resolution(),
        h.pixel_size_microns(),
        mirror,
    )
    .map_err(|e| e.to_string())?
    .with_calibration(calibration);
    Ok(fork)
}

fn machine_layer_height(
    ctrl: &ViewController,
    prefs: &PreferenceHub,
    printer: &sc_print_profiles::PrinterProfile,
) -> Result<f64, String> {
    let doc = ctrl.scene_doc();
    if let Some(h) = doc.chosen_first_layer() {
        return Ok(h.value());
    }
    let id = doc
        .chosen_resin()
        .or(prefs.current_values().default_resin)
        .ok_or_else(|| crate::i18n::tr!("Select a material first").to_string())?;
    let (material, _) = sc_print_profiles::ProfileLibraries::platform_default()
        .effective_material_by_id(&id)
        .map_err(|e| e.to_string())?;
    let config = sc_print_profiles::resolve(printer, &material, &doc.process_overrides.borrow())
        .map_err(|e| e.to_string())?;
    Ok(config.material_resolved.layer_height_mm.value())
}

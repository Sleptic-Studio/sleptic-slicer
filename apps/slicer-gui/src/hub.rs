// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The Home, Device and Project tabs: fills each page when it opens and
//! handles its actions (send to a drive, print history outcomes, time
//! estimate calibration, new project).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use slint::{ComponentHandle, Model as _, ModelRc, SharedString, VecModel};

use crate::preferences_bus::PreferenceHub;
use crate::print_history::{self, Entry, Outcome};
use crate::scene_document::SceneDocument;
use crate::send_to_printer::{self, Drive, SendSettings};
use crate::viewport::ViewController;
use crate::{DriveRow, HistoryRow, MainWindow, PlateRow, RecentProject, WorkspaceSection};

pub(crate) const PAGE_HOME: i32 = 1;
pub(crate) const PAGE_DEVICE: i32 = 2;
pub(crate) const PAGE_PROJECT: i32 = 3;

/// The machine schedule (exposure and motion per layer) of a slice of
/// `plate`, as the export will run it.
pub(crate) fn plate_schedule(
    doc: &SceneDocument,
    plate: u32,
    params: &crate::slice_result::SliceParams,
    grid: &sc_sl1cer::profile::LayerSchedule,
) -> Option<sc_print_job::ExposureSchedule> {
    let (printer, material) =
        crate::slice_projection::load_chosen_profiles(&params.printer, &params.resin).ok()?;
    let config =
        sc_print_profiles::resolve(&printer, &material, &doc.process_overrides.borrow()).ok()?;
    let regions = doc
        .read_scene(|s| {
            s.graph()
                .plate_scene(plate)
                .ok()
                .and_then(|s| sc_print_glue::calibration::regions(&s).ok())
        })
        .flatten()
        .unwrap_or_default();
    Some(sc_print_glue::calibration::schedule(&config.material_resolved, grid, &regions).0)
}

/// Print time for `schedule` on `printer`, with that printer's fitted
/// time-estimate correction when one was calibrated.
pub(crate) fn estimate_seconds(
    printer: &sc_print_primitives::PrinterId,
    schedule: &sc_print_job::ExposureSchedule,
) -> f64 {
    let correction = crate::estimate_calibration::for_printer(printer.as_str());
    if correction.samples == 0 {
        schedule.total_time_s()
    } else {
        correction.total_s(schedule)
    }
}

/// The held slice of the active plate: its printer and machine schedule.
fn held_schedule(
    doc: &SceneDocument,
) -> Option<(sc_print_primitives::PrinterId, sc_print_job::ExposureSchedule)> {
    let params = doc.slicing_inputs()?;
    let grid = doc.slice_layer_schedule()?;
    let schedule = plate_schedule(doc, doc.active_plate(), &params, &grid)?;
    Some((params.printer, schedule))
}

/// Records a finished export in the print history.
pub(crate) fn record_export(doc: &SceneDocument, file: &Path, layers: usize) {
    let Some(path) = print_history::history_path() else {
        return;
    };
    let params = doc.slicing_inputs();
    let estimate = held_schedule(doc)
        .map(|(printer, s)| sc_print_job::format_duration(estimate_seconds(&printer, &s)))
        .unwrap_or_default();
    let entry = Entry {
        exported_at: print_history::now_unix(),
        file: file.to_path_buf(),
        printer: params.as_ref().map(|p| p.printer.to_string()).unwrap_or_default(),
        material: params.as_ref().map(|p| p.resin.to_string()).unwrap_or_default(),
        layers,
        estimate,
        outcome: Outcome::NotRecorded,
        failed_layer: None,
    };
    if let Err(e) = print_history::append(&path, &entry) {
        eprintln!("print history: {e}");
    }
}

struct Hub {
    ui: slint::Weak<MainWindow>,
    ctrl: Rc<ViewController>,
    prefs: Rc<PreferenceHub>,
    drives: RefCell<Vec<Drive>>,
}

pub(crate) fn wire(ui: &MainWindow, ctrl: Rc<ViewController>, prefs: Rc<PreferenceHub>) {
    let hub = Rc::new(Hub {
        ui: ui.as_weak(),
        ctrl,
        prefs,
        drives: RefCell::new(Vec::new()),
    });
    ui.set_post_export_command(SendSettings::load().post_export_command.into());
    {
        let hub = hub.clone();
        ui.on_hub_page_opened(move |page| hub.refresh(page));
    }
    {
        let hub = hub.clone();
        ui.on_new_project_requested(move || {
            if let Some(ui) = hub.ui.upgrade() {
                ui.set_hub_page(0);
                ui.invoke_section_picked(WorkspaceSection::Transform);
            }
            hub.ctrl.close_scene();
        });
    }
    {
        let hub = hub.clone();
        ui.on_refresh_drives_requested(move || hub.refresh_drives());
    }
    {
        let hub = hub.clone();
        ui.on_remove_recent_requested(move |index| {
            let Some(ui) = hub.ui.upgrade() else {
                return;
            };
            let Some(path) = ui.get_recent_files().row_data(index as usize) else {
                return;
            };
            let path = PathBuf::from(path.as_str());
            hub.prefs.edit_preferences(move |snap| {
                snap.recent_files.retain(|p| *p != path);
                crate::preferences_bus::PreferenceEdit::RecentFiles
            });
            hub.refresh_home();
        });
    }
    {
        let hub = hub.clone();
        ui.on_export_to_drive_requested(move |i| {
            let root = hub.drives.borrow().get(i as usize).map(|d| d.root.clone());
            let (Some(root), Some(ui)) = (root, hub.ui.upgrade()) else {
                return;
            };
            let kind = crate::wiring::export_kind_for_format(ui.get_export_format());
            let hub = hub.clone();
            if let Err(e) = slint::spawn_local(async move {
                crate::wiring::run_export_sliced_flow(hub.doc(), hub.ctrl.dialog_manager(), kind, Some(root))
                    .await;
                hub.refresh_history();
            }) {
                eprintln!("export to drive: {e}");
            }
        });
    }
    ui.on_post_export_command_edited(|text| {
        SendSettings {
            post_export_command: text.to_string(),
        }
        .save();
    });
    {
        let hub = hub.clone();
        ui.on_history_outcome_changed(move |row, outcome| {
            hub.edit_history(row, |e| {
                e.outcome = Outcome::from_index(outcome);
                if e.outcome != Outcome::Failed {
                    e.failed_layer = None;
                }
            });
        });
    }
    {
        let hub = hub.clone();
        ui.on_history_failed_layer_changed(move |row, text| {
            let layer = text.trim().parse::<u32>().ok().filter(|&l| l > 0);
            hub.edit_history(row, |e| e.failed_layer = layer);
        });
    }
    {
        let hub = hub.clone();
        ui.on_calibrate_estimate_requested(move || {
            let hub = hub.clone();
            if let Err(e) = slint::spawn_local(async move { hub.calibrate_estimate().await }) {
                eprintln!("calibrate estimate: {e}");
            }
        });
    }
    {
        let hub = hub.clone();
        ui.on_reset_estimate_requested(move || {
            if let Some(printer) = hub.current_printer() {
                let _ = crate::estimate_calibration::save(printer.as_str(), None);
            }
            hub.refresh(PAGE_DEVICE);
        });
    }
}

/// Opens the Home tab (startup without a file to open).
pub(crate) fn show_home(ui: &MainWindow) {
    ui.set_hub_page(PAGE_HOME);
    ui.invoke_hub_page_opened(PAGE_HOME);
}

impl Hub {
    fn doc(&self) -> Rc<SceneDocument> {
        self.ctrl.scene_doc()
    }

    fn current_printer(&self) -> Option<sc_print_primitives::PrinterId> {
        let doc = self.doc();
        doc.chosen_printer()
            .or_else(|| self.prefs.current_values().default_printer)
    }

    fn refresh(&self, page: i32) {
        match page {
            PAGE_HOME => self.refresh_home(),
            PAGE_DEVICE => {
                self.refresh_device();
                self.refresh_drives();
                self.refresh_history();
            }
            PAGE_PROJECT => self.refresh_project(),
            _ => {}
        }
    }

    fn refresh_home(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let offset = print_history::local_offset_secs();
        let rows: Vec<RecentProject> = ui
            .get_recent_files()
            .iter()
            .map(|path| {
                let path = PathBuf::from(path.as_str());
                let modified = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| print_history::format_time(d.as_secs(), offset))
                    .unwrap_or_default();
                let thumbnail = crate::thumbnails::load(&path);
                RecentProject {
                    name: path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                        .into(),
                    folder: path
                        .parent()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default()
                        .into(),
                    missing: !path.exists(),
                    modified: modified.into(),
                    thumbnail: thumbnail.clone().unwrap_or_default(),
                    has_thumbnail: thumbnail.is_some(),
                }
            })
            .collect();
        ui.set_home_recent(ModelRc::new(VecModel::from(rows)));
    }

    fn refresh_device(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let doc = self.doc();
        let snapshot = self.prefs.current_values();
        let printer = doc.chosen_printer().or(snapshot.default_printer);
        let resin = doc.chosen_resin().or(snapshot.default_resin);
        let profiles = printer
            .as_ref()
            .zip(resin.as_ref())
            .and_then(|(p, r)| crate::slice_projection::load_chosen_profiles(p, r).ok());
        if let Some((printer, material)) = &profiles {
            let id = &printer.identity;
            let h = &printer.hardware;
            let v = h.build_volume();
            let px = h.pixel_resolution();
            let pitch = h.pixel_size_microns();
            let m = h.mirror();
            ui.set_device_printer_name(id.name.to_string().into());
            ui.set_device_printer_model(
                crate::i18n::tr!("{} · {}", id.manufacturer.to_string(), id.family.to_string()),
            );
            ui.set_device_build_volume(
                {
                    use crate::slice_panel::short_number as n;
                    format!("{} x {} x {} mm", n(v.x_mm()), n(v.y_mm()), n(v.z_mm())).into()
                },
            );
            ui.set_device_resolution(
                {
                    use crate::slice_panel::short_number as n;
                    format!(
                        "{} x {} px · {} x {} µm",
                        px.x(),
                        px.y(),
                        n(f64::from(pitch.x)),
                        n(f64::from(pitch.y))
                    )
                    .into()
                },
            );
            ui.set_device_output(match (m.x, m.y) {
                (false, false) => crate::i18n::tr!("PNG layers + run.gcode, not mirrored"),
                (true, false) => crate::i18n::tr!("PNG layers + run.gcode, mirrored in X"),
                (false, true) => crate::i18n::tr!("PNG layers + run.gcode, mirrored in Y"),
                (true, true) => crate::i18n::tr!("PNG layers + run.gcode, mirrored in X and Y"),
            });
            ui.set_device_material(material.identity().name.to_string().into());
        } else {
            ui.set_device_printer_name(crate::i18n::tr!("No printer selected"));
            ui.set_device_printer_model(SharedString::default());
        }
        let correction = printer
            .as_ref()
            .map(|p| crate::estimate_calibration::for_printer(p.as_str()));
        ui.set_estimate_correction(match correction {
            Some(c) if c.samples > 0 => crate::i18n::tr!(
                "Time estimate calibrated from {} real layers: motion x{:.2}, {:+.1} s per layer.",
                c.samples,
                c.motion_scale,
                c.layer_overhead_s
            ),
            _ => SharedString::default(),
        });
    }

    fn refresh_drives(&self) {
        let drives = send_to_printer::removable_drives();
        if let Some(ui) = self.ui.upgrade() {
            let rows: Vec<DriveRow> = drives
                .iter()
                .map(|d| DriveRow {
                    label: d.label.clone().into(),
                })
                .collect();
            ui.set_device_drives(ModelRc::new(VecModel::from(rows)));
        }
        *self.drives.borrow_mut() = drives;
    }

    /// History rows newest first; row `i` is entry `len - 1 - i`.
    fn refresh_history(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let entries = print_history::history_path()
            .map(|p| print_history::load(&p))
            .unwrap_or_default();
        let offset = print_history::local_offset_secs();
        let rows: Vec<HistoryRow> = entries
            .iter()
            .rev()
            .map(|e| HistoryRow {
                when: print_history::format_time(e.exported_at, offset).into(),
                job: e
                    .file
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
                    .into(),
                printer: e.printer.clone().into(),
                material: e.material.clone().into(),
                layers: e.layers as i32,
                estimate: e.estimate.clone().into(),
                outcome: e.outcome.index(),
                failed_layer: e.failed_layer.map(|l| l.to_string()).unwrap_or_default().into(),
            })
            .collect();
        ui.set_print_history(ModelRc::new(VecModel::from(rows)));
    }

    fn edit_history(&self, row: i32, edit: impl FnOnce(&mut Entry)) {
        let Some(path) = print_history::history_path() else {
            return;
        };
        let mut entries = print_history::load(&path);
        let Some(index) = entries.len().checked_sub(1 + row as usize) else {
            return;
        };
        edit(&mut entries[index]);
        if let Err(e) = print_history::save_all(&path, &entries) {
            eprintln!("print history: {e}");
        }
        self.refresh_history();
    }

    fn refresh_project(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let doc = self.doc();
        let path = doc.file_path();
        ui.set_project_name(
            path.as_ref()
                .and_then(|p| p.file_stem())
                .map_or_else(|| crate::i18n::tr!("Untitled project").to_string(), |s| s.to_string_lossy().into_owned())
                .into(),
        );
        ui.set_project_path(path.map(|p| p.display().to_string()).unwrap_or_default().into());
        ui.set_project_dirty(doc.has_unsaved_changes());
        let slices = doc.all_plate_slices();
        let active = doc.active_plate();
        let mut total_time = 0.0;
        let mut sliced = 0;
        let rows: Vec<PlateRow> = doc
            .read_scene(|s| {
                let graph = s.graph();
                graph
                    .plates()
                    .order
                    .iter()
                    .enumerate()
                    .map(|(i, plate)| {
                        // Plate membership is kept per leaf (mesh); count the
                        // models on the plate, not the supports generated for them.
                        let objects = s
                            .leaf_ids()
                            .into_iter()
                            .filter(|&leaf| {
                                graph.plate_of(leaf) == plate.id
                                    && !graph
                                        .node(leaf)
                                        .and_then(|n| n.meta.as_ref())
                                        .and_then(|m| m.comment.as_deref())
                                        .is_some_and(|c| c.starts_with("sc-generated-support"))
                            })
                            .count();
                        let name = plate
                            .settings
                            .as_deref()
                            .and_then(|s| sc_print_profiles::scene_overrides::PlateSettings::parse(s).ok())
                            .and_then(|s| s.name)
                            .unwrap_or_else(|| crate::i18n::tr!("Plate {}", i + 1).to_string());
                        let slice = slices.iter().find(|(n, _)| *n == i + 1).map(|(_, r)| r);
                        let (layers, time) = slice
                            .map(|r| {
                                let seconds = plate_schedule(&doc, plate.id, &r.params, &r.schedule)
                                    .map(|s| estimate_seconds(&r.params.printer, &s));
                                if let Some(s) = seconds {
                                    total_time += s;
                                }
                                (
                                    r.merged.layers().len() as i32,
                                    seconds.map(sc_print_job::format_duration).unwrap_or_default(),
                                )
                            })
                            .unwrap_or((0, String::new()));
                        if slice.is_some() {
                            sliced += 1;
                        }
                        PlateRow {
                            number: i as i32 + 1,
                            name: name.into(),
                            objects: objects as i32,
                            status: if objects == 0 {
                                crate::i18n::tr!("Empty")
                            } else if slice.is_some() {
                                crate::i18n::tr!("Sliced")
                            } else {
                                crate::i18n::tr!("Not sliced")
                            },
                            stale: objects > 0 && slice.is_none(),
                            layers,
                            time: time.into(),
                            active: plate.id == active,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let objects: i32 = rows.iter().map(|r| r.objects).sum();
        let counts = format!(
            "{} \u{b7} {}",
            count_label(rows.len(), "plate", "plates"),
            count_label(objects.max(0) as usize, "object", "objects")
        );
        ui.set_project_totals(if sliced > 0 {
            crate::i18n::tr!(
                "{} \u{b7} {} sliced \u{b7} {} printing",
                counts,
                sliced,
                sc_print_job::format_duration(total_time)
            )
        } else {
            counts.into()
        });
        ui.set_project_plates(ModelRc::new(VecModel::from(rows)));
    }

    async fn calibrate_estimate(self: Rc<Self>) {
        let doc = self.doc();
        let dialogs = self.ctrl.dialog_manager();
        let Some((printer, schedule)) = held_schedule(&doc) else {
            dialogs.show_notice(
                &crate::i18n::tr!("Calibrate time estimate"),
                crate::i18n::tr!(
                    "Open and slice the job you printed first, then import its print log. The log is compared layer by layer with this slice."
                )
                .to_string(),
                String::new(),
            );
            return;
        };
        let Some(file) = rfd::AsyncFileDialog::new()
            .set_title(crate::i18n::tr!("Print log of this job").as_str())
            .add_filter(crate::i18n::tr!("Print logs").to_string(), &["log", "txt", "csv"])
            .add_filter(crate::i18n::tr!("All files").to_string(), &["*"])
            .pick_file()
            .await
        else {
            return;
        };
        let text = std::fs::read_to_string(file.path()).unwrap_or_default();
        let measured = crate::estimate_calibration::measured_durations(
            &crate::estimate_calibration::parse_log(&text),
        );
        let Some(fit) = crate::estimate_calibration::fit(&schedule, &measured) else {
            dialogs.show_notice(
                &crate::i18n::tr!("Calibrate time estimate"),
                crate::i18n::tr!(
                    "Found {} usable layer times in that file; at least 5 consecutive layers are needed. Each line should hold a layer number and a time, e.g. \"12, 00:05:45\".",
                    measured.len()
                )
                .to_string(),
                String::new(),
            );
            return;
        };
        let before = estimate_seconds(&printer, &schedule);
        let real: f64 = measured.iter().map(|m| m.1).sum();
        let modelled: f64 = measured
            .iter()
            .filter_map(|&(i, _)| schedule.layers.get(i))
            .map(|l| fit.layer_seconds(l.exposure_s, l.duration_s()))
            .sum();
        let choice = rfd::AsyncMessageDialog::new()
            .set_title(crate::i18n::tr!("Calibrate time estimate").as_str())
            .set_description(
                crate::i18n::tr!(
                    "{} layers measured: {} real against {} with the fitted model.\n\nMotion x{:.2}, {:+.1} s per layer.\nThis job's estimate becomes {} (was {}).\n\nSave this correction for {}?",
                    fit.samples,
                    sc_print_job::format_duration(real),
                    sc_print_job::format_duration(modelled),
                    fit.motion_scale,
                    fit.layer_overhead_s,
                    sc_print_job::format_duration(fit.total_s(&schedule)),
                    sc_print_job::format_duration(before),
                    printer.as_str()
                )
                .as_str(),
            )
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            .await;
        if choice != rfd::MessageDialogResult::Yes {
            return;
        }
        if let Err(e) = crate::estimate_calibration::save(printer.as_str(), Some(fit)) {
            eprintln!("estimate calibration: {e}");
        }
        if let Some(ui) = self.ui.upgrade() {
            ui.set_estimated_print_time_text(
                sc_print_job::format_duration(fit.total_s(&schedule)).into(),
            );
        }
        self.refresh(PAGE_DEVICE);
    }
}

/// "1 plate" / "3 plates".
pub(crate) fn count_label(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

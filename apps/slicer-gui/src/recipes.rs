// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Recipe editor and cancellable worker; live document commits exactly once.
use crate::{MainWindow, scene_document::SceneDocument};
use sc_print_profiles::recipes::{self, Recipe, Step};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc};

/// Edit the typed document transactionally: invalid fields never replace it.
pub(crate) fn edit_field(
    text: &str,
    index: usize,
    key: &str,
    value: &str,
) -> Result<String, String> {
    let mut document: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut field = document
        .get_mut("steps")
        .and_then(|v| v.as_array_mut())
        .and_then(|s| s.get_mut(index))
        .ok_or_else(|| crate::i18n::tr!("Choose a step").to_string())?;
    if key == "preset" {
        let table = field
            .as_table_mut()
            .ok_or_else(|| crate::i18n::tr!("Missing step").to_string())?;
        if value == "custom" {
            table.remove("preset");
        } else {
            table.insert("preset".into(), toml::Value::String(value.into()));
        }
        return Recipe::parse(&toml::to_string(&document).map_err(|e| e.to_string())?)?.serialize();
    }
    for part in key.split('.') {
        field = field
            .get_mut(part)
            .ok_or_else(|| crate::i18n::tr!("Unknown field").to_string())?;
    }
    *field = match field {
        toml::Value::Boolean(_) => toml::Value::Boolean(
            value
                .parse()
                .map_err(|_| crate::i18n::tr!("Expected a checkbox value").to_string())?,
        ),
        toml::Value::Integer(_) => toml::Value::Integer(
            value
                .parse()
                .map_err(|_| crate::i18n::tr!("Expected an integer").to_string())?,
        ),
        toml::Value::Float(_) => toml::Value::Float(
            value
                .parse()
                .map_err(|_| crate::i18n::tr!("Expected a number").to_string())?,
        ),
        toml::Value::String(_) => toml::Value::String(value.to_owned()),
        _ => return Err(crate::i18n::tr!("Unsupported field").to_string()),
    };
    Recipe::parse(&toml::to_string(&document).map_err(|e| e.to_string())?)?.serialize()
}

pub(crate) fn sync_form(ui: &MainWindow) -> Result<(), String> {
    let recipe = Recipe::parse(ui.get_recipe_toml().as_str())?;
    let index = (ui.get_recipe_step_index().max(0) as usize).min(recipe.steps.len() - 1);
    ui.set_recipe_step_index(index as i32);
    let labels = [
        crate::i18n::tr!("Scale"),
        crate::i18n::tr!("Auto orient"),
        crate::i18n::tr!("Hollow"),
        crate::i18n::tr!("Drain holes"),
        crate::i18n::tr!("Supports"),
        crate::i18n::tr!("Arrange"),
        crate::i18n::tr!("Slice"),
        crate::i18n::tr!("Export"),
    ];
    let kind = |s: &Step| match s {
        Step::Scale { .. } => 0,
        Step::AutoOrient { .. } => 1,
        Step::Hollow { .. } => 2,
        Step::DrainHoles { .. } => 3,
        Step::Supports { .. } => 4,
        Step::Arrange => 5,
        Step::Slice => 6,
        Step::Export { .. } => 7,
    };
    let icons = [
        "\u{2197}", "\u{21bb}", "\u{25a1}", "\u{25cb}", "\u{25b3}", "\u{25a6}", "\u{2261}",
        "\u{2191}",
    ];
    ui.set_recipe_steps(ModelRc::new(VecModel::from(
        recipe
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| format!("{}  {}. {}", icons[kind(s)], i + 1, labels[kind(s)]).into())
            .collect::<Vec<slint::SharedString>>(),
    )));
    let document: toml::Value =
        toml::from_str(ui.get_recipe_toml().as_str()).map_err(|e| e.to_string())?;
    let table = document["steps"][index]
        .as_table()
        .ok_or_else(|| crate::i18n::tr!("Missing step").to_string())?;
    let mut fields = Vec::new();
    fn append(fields: &mut Vec<crate::RecipeField>, key: String, name: &str, value: &toml::Value) {
        if let Some(table) = value.as_table() {
            for (name, value) in table {
                append(fields, format!("{key}.{name}"), name, value);
            }
            return;
        }
        if name == "kind" {
            return;
        }
        let (kind, text) = match value {
            toml::Value::Boolean(v) => (1, v.to_string()),
            toml::Value::String(v) => (2, v.clone()),
            _ => (0, value.to_string()),
        };
        let choices: Vec<slint::SharedString> = match name {
            "format" => vec![
                "gcode-zip",
                "3mf",
                "png-per-layer",
                "svg-per-layer",
                "svg-combined",
            ],
            "preset"
                if value.as_str().is_some_and(|v| {
                    [
                        "balanced",
                        "fewest_supports",
                        "fastest",
                        "best_quality",
                        "most_per_plate",
                    ]
                    .contains(&v)
                }) =>
            {
                vec![
                    "balanced",
                    "fewest_supports",
                    "fastest",
                    "best_quality",
                    "most_per_plate",
                ]
            }
            "preset" => vec!["custom", "light", "medium", "heavy"],
            _ => vec![],
        }
        .into_iter()
        .map(Into::into)
        .collect();
        let label = field_label(name);
        fields.push(crate::RecipeField {
            key: key.into(),
            label,
            value: text.into(),
            kind: if choices.is_empty() { kind } else { 3 },
            choices: ModelRc::new(VecModel::from(choices)),
        });
    }
    for (name, value) in table {
        append(&mut fields, name.clone(), name, value);
    }
    if matches!(recipe.steps[index], Step::Supports { preset: None, .. }) {
        append(
            &mut fields,
            "preset".into(),
            "preset",
            &toml::Value::String("custom".into()),
        );
    }
    ui.set_recipe_fields(ModelRc::new(VecModel::from(fields)));
    Ok(())
}

fn field_label(name: &str) -> slint::SharedString {
    match name {
        "percent" => crate::i18n::tr!("Scale (%)"),
        "wall_mm" => crate::i18n::tr!("Wall thickness (mm)"),
        "count" => crate::i18n::tr!("Hole count"),
        "diameter_mm" => crate::i18n::tr!("Hole diameter (mm)"),
        "format" => crate::i18n::tr!("Format"),
        "folder" => crate::i18n::tr!("Output folder"),
        "name_pattern" => crate::i18n::tr!("Name pattern"),
        "preset" => crate::i18n::tr!("Preset"),
        "overhang_angle_deg" => crate::i18n::tr!("Overhang angle (deg)"),
        "spacing_mm" => crate::i18n::tr!("Support spacing (mm)"),
        "tip_contact_diameter_mm" => crate::i18n::tr!("Contact diameter (mm)"),
        "tip_length_mm" => crate::i18n::tr!("Tip length (mm)"),
        "shaft_diameter_mm" => crate::i18n::tr!("Shaft diameter (mm)"),
        "base_diameter_mm" => crate::i18n::tr!("Base diameter (mm)"),
        "base_height_mm" => crate::i18n::tr!("Base height (mm)"),
        "raft" => crate::i18n::tr!("Generate raft"),
        "raft_thickness_mm" => crate::i18n::tr!("Raft thickness (mm)"),
        "raft_margin_mm" => crate::i18n::tr!("Raft margin (mm)"),
        "lift_mm" => crate::i18n::tr!("Model lift (mm)"),
        "contact_depth_mm" => crate::i18n::tr!("Contact depth (mm)"),
        "lattice_raft" => crate::i18n::tr!("Lattice raft"),
        "skate_raft" => crate::i18n::tr!("Skate raft"),
        "min_column_diameter_mm" => crate::i18n::tr!("Minimum column diameter (mm)"),
        "max_column_diameter_mm" => crate::i18n::tr!("Maximum column diameter (mm)"),
        "symmetric_supports" => crate::i18n::tr!("Symmetric supports"),
        "tree_supports" => crate::i18n::tr!("Tree supports"),
        "branch_angle_deg" => crate::i18n::tr!("Branch angle (deg)"),
        _ => name.into(),
    }
}

pub(crate) fn save_copy(dir: &std::path::Path, mut recipe: Recipe) -> Result<usize, String> {
    let entries = recipes::list(dir)?;
    let original = recipe.name.clone();
    let mut n = 1;
    while entries.iter().any(|(_, r)| r.name == recipe.name) {
        recipe.name = format!("{original} (copy {n})");
        n += 1;
    }
    let path = recipes::save_new(dir, &recipe)?;
    recipes::list(dir)?
        .iter()
        .position(|(p, _)| p.as_ref() == Some(&path))
        .ok_or_else(|| "Saved recipe is missing from the library".into())
}

pub(crate) fn refresh(ui: &MainWindow) -> Result<(), String> {
    let entries = recipes::list(&recipes::user_dir()?)?;
    ui.set_recipe_names(ModelRc::new(VecModel::from(
        entries
            .iter()
            .map(|(_, r)| r.name.clone().into())
            .collect::<Vec<_>>(),
    )));
    let index = (ui.get_recipe_index().max(0) as usize).min(entries.len().saturating_sub(1));
    ui.set_recipe_index(index as i32);
    if let Some((_, recipe)) = entries.get(index) {
        ui.set_recipe_toml(recipe.serialize()?.into());
    }
    sync_form(ui)
}

pub(crate) fn install(
    ui: &MainWindow,
    document: Rc<SceneDocument>,
    preferences: Rc<crate::preferences_bus::PreferenceHub>,
    slice_run: Rc<crate::slice_run::SliceActionDriver>,
) {
    let weak = ui.as_weak();
    ui.on_recipe_sync_form(move || {
        if let Some(ui) = weak.upgrade()
            && let Err(e) = sync_form(&ui)
        {
            ui.set_recipe_summary(e.into());
        }
    });
    let weak = ui.as_weak();
    ui.on_recipe_select_step(move |index| {
        if let Some(ui) = weak.upgrade() {
            ui.set_recipe_step_index(index);
            let _ = sync_form(&ui);
        }
    });
    let weak = ui.as_weak();
    ui.on_recipe_field_edit(move |key, value| {
        if let Some(ui) = weak.upgrade() {
            match edit_field(
                ui.get_recipe_toml().as_str(),
                ui.get_recipe_step_index().max(0) as usize,
                key.as_str(),
                value.as_str(),
            ) {
                Ok(text) => {
                    ui.set_recipe_toml(text.into());
                    ui.set_recipe_summary("".into());
                    let _ = sync_form(&ui);
                }
                Err(e) => ui.set_recipe_summary(e.into()),
            }
        }
    });
    let cancel = Rc::new(RefCell::new(None::<sc_sl1cer::CancelToken>));
    let weak = ui.as_weak();
    ui.on_recipes_requested(move || {
        if let Some(ui) = weak.upgrade() {
            if let Err(e) = refresh(&ui) {
                ui.set_recipe_summary(e.into());
            }
            ui.set_recipes_visible(true);
        }
    });
    let weak = ui.as_weak();
    ui.on_recipe_selected(move |_| {
        if let Some(ui) = weak.upgrade()
            && let Err(e) = refresh(&ui)
        {
            ui.set_recipe_summary(e.into());
        }
    });
    let weak = ui.as_weak();
    ui.on_recipe_edit(move |action| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let result = (|| {
            let dir = recipes::user_dir()?;
            if action == 1 {
                let entries = recipes::list(&dir)?;
                let path = entries
                    .get(ui.get_recipe_index().max(0) as usize)
                    .and_then(|(p, _)| p.as_ref())
                    .ok_or("Bundled defaults cannot be deleted; save a copy first")?;
                std::fs::remove_file(path).map_err(|e| e.to_string())?;
                return refresh(&ui);
            }
            let mut recipe = Recipe::parse(ui.get_recipe_toml().as_str())?;
            let index = ui.get_recipe_step_index().max(0) as usize;
            match action {
                0 => {
                    ui.set_recipe_index(save_copy(&dir, recipe)? as i32);
                    return refresh(&ui);
                }
                6 => {
                    let entries = recipes::list(&dir)?;
                    let path = entries
                        .get(ui.get_recipe_index().max(0) as usize)
                        .and_then(|(p, _)| p.as_ref())
                        .ok_or("Bundled defaults cannot be overwritten; save a copy first")?;
                    std::fs::write(path, recipe.serialize()?).map_err(|e| e.to_string())?;
                    return refresh(&ui);
                }
                2 => recipe.steps.push(match ui.get_recipe_step_kind() {
                    0 => Step::Scale { percent: 100.0 },
                    1 => Step::AutoOrient {
                        preset: Default::default(),
                    },
                    2 => Step::Hollow { wall_mm: 2.0 },
                    3 => Step::DrainHoles {
                        count: 2,
                        diameter_mm: 3.0,
                    },
                    4 => Step::Supports {
                        preset: None,
                        settings: Default::default(),
                    },
                    5 => Step::Arrange,
                    6 => Step::Slice,
                    _ => Step::Export {
                        format: "gcode-zip".into(),
                        folder: ".".into(),
                        name_pattern: "{name}_plate{plate}.zip".into(),
                    },
                }),
                3 if index > 0 && index < recipe.steps.len() => recipe.steps.swap(index, index - 1),
                4 if index + 1 < recipe.steps.len() => recipe.steps.swap(index, index + 1),
                5 if recipe.steps.len() > 1 && index < recipe.steps.len() => {
                    recipe.steps.remove(index);
                }
                _ => return Err("Choose a valid step number".into()),
            }
            ui.set_recipe_step_index(match action {
                2 => recipe.steps.len() - 1,
                3 => index - 1,
                4 => index + 1,
                _ => index.min(recipe.steps.len() - 1),
            } as i32);
            ui.set_recipe_toml(recipe.serialize()?.into());
            sync_form(&ui)
        })();
        ui.set_recipe_summary(result.err().unwrap_or_default().into());
    });
    let weak = ui.as_weak();
    let cancellation = cancel.clone();
    ui.on_recipe_cancel(move || {
        if let Some(token) = cancellation.borrow().as_ref() {
            token.cancel();
        } else if let Some(ui) = weak.upgrade() {
            ui.set_recipes_visible(false);
        }
    });
    let weak = ui.as_weak();
    ui.on_recipe_run(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_recipe_busy() {
            return;
        }
        let setup = (|| {
            let recipe = Recipe::parse(ui.get_recipe_toml().as_str())?;
            let scene = document
                .read_scene(|s| s.graph().clone())
                .ok_or("Open a model first")?;
            let snapshot = preferences.current_values();
            let (printer, material) = document
                .chosen_printer()
                .or(snapshot.default_printer)
                .zip(document.chosen_resin().or(snapshot.default_resin))
                .ok_or("Select a printer and resin first")
                .and_then(|(p, m)| {
                    crate::slice_projection::load_chosen_profiles(&p, &m)
                        .map_err(|_| "Could not load profiles")
                })?;
            let selected = document.chosen().ids();
            let targets = scene
                .reachable_leaves(sc_scene::CsgPolicy::Refuse, sc_scene::ReferencePolicy::Skip)
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter(|id| match ui.get_recipe_scope() {
                    0 => selected.contains(id),
                    1 => scene.plate_of(*id) == document.active_plate(),
                    _ => true,
                })
                .collect::<Vec<_>>();
            let params = sc_print_glue::SlicingParams {
                process_overrides: *document.process_overrides.borrow(),
                layer_height_request: None,
                first_layer_height: document.chosen_first_layer(),
                aa_level: document.scene_aa.get().unwrap_or(snapshot.aa_level),
            };
            Ok::<_, String>((recipe, scene, targets, printer, material, params))
        })();
        let job_name = document
            .read_file_path(|p| {
                p.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
            .unwrap_or_else(|| "Untitled".into());
        let (recipe, scene, targets, printer, material, params) = match setup {
            Ok(s) => s,
            Err(e) => {
                ui.set_recipe_summary(e.into());
                return;
            }
        };
        let rows = recipe
            .steps
            .iter()
            .map(|s| format!("{} — pending", s.name()).into())
            .collect::<Vec<slint::SharedString>>();
        ui.set_recipe_progress(ModelRc::new(VecModel::from(rows)));
        ui.set_recipe_summary(crate::i18n::tr!("Running recipe..."));
        ui.set_recipe_busy(true);
        let revision = document.revision();
        let token = sc_sl1cer::CancelToken::new();
        *cancel.borrow_mut() = Some(token.clone());
        let progress = weak.clone();
        let publish_params = crate::slice_result::SliceParams {
            printer: printer.identity.id.clone(),
            resin: material.identity().id.clone(),
            first_layer_height: params.first_layer_height,
            aa_level: params.aa_level,
            geometry: revision,
        };
        let minimum = ui.get_minimum_feature_mm().parse().unwrap_or(0.3);
        let (tx, rx) = futures_channel::oneshot::channel();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::recipe_service::run(
                    &scene,
                    &targets,
                    &recipe,
                    &crate::recipe_service::Context {
                        printer: &printer,
                        material: &material,
                        params,
                        job_name: &job_name,
                    },
                    &token,
                    |i, status| {
                        let status = status.to_string();
                        let _ = progress.clone().upgrade_in_event_loop(move |ui| {
                            if let Some(rows) = ui
                                .get_recipe_progress()
                                .as_any()
                                .downcast_ref::<VecModel<slint::SharedString>>()
                            {
                                rows.set_row_data(i, status.into());
                            }
                        });
                    },
                )
                .and_then(|mut prepared| {
                    let outputs = std::mem::take(&mut prepared.slices);
                    let mut completed = Vec::new();
                    for (plate, slices) in outputs {
                        let scene = prepared
                            .scene
                            .plate_scene(plate)
                            .map_err(|e| e.to_string())?;
                        completed.push((
                            plate,
                            crate::slice_run::capture_recipe(&scene, slices, minimum)?,
                        ));
                    }
                    if token.is_cancelled() {
                        return Err("Recipe cancelled; no scene changes applied".into());
                    }
                    Ok((prepared, completed))
                })
            }))
            .unwrap_or_else(|e| Err(crate::mesh_loader::panic_text(&e)));
            let _ = tx.send(result);
        });
        let document = document.clone();
        let weak = weak.clone();
        let cancel = cancel.clone();
        let slice_run = slice_run.clone();
        let _ = slint::spawn_local(async move {
            let result = rx.await;
            let Some(ui) = weak.upgrade() else {
                return;
            };
            ui.set_recipe_busy(false);
            cancel.borrow_mut().take();
            let summary = match result {
                Ok(Ok((prepared, completed))) if revision == document.revision() => {
                    debug_assert_eq!(prepared.slots, prepared.scene.nodes_len());
                    document
                        .apply_batch(prepared.edit, prepared.slots)
                        .map(|_| {
                            let active = document.active_plate();
                            for (plate, slices) in completed {
                                slice_run.accept_recipe(plate, slices, publish_params.clone());
                            }
                            document.set_active_plate(active);
                            prepared.summary
                        })
                }
                Ok(Ok(_)) => Err("The project changed; recipe edits were discarded".into()),
                Ok(Err(e)) => Err(e),
                Err(e) => Err(e.to_string()),
            };
            ui.set_recipe_summary(summary.unwrap_or_else(|e| e).into());
        });
    });
}

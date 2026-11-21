// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Cancellable batch preparation. Work is staged off-thread and committed as
//! one compound edit only if the document still matches the captured revision.
use crate::{MainWindow, build_volume::BuildVolume, scene_document::SceneDocument};
use sc_scene::{CsgPolicy, ReferencePolicy, Scene, node::LeafAttachment};
use sc_scene_edit::SceneEdit;
use sc_sl1cer::CancelToken;
use slint::ComponentHandle;
use std::{cell::RefCell, path::PathBuf, rc::Rc, sync::Arc};

#[derive(Clone)]
pub(crate) struct Options {
    pub recipe: Option<sc_print_profiles::recipes::Recipe>,
    pub orient: bool,
    pub orient_weights: [f64; 4],
    pub orient_convenience_rate: f64,
    pub arrange_gap_mm: f64,
    pub hollow: bool,
    pub supports: bool,
    pub wall_mm: f64,
    pub support: sc_print_profiles::support_settings::SupportSettings,
    pub slicing: sc_print_glue::SlicingParams,
}

pub(crate) struct Prepared {
    pub edit: SceneEdit,
    pub node_slots: usize,
    pub imported: usize,
    pub failures: Vec<String>,
    pub notices: Vec<String>,
}

/// Import already prepared sources with stable owner remapping, then overflow
/// arrange the entire batch. No live document is mutated by the planner.
pub(crate) fn plan(
    mut base: Scene,
    sources: &[Scene],
    volume: BuildVolume,
) -> Result<Prepared, String> {
    let mut edits = Vec::new();
    let mut imported = 0;
    for source in sources {
        let ids = source
            .reachable_leaves(CsgPolicy::Descend, ReferencePolicy::Skip)
            .map_err(|e| e.to_string())?;
        let mapped: std::collections::BTreeMap<_, _> = ids
            .iter()
            .map(|id| (*id, base.allocate_unbound_node_id()))
            .collect();
        for id in ids {
            let mut meta = source.node(id).and_then(|n| n.meta.clone());
            if let Some(comment) = meta.as_mut().and_then(|m| m.comment.as_mut()) {
                if comment == "sc-generated-support-raft"
                    && let Some(owner) = crate::scene_document::support_owner(source, id)
                {
                    comment.push_str(&format!("\nowner = {}\n", owner.get()));
                }

                let owner = comment
                    .lines()
                    .find_map(|l| l.strip_prefix("owner = ")?.parse::<u32>().ok());
                if let Some((_, new)) = mapped.iter().find(|(old, _)| Some(old.get()) == owner) {
                    *comment = comment
                        .lines()
                        .map(|l| {
                            if l.starts_with("owner = ") {
                                format!("owner = {}", new.get())
                            } else {
                                l.to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                }
            }
            let edit = SceneEdit::AddLeaf(sc_scene_edit::LeafRecord {
                node_id: mapped[&id],
                attachment: LeafAttachment::Root {
                    position: base.roots().len(),
                },
                payload: sc_scene::LeafPayload::Mesh(Arc::clone(
                    source.leaf_mesh_arc(id).ok_or("Missing mesh")?,
                )),
                local_transform: source.world_transform_of(id).map_err(|e| e.to_string())?,
                meta,
            });
            edit.apply_to_scene(&mut base).map_err(|e| e.to_string())?;
            edits.push(edit);
        }
        imported += 1;
    }
    let arrangement = crate::layout_tools::arrange_all(&base, volume, 3.0)
        .map_err(|ids| format!("{} objects exceed the build volume", ids.len()))?;
    edits.push(arrangement);
    Ok(Prepared {
        edit: SceneEdit::Compound(edits),
        node_slots: base.nodes_len(),
        imported,
        failures: Vec::new(),
        notices: Vec::new(),
    })
}

fn prepare_file(
    path: &std::path::Path,
    options: &Options,
    token: &CancelToken,
    profiles: Option<&(
        sc_print_profiles::PrinterProfile,
        sc_print_profiles::EffectiveMaterial,
    )>,
) -> Result<Scene, String> {
    let mut scene = sc_project_formats::load_scene(
        path,
        Some(sc_mesh_formats::DEFAULT_MAX_MESH_FILE_SIZE_BYTES),
        |_| {},
    )
    .map_err(|e| e.to_string())?
    .scene;
    crate::mesh_loader::name_part_after_file(&mut scene, path);
    repair_source(&mut scene)?;
    if let Some(recipe) = &options.recipe {
        let (printer, material) = profiles.ok_or("Select a printer and resin first")?;
        let ids = scene
            .reachable_leaves(CsgPolicy::Refuse, ReferencePolicy::Skip)
            .map_err(|e| e.to_string())?;
        let prepared = crate::recipe_service::run(
            &scene,
            &ids,
            recipe,
            &crate::recipe_service::Context {
                printer,
                material,
                params: options.slicing,
                job_name: &path.file_stem().unwrap_or_default().to_string_lossy(),
            },
            token,
            |_, _| {},
        )?;
        if !prepared.summary.starts_with("Recipe complete") {
            return Err(prepared.summary);
        }
        return Ok(prepared.scene);
    }

    let leaves = scene
        .reachable_leaves(CsgPolicy::Refuse, ReferencePolicy::Skip)
        .map_err(|e| e.to_string())?;
    for id in leaves {
        if options.orient {
            let edit = crate::layout_tools::auto_orient(
                &scene,
                id,
                options.support.overhang_angle_deg,
                {
                    let v = profiles
                        .ok_or("Select a printer first")?
                        .0
                        .hardware
                        .build_volume();
                    BuildVolume::new(v.x_mm() as f32, v.y_mm() as f32, v.z_mm() as f32)
                },
                options.orient_weights,
                {
                    let (p, m) = profiles.ok_or("Select a printer and resin first")?;
                    let mut context = sc_print_glue::orient::Context::for_profiles(
                        p,
                        m,
                        &options.slicing.process_overrides,
                    )
                    .map_err(|e| e.to_string())?;
                    context.aa = options.slicing.aa_level;
                    context.first_layer_height_mm = options
                        .slicing
                        .first_layer_height
                        .map_or(0.0, |v| v.value());
                    context.supports = options.supports;
                    context.support = options.support;
                    context.convenience_rate = options.orient_convenience_rate;
                    context.gap_mm = options.arrange_gap_mm;
                    context
                },
            )
            .ok_or("Could not orient model")?;
            edit.apply_to_scene(&mut scene).map_err(|e| e.to_string())?;
        }
        if options.hollow {
            let edit = crate::hollow_tools::edit(
                &scene,
                id,
                options.wall_mm,
                crate::hollow_tools::HoleChange::Keep,
            )?;
            edit.apply_to_scene(&mut scene).map_err(|e| e.to_string())?;
        }
    }
    if options.supports {
        let (printer, material) = profiles.ok_or("Select a printer and resin first")?;
        let geometric =
            sc_print_glue::supports::geometric_supported_scene(&scene, &options.support)
                .map_err(|e| e.to_string())?
                .0;
        let profile = support_profile(&geometric, options, printer, material)?;
        scene = sc_print_glue::supports::auto_supported_scene_analyzed_with_fallback(
            &scene,
            &options.support,
            &profile,
            true,
        )
        .map_err(|e| e.to_string())?
        .0;
    }
    Ok(scene)
}

fn repair_source(scene: &mut Scene) -> Result<(), String> {
    let refusals: Vec<_> = scene
        .repair_meshes(sc_mesh_core::indexed_mesh::RepairPasses::default_repair())
        .into_iter()
        .filter_map(|outcome| outcome.refusal)
        .map(|refusal| refusal.to_string())
        .collect();
    if refusals.is_empty() {
        Ok(())
    } else {
        Err(crate::i18n::tr!("Repair failed: {}", refusals.join(", ")).to_string())
    }
}

fn support_profile(
    scene: &Scene,
    options: &Options,
    printer: &sc_print_profiles::PrinterProfile,
    material: &sc_print_profiles::EffectiveMaterial,
) -> Result<sc_sl1cer::profile::SliceProfile, String> {
    let profile = sc_print_glue::slice_profile_for_selection(
        scene,
        printer,
        material,
        options.slicing.process_overrides.layer_height_mm,
        options.slicing.first_layer_height,
        options.slicing.aa_level,
        sc_sl1cer::profile::PackingMode::None,
    )
    .map_err(|e| e.to_string())?;
    Ok(sc_print_glue::with_accuracy_for_selection(
        profile,
        printer,
        material,
        &options.slicing.process_overrides,
    ))
}

pub(crate) fn prepare_paths(
    base: Scene,
    paths: &[PathBuf],
    options: &Options,
    volume: BuildVolume,
    token: &CancelToken,
    profiles: Option<&(
        sc_print_profiles::PrinterProfile,
        sc_print_profiles::EffectiveMaterial,
    )>,
    mut progress: impl FnMut(usize, usize),
) -> Result<Prepared, String> {
    let mut sources = Vec::new();
    let mut failures = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        if token.is_cancelled() {
            return Err(crate::i18n::tr!("Batch cancelled; no changes were applied.").to_string());
        }
        progress(i + 1, paths.len());
        let result = prepare_file(path, options, token, profiles).and_then(|scene| {
            let bounds = scene
                .tight_aabb()
                .map_err(|e| e.to_string())?
                .ok_or_else(|| crate::i18n::tr!("The model has no geometry.").to_string())?;
            let size = [0, 1, 2].map(|axis| bounds.max()[axis] - bounds.min()[axis]);
            if size[0] > volume.width_mm()
                || size[1] > volume.depth_mm()
                || size[2] > volume.height_mm()
            {
                return Err(crate::i18n::tr!(
                    "The objects could not be placed within the build volume."
                )
                .to_string());
            }
            Ok(scene)
        });
        match result {
            Ok(scene) => sources.push(scene),
            Err(e) => failures.push(format!("{}: {e}", path.display())),
        }
    }
    if token.is_cancelled() {
        return Err(crate::i18n::tr!("Batch cancelled; no changes were applied.").to_string());
    }
    let mut result = if sources.is_empty() {
        Prepared {
            edit: SceneEdit::Compound(Vec::new()),
            node_slots: base.nodes_len(),
            imported: 0,
            failures: Vec::new(),
            notices: Vec::new(),
        }
    } else {
        plan(base, &sources, volume)?
    };
    if token.is_cancelled() {
        return Err(crate::i18n::tr!("Batch cancelled; no changes were applied.").to_string());
    }
    if options.supports {
        let empty = sc_print_glue::supports::SupportStructure::default();
        for scene in &sources {
            for model in sc_print_glue::supports::model_results(scene, &empty, &options.support) {
                if model.direct {
                    result.notices.push(
                        crate::i18n::tr!(
                            "{}: prints directly on the plate, no supports needed",
                            model.name.as_str()
                        )
                        .to_string(),
                    );
                }
            }
        }
    }
    result.failures = failures;
    Ok(result)
}

pub(crate) fn install(
    ui: &MainWindow,
    document: Rc<SceneDocument>,
    preferences: Rc<crate::preferences_bus::PreferenceHub>,
    volume: impl Fn() -> BuildVolume + 'static,
) {
    let cancel: Rc<RefCell<Option<CancelToken>>> = Rc::new(RefCell::new(None));
    let picked = Rc::new(RefCell::new(Vec::<PathBuf>::new()));
    {
        let weak = ui.as_weak();
        ui.on_batch_prepare_requested(move || {
            if let Some(ui) = weak.upgrade() {
                if let Err(e) = crate::recipes::refresh(&ui) {
                    ui.set_batch_summary(e.into());
                }
                ui.set_batch_visible(true);
            }
        });
    }
    {
        let picked = picked.clone();
        let weak = ui.as_weak();
        ui.on_batch_add_path(move |path| {
            picked.borrow_mut().push(PathBuf::from(path.as_str()));
            if let Some(ui) = weak.upgrade() {
                ui.set_batch_summary(crate::i18n::tr!("{} files selected", picked.borrow().len()));
            }
        });
    }
    for folder in [false, true] {
        let picked = picked.clone();
        let weak = ui.as_weak();
        let choose = move || {
            let picked = picked.clone();
            let weak = weak.clone();
            let _ = slint::spawn_local(async move {
                let paths = if folder {
                    let Some(dir) = rfd::AsyncFileDialog::new().pick_folder().await else {
                        return;
                    };
                    let mut files: Vec<_> = std::fs::read_dir(dir.path())
                        .into_iter()
                        .flatten()
                        .filter_map(Result::ok)
                        .map(|e| e.path())
                        .filter(|p| {
                            p.extension().and_then(|s| s.to_str()).is_some_and(|s| {
                                ["stl", "obj", "3mf"].contains(&s.to_ascii_lowercase().as_str())
                            })
                        })
                        .collect();
                    files.sort();
                    files
                } else {
                    let Some(files) = rfd::AsyncFileDialog::new()
                        .add_filter(
                            crate::i18n::tr!("Models").to_string(),
                            &["stl", "obj", "3mf"],
                        )
                        .pick_files()
                        .await
                    else {
                        return;
                    };
                    files.into_iter().map(|f| f.path().to_path_buf()).collect()
                };
                if let Some(ui) = weak.upgrade() {
                    ui.set_batch_summary(crate::i18n::tr!("{} files selected", paths.len()));
                }
                *picked.borrow_mut() = paths;
            });
        };
        if folder {
            ui.on_batch_choose_folder(choose);
        } else {
            ui.on_batch_choose_files(choose);
        }
    }
    {
        let cancel = cancel.clone();
        let weak = ui.as_weak();
        ui.on_batch_cancel(move || {
            if let Some(token) = cancel.borrow().as_ref() {
                token.cancel();
            } else if let Some(ui) = weak.upgrade() {
                ui.set_batch_visible(false);
            }
        });
    }
    let weak = ui.as_weak();
    ui.on_batch_start(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_batch_busy() || picked.borrow().is_empty() {
            return;
        }
        let support = match crate::auto_support_run::settings(&ui) {
            Ok(s) => s,
            Err(e) => {
                ui.set_batch_summary(e.into());
                return;
            }
        };
        let snapshot = preferences.current_values();
        let recipe = if ui.get_batch_use_recipe() {
            match sc_print_profiles::recipes::Recipe::parse(ui.get_recipe_toml().as_str()) {
                Ok(r)=>Some(r), Err(e)=>{ ui.set_batch_summary(e.into()); return; }
            }
        } else { None };
        let options = Options {
            recipe,
            orient: ui.get_batch_orient(),
            orient_weights:crate::layout_tools::orient_weights(&ui),
            orient_convenience_rate: 1.0,
            arrange_gap_mm: ui.get_arrange_gap_mm().parse().unwrap_or(3.0),
            hollow: ui.get_batch_hollow(),
            supports: ui.get_batch_supports(),
            wall_mm: ui.get_hollow_wall_mm().parse().unwrap_or(2.0),
            support,
            slicing: sc_print_glue::SlicingParams { process_overrides: *document.process_overrides.borrow(), layer_height_request: None, first_layer_height: document.chosen_first_layer(), aa_level: document.scene_aa.get().unwrap_or(snapshot.aa_level) },
        };
        let profiles = document
            .chosen_printer().or(snapshot.default_printer)
            .zip(document.chosen_resin().or(snapshot.default_resin))
            .and_then(|(p, m)| crate::slice_projection::load_chosen_profiles(&p, &m).ok());
        if (options.supports || options.recipe.is_some()) && profiles.is_none() {
            ui.set_batch_summary(crate::i18n::tr!("Select a printer and resin first."));
            return;
        }
        let operations = (options.orient, options.supports, options.hollow);
        let volume = volume();
        let base = document
            .read_scene(|s| s.graph().clone())
            .unwrap_or_default();
        let revision = document.revision();
        let paths = picked.borrow().clone();
        let slice = ui.get_batch_slice() || ui.get_batch_export();
        let export = ui.get_batch_export();
        let token = CancelToken::new();
        *cancel.borrow_mut() = Some(token.clone());
        ui.set_batch_busy(true);
        let (tx, rx) = futures_channel::oneshot::channel();
        let progress_ui = weak.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                prepare_paths(base, &paths, &options, volume, &token, profiles.as_ref(), |done, total| {
                    let message = crate::i18n::tr!("Preparing {} of {}", done, total).to_string();
                    let _ = progress_ui.clone().upgrade_in_event_loop(move |ui| {
                        if ui.get_batch_busy() {
                            ui.set_batch_summary(message.into());
                        }
                    });
                })
            }))
            .unwrap_or_else(|e| Err(crate::mesh_loader::panic_text(&e)));
            let _ = tx.send(result);
        });
        let document = document.clone();
        let weak = weak.clone();
        let cancel = cancel.clone();
        let _ = slint::spawn_local(async move {
            let result = rx.await;
            let Some(ui) = weak.upgrade() else {
                return;
            };
            ui.set_batch_busy(false);
            cancel.borrow_mut().take();
            match result {
                Ok(Ok(prepared)) if revision == document.revision() => {
                    let summary = crate::i18n::tr!(
                        "Prepared {} files; {} failures. Oriented: {}, supported: {}, hollowed: {}.",
                        prepared.imported, prepared.failures.len(),
                        if operations.0 { prepared.imported } else { 0 },
                        if operations.1 { prepared.imported } else { 0 },
                        if operations.2 { prepared.imported } else { 0 }
                    );
                    match document.apply_batch(prepared.edit, prepared.node_slots) {
                        Ok(()) => {
                            ui.set_batch_summary(
                                format!("{summary}\n{}\n{}", prepared.notices.join("\n"), prepared.failures.join("\n")).into(),
                            );
                            ui.set_batch_export_pending(export);
                            if slice {
                                ui.invoke_slice_all_requested();
                            }
                        }
                        Err(e) => ui.set_batch_summary(e.into()),
                    }
                }
                Ok(Ok(_)) => ui.set_batch_summary(crate::i18n::tr!(
                    "The project changed. Run the batch again."
                )),
                Ok(Err(e)) => ui.set_batch_summary(e.into()),
                Err(e) => ui.set_batch_summary(e.to_string().into()),
            }
        });
    });
}

#[cfg(test)]
#[path = "tests_batch_prepare.rs"]
mod tests;

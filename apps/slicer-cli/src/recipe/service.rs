// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Shared GUI/CLI recipe execution. A staged scene and one compound edit are
//! returned only after the complete workflow; cancellation discards scene edits.
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_print_glue::{SliceRun, SlicedPlate, SlicingParams};
use sc_print_profiles::{
    EffectiveMaterial, PrinterProfile,
    recipes::{Recipe, Step},
};
use sc_scene::{CsgPolicy, LeafPayload, NodeId, ReferencePolicy, Scene, node::LeafAttachment};
use sc_scene_edit::SceneEdit;
use sc_sl1cer::CancelToken;
use std::{collections::BTreeMap, path::Path, sync::Arc};

#[path = "recipe_layout.rs"]
mod layout;

pub(crate) struct ResultScene {
    pub edit: SceneEdit,
    pub slots: usize,
    pub scene: Scene,
    pub summary: String,
    pub slices: Vec<(u32, SlicedPlate)>,
}

fn leaves(scene: &Scene) -> Result<Vec<NodeId>, String> {
    scene
        .reachable_leaves(CsgPolicy::Refuse, ReferencePolicy::Skip)
        .map_err(|e| e.to_string())
}
fn support(scene: &Scene, id: NodeId) -> bool {
    scene
        .node(id)
        .and_then(|n| n.meta.as_ref())
        .and_then(|m| m.comment.as_deref())
        .is_some_and(|c| c.starts_with("sc-generated-support"))
}

/// Snapshot changed leaves without changing parent attachment, metadata or IDs.
pub(crate) fn difference(before: &Scene, after: &Scene) -> Result<SceneEdit, String> {
    let mut staged = before.clone();
    while staged.nodes_len() < after.nodes_len() {
        staged.allocate_unbound_node_id();
    }
    let mut edits = Vec::new();
    for id in leaves(before)? {
        let old = before.node(id).ok_or("Missing source node")?;
        let same = after.node(id).is_some_and(|new| {
            old.local_transform == new.local_transform
                && old.meta == new.meta
                && before
                    .leaf_mesh_arc(id)
                    .zip(after.leaf_mesh_arc(id))
                    .is_some_and(|(a, b)| Arc::ptr_eq(a, b))
        });
        if same {
            continue;
        }
        let removed = staged.remove_leaf(id).map_err(|e| e.to_string())?;
        edits.push(SceneEdit::RemoveLeaf(sc_scene_edit::LeafRecord {
            node_id: id,
            attachment: removed.attachment,
            payload: removed.payload,
            local_transform: removed.local_transform,
            meta: removed.meta,
        }));
        if let Some(node) = after.node(id) {
            let edit = SceneEdit::AddLeaf(sc_scene_edit::LeafRecord {
                node_id: id,
                attachment: removed.attachment,
                payload: LeafPayload::Mesh(
                    after
                        .leaf_mesh_arc(id)
                        .ok_or("Missing result mesh")?
                        .clone(),
                ),
                local_transform: node.local_transform,
                meta: node.meta.clone(),
            });
            edit.apply_to_scene(&mut staged)
                .map_err(|e| e.to_string())?;
            edits.push(edit);
        }
    }
    for id in leaves(after)? {
        if before.node(id).is_some() {
            continue;
        }
        let node = after.node(id).ok_or("Missing added node")?;
        let edit = SceneEdit::AddLeaf(sc_scene_edit::LeafRecord {
            node_id: id,
            attachment: LeafAttachment::Root {
                position: staged.roots().len(),
            },
            payload: LeafPayload::Mesh(
                after.leaf_mesh_arc(id).ok_or("Missing added mesh")?.clone(),
            ),
            local_transform: after.world_transform_of(id).map_err(|e| e.to_string())?,
            meta: node.meta.clone(),
        });
        edit.apply_to_scene(&mut staged)
            .map_err(|e| e.to_string())?;
        edits.push(edit);
    }
    edits.push(SceneEdit::SetPlates {
        before: before.plates().clone(),
        after: after.plates().clone(),
    });
    Ok(SceneEdit::Compound(edits))
}

fn replace(scene: &mut Scene, id: NodeId, mesh: sc_mesh_core::IndexedMesh) -> Result<(), String> {
    let old = scene.remove_leaf(id).map_err(|e| e.to_string())?;
    scene
        .install_leaf_at(
            id,
            old.attachment,
            LeafPayload::Mesh(Arc::new(mesh)),
            old.local_transform,
            old.meta,
        )
        .map_err(|e| e.to_string())
}

pub(crate) struct Context<'a> {
    pub printer: &'a PrinterProfile,
    pub material: &'a EffectiveMaterial,
    pub params: SlicingParams,
    pub job_name: &'a str,
}

/// Execute on explicit model IDs. Each plate is isolated; locked targets refuse
/// the entire run. Failed steps retain their input and appear in the summary.
pub(crate) fn run(
    base: &Scene,
    targets: &[NodeId],
    recipe: &Recipe,
    context: &Context<'_>,
    token: &CancelToken,
    mut progress: impl FnMut(usize, &str),
) -> Result<ResultScene, String> {
    recipe.validate()?;
    if targets.is_empty() {
        return Err("Select at least one model".into());
    }
    let mut plates = BTreeMap::<u32, Vec<NodeId>>::new();
    for &id in targets {
        if support(base, id) {
            continue;
        }
        let plate = base.plate_of(id);
        if base
            .plates()
            .order
            .iter()
            .any(|p| p.id == plate && p.locked)
        {
            return Err("Unlock the target plate before running a recipe".into());
        }
        plates.entry(plate).or_default().push(id);
    }
    if plates.is_empty() {
        return Err("Select at least one model".into());
    }
    let mut slice_results = Vec::new();
    let mut result = base.clone();
    let mut summary = Vec::new();
    for (plate, ids) in plates {
        let saved = base
            .plates()
            .order
            .iter()
            .find(|p| p.id == plate)
            .and_then(|p| p.settings.as_deref())
            .map(sc_print_profiles::scene_overrides::PlateSettings::parse)
            .transpose()
            .map_err(|e| e.to_string())?;
        let effective = Context {
            printer: context.printer,
            material: context.material,
            job_name: context.job_name,
            params: saved.map_or(context.params, |settings| SlicingParams {
                process_overrides: settings.process,
                aa_level: settings.aa.unwrap_or(context.params.aa_level),
                first_layer_height: settings
                    .first_layer_height
                    .or(context.params.first_layer_height),
                layer_height_request: settings
                    .process
                    .layer_height_mm
                    .or(context.params.layer_height_request),
            }),
        };
        let mut scope = result.clone();
        for id in leaves(&scope)? {
            let owner = scope
                .node(id)
                .and_then(|n| n.meta.as_ref())
                .and_then(|m| m.comment.as_deref())
                .and_then(|c| {
                    c.lines()
                        .find_map(|l| l.strip_prefix("owner = ")?.parse::<u32>().ok())
                });
            if !ids.contains(&id) && !owner.is_some_and(|o| ids.iter().any(|id| id.get() == o)) {
                scope.remove_leaf(id).map_err(|e| e.to_string())?;
            }
        }
        let mut slices = None;
        let mut hollows = BTreeMap::new();
        for (index, step) in recipe.steps.iter().enumerate() {
            if token.is_cancelled() {
                return Err("Recipe cancelled; no scene changes applied".into());
            }
            progress(index, &format!("Plate {plate}: {} — running", step.name()));
            let mut staged = scope.clone();
            let outcome = execute(
                &mut staged,
                &ids,
                step,
                &effective,
                token,
                &mut hollows,
                &mut slices,
                context.job_name,
                plate,
            );
            match outcome {
                Ok(()) => {
                    scope = staged;
                    if let Step::Supports { settings, .. } = step {
                        let empty = sc_print_glue::supports::SupportStructure::default();
                        // Owners named by the generated support leaves: a model
                        // with none of them prints directly.
                        let supported: std::collections::BTreeSet<String> = leaves(&scope)?
                            .into_iter()
                            .filter_map(|id| {
                                let comment = scope.node(id)?.meta.as_ref()?.comment.as_deref()?;
                                comment.starts_with("sc-generated-support").then(|| {
                                    comment
                                        .lines()
                                        .find_map(|l| l.strip_prefix("owner = "))
                                        .unwrap_or("")
                                        .to_owned()
                                })
                            })
                            .collect();
                        for (result, id) in
                            sc_print_glue::supports::model_results(&scope, &empty, settings)
                                .into_iter()
                                .zip(sc_print_glue::supports::model_ids(&scope))
                        {
                            if result.direct && !supported.contains(&id.get().to_string()) {
                                let message = format!(
                                    "{}: prints directly on the plate, no supports needed",
                                    result.name
                                );
                                progress(index, &message);
                                summary.push(message);
                            }
                        }
                    }
                    progress(index, &format!("Plate {plate}: {} — complete", step.name()));
                }
                Err(error) => {
                    let message = format!("Plate {plate}: {} skipped: {error}", step.name());
                    progress(index, &message);
                    summary.push(message);
                }
            }
        }
        if token.is_cancelled() {
            return Err("Recipe cancelled; no scene changes applied".into());
        }
        // Merge only scope members; unrelated plates/models retain their storage.
        let scoped_ids = leaves(&scope)?;
        let mut merged = result.clone();
        while merged.nodes_len() < scope.nodes_len() {
            merged.allocate_unbound_node_id();
        }
        for id in &scoped_ids {
            let node = scope.node(*id).ok_or("Missing staged model")?;
            let world = scope.world_transform_of(*id).map_err(|e| e.to_string())?;
            let parent = merged
                .parent_of(*id)
                .map(|p| merged.world_transform_of(p))
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or(Transform3D::Identity);
            let local = parent.inverse().map_err(|e| e.to_string())?.compose(&world);
            let attachment = if merged.node(*id).is_some() {
                merged
                    .remove_leaf(*id)
                    .map_err(|e| e.to_string())?
                    .attachment
            } else {
                LeafAttachment::Root {
                    position: merged.roots().len(),
                }
            };
            merged
                .install_leaf_at(
                    *id,
                    attachment,
                    LeafPayload::Mesh(
                        scope
                            .leaf_mesh_arc(*id)
                            .ok_or("Missing staged mesh")?
                            .clone(),
                    ),
                    local,
                    node.meta.clone(),
                )
                .map_err(|e| e.to_string())?;
        }
        for id in leaves(&result)? {
            if support(&result, id)
                && result.plate_of(id) == plate
                && !scoped_ids.contains(&id)
                && result
                    .node(id)
                    .and_then(|n| n.meta.as_ref())
                    .and_then(|m| m.comment.as_deref())
                    .is_some_and(|c| {
                        ids.iter()
                            .any(|owner| c.lines().any(|l| l == format!("owner = {}", owner.get())))
                    })
            {
                merged.remove_leaf(id).map_err(|e| e.to_string())?;
            }
        }
        let mut membership = merged.plates().clone();
        for id in scoped_ids {
            if result.node(id).is_none() {
                membership.assignments.insert(id, plate);
            }
        }
        if !merged.set_plates(membership) {
            return Err("Invalid plate membership".into());
        }
        let full_plate = leaves(&result)?
            .into_iter()
            .filter(|id| result.plate_of(*id) == plate && !support(&result, *id))
            .all(|id| ids.contains(&id));
        if full_plate && let Some(slices) = slices {
            slice_results.push((plate, slices));
        }
        result = merged;
    }
    if summary.is_empty() {
        summary.push("Recipe complete; all steps succeeded".into());
    }
    Ok(ResultScene {
        edit: difference(base, &result)?,
        slots: result.nodes_len(),
        scene: result,
        summary: summary.join("\n"),
        slices: slice_results,
    })
}

#[allow(clippy::too_many_arguments)]
fn execute(
    scene: &mut Scene,
    ids: &[NodeId],
    step: &Step,
    context: &Context<'_>,
    token: &CancelToken,
    hollows: &mut BTreeMap<NodeId, (sc_mesh_core::IndexedMesh, f64, Vec<[f64; 3]>)>,
    slices: &mut Option<SlicedPlate>,
    name: &str,
    plate: u32,
) -> Result<(), String> {
    let apply =
        |scene: &mut Scene, edit: SceneEdit| edit.apply_to_scene(scene).map_err(|e| e.to_string());
    match step {
        Step::Scale { percent } => {
            for &id in ids {
                let delta =
                    Transform3D::Affine(AffineTransform3D::from_uniform_scale(percent / 100.0));
                apply(
                    scene,
                    layout::world_edit(scene, id, delta).ok_or("Cannot scale model")?,
                )?;
            }
        }
        Step::AutoOrient { preset } => {
            for &id in ids {
                apply(
                    scene,
                    layout::auto_orient(
                        scene,
                        id,
                        45.0,
                        context.printer.hardware.build_volume(),
                        *preset,
                        {
                            let mut orient = sc_print_glue::orient::Context::for_profiles(
                                context.printer,
                                context.material,
                                &context.params.process_overrides,
                            )
                            .map_err(|e| e.to_string())?;
                            orient.aa = context.params.aa_level;
                            orient.first_layer_height_mm =
                                context.params.first_layer_height.map_or(0.0, |v| v.value());
                            orient
                        },
                    )
                    .ok_or("Cannot orient model")?,
                )?;
            }
        }
        Step::Arrange => apply(
            scene,
            layout::arrange_with_gap(scene, context.printer.hardware.build_volume(), 3.0)
                .map_err(|ids| format!("{} models exceed the plate", ids.len()))?,
        )?,
        Step::Hollow { wall_mm } => {
            let mut pending = Vec::new();
            for &id in ids {
                let mesh = sc_scene::bake_leaf_to_world(scene, id).map_err(|e| e.to_string())?;
                let mesh = sc_print_glue::hollow::hollow_source(&mesh).into_owned();
                let hollow =
                    sc_print_glue::hollow::hollow(&mesh, *wall_mm).map_err(|e| e.to_string())?;
                let inverse = scene
                    .world_transform_of(id)
                    .map_err(|e| e.to_string())?
                    .inverse()
                    .map_err(|e| e.to_string())?;
                let local = sc_scene::bake_mesh_to_world(&hollow.mesh, &inverse)
                    .map_err(|e| e.to_string())?;
                pending.push((id, mesh, hollow.suggested_holes, local));
            }
            for (id, mesh, holes, local) in pending {
                replace(scene, id, local)?;
                hollows.insert(id, (mesh, *wall_mm, holes));
            }
        }
        Step::DrainHoles { count, diameter_mm } => {
            for &id in ids {
                let (source, wall, suggestions) = hollows
                    .get(&id)
                    .ok_or("Run hollow successfully before drain holes")?;
                if *count > suggestions.len() {
                    return Err(format!(
                        "Requested {count} holes; only {} safe cavity-floor positions available",
                        suggestions.len()
                    ));
                }
                let bottom = source
                    .vertices
                    .iter()
                    .map(|v| f64::from(v.coords()[2]))
                    .fold(f64::INFINITY, f64::min);
                let holes: Vec<_> = suggestions
                    .iter()
                    .take(*count)
                    .map(|&p| sc_print_glue::hollow::DrainHole {
                        position: p,
                        direction: [0., 0., -1.],
                        diameter_mm: *diameter_mm,
                        depth_mm: p[2] - bottom + 1.0,
                    })
                    .collect();
                let hollow = sc_print_glue::hollow::hollow_with_holes(source, *wall, &holes)
                    .map_err(|e| e.to_string())?;
                let inverse = scene
                    .world_transform_of(id)
                    .map_err(|e| e.to_string())?
                    .inverse()
                    .map_err(|e| e.to_string())?;
                replace(
                    scene,
                    id,
                    sc_scene::bake_mesh_to_world(&hollow.mesh, &inverse)
                        .map_err(|e| e.to_string())?,
                )?;
            }
        }
        Step::Supports { settings, preset } => {
            let mut configured = *settings;
            if let Some(preset) = preset {
                use sc_print_profiles::support_settings::SupportPreset;
                configured.apply_preset(match preset.as_str() {
                    "light" => SupportPreset::Light,
                    "heavy" => SupportPreset::Heavy,
                    _ => SupportPreset::Medium,
                });
            }
            let settings = &configured;
            for id in leaves(scene)?
                .into_iter()
                .filter(|id| support(scene, *id))
                .collect::<Vec<_>>()
            {
                scene.remove_leaf(id).map_err(|e| e.to_string())?;
            }
            let geometric = sc_print_glue::supports::geometric_supported_scene(scene, settings)
                .map_err(|e| e.to_string())?
                .0;
            let profile = sc_print_glue::slice_profile_for_selection(
                &geometric,
                context.printer,
                context.material,
                context.params.layer_height_request,
                context.params.first_layer_height,
                context.params.aa_level,
                sc_sl1cer::profile::PackingMode::None,
            )
            .map_err(|e| e.to_string())?;
            *scene = sc_print_glue::supports::auto_supported_scene_analyzed_with_fallback(
                scene, settings, &profile, true,
            )
            .map_err(|e| e.to_string())?
            .0;
        }
        Step::Slice => {
            *slices = match sc_print_glue::PlateJob::new(scene, context.printer, context.material, &context.params).slice(token, |_| {}, true)
            .map_err(|e| e.to_string())?
            {
                SliceRun::Completed(s) => Some(s),
                SliceRun::Cancelled => return Err("Cancelled".into()),
            };
        }
        Step::Export {
            format,
            folder,
            name_pattern,
        } => {
            let filename = name_pattern
                .replace(
                    "{name}",
                    &name
                        .chars()
                        .map(|c| {
                            if c.is_ascii_alphanumeric() || c == '-' {
                                c
                            } else {
                                '_'
                            }
                        })
                        .collect::<String>(),
                )
                .replace("{plate}", &plate.to_string());
            std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
            export(
                scene,
                slices
                    .as_ref()
                    .ok_or("Run slice successfully before export")?,
                &folder.join(filename),
                format,
                context,
            )?;
        }
    }
    if !matches!(step, Step::Slice | Step::Export { .. }) {
        *slices = None;
    }
    if matches!(
        step,
        Step::Scale { .. } | Step::AutoOrient { .. } | Step::Arrange | Step::Supports { .. }
    ) {
        hollows.clear();
    }
    if matches!(
        step,
        Step::Scale { .. }
            | Step::AutoOrient { .. }
            | Step::Hollow { .. }
            | Step::DrainHoles { .. }
    ) {
        for id in leaves(scene)?
            .into_iter()
            .filter(|id| support(scene, *id))
            .collect::<Vec<_>>()
        {
            let mut old = scene.remove_leaf(id).map_err(|e| e.to_string())?;
            if let Some(comment) = old.meta.as_mut().and_then(|m| m.comment.as_mut()) {
                *comment = comment
                    .lines()
                    .filter(|l| !l.starts_with("geometry_stale = "))
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\ngeometry_stale = true\n";
            }
            scene
                .install_leaf_at(
                    id,
                    old.attachment,
                    old.payload,
                    old.local_transform,
                    old.meta,
                )
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub(crate) fn export(
    scene: &Scene,
    slices: &SlicedPlate,
    path: &Path,
    format: &str,
    context: &Context<'_>,
) -> Result<(), String> {
    use sc_sla_formats::{Format, WriterOptions};
    let format: Format = format.parse().map_err(|e| format!("{e}"))?;
    let mut options = WriterOptions::from_profile(slices.profile(), sc_sl1cer::VERSION);
    if let Some(mesh) = scene.merge_meshes().map_err(|e| e.to_string())? {
        options = options.with_preview_mesh(&mesh);
    }
    let calibration = sc_print_glue::calibration::regions(scene)?;
    if !calibration.is_empty() && format != Format::GcodeZip {
        return Err("Exposure calibration requires G-code ZIP with sequential PNG masks".into());
    }
    if path.exists() {
        return Err(format!("Output already exists: {}", path.display()));
    }
    if format == Format::ThreeMf {
        return sc_sla_formats::write_scene_slices(scene, slices.leaves(), path, &options)
            .map_err(|e| e.to_string());
    }
    let profile = slices.profile().clone();
    let stack = sc_sl1cer::contour::merge_stacks(
        slices
            .leaves()
            .iter()
            .cloned()
            .map(sc_sl1cer::LeafSlices::into_stack)
            .collect(),
    )
    .map_err(|e| format!("{e:?}"))?;
    if format == Format::GcodeZip {
        let config = sc_print_profiles::resolve(
            context.printer,
            context.material,
            &context.params.process_overrides,
        )
        .map_err(|e| e.to_string())?;
        let (schedule, exposures) = sc_print_glue::calibration::schedule(
            &config.material_resolved,
            profile.layer_schedule(),
            &calibration,
        );
        let estimate = sc_print_job::PrintEstimate::new(
            &schedule,
            sc_print_job::raster_volume_ml(&stack, &profile),
            context.material.defaults().density_g_ml,
            context
                .material
                .defaults()
                .price_per_liter
                .map(|p| p.value()),
        );
        let script = if exposures.is_empty() {
            sc_print_job::render_job(&config, &schedule, &estimate)
        } else {
            sc_print_job::render_multi_job(&config, &schedule, &estimate, &exposures)
        }
        .map_err(|e| e.to_string())?;
        let job = sc_sla_formats::PreparedJob {
            regions: calibration.iter().map(|r| (r.min, r.max)).collect(),
            layer_z_mm: schedule.layers.iter().map(|l| l.z_mm).collect(),
            exposure_s: schedule.layers.iter().map(|l| l.exposure_s).collect(),
            script,
        };
        sc_sla_formats::write_gcode_zip(&stack, path, &options, &job).map_err(|e| e.to_string())
    } else {
        sc_sla_formats::write_slice(&stack, path, format, &options).map_err(|e| e.to_string())
    }
}

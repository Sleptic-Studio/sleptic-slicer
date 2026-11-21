// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Stage model edits and their support metadata in a single reversible command.
use sc_mesh_core::IndexedMesh;
use sc_print_glue::model_tools::{self, BooleanOp};
use sc_scene::{LeafPayload, NodeId, Scene, node::LeafAttachment};
use sc_scene_edit::SceneEdit;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub(crate) enum Operation {
    Cut {
        z: f64,
        keep: i32,
        cap: bool,
    },
    Split,
    Simplify {
        target: f64,
        percentage: bool,
        deviation: f64,
    },
    Boolean(BooleanOp),
    Label {
        text: String,
        point: [f64; 3],
        normal: [f64; 3],
        size: f64,
        depth: f64,
        engrave: bool,
    },
}

pub(crate) struct Plan {
    pub edit: SceneEdit,
    pub slots: usize,
    pub selected: Vec<NodeId>,
    pub summary: Option<String>,
}

fn tool_error(error: model_tools::ModelToolError) -> String {
    use model_tools::ModelToolError;
    match error {
        ModelToolError::InvalidMesh => crate::i18n::tr!("Repair the mesh first: this tool requires closed, consistently wound manifold surfaces."),
        ModelToolError::InvalidSettings => crate::i18n::tr!("Enter finite coordinates and positive label size and depth."),
        ModelToolError::TooComplex => crate::i18n::tr!("The model edit exceeds the geometry complexity limit."),
        ModelToolError::InvalidResult => crate::i18n::tr!("The edit could not produce a closed manifold. The original objects were retained."),
        ModelToolError::InvalidText => crate::i18n::tr!("Enter 1 to 80 printable ASCII characters."),
    }.to_string()
}

pub(crate) fn plan(
    scene: &Scene,
    selected: &[NodeId],
    operation: Operation,
) -> Result<Plan, String> {
    plan_with_progress(scene, selected, operation, |_| true)
}

/// Stage a cancellable edit without touching the live document.
pub(crate) fn plan_with_progress(
    scene: &Scene,
    selected: &[NodeId],
    operation: Operation,
    progress: impl FnMut(f32) -> bool,
) -> Result<Plan, String> {
    let &first = selected
        .first()
        .ok_or_else(|| crate::i18n::tr!("Select a model first.").to_string())?;
    if selected.iter().any(|&id| {
        scene
            .plates()
            .order
            .iter()
            .any(|p| p.id == scene.plate_of(id) && p.locked)
    }) {
        return Err(crate::i18n::tr!("Unlock the plate before editing its models.").to_string());
    }
    if selected.iter().any(|&id| {
        scene
            .node(id)
            .and_then(|n| n.meta.as_ref())
            .and_then(|m| m.comment.as_deref())
            .is_some_and(|c| c.starts_with("sc-generated-support"))
    }) {
        return Err(crate::i18n::tr!("Select models rather than generated supports.").to_string());
    }
    let source = sc_scene::bake_leaf_to_world(scene, first).map_err(|e| e.to_string())?;
    let mut removed = vec![first];
    let is_split = matches!(&operation, Operation::Split);
    let mut summary = None;
    let parts = match operation {
        Operation::Cut { z, keep, cap } => {
            let (lower, upper) = model_tools::cut(&source, z, cap).map_err(tool_error)?;
            match keep {
                0 => vec![lower],
                1 => vec![upper],
                _ => vec![lower, upper],
            }
        }
        Operation::Split => model_tools::split_parts(&source).map_err(tool_error)?,
        Operation::Simplify {
            target,
            percentage,
            deviation,
        } => {
            if !target.is_finite() || target <= 0.0 || (percentage && target > 100.0) {
                return Err(crate::i18n::tr!(
                    "Enter a positive triangle count or a percentage from 0 to 100."
                )
                .to_string());
            }
            let count = if percentage {
                (source.faces.len() as f64 * target / 100.0).round() as usize
            } else {
                target.round() as usize
            };
            let simplified = sc_mesh_core::simplify::simplify_with_progress(
                &source,
                count.max(4),
                deviation,
                progress,
            )
            .map_err(|error| match error {
                sc_mesh_core::simplify::SimplifyError::Cancelled => {
                    crate::i18n::tr!("Simplification cancelled; no changes were applied.")
                        .to_string()
                }
                sc_mesh_core::simplify::SimplifyError::InvalidMesh => {
                    crate::i18n::tr!("Repair the mesh before simplifying it.").to_string()
                }
                sc_mesh_core::simplify::SimplifyError::InvalidSettings => {
                    crate::i18n::tr!("Enter a positive maximum deviation.").to_string()
                }
            })?;
            summary = Some(
                crate::i18n::tr!(
                    "Simplified: {} to {} triangles; deviation bound {} mm",
                    source.faces.len(),
                    simplified.mesh.faces.len(),
                    format!("{:.5}", simplified.deviation_bound_mm)
                )
                .to_string(),
            );
            vec![simplified.mesh]
        }
        Operation::Boolean(op) => {
            if selected.len() != 2 {
                return Err(
                    crate::i18n::tr!("Select exactly two models for a Boolean operation.")
                        .to_string(),
                );
            }
            let second = selected[1];
            if scene.plate_of(first) != scene.plate_of(second) {
                return Err(crate::i18n::tr!("Both models must be on the same plate.").to_string());
            }
            removed.push(second);
            let other = sc_scene::bake_leaf_to_world(scene, second).map_err(|e| e.to_string())?;
            vec![model_tools::boolean(&source, &other, op).map_err(tool_error)?]
        }
        Operation::Label {
            text,
            point,
            normal,
            size,
            depth,
            engrave,
        } => vec![
            model_tools::label(&source, &text, point, normal, size, depth, engrave)
                .map_err(tool_error)?,
        ],
    };
    let parts: Vec<_> = parts.into_iter().filter(|m| !m.faces.is_empty()).collect();
    if parts.is_empty() {
        return Err(crate::i18n::tr!("The operation leaves no printable volume.").to_string());
    }
    if is_split && parts.len() == 1 {
        return Err(crate::i18n::tr!("This model contains one connected shell.").to_string());
    }
    let node = scene.node(first).ok_or("Missing model")?;
    let world = scene.world_transform_of(first).map_err(|e| e.to_string())?;
    let inverse = world.inverse().map_err(|e| e.to_string())?;
    let mut staged = scene.clone();
    let mut edits = Vec::new();
    let mut first_attachment = None;
    // Snapshot positions as leaves are removed; inverse edits restore that order.
    for &id in removed.iter().rev() {
        let leaf = staged.remove_leaf(id).map_err(|e| e.to_string())?;
        if id == first {
            first_attachment = Some(leaf.attachment);
        }
        edits.push(SceneEdit::RemoveLeaf(sc_scene_edit::LeafRecord {
            node_id: id,
            attachment: leaf.attachment,
            payload: leaf.payload,
            local_transform: leaf.local_transform,
            meta: leaf.meta,
        }));
    }
    let mut ids = Vec::new();
    for (index, part) in parts.into_iter().enumerate() {
        let mut mesh = sc_scene::bake_mesh_to_world(&part, &inverse).map_err(|e| e.to_string())?;
        let id = if index == 0 {
            first
        } else {
            staged.allocate_unbound_node_id()
        };
        let attachment = if index == 0 {
            first_attachment.ok_or("Missing attachment")?
        } else {
            LeafAttachment::Root {
                position: staged.roots().len(),
            }
        };
        let transform = if index == 0 {
            node.local_transform
        } else {
            world
        };
        let mut meta = node.meta.clone().unwrap_or_default();
        // Preserve user comments while invalidating the previous paint projection.
        meta.comment = meta.comment.map(|c| {
            c.lines()
                .filter(|l| !l.starts_with("sc-support-paint "))
                .collect::<Vec<_>>()
                .join("\n")
        });
        if index > 0 {
            meta.name = Some(format!(
                "{} {}",
                meta.name.as_deref().unwrap_or("Part"),
                index + 1
            ));
        }
        mesh.meta = source.meta.clone();
        let mesh_meta = mesh.meta.get_or_insert_with(Default::default);
        if meta.name.is_some() {
            mesh_meta.name = meta.name.clone();
        }
        if mesh_meta
            .header_comment
            .as_deref()
            .is_some_and(|c| c.starts_with("sleptic-hollow-v1\n"))
        {
            mesh_meta.header_comment = None;
        }
        let payload = LeafPayload::Mesh(Arc::new(mesh));
        staged
            .install_leaf_at(
                id,
                attachment,
                payload.clone(),
                transform,
                Some(meta.clone()),
            )
            .map_err(|e| e.to_string())?;
        edits.push(SceneEdit::AddLeaf(sc_scene_edit::LeafRecord {
            node_id: id,
            attachment,
            payload,
            local_transform: transform,
            meta: Some(meta),
        }));
        ids.push(id);
    }
    let before_plates = scene.plates().clone();
    let mut after_plates = before_plates.clone();
    for &id in &ids {
        after_plates.assignments.insert(id, scene.plate_of(first));
    }
    edits.push(SceneEdit::SetPlates {
        before: before_plates,
        after: after_plates,
    });
    for &owner in &removed {
        for support in crate::scene_document::owned_support_ids(scene, owner) {
            let leaf = staged.remove_leaf(support).map_err(|e| e.to_string())?;
            let mut meta = leaf.meta.clone().unwrap_or_default();
            let comment = meta.comment.clone().unwrap_or_default();
            let mut lines: Vec<_> = comment
                .lines()
                .filter(|l| !l.starts_with("owner = ") && !l.starts_with("geometry_stale = "))
                .map(str::to_owned)
                .collect();
            lines.push(format!("owner = {}", first.get()));
            lines.push("geometry_stale = true".into());
            meta.comment = Some(format!("{}\n", lines.join("\n")));
            edits.push(SceneEdit::RemoveLeaf(sc_scene_edit::LeafRecord {
                node_id: support,
                attachment: leaf.attachment,
                payload: leaf.payload.clone(),
                local_transform: leaf.local_transform,
                meta: leaf.meta,
            }));
            edits.push(SceneEdit::AddLeaf(sc_scene_edit::LeafRecord {
                node_id: support,
                attachment: leaf.attachment,
                payload: leaf.payload.clone(),
                local_transform: leaf.local_transform,
                meta: Some(meta.clone()),
            }));
            staged
                .install_leaf_at(
                    support,
                    leaf.attachment,
                    leaf.payload,
                    leaf.local_transform,
                    Some(meta),
                )
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(Plan {
        edit: SceneEdit::Compound(edits),
        slots: staged.nodes_len(),
        selected: ids,
        summary,
    })
}

/// Geometric normal of the picked face; world-space nearest-triangle distance
/// handles mirrored/nonuniformly scaled models without trusting stored normals.
pub(crate) fn face_normal(mesh: &IndexedMesh, point: [f64; 3]) -> Option<[f64; 3]> {
    use glam::DVec3;
    let point = DVec3::from_array(point);
    mesh.faces
        .iter()
        .filter_map(|f| {
            let [a, b, c] = f
                .vertices
                .map(|i| DVec3::from_array(mesh.vertices[usize::from(i)].coords().map(f64::from)));
            let n = (b - a).cross(c - a).try_normalize()?;
            let distance = (point - a).dot(n).abs();
            let q = point - n * (point - a).dot(n);
            let inside = [(a, b), (b, c), (c, a)]
                .iter()
                .all(|&(u, v)| (v - u).cross(q - u).dot(n) >= -1e-5);
            inside.then_some((distance, n.to_array()))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, n)| n)
}

#[cfg(test)]
#[path = "tests_model_tools.rs"]
mod tests;

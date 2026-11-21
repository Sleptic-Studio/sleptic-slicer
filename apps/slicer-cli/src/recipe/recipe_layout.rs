// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Undoable placement tools; all calculations use baked world coordinates.
use glam::DVec3;
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_print_primitives::BuildVolume;
use sc_scene::{Scene, node::NodeId};
use sc_scene_edit::SceneEdit;

fn points(scene: &Scene, id: NodeId) -> Option<Vec<DVec3>> {
    let mesh = sc_scene::bake_leaf_to_world(scene, id).ok()?;
    Some(
        mesh.vertices
            .iter()
            .map(|v| DVec3::from_array(v.coords().map(f64::from)))
            .collect(),
    )
}

fn bounds(points: &[DVec3]) -> Option<(DVec3, DVec3)> {
    let first = *points.first()?;
    Some(
        points
            .iter()
            .fold((first, first), |(min, max), p| (min.min(*p), max.max(*p))),
    )
}

pub(crate) fn world_edit(scene: &Scene, id: NodeId, delta: Transform3D) -> Option<SceneEdit> {
    let before = scene.node(id)?.local_transform;
    let world = scene.world_transform_of(id).ok()?;
    let parent = world.compose(&before.inverse().ok()?);
    let after = parent.inverse().ok()?.compose(&delta).compose(&world);
    Some(SceneEdit::SetLocalTransform {
        node_id: id,
        before,
        after,
    })
}

/// Returns the stable IDs of every group that cannot fit; failure applies no partial edit.
pub(crate) fn arrange_with_gap(
    scene: &Scene,
    volume: BuildVolume,
    gap: f64,
) -> Result<SceneEdit, Vec<NodeId>> {
    let leaves = scene
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .map_err(|_| Vec::new())?;
    let comment = |id| {
        scene
            .node(id)
            .and_then(|n| n.meta.as_ref())
            .and_then(|m| m.comment.as_deref())
            .unwrap_or("")
    };
    let models: Vec<_> = leaves
        .iter()
        .copied()
        .filter(|&id| !comment(id).starts_with("sc-generated-support"))
        .collect();
    let mut groups: Vec<Vec<NodeId>> = models.iter().map(|&id| vec![id]).collect();
    for &id in &leaves {
        if !comment(id).starts_with("sc-generated-support") {
            continue;
        }
        let owner = comment(id).lines().find_map(|l| {
            l.strip_prefix("owner = ")
                .and_then(|v| v.parse::<u32>().ok())
        });
        let index = models
            .iter()
            .position(|m| Some(m.get()) == owner)
            .unwrap_or(0);
        if let Some(group) = groups.get_mut(index) {
            group.push(id);
        } else {
            groups.push(vec![id]);
        }
    }
    let gap = if gap.is_finite() { gap.max(0.0) } else { 3.0 };
    let (left, bottom) = (-volume.x_mm() / 2.0, -volume.y_mm() / 2.0);
    let (right, top) = (-left, -bottom);
    let (mut x, mut y, mut row_height) = (left, bottom, 0.0f64);
    let mut edits = Vec::new();
    let mut failed = Vec::new();
    let mut packed_max = DVec3::new(left, bottom, 0.0);
    for group in groups {
        let all: Vec<_> = group
            .iter()
            .flat_map(|&id| points(scene, id).unwrap_or_default())
            .collect();
        let Some((min, max)) = bounds(&all) else {
            failed.push(group[0]);
            continue;
        };
        let size = max - min;
        if x + size.x > right {
            x = left;
            y += row_height + gap;
            row_height = 0.0;
        }
        if x + size.x > right || y + size.y > top || size.z > volume.z_mm() {
            failed.push(group[0]);
            continue;
        }
        let delta = Transform3D::Affine(AffineTransform3D::from_translation([
            x - min.x,
            y - min.y,
            -min.z,
        ]));
        packed_max = packed_max.max(DVec3::new(x + size.x, y + size.y, size.z));
        for id in group {
            if let Some(edit) = world_edit(scene, id, delta) {
                edits.push(edit);
            } else {
                failed.push(id);
            }
        }
        x += size.x + gap;
        row_height = row_height.max(size.y);
    }
    if failed.is_empty() {
        let center = Transform3D::Affine(AffineTransform3D::from_translation([
            -(left + packed_max.x) * 0.5,
            -(bottom + packed_max.y) * 0.5,
            0.0,
        ]));
        // Compose in world space, including for leaves under transformed parents.
        for edit in &mut edits {
            if let SceneEdit::SetLocalTransform {
                node_id,
                before,
                after,
            } = edit
            {
                let world = scene
                    .world_transform_of(*node_id)
                    .map_err(|_| vec![*node_id])?;
                let parent = world.compose(&before.inverse().map_err(|_| vec![*node_id])?);
                *after = parent
                    .inverse()
                    .map_err(|_| vec![*node_id])?
                    .compose(&center)
                    .compose(&parent)
                    .compose(after);
            }
        }
        Ok(SceneEdit::Compound(edits))
    } else {
        Err(failed)
    }
}

/// Shared resin orientation with a hard build-envelope constraint.
pub(crate) fn auto_orient(
    scene: &Scene,
    id: NodeId,
    support_angle_deg: f64,
    volume: BuildVolume,
    preset: sc_print_profiles::orient::Preset,
    context: sc_print_glue::orient::Context,
) -> Option<SceneEdit> {
    let proposal = sc_print_glue::orient::search_with_context(
        scene,
        id,
        volume,
        support_angle_deg,
        preset.weights(),
        context,
    )?;
    eprintln!(
        "Auto orient: support area {:.1} -> {:.1} mm2, height {:.2} -> {:.2} mm, fits {}, retained {}",
        proposal.original.support_area,
        proposal.chosen.support_area,
        proposal.original.height,
        proposal.chosen.height,
        proposal.chosen.fits,
        proposal.retained
    );
    if proposal.retained {
        let before = scene.node(id)?.local_transform;
        return Some(SceneEdit::SetLocalTransform {
            node_id: id,
            before,
            after: before,
        });
    }
    world_edit(scene, id, proposal.delta)
}

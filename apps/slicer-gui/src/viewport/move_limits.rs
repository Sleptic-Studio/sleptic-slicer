// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Constrain translation and keep edited parts at their intended height.

use crate::build_volume::BuildVolume;
use glam::Vec3;
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_mesh_spatial::SceneSpatial;
use sc_scene::NodeId;

/// Apply the plate limits in world coordinates, including owned supports.
pub(super) fn clamp_move_edit(
    spatial: &SceneSpatial,
    node_id: NodeId,
    before: Transform3D,
    proposed_after: Transform3D,
    build_volume: BuildVolume,
) -> Transform3D {
    let Some(leaf) = spatial.leaf(node_id) else {
        return proposed_after;
    };
    let parent = leaf.parent_world();
    let old_world = parent.compose(&before);
    let new_world = parent.compose(&proposed_after);
    let Some(mut bounds) = leaf.transformed_aabb(old_world) else {
        return proposed_after;
    };
    for id in crate::scene_document::owned_support_ids(spatial.graph(), node_id) {
        if let Some(other) = spatial
            .leaf(id)
            .and_then(|q| q.transformed_aabb(q.world_transform()))
        {
            bounds.union(&other);
        }
    }
    let offset = |t: Transform3D| match t {
        Transform3D::Identity => [0.0; 3],
        Transform3D::Affine(a) => *a.translation(),
    };
    let a = offset(old_world);
    let b = offset(new_world);
    let requested = Vec3::new(
        (b[0] - a[0]) as f32,
        (b[1] - a[1]) as f32,
        (b[2] - a[2]) as f32,
    );
    let allowed = build_volume.allowed_move(bounds, requested);
    if allowed == requested {
        return proposed_after;
    }
    if allowed == Vec3::ZERO {
        return before;
    }
    let local = match parent {
        Transform3D::Identity => allowed,
        Transform3D::Affine(matrix) => {
            let Ok(inverse) = matrix.inverse() else {
                return before;
            };
            use sc_mesh_core::primitives::Vertex;
            let point = |v: [f32; 3]| Vec3::from(inverse.apply_to_vertex(Vertex::new(v)).coords());
            point(allowed.to_array()) - point([0.0; 3])
        }
    };
    Transform3D::Affine(AffineTransform3D::from_translation([
        f64::from(local.x),
        f64::from(local.y),
        f64::from(local.z),
    ]))
    .compose(&before)
}

/// Preserve a supported model's height; otherwise place its bottom on the plate.
pub(super) fn settle_on_plate(
    spatial: &SceneSpatial,
    node_id: NodeId,
    proposed: Transform3D,
) -> Transform3D {
    let Some(leaf) = spatial.leaf(node_id) else {
        return proposed;
    };
    let parent = leaf.parent_world();
    let world = parent.compose(&proposed);
    let Some(after) = leaf.transformed_aabb(world) else {
        return proposed;
    };
    let mut shift = f64::from(after.min().coords()[2]);
    if !crate::scene_document::owned_support_ids(spatial.graph(), node_id).is_empty() {
        let current = parent.compose(&leaf.local_transform());
        let Some(before) = leaf.transformed_aabb(current) else {
            return proposed;
        };
        // Subtract in f32 to retain the original rounding behavior.
        shift = f64::from(after.min().coords()[2] - before.min().coords()[2]);
    }
    if shift.abs() < 1e-6 {
        return proposed;
    }
    let lift = Transform3D::Affine(AffineTransform3D::from_translation([0.0, 0.0, -shift]));
    match parent.inverse() {
        Ok(inverse) => inverse.compose(&lift).compose(&world),
        Err(_) => proposed,
    }
}

#[cfg(test)]
#[path = "tests_move_limits.rs"]
mod tests;

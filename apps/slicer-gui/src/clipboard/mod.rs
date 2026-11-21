// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The app's clipboard: the copied parts as a scene fragment, and where
//! they stood, which a paste on the same plate searches from.
//!
//! It belongs to the app, not a window, so a copy in one window pastes
//! in another, whatever that one has open. The fragment holds its meshes
//! itself, so closing the source scene does not empty it.
pub(crate) mod placement;

use std::collections::HashMap;

use sc_mesh_core::Aabb;
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_scene::Scene;
use sc_scene::node::{LeafAttachment, LeafPayload};
use sc_scene::plates::PlateId;
use sc_scene_edit::SceneFragment;

use crate::scene_document::{SceneDocument, owned_support_ids, support_owner};

pub(crate) struct Clipboard {
    fragment: SceneFragment,
    /// The plate the copied parts stood on and their plate XY centre. A
    /// paste on that plate looks for room next to them (as Orca does);
    /// elsewhere it starts at the plate centre.
    source: Option<(PlateId, [f64; 2])>,
}

impl Clipboard {
    pub(crate) fn new(fragment: SceneFragment) -> Self {
        Self {
            fragment,
            source: None,
        }
    }

    pub(crate) fn with_source(self, source: Option<(PlateId, [f64; 2])>) -> Self {
        Self { source, ..self }
    }

    pub(crate) fn fragment(&self) -> &SceneFragment {
        &self.fragment
    }

    pub(crate) fn source(&self) -> Option<(PlateId, [f64; 2])> {
        self.source
    }
}

/// The plate of the last selected part and the XY centre of the selected
/// parts with their supports: what a copy records as its source.
pub(crate) fn selection_anchor(doc: &SceneDocument) -> Option<(PlateId, [f64; 2])> {
    let ids = doc.chosen().ids();
    let primary = *ids.last()?;
    doc.read_scene(|spatial| {
        let graph = spatial.graph();
        let footprints = ids
            .iter()
            .flat_map(|&model| std::iter::once(model).chain(owned_support_ids(graph, model)))
            .filter_map(|id| {
                let part = spatial.leaf(id)?;
                let aabb = part.transformed_aabb(part.world_transform())?;
                let (min, max) = (aabb.min().coords(), aabb.max().coords());
                Some(([min[0], min[1]], [max[0], max[1]]))
            });
        let (lo, hi) = footprints.reduce(|(lo, hi), (min, max)| {
            (
                [lo[0].min(min[0]), lo[1].min(min[1])],
                [hi[0].max(max[0]), hi[1].max(max[1])],
            )
        })?;
        let centre = |axis: usize| f64::from(lo[axis] + hi[axis]) * 0.5;
        Some((graph.plate_of(primary), [centre(0), centre(1)]))
    })
    .flatten()
}

/// The selected parts and their supports as a fragment; `None` without a
/// scene, a selection, or a mesh in every selected node.
///
/// Positions are kept relative to the last selected part, whose own
/// placement is dropped (the paste places it); turns and scales stay.
/// Meshes are shared, not copied, so pasting into the same scene can
/// reuse them (`Scene::find_mesh_resource_by_arc`). A support's owner
/// note is pointed at its owner's node in the fragment.
pub(crate) fn copy_of_selection(doc: &SceneDocument) -> Option<SceneFragment> {
    let mut ids = doc.chosen().ids();
    if ids.is_empty() {
        return None;
    }
    doc.read_scene(|spatial| {
        let source = spatial.graph();
        let origin = source.world_transform_of(*ids.last()?).ok()?;
        let to_origin = match origin {
            Transform3D::Affine(placed) if !origin.is_identity() => Transform3D::Affine(
                AffineTransform3D::from_translation(placed.translation().map(|v| -v)),
            ),
            _ => Transform3D::Identity,
        };
        for model in ids.clone() {
            for support in owned_support_ids(source, model) {
                if !ids.contains(&support) {
                    ids.push(support);
                }
            }
        }
        let mut fragment = Scene::new();
        let copies: HashMap<_, _> = ids
            .iter()
            .map(|&id| (id, fragment.allocate_unbound_node_id()))
            .collect();
        for id in ids {
            let mesh = source.leaf_mesh_arc(id)?.clone();
            let resource = fragment
                .find_mesh_resource_by_arc(&mesh)
                .unwrap_or_else(|| fragment.add_mesh_resource_arc(mesh));
            let placed = to_origin.compose(&source.world_transform_of(id).ok()?);
            let mut meta = source.node(id)?.meta.clone();
            let new_owner = support_owner(source, id).and_then(|owner| copies.get(&owner));
            if let (Some(owner), Some(comment)) =
                (new_owner, meta.as_mut().and_then(|m| m.comment.as_mut()))
            {
                *comment = with_owner(comment, owner.get());
            }
            let at_end = LeafAttachment::Root {
                position: fragment.roots().len(),
            };
            fragment
                .install_leaf_at(
                    copies[&id],
                    at_end,
                    LeafPayload::MeshResource(resource),
                    placed,
                    meta,
                )
                .ok()?;
        }
        Some(SceneFragment::from_scene(fragment))
    })
    .flatten()
}

/// `comment` with its `owner = ` line replaced by one naming `owner`, last.
fn with_owner(comment: &str, owner: impl std::fmt::Display) -> String {
    let mut lines: Vec<&str> = comment
        .lines()
        .filter(|line| !line.starts_with("owner = "))
        .collect();
    lines.push("");
    format!("{}owner = {owner}\n", lines.join("\n"))
}

/// The fragment's exact bounds, which a paste at the cursor centres.
pub(crate) fn copied_box(fragment: &SceneFragment) -> Option<Aabb> {
    fragment.scene().tight_aabb().ok().flatten()
}

#[cfg(test)]
mod tests;

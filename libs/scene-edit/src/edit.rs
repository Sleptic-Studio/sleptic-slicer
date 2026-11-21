// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! One undoable change to a scene, built from what it changes and what
//! was there before, so it can be run backwards.
use std::sync::Arc;

use sc_mesh_core::{IndexedMesh, transform::Transform3D};
use sc_scene::{
    LeafPayload, MeshResourceId, NodeId, NodeKind, Scene, SceneError, SceneMetadata, node::LeafAttachment,
    plates::Plates,
};

/// Everything a leaf is, so adding it and removing it are each other's
/// undo: its id, where it hangs, what it shows, its placement and its
/// metadata.
#[derive(Clone, Debug, PartialEq)]
pub struct LeafRecord {
    pub node_id: NodeId,
    pub attachment: LeafAttachment,
    pub payload: LeafPayload,
    pub local_transform: Transform3D,
    pub meta: Option<SceneMetadata>,
}

/// A shared mesh resource and its mesh, for adding or removing it.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshRecord {
    pub rid: MeshResourceId,
    pub mesh: Arc<IndexedMesh>,
}

/// A change to a scene holding what it replaces, so the same value can be
/// applied again (redo) or turned around (undo). A mesh swap is checked by
/// allocation (the very `Arc` expected), never by comparing contents.
#[derive(Clone, Debug, PartialEq)]
pub enum SceneEdit {
    AddLeaf(LeafRecord),
    RemoveLeaf(LeafRecord),
    AddMeshResource(MeshRecord),
    RemoveMeshResource(MeshRecord),
    SetLocalTransform { node_id: NodeId, before: Transform3D, after: Transform3D },
    SetPlates { before: Plates, after: Plates },
    ReplaceMeshResource { rid: MeshResourceId, before: Arc<IndexedMesh>, after: Arc<IndexedMesh> },
    ReplaceLeafMesh { node_id: NodeId, before: Arc<IndexedMesh>, after: Arc<IndexedMesh> },
    Compound(Vec<SceneEdit>),
}

impl SceneEdit {
    /// Whether applying it would change nothing: the same value before and
    /// after, a mesh allocation swapped for itself, or a compound of such.
    pub fn is_noop(&self) -> bool {
        match self {
            Self::Compound(members) => members.iter().all(Self::is_noop),
            Self::SetLocalTransform { before, after, .. } => before == after,
            Self::SetPlates { before, after } => before == after,
            Self::ReplaceMeshResource { before, after, .. } | Self::ReplaceLeafMesh { before, after, .. } => {
                Arc::ptr_eq(before, after)
            }
            Self::AddLeaf(_) | Self::RemoveLeaf(_) | Self::AddMeshResource(_) | Self::RemoveMeshResource(_) => false,
        }
    }

    /// Whether it changes more than placements: anything but a transform
    /// change, at any depth of a compound.
    pub fn is_structural(&self) -> bool {
        match self {
            Self::SetLocalTransform { .. } => false,
            Self::Compound(members) => members.iter().any(Self::is_structural),
            _ => true,
        }
    }

    /// The edit that undoes this one. A compound is undone member by member
    /// from the last, so a resource comes back before the leaves using it
    /// and goes only after them.
    pub fn inverse(&self) -> Self {
        match self {
            Self::AddLeaf(leaf) => Self::RemoveLeaf(leaf.clone()),
            Self::RemoveLeaf(leaf) => Self::AddLeaf(leaf.clone()),
            Self::AddMeshResource(resource) => Self::RemoveMeshResource(resource.clone()),
            Self::RemoveMeshResource(resource) => Self::AddMeshResource(resource.clone()),
            &Self::SetLocalTransform { node_id, before, after } => Self::SetLocalTransform { node_id, before: after, after: before },
            Self::SetPlates { before, after } => Self::SetPlates { before: after.clone(), after: before.clone() },
            Self::ReplaceMeshResource { rid, before, after } => {
                Self::ReplaceMeshResource { rid: *rid, before: Arc::clone(after), after: Arc::clone(before) }
            }
            Self::ReplaceLeafMesh { node_id, before, after } => {
                Self::ReplaceLeafMesh { node_id: *node_id, before: Arc::clone(after), after: Arc::clone(before) }
            }
            Self::Compound(members) => Self::Compound(members.iter().rev().map(Self::inverse).collect()),
        }
    }

    /// Applies the edit. A compound is all or nothing: when a member fails,
    /// those already applied are undone, newest first, and that member's
    /// error is returned.
    pub fn apply_to_scene(&self, scene: &mut Scene) -> Result<(), SceneError> {
        match self {
            Self::AddLeaf(leaf) => scene.install_leaf_at(
                leaf.node_id,
                leaf.attachment,
                leaf.payload.clone(),
                leaf.local_transform,
                leaf.meta.clone(),
            ),
            Self::RemoveLeaf(leaf) => scene.remove_leaf(leaf.node_id).map(drop),
            Self::AddMeshResource(resource) => scene.install_mesh_resource_at(resource.rid, Arc::clone(&resource.mesh)),
            Self::RemoveMeshResource(resource) => scene.remove_mesh_resource(resource.rid).map(drop),
            &Self::SetLocalTransform { node_id, after, .. } => scene.set_local_transform(node_id, after),
            Self::SetPlates { after, .. } => match scene.set_plates(after.clone()) {
                true => Ok(()),
                false => Err(SceneError::InvalidPlates),
            },
            Self::ReplaceMeshResource { rid, before, after } => scene.replace_mesh_resource(*rid, before, Arc::clone(after)),
            Self::ReplaceLeafMesh { node_id, before, after } => {
                scene.replace_inline_leaf_mesh(*node_id, before, Arc::clone(after))
            }
            Self::Compound(members) => apply_all_or_none(members, scene),
        }
    }

    /// The edit removing leaf `node_id`, which remembers where the leaf sat
    /// among its siblings so undo puts it back there. When the leaf was the
    /// last user of a shared mesh resource, the resource goes too, after
    /// it. None when `node_id` is not a leaf of `scene`.
    pub fn remove_leaf_with_gc(scene: &Scene, node_id: NodeId) -> Option<Self> {
        let node = scene.node(node_id)?;
        let NodeKind::Leaf(payload) = &node.kind else {
            return None;
        };
        let leaf = Self::RemoveLeaf(LeafRecord {
            node_id,
            attachment: scene.attachment_of(node_id)?,
            payload: payload.clone(),
            local_transform: node.local_transform,
            meta: node.meta.clone(),
        });
        match payload {
            LeafPayload::MeshResource(rid) if !scene.other_leaf_references_resource(*rid, node_id) => {
                let mesh = Arc::clone(scene.mesh_resource_arc(*rid)?);
                Some(Self::Compound(vec![leaf, Self::RemoveMeshResource(MeshRecord { rid: *rid, mesh })]))
            }
            _ => Some(leaf),
        }
    }
}

/// Applies `members` in order; on the first failure the ones applied are
/// undone, newest first (best effort: the caller keeps its own snapshot,
/// and the first failure is what it needs to see).
fn apply_all_or_none(members: &[SceneEdit], scene: &mut Scene) -> Result<(), SceneError> {
    for (count, member) in members.iter().enumerate() {
        if let Err(failure) = member.apply_to_scene(scene) {
            for applied in members[..count].iter().rev() {
                let _ = applied.inverse().apply_to_scene(scene);
            }
            return Err(failure);
        }
    }
    Ok(())
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! What a scene is built from: nodes, what each kind holds, and the ids
//! that point at nodes and shared resources.
use sc_mesh_core::{IndexedMesh, transform::Transform3D};
use std::{num::NonZeroU32, sync::Arc};

/// A node in the scene: what it is, where it sits relative to its parent,
/// and what the file called it.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub kind: NodeKind,
    pub local_transform: Transform3D,
    pub meta: Option<SceneMetadata>,
}

/// The four things a node can be. Groups and CSG operations have children
/// (in order: for a difference, the first child is what gets cut).
#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    Leaf(LeafPayload),
    Group { children: Vec<NodeId> },
    Reference { resource: SubtreeResourceId },
    Csg { children: Vec<NodeId>, op: CsgOp },
}

impl Node {
    /// The ordered children of a group or CSG node; none for the others.
    pub(crate) fn children(&self) -> Option<&Vec<NodeId>> {
        let (NodeKind::Group { children } | NodeKind::Csg { children, .. }) = &self.kind else {
            return None;
        };
        Some(children)
    }

    pub(crate) fn children_mut(&mut self) -> Option<&mut Vec<NodeId>> {
        let (NodeKind::Group { children } | NodeKind::Csg { children, .. }) = &mut self.kind else {
            return None;
        };
        Some(children)
    }
}

/// A leaf's geometry: a mesh of its own, or one the scene shares between
/// leaves.
#[derive(Clone, Debug, PartialEq)]
pub enum LeafPayload {
    MeshResource(MeshResourceId),
    Mesh(Arc<IndexedMesh>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsgOp {
    Difference,
    Intersection,
    Union,
}

/// A node's name and comment, as the file had them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SceneMetadata {
    pub comment: Option<String>,
    pub name: Option<String>,
}

/// A leaf taken out of the scene, with everything needed to put it back
/// where it was.
#[derive(Clone, Debug, PartialEq)]
pub struct RemovedLeaf {
    pub payload: LeafPayload,
    pub meta: Option<SceneMetadata>,
    pub local_transform: Transform3D,
    pub attachment: LeafAttachment,
}

/// Where a leaf hangs: its place among the roots, or among a parent's
/// children.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LeafAttachment {
    Parent { parent: NodeId, position: usize },
    Root { position: usize },
}

/// An id that is never zero, so `Option<Id>` costs nothing. Ids for things
/// kept in a list (`slot`) also give the list position: the id minus one.
macro_rules! scene_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub(crate) NonZeroU32);

        impl $name {
            pub fn get(self) -> u32 {
                self.0.get()
            }
        }
    };
    ($(#[$doc:meta])* $name:ident, slot) => {
        scene_id!($(#[$doc])* $name);

        impl $name {
            pub fn index(self) -> usize {
                (self.0.get() - 1) as usize
            }
        }
    };
}

scene_id!(
    /// A node; also its position in the scene's node list.
    NodeId, slot
);
scene_id!(
    /// A mesh the scene shares between leaves; also its position in the
    /// scene's mesh list.
    MeshResourceId, slot
);
scene_id!(
    /// A subtree that reference nodes repeat.
    SubtreeResourceId
);

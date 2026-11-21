// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Compatibility errors with stable diagnostic text.
use crate::node::*;
use sc_mesh_core::transform::TransformError;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SceneError {
    InvalidPlates,
    DanglingNodeId {
        id: NodeId,
    },
    DanglingMeshResource {
        id: MeshResourceId,
    },
    DanglingSubtreeResource {
        id: SubtreeResourceId,
    },
    EmptyIntersection {
        node_id: NodeId,
    },
    ExpansionCycle {
        node_id: NodeId,
    },
    ParentMismatch {
        node_id: NodeId,
        recorded: Option<NodeId>,
    },
    AlreadyHasParent {
        node_id: NodeId,
        parent: NodeId,
    },
    AlreadyARoot {
        node_id: NodeId,
    },
    AlreadyASubtreeResourceRoot {
        node_id: NodeId,
    },
    DuplicateChild {
        parent: NodeId,
        child: NodeId,
    },
    NotABranch {
        node_id: NodeId,
    },
    NotALeaf {
        node_id: NodeId,
    },
    NodeSlotOccupied {
        id: NodeId,
    },
    NotARoot {
        node_id: NodeId,
    },
    AttachmentPositionOutOfRange {
        position: usize,
        len: usize,
    },
    NotAnInlineMeshLeaf {
        node_id: NodeId,
    },
    MeshResourceSlotOccupied {
        id: MeshResourceId,
    },
    MeshResourceInUse {
        id: MeshResourceId,
    },
    MeshResourceMismatch {
        id: MeshResourceId,
    },
    InlineMeshMismatch {
        node_id: NodeId,
    },
    UnevaluatedCsg {
        node_id: NodeId,
    },
    UnexpandedReference {
        node_id: NodeId,
    },
    Transform(TransformError),
}
impl std::fmt::Display for SceneError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Most messages are about one node: "node N <what>".
        let about_node = |node: &NodeId, what: &str| format!("node {} {what}", node.get());
        let text = match self {
            Self::InvalidPlates => "invalid project plate table".to_owned(),
            Self::Transform(inner) => format!("transform error: {inner}"),
            Self::AttachmentPositionOutOfRange { position: at, len: count } => {
                format!("attachment position {at} is past the end of a list of {count}")
            }
            Self::DanglingNodeId { id } => format!("dangling node id {}", id.get()),
            Self::DanglingMeshResource { id } => format!("dangling mesh resource id {}", id.get()),
            Self::DanglingSubtreeResource { id } => format!("dangling subtree resource id {}", id.get()),
            Self::EmptyIntersection { node_id: node } => format!("empty intersection at node {}", node.get()),
            Self::ExpansionCycle { node_id: node } => {
                format!("cycle in the expansion graph reachable from node {}", node.get())
            }
            Self::ParentMismatch { node_id: node, recorded } => {
                let seen = recorded.map_or_else(|| "None".to_owned(), |p| format!("Some({})", p.get()));
                format!("parent mismatch at node {}: recorded {seen}", node.get())
            }
            Self::DuplicateChild { parent, child } => {
                format!("duplicate child {} under parent {}", child.get(), parent.get())
            }
            Self::UnevaluatedCsg { node_id: node } => format!("unevaluated CSG node {}", node.get()),
            Self::UnexpandedReference { node_id: node } => format!("unexpanded Reference node {}", node.get()),
            Self::NodeSlotOccupied { id } => format!("node slot {} is already occupied", id.get()),
            Self::AlreadyHasParent { node_id: node, parent } => {
                about_node(node, &format!("already has parent {}", parent.get()))
            }
            Self::AlreadyARoot { node_id: node } => about_node(node, "is already a scene root"),
            Self::NotARoot { node_id: node } => about_node(node, "is not a scene root"),
            Self::NotABranch { node_id: node } => about_node(node, "is not a Group or Csg branch"),
            Self::NotALeaf { node_id: node } => about_node(node, "is not a Leaf"),
            Self::AlreadyASubtreeResourceRoot { node_id: node } => {
                about_node(node, "is already a subtree resource root")
            }
            Self::NotAnInlineMeshLeaf { node_id: node } => {
                about_node(node, "is a leaf but not an inline-mesh leaf")
            }
            Self::InlineMeshMismatch { node_id: node } => {
                about_node(node, "does not hold the inline mesh the swap expected")
            }
            Self::MeshResourceSlotOccupied { id } => format!("mesh resource slot {} is already occupied", id.get()),
            Self::MeshResourceInUse { id } => {
                format!("mesh resource {} is still referenced by at least one leaf", id.get())
            }
            Self::MeshResourceMismatch { id } => {
                format!("mesh resource {} does not hold the mesh the swap expected", id.get())
            }
        };
        out.write_str(&text)
    }
}
impl std::error::Error for SceneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transform(inner) => Some(inner),
            _ => None,
        }
    }
}
impl From<TransformError> for SceneError {
    fn from(inner: TransformError) -> Self {
        Self::Transform(inner)
    }
}

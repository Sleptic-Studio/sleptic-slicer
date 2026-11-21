// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Every `SceneError` message, word for word: callers wrap and show them, so
//! rewording one is a change to review, not a side effect.
use super::*;
use sc_mesh_core::transform::TransformError;

fn node(n: usize) -> NodeId {
    NodeId(number(n, "test"))
}

fn mesh_res(n: usize) -> MeshResourceId {
    MeshResourceId(number(n, "test"))
}

#[test]
fn scene_error_messages() {
    let cases: Vec<(SceneError, &str)> = vec![
        (SceneError::InvalidPlates, "invalid project plate table"),
        (SceneError::DanglingNodeId { id: node(2) }, "dangling node id 3"),
        (SceneError::DanglingMeshResource { id: mesh_res(2) }, "dangling mesh resource id 3"),
        (
            SceneError::DanglingSubtreeResource { id: SubtreeResourceId(number(2, "test")) },
            "dangling subtree resource id 3",
        ),
        (SceneError::EmptyIntersection { node_id: node(4) }, "empty intersection at node 5"),
        (
            SceneError::ExpansionCycle { node_id: node(4) },
            "cycle in the expansion graph reachable from node 5",
        ),
        (
            SceneError::ParentMismatch { node_id: node(4), recorded: Some(node(1)) },
            "parent mismatch at node 5: recorded Some(2)",
        ),
        (
            SceneError::ParentMismatch { node_id: node(4), recorded: None },
            "parent mismatch at node 5: recorded None",
        ),
        (SceneError::AlreadyHasParent { node_id: node(4), parent: node(1) }, "node 5 already has parent 2"),
        (SceneError::AlreadyARoot { node_id: node(4) }, "node 5 is already a scene root"),
        (
            SceneError::AlreadyASubtreeResourceRoot { node_id: node(4) },
            "node 5 is already a subtree resource root",
        ),
        (SceneError::DuplicateChild { parent: node(1), child: node(4) }, "duplicate child 5 under parent 2"),
        (SceneError::NotABranch { node_id: node(4) }, "node 5 is not a Group or Csg branch"),
        (SceneError::NotALeaf { node_id: node(4) }, "node 5 is not a Leaf"),
        (SceneError::NodeSlotOccupied { id: node(4) }, "node slot 5 is already occupied"),
        (SceneError::NotARoot { node_id: node(4) }, "node 5 is not a scene root"),
        (
            SceneError::AttachmentPositionOutOfRange { position: 6, len: 2 },
            "attachment position 6 is past the end of a list of 2",
        ),
        (
            SceneError::NotAnInlineMeshLeaf { node_id: node(4) },
            "node 5 is a leaf but not an inline-mesh leaf",
        ),
        (SceneError::MeshResourceSlotOccupied { id: mesh_res(2) }, "mesh resource slot 3 is already occupied"),
        (
            SceneError::MeshResourceInUse { id: mesh_res(2) },
            "mesh resource 3 is still referenced by at least one leaf",
        ),
        (
            SceneError::MeshResourceMismatch { id: mesh_res(2) },
            "mesh resource 3 does not hold the mesh the swap expected",
        ),
        (
            SceneError::InlineMeshMismatch { node_id: node(4) },
            "node 5 does not hold the inline mesh the swap expected",
        ),
        (SceneError::UnevaluatedCsg { node_id: node(4) }, "unevaluated CSG node 5"),
        (SceneError::UnexpandedReference { node_id: node(4) }, "unexpanded Reference node 5"),
    ];
    for (error, text) in cases {
        assert_eq!(error.to_string(), text);
    }
    let transform = TransformError::ZeroAxis;
    let expected = format!("transform error: {transform}");
    assert_eq!(SceneError::Transform(transform).to_string(), expected);
}

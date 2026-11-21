// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_sl1cer::error::{InvalidLeafGeometry, LeafValidationFailure, SliceError, SliceSceneError};
use sc_scene::Scene;
use std::error::Error;

#[test]
fn error_rendering_contract() {
    let id = Scene::new().add_group(None, Default::default());
    let nested = SliceSceneError::LeafSlicingFailed { leaf: id, leaf_name: Some("part".into()), error: SliceError::EmptyScene };
    assert_eq!(nested.to_string(), "slicing leaf #1 (\"part\") failed: scene is empty: no nodes to slice");
    assert_eq!(nested.source().unwrap().to_string(), "scene is empty: no nodes to slice");
    let failures = SliceSceneError::LeavesValidationFailed(vec![LeafValidationFailure { leaf: id, leaf_name: None, geometry: InvalidLeafGeometry::Faces(vec![]) }]);
    assert_eq!(failures.to_string(), "mesh validation failed for 1 of the scene's leaves:\nleaf #1: mesh face validation failed: 0 error(s)");
    assert!(failures.source().is_none());
}


#[test]
fn validation_summary_groups_and_limits_examples() {
    use sc_mesh_core::indexed_mesh::{MeshFaceError, MeshVertexError};
    use sc_mesh_core::primitives::{CoordAxis, TriVertexSlot};
    let faces = (0..6).map(|i| MeshFaceError::OutOfRangeVertexIndex {
        face_idx: i.into(), vertex_pos: TriVertexSlot::ALL[0], vertex_idx: 9u32.into(),
    }).collect();
    assert_eq!(InvalidLeafGeometry::Faces(faces).to_string(), "mesh face validation failed: 6 error(s)\n  6x OutOfRangeVertexIndex (e.g. faces #0, #1, #2, #3, #4)");
    let vertices = vec![MeshVertexError::NaNCoordinate { vertex_idx: 0, coord: CoordAxis::ALL[0] }, MeshVertexError::InfiniteCoordinate { vertex_idx: 1, coord: CoordAxis::ALL[1] }, MeshVertexError::NaNCoordinate { vertex_idx: 2, coord: CoordAxis::ALL[2] }];
    assert_eq!(InvalidLeafGeometry::Vertices(vertices).to_string(), "mesh vertex validation failed: 3 error(s)\n  2x NaNCoordinate\n  1x InfiniteCoordinate");
}

#[test]
fn node_diagnostics_preserve_exact_messages() {
    let node_id = Scene::new().add_group(None, Default::default());
    let cases = [
        (SliceError::NodeNotFound { node_id }, "node #1 not found in scene"),
        (SliceError::NotALeaf { node_id }, "node #1 is a Group, expected a Leaf carrying a mesh"),
        (SliceError::CsgNotYetSupported { node_id }, "node #1 is a CSG combinator; CSG slicing is not yet supported"),
        (SliceError::SubtreeReferenceNotYetSupported { node_id }, "node #1 is a subtree reference; slicing scenes with subtree references is not yet supported"),
    ];
    for (error, expected) in cases { assert_eq!(error.to_string(), expected); }
}

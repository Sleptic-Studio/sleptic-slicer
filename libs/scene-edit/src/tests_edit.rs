// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
use sc_mesh_core::{
    IndexedMesh, Vertex,
    transform::{AffineTransform3D, Transform3D},
};
use sc_scene::{LeafPayload, Scene, SceneError, SceneMetadata, node::LeafAttachment};
use std::sync::Arc;
fn mesh(x: f32) -> Arc<IndexedMesh> {
    Arc::new(IndexedMesh::new(
        None,
        vec![Vertex::new([x, 0., 0.])],
        vec![],
    ))
}
fn shift(x: f64) -> Transform3D {
    Transform3D::Affine(AffineTransform3D::from_translation([x, 0., 0.]))
}
fn leaf(scene: &mut Scene, payload: LeafPayload) -> sc_scene::NodeId {
    let id = scene
        .add_leaf(
            Some(SceneMetadata {
                name: Some("part".into()),
                comment: Some("note".into()),
            }),
            shift(2.),
            payload,
        )
        .unwrap();
    scene.add_root(id).unwrap();
    id
}
#[test]
fn fragment_shares_allocations_and_preserves_transforms() {
    let m = mesh(3.);
    let f = SceneFragment::from_leaf_snapshot(m.clone(), shift(5.));
    assert_eq!(f.scene().roots().len(), 1);
    assert_eq!(f.scene().mesh_resources_len(), 1);
    let id = f.scene().roots()[0];
    assert!(Arc::ptr_eq(f.scene().leaf_mesh_arc(id).unwrap(), &m));
    assert_eq!(f.scene().node(id).unwrap().local_transform, shift(5.));
    assert!(f.scene().node(id).unwrap().meta.is_none());
    let copy = f.clone();
    assert_eq!(copy, f);
    assert!(Arc::ptr_eq(copy.scene().leaf_mesh_arc(id).unwrap(), &m));
    assert_eq!(SceneFragment::from_scene(copy.into_scene()), f);
}
#[test]
fn fragment_deduplicates_only_allocation_identity() {
    let m = mesh(0.);
    let other = mesh(0.);
    let f = SceneFragment::from_leaf_snapshots([
        (m.clone(), shift(1.)),
        (m, shift(2.)),
        (other, shift(3.)),
    ]);
    assert_eq!(f.scene().mesh_resources_len(), 2);
    assert_eq!(f.scene().roots().len(), 3);
    assert!(
        SceneFragment::from_leaf_snapshots([])
            .scene()
            .roots()
            .is_empty()
    );
}
#[test]
fn replacement_identity_and_classification() {
    let a = mesh(0.);
    let equal = mesh(0.);
    let mut scene = Scene::new();
    let id = leaf(&mut scene, LeafPayload::Mesh(a.clone()));
    let edit = SceneEdit::ReplaceLeafMesh {
        node_id: id,
        before: a.clone(),
        after: equal.clone(),
    };
    assert!(!edit.is_noop());
    assert!(edit.is_structural());
    assert_eq!(edit.inverse().inverse(), edit);
    edit.apply_to_scene(&mut scene).unwrap();
    assert!(Arc::ptr_eq(scene.leaf_mesh_arc(id).unwrap(), &equal));
    assert_eq!(
        edit.apply_to_scene(&mut scene),
        Err(SceneError::InlineMeshMismatch { node_id: id })
    );
    edit.inverse().apply_to_scene(&mut scene).unwrap();
    assert!(Arc::ptr_eq(scene.leaf_mesh_arc(id).unwrap(), &a));
    assert!(
        SceneEdit::ReplaceLeafMesh {
            node_id: id,
            before: a.clone(),
            after: a
        }
        .is_noop()
    );
}
#[test]
fn compound_rollback_restores_roots_metadata_and_identity() {
    let mut s = Scene::new();
    let a = leaf(&mut s, LeafPayload::Mesh(mesh(0.)));
    let b = leaf(&mut s, LeafPayload::Mesh(mesh(1.)));
    let empty = s.allocate_unbound_node_id();
    let original = s.clone();
    let edit = SceneEdit::Compound(vec![
        SceneEdit::remove_leaf_with_gc(&s, a).unwrap(),
        SceneEdit::Compound(vec![
            SceneEdit::SetLocalTransform {
                node_id: b,
                before: shift(2.),
                after: shift(9.),
            },
            SceneEdit::SetLocalTransform {
                node_id: empty,
                before: shift(0.),
                after: shift(1.),
            },
        ]),
    ]);
    assert_eq!(
        edit.apply_to_scene(&mut s),
        Err(SceneError::DanglingNodeId { id: empty })
    );
    assert_eq!(s, original);
    assert!(Arc::ptr_eq(
        s.leaf_mesh_arc(a).unwrap(),
        original.leaf_mesh_arc(a).unwrap()
    ));
}
#[test]
fn gc_last_reference_and_inverse_order() {
    let mut s = Scene::new();
    let m = mesh(2.);
    let rid = s.add_mesh_resource_arc(m.clone());
    let id = leaf(&mut s, LeafPayload::MeshResource(rid));
    let original = s.clone();
    let edit = SceneEdit::remove_leaf_with_gc(&s, id).unwrap();
    assert!(
        matches!(&edit,SceneEdit::Compound(v) if matches!(v.as_slice(),[SceneEdit::RemoveLeaf(LeafRecord {..}),SceneEdit::RemoveMeshResource(MeshRecord {..})]))
    );
    edit.apply_to_scene(&mut s).unwrap();
    assert!(s.mesh_resource(rid).is_none());
    edit.inverse().apply_to_scene(&mut s).unwrap();
    assert_eq!(s, original);
    assert!(Arc::ptr_eq(s.mesh_resource_arc(rid).unwrap(), &m));
}
#[test]
fn unreachable_reference_prevents_gc_and_removal() {
    let mut s = Scene::new();
    let rid = s.add_mesh_resource_arc(mesh(0.));
    let id = leaf(&mut s, LeafPayload::MeshResource(rid));
    s.add_leaf(None, Transform3D::Identity, LeafPayload::MeshResource(rid))
        .unwrap();
    assert!(matches!(
        SceneEdit::remove_leaf_with_gc(&s, id),
        Some(SceneEdit::RemoveLeaf(LeafRecord { .. }))
    ));
    assert!(matches!(
        s.remove_mesh_resource(rid),
        Err(SceneError::MeshResourceInUse { .. })
    ));
}
#[test]
fn attachment_restoration_for_group_and_csg() {
    for csg in [false, true] {
        let mut s = Scene::new();
        let parent = if csg {
            s.add_csg(None, Transform3D::Identity, sc_scene::CsgOp::Union, vec![])
                .unwrap()
        } else {
            s.add_group(None, Transform3D::Identity)
        };
        s.add_root(parent).unwrap();
        let mut children = vec![];
        for _ in 0..3 {
            let id = s
                .add_leaf(None, shift(1.), LeafPayload::Mesh(mesh(0.)))
                .unwrap();
            s.add_child(parent, id).unwrap();
            children.push(id);
        }
        let before = s.clone();
        let edit = SceneEdit::remove_leaf_with_gc(&s, children[1]).unwrap();
        edit.apply_to_scene(&mut s).unwrap();
        edit.inverse().apply_to_scene(&mut s).unwrap();
        assert_eq!(s, before);
    }
}
#[test]
fn add_validation_precedence_and_atomicity() {
    let mut s = Scene::new();
    let occupied = leaf(&mut s, LeafPayload::Mesh(mesh(0.)));
    let empty = s.allocate_unbound_node_id();
    let rid = s.allocate_unbound_mesh_resource_id();
    let add = |id| SceneEdit::AddLeaf(LeafRecord {
        node_id: id,
        attachment: LeafAttachment::Root { position: 99 },
        payload: LeafPayload::MeshResource(rid),
        local_transform: shift(0.),
        meta: None,
    });
    let original = s.clone();
    assert_eq!(
        add(occupied).apply_to_scene(&mut s),
        Err(SceneError::NodeSlotOccupied { id: occupied })
    );
    assert_eq!(
        add(empty).apply_to_scene(&mut s),
        Err(SceneError::DanglingMeshResource { id: rid })
    );
    assert_eq!(s, original);
    SceneEdit::AddMeshResource(MeshRecord {
        rid,
        mesh: mesh(0.),
    })
    .apply_to_scene(&mut s)
    .unwrap();
    assert_eq!(
        add(empty).apply_to_scene(&mut s),
        Err(SceneError::AttachmentPositionOutOfRange {
            position: 99,
            len: 1
        })
    );
}
#[test]
fn setters_ignore_before_and_remove_uses_only_id() {
    let mut s = Scene::new();
    let id = leaf(&mut s, LeafPayload::Mesh(mesh(0.)));
    SceneEdit::SetLocalTransform {
        node_id: id,
        before: shift(99.),
        after: shift(7.),
    }
    .apply_to_scene(&mut s)
    .unwrap();
    assert_eq!(s.node(id).unwrap().local_transform, shift(7.));
    SceneEdit::RemoveLeaf(LeafRecord {
        node_id: id,
        attachment: LeafAttachment::Root { position: 99 },
        payload: LeafPayload::Mesh(mesh(99.)),
        local_transform: shift(99.),
        meta: None,
    })
    .apply_to_scene(&mut s)
    .unwrap();
    assert!(s.node(id).is_none());
}
#[test]
fn compound_classification_and_paste_roundtrip() {
    let empty = SceneEdit::Compound(vec![]);
    assert!(empty.is_noop());
    assert!(!empty.is_structural());
    let mut s = Scene::new();
    let id = s.allocate_unbound_node_id();
    let rid = s.allocate_unbound_mesh_resource_id();
    let original = s.clone();
    let edit = SceneEdit::Compound(vec![
        SceneEdit::AddMeshResource(MeshRecord {
            rid,
            mesh: mesh(0.),
        }),
        SceneEdit::AddLeaf(LeafRecord {
            node_id: id,
            attachment: LeafAttachment::Root { position: 0 },
            payload: LeafPayload::MeshResource(rid),
            local_transform: shift(3.),
            meta: None,
        }),
    ]);
    edit.apply_to_scene(&mut s).unwrap();
    edit.inverse().apply_to_scene(&mut s).unwrap();
    assert_eq!(s, original);
    assert_eq!(edit.inverse().inverse(), edit);
}

#[test]
fn plate_validation_noop_and_undo() {
    let mut s = Scene::new();
    let before = s.plates().clone();
    let mut after = before.clone();
    after.add();
    let edit = SceneEdit::SetPlates {
        before: before.clone(),
        after: after.clone(),
    };
    assert!(!edit.is_noop());
    assert!(edit.is_structural());
    edit.apply_to_scene(&mut s).unwrap();
    assert_eq!(s.plates(), &after);
    edit.inverse().apply_to_scene(&mut s).unwrap();
    assert_eq!(s.plates(), &before);
    let mut invalid = before.clone();
    invalid.order.clear();
    assert_eq!(
        SceneEdit::SetPlates {
            before: after,
            after: invalid
        }
        .apply_to_scene(&mut s),
        Err(SceneError::InvalidPlates)
    );
    assert_eq!(s.plates(), &before);
    assert!(
        SceneEdit::SetPlates {
            before: before.clone(),
            after: before
        }
        .is_noop()
    );
}
#[test]
fn resource_preconditions_and_error_texts() {
    let mut s = Scene::new();
    let rid = s.allocate_unbound_mesh_resource_id();
    let a = mesh(0.);
    let equal = mesh(0.);
    let replacement = SceneEdit::ReplaceMeshResource {
        rid,
        before: a.clone(),
        after: mesh(2.),
    };
    assert_eq!(
        replacement.apply_to_scene(&mut s),
        Err(SceneError::DanglingMeshResource { id: rid })
    );
    let add = SceneEdit::AddMeshResource(MeshRecord {
        rid,
        mesh: equal.clone(),
    });
    add.apply_to_scene(&mut s).unwrap();
    assert_eq!(
        add.apply_to_scene(&mut s),
        Err(SceneError::MeshResourceSlotOccupied { id: rid })
    );
    assert_eq!(
        replacement.apply_to_scene(&mut s),
        Err(SceneError::MeshResourceMismatch { id: rid })
    );
    let id = leaf(&mut s, LeafPayload::MeshResource(rid));
    assert_eq!(
        SceneEdit::ReplaceLeafMesh {
            node_id: id,
            before: a,
            after: equal
        }
        .apply_to_scene(&mut s),
        Err(SceneError::NotAnInlineMeshLeaf { node_id: id })
    );
    let error = SceneEdit::RemoveMeshResource(MeshRecord {
        rid,
        mesh: mesh(99.),
    })
    .apply_to_scene(&mut s)
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        format!(
            "mesh resource {} is still referenced by at least one leaf",
            rid.get()
        )
    );
    s.remove_leaf(id).unwrap();
    SceneEdit::RemoveMeshResource(MeshRecord {
        rid,
        mesh: mesh(99.),
    })
    .apply_to_scene(&mut s)
    .unwrap();
    assert!(s.mesh_resource(rid).is_none());
}
#[test]
fn missing_nonleaf_unattached_and_parent_errors() {
    let mut s = Scene::new();
    let group = s.add_group(None, Transform3D::Identity);
    let hidden = s
        .add_leaf(None, Transform3D::Identity, LeafPayload::Mesh(mesh(0.)))
        .unwrap();
    let empty = s.allocate_unbound_node_id();
    assert!(SceneEdit::remove_leaf_with_gc(&s, group).is_none());
    assert!(SceneEdit::remove_leaf_with_gc(&s, hidden).is_none());
    let removal = |node_id| SceneEdit::RemoveLeaf(LeafRecord {
        node_id,
        attachment: LeafAttachment::Root { position: 0 },
        payload: LeafPayload::Mesh(mesh(0.)),
        local_transform: Transform3D::Identity,
        meta: None,
    });
    assert_eq!(
        removal(group).apply_to_scene(&mut s),
        Err(SceneError::NotALeaf { node_id: group })
    );
    assert_eq!(
        removal(hidden).apply_to_scene(&mut s),
        Err(SceneError::NotARoot { node_id: hidden })
    );
    assert_eq!(
        removal(empty).apply_to_scene(&mut s),
        Err(SceneError::DanglingNodeId { id: empty })
    );
    let add = |parent| SceneEdit::AddLeaf(LeafRecord {
        node_id: empty,
        attachment: LeafAttachment::Parent {
            parent,
            position: 1,
        },
        payload: LeafPayload::Mesh(mesh(0.)),
        local_transform: Transform3D::Identity,
        meta: None,
    });
    assert_eq!(
        add(hidden).apply_to_scene(&mut s),
        Err(SceneError::NotABranch { node_id: hidden })
    );
    assert_eq!(
        add(empty).apply_to_scene(&mut s),
        Err(SceneError::DanglingNodeId { id: empty })
    );
    assert_eq!(
        add(group).apply_to_scene(&mut s),
        Err(SceneError::AttachmentPositionOutOfRange {
            position: 1,
            len: 0
        })
    );
    let mut foreign = Scene::new();
    let mut beyond = foreign.allocate_unbound_node_id();
    for _ in 0..10 {
        beyond = foreign.allocate_unbound_node_id();
    }
    assert_eq!(
        removal(beyond).inverse().apply_to_scene(&mut s),
        Err(SceneError::DanglingNodeId { id: beyond })
    );
    SceneEdit::SetLocalTransform {
        node_id: group,
        before: shift(99.),
        after: shift(4.),
    }
    .apply_to_scene(&mut s)
    .unwrap();
    assert_eq!(s.node(group).unwrap().local_transform, shift(4.));
}

#[test]
fn classification_and_inverse_preserve_nested_edit_order() {
    let mut scene = Scene::new();
    let id = leaf(&mut scene, LeafPayload::Mesh(mesh(1.)));
    let stationary = SceneEdit::SetLocalTransform {
        node_id: id,
        before: shift(1.),
        after: shift(1.),
    };
    let moved = SceneEdit::SetLocalTransform {
        node_id: id,
        before: shift(1.),
        after: shift(2.),
    };
    let replacement = SceneEdit::ReplaceLeafMesh {
        node_id: id,
        before: mesh(1.),
        after: mesh(1.),
    };
    assert!(stationary.is_noop());
    assert!(!stationary.is_structural());
    assert!(!moved.is_noop());
    assert!(!moved.is_structural());
    assert!(!replacement.is_noop());
    assert!(replacement.is_structural());
    assert!(SceneEdit::Compound(vec![stationary.clone()]).is_noop());
    assert!(!SceneEdit::Compound(vec![moved.clone()]).is_noop());
    assert!(!SceneEdit::Compound(vec![moved.clone()]).is_structural());
    assert!(SceneEdit::Compound(vec![replacement.clone()]).is_structural());
    let nested = SceneEdit::Compound(vec![
        stationary.clone(),
        SceneEdit::Compound(vec![moved.clone(), replacement.clone()]),
    ]);
    assert!(!nested.is_noop());
    assert!(nested.is_structural());
    assert_eq!(
        nested.inverse(),
        SceneEdit::Compound(vec![
            SceneEdit::Compound(vec![replacement.inverse(), moved.inverse()]),
            stationary.inverse()
        ])
    );
    assert_eq!(nested.inverse().inverse(), nested);
}

#[test]
fn inverse_leaf_capture_contract() {
    let mut scene = Scene::new();
    let shared = mesh(4.);
    let id = leaf(&mut scene, LeafPayload::Mesh(shared.clone()));
    let original = SceneEdit::AddLeaf(LeafRecord {
        node_id: id,
        attachment: LeafAttachment::Root { position: 7 },
        payload: LeafPayload::Mesh(shared.clone()),
        local_transform: shift(8.),
        meta: Some(SceneMetadata {
            name: Some("capture".into()),
            comment: Some("detail".into()),
        }),
    });
    let inverse = original.inverse();
    match &inverse {
        SceneEdit::RemoveLeaf(LeafRecord {
            node_id,
            attachment,
            payload: LeafPayload::Mesh(actual),
            local_transform,
            meta,
        }) => {
            assert_eq!(*node_id, id);
            assert_eq!(*attachment, LeafAttachment::Root { position: 7 });
            assert!(Arc::ptr_eq(actual, &shared));
            assert_eq!(*local_transform, shift(8.));
            assert_eq!(meta.as_ref().unwrap().name.as_deref(), Some("capture"));
            assert_eq!(meta.as_ref().unwrap().comment.as_deref(), Some("detail"));
        }
        other => panic!("unexpected inverse: {other:?}"),
    }
    assert_eq!(inverse.inverse(), original);
}

#[test]
fn classification_resource_identity_contract() {
    let mut scene = Scene::new();
    let rid = scene.allocate_unbound_mesh_resource_id();
    let shared = mesh(1.0);
    let same = SceneEdit::ReplaceMeshResource {
        rid,
        before: shared.clone(),
        after: shared.clone(),
    };
    let equal = SceneEdit::ReplaceMeshResource {
        rid,
        before: shared.clone(),
        after: mesh(1.0),
    };
    let add = SceneEdit::AddMeshResource(MeshRecord { rid, mesh: shared });
    assert!(same.is_noop());
    assert!(!equal.is_noop());
    assert!(same.is_structural());
    assert!(!add.is_noop());
    let noops = SceneEdit::Compound(vec![same, SceneEdit::Compound(vec![])]);
    assert!(noops.is_noop());
    assert!(noops.is_structural());
    let node_id = leaf(&mut scene, LeafPayload::Mesh(mesh(0.0)));
    let transform = SceneEdit::SetLocalTransform {
        node_id,
        before: shift(0.0),
        after: shift(1.0),
    };
    assert!(SceneEdit::Compound(vec![transform, equal.clone()]).is_structural());
    let mixed = SceneEdit::Compound(vec![noops, equal, add]);
    assert!(!mixed.is_noop());
    assert!(mixed.is_structural());
}

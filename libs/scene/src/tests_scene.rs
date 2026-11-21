// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
use crate::{
    mesh_targets::MeshTarget,
    node::LeafAttachment,
    plates::{Plate, Plates},
};
use sc_mesh_core::{
    IndexedTriangle, Normal, Vertex,
    indexed_mesh::{MeshMetadata, RepairPasses},
    transform::AffineTransform3D,
};
use std::{collections::BTreeMap, mem::size_of};
fn mesh() -> IndexedMesh {
    IndexedMesh::new(
        None,
        vec![
            Vertex::new([0., 0., 0.]),
            Vertex::new([2., 0., 0.]),
            Vertex::new([0., 2., 0.]),
        ],
        vec![IndexedTriangle::from_raw_indices(
            Normal::new([0., 0., 1.]),
            [0, 1, 2],
        )],
    )
}
fn leaf(s: &mut Scene) -> NodeId {
    s.add_leaf(
        None,
        Transform3D::Identity,
        LeafPayload::Mesh(Arc::new(mesh())),
    )
    .unwrap()
}
fn shift(x: f64, y: f64, z: f64) -> Transform3D {
    Transform3D::Affine(AffineTransform3D::from_translation([x, y, z]))
}
#[test]
fn identity_layout_and_allocation_history() {
    assert_eq!(size_of::<Option<NodeId>>(), 4);
    assert_eq!(size_of::<Option<MeshResourceId>>(), 4);
    assert_eq!(size_of::<Option<SubtreeResourceId>>(), 4);
    let mut s = Scene::new();
    assert_eq!(s, Scene::default());
    let reserved = s.allocate_unbound_node_id();
    let id = leaf(&mut s);
    assert_eq!(
        (reserved.get(), id.get(), id.index(), s.nodes_len()),
        (1, 2, 1, 2)
    );
    assert!(s.node(reserved).is_none());
    let r = s.allocate_unbound_mesh_resource_id();
    assert_eq!(r.get(), 1);
    assert_eq!(s.mesh_resources_len(), 1);
    assert_ne!(s, Scene::new());
}
#[test]
fn owned_mesh_metadata_and_snapshot_sharing() {
    let mut m = mesh();
    m.meta = Some(MeshMetadata {
        name: Some("model".into()),
        header_comment: Some("opaque".into()),
    });
    let s = Scene::from(m.clone());
    let id = s.roots()[0];
    assert_eq!(s.mesh_at_leaf(id).unwrap(), &m);
    assert!(s.meta().is_none());
    let clone = s.clone();
    assert_eq!(s, clone);
    assert!(Arc::ptr_eq(
        s.leaf_mesh_arc(id).unwrap(),
        clone.leaf_mesh_arc(id).unwrap()
    ));
}
#[test]
fn checked_constructors_are_atomic() {
    let mut s = Scene::new();
    let reserved = s.allocate_unbound_mesh_resource_id();
    let before = s.clone();
    assert_eq!(
        s.add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::MeshResource(reserved)
        ),
        Err(SceneError::DanglingMeshResource { id: reserved })
    );
    assert_eq!(s, before);
    let prospective = NodeId(number(0, "test"));
    assert_eq!(
        s.add_csg(None, Transform3D::Identity, CsgOp::Intersection, vec![]),
        Err(SceneError::EmptyIntersection {
            node_id: prospective
        })
    );
    assert_eq!(s, before);
    let a = leaf(&mut s);
    let before = s.clone();
    assert_eq!(
        s.add_csg(None, Transform3D::Identity, CsgOp::Union, vec![a, a]),
        Err(SceneError::DuplicateChild {
            parent: NodeId(number(1, "test")),
            child: a
        })
    );
    assert_eq!(s, before);
}
#[test]
fn attachment_precedence_and_cycle_rejection() {
    let mut s = Scene::new();
    let a = s.add_group(None, Transform3D::Identity);
    let b = s.add_group(None, Transform3D::Identity);
    s.add_child(a, b).unwrap();
    let before = s.clone();
    assert_eq!(
        s.add_child(b, a),
        Err(SceneError::ExpansionCycle { node_id: b })
    );
    assert_eq!(s, before);
    assert_eq!(
        s.add_root(b),
        Err(SceneError::AlreadyHasParent {
            node_id: b,
            parent: a
        })
    );
    assert_eq!(
        s.add_child(a, b),
        Err(SceneError::AlreadyHasParent {
            node_id: b,
            parent: a
        })
    );
    s.add_root(a).unwrap();
    assert_eq!(
        s.register_subtree_resource(a),
        Err(SceneError::AlreadyARoot { node_id: a })
    );
}
#[test]
fn reference_edge_cycle_rejection() {
    let mut s = Scene::new();
    let g = s.add_group(None, Transform3D::Identity);
    let resource = s.register_subtree_resource(g).unwrap();
    let r = s
        .add_reference(None, Transform3D::Identity, resource)
        .unwrap();
    let before = s.clone();
    assert_eq!(
        s.add_child(g, r),
        Err(SceneError::ExpansionCycle { node_id: g })
    );
    assert_eq!(s, before);
    assert_eq!(
        s.add_root(g),
        Err(SceneError::AlreadyASubtreeResourceRoot { node_id: g })
    );
}
#[test]
fn remove_restore_preserves_order_ids_and_arcs() {
    let mut s = Scene::new();
    let g = s.add_group(None, shift(1., 2., 3.));
    let a = leaf(&mut s);
    let b = leaf(&mut s);
    s.add_child(g, a).unwrap();
    s.add_child(g, b).unwrap();
    s.add_root(g).unwrap();
    let before = s.clone();
    let removed = s.remove_leaf(a).unwrap();
    assert_eq!(
        removed.attachment,
        LeafAttachment::Parent {
            parent: g,
            position: 0
        }
    );
    assert!(s.node(a).is_none());
    assert!(s.parent_of(a).is_none());
    assert_eq!(s.nodes_len(), 3);
    s.install_leaf_at(
        a,
        removed.attachment,
        removed.payload,
        removed.local_transform,
        removed.meta,
    )
    .unwrap();
    assert_eq!(s, before);
    assert!(Arc::ptr_eq(
        s.leaf_mesh_arc(a).unwrap(),
        before.leaf_mesh_arc(a).unwrap()
    ));
}
#[test]
fn install_reserved_and_root_removal() {
    let mut s = Scene::new();
    let id = s.allocate_unbound_node_id();
    let before = s.clone();
    let p = LeafPayload::Mesh(Arc::new(mesh()));
    assert_eq!(
        s.install_leaf_at(
            id,
            LeafAttachment::Root { position: 1 },
            p.clone(),
            Transform3D::Identity,
            None
        ),
        Err(SceneError::AttachmentPositionOutOfRange {
            position: 1,
            len: 0
        })
    );
    assert_eq!(s, before);
    s.install_leaf_at(
        id,
        LeafAttachment::Root { position: 0 },
        p,
        Transform3D::Identity,
        Some(SceneMetadata::default()),
    )
    .unwrap();
    let before = s.clone();
    let r = s.remove_leaf(id).unwrap();
    s.install_leaf_at(id, r.attachment, r.payload, r.local_transform, r.meta)
        .unwrap();
    assert_eq!(s, before);
}
#[test]
fn detached_and_subtree_leaf_removal() {
    let mut s = Scene::new();
    let a = leaf(&mut s);
    assert_eq!(s.remove_leaf(a), Err(SceneError::NotARoot { node_id: a }));
    s.register_subtree_resource(a).unwrap();
    assert_eq!(s.remove_leaf(a), Err(SceneError::NotARoot { node_id: a }));
    let g = s.add_group(None, Transform3D::Identity);
    assert_eq!(s.remove_leaf(g), Err(SceneError::NotALeaf { node_id: g }));
}
#[test]
fn resource_pointer_contracts_and_liveness() {
    let mut s = Scene::new();
    let arc = Arc::new(mesh());
    let r = s.add_mesh_resource_arc(arc.clone());
    let r2 = s.add_mesh_resource_arc(arc.clone());
    assert_eq!(s.find_mesh_resource_by_arc(&arc), Some(r));
    assert_eq!(s.find_mesh_resource_by_arc(&Arc::new(mesh())), None);
    let id = s
        .add_leaf(None, Transform3D::Identity, LeafPayload::MeshResource(r))
        .unwrap();
    assert!(!s.other_leaf_references_resource(r, id));
    assert!(s.other_leaf_references_resource(r, NodeId(number(50, "test"))));
    assert_eq!(
        s.remove_mesh_resource(r),
        Err(SceneError::MeshResourceInUse { id: r })
    );
    let before = s.clone();
    assert_eq!(
        s.replace_mesh_resource(r, &Arc::new(mesh()), arc.clone()),
        Err(SceneError::MeshResourceMismatch { id: r })
    );
    assert_eq!(s, before);
    let old = s.remove_mesh_resource(r2).unwrap();
    assert!(Arc::ptr_eq(&old, &arc));
    s.install_mesh_resource_at(r2, old).unwrap();
    assert_eq!(s, before);
}
#[test]
fn conditional_inline_swap() {
    let mut s = Scene::new();
    let a = leaf(&mut s);
    let old = s.leaf_mesh_arc(a).unwrap().clone();
    let other = Arc::new(mesh());
    let before = s.clone();
    assert_eq!(
        s.replace_inline_leaf_mesh(a, &other, old.clone()),
        Err(SceneError::InlineMeshMismatch { node_id: a })
    );
    assert_eq!(s, before);
    s.replace_inline_leaf_mesh(a, &old, other.clone()).unwrap();
    assert!(Arc::ptr_eq(s.leaf_mesh_arc(a).unwrap(), &other));
    assert!(Arc::ptr_eq(before.leaf_mesh_arc(a).unwrap(), &old));
}
#[test]
fn structural_order_and_policy_stops() {
    let mut s = Scene::new();
    let a = leaf(&mut s);
    let b = leaf(&mut s);
    let c = s
        .add_csg(None, Transform3D::Identity, CsgOp::Difference, vec![b, a])
        .unwrap();
    s.add_root(c).unwrap();
    assert_eq!(
        s.reachable_leaves(CsgPolicy::Descend, ReferencePolicy::Skip)
            .unwrap(),
        vec![b, a]
    );
    assert_eq!(s.first_leaf_id(), Some(b));
    assert_eq!(s.reachable_leaf_count(), 2);
    assert_eq!(
        s.reachable_leaves(CsgPolicy::Refuse, ReferencePolicy::Skip),
        Err(SceneError::UnevaluatedCsg { node_id: c })
    );
    let mut visited = vec![];
    s.for_each_reachable_node(|id, _| visited.push(id));
    assert_eq!(visited, vec![c, b, a]);
    let mut calls = 0;
    let _: Result<(), SceneError> =
        s.for_each_reachable_leaf(CsgPolicy::Descend, ReferencePolicy::Skip, |id| {
            calls += 1;
            Err(SceneError::NotALeaf { node_id: id })
        });
    assert_eq!(calls, 1);
}
#[test]
fn nested_expansion_and_structural_placement() {
    let mut s = Scene::new();
    let a = leaf(&mut s);
    s.set_local_transform(a, shift(1., 0., 0.)).unwrap();
    let r = s.register_subtree_resource(a).unwrap();
    let g = s.add_group(None, shift(0., 2., 0.));
    for z in [3., 7.] {
        let reference = s.add_reference(None, shift(0., 0., z), r).unwrap();
        s.add_child(g, reference).unwrap();
    }
    s.add_root(g).unwrap();
    assert_eq!(s.reachable_leaf_count(), 0);
    assert_eq!(s.first_reference_node().unwrap().get(), 3);
    assert_eq!(s.expanded_leaf_count(CsgPolicy::Descend).unwrap(), 2);
    let mut points = vec![];
    s.for_each_expanded_leaf::<SceneError, _>(CsgPolicy::Descend, |id, t, _| {
        assert_eq!(id, a);
        points.push(t.apply_to_vertex(Vertex::new([0.; 3])).coords());
        Ok(())
    })
    .unwrap();
    assert_eq!(points, vec![[1., 2., 3.], [1., 2., 7.]]);
    assert_eq!(
        bake_leaf_to_world(&s, a).unwrap().vertices[0].coords(),
        [1., 0., 0.]
    );
    assert!(matches!(
        s.reachable_leaves(CsgPolicy::Descend, ReferencePolicy::Refuse),
        Err(SceneError::UnexpandedReference { .. })
    ));
}
#[test]
fn exponential_count_saturates_without_expanding() {
    let mut s = Scene::new();
    let l = leaf(&mut s);
    let mut resource = s.register_subtree_resource(l).unwrap();
    for _ in 0..70 {
        let g = s.add_group(None, Transform3D::Identity);
        for _ in 0..2 {
            let r = s
                .add_reference(None, Transform3D::Identity, resource)
                .unwrap();
            s.add_child(g, r).unwrap();
        }
        resource = s.register_subtree_resource(g).unwrap();
    }
    let root = s
        .add_reference(None, Transform3D::Identity, resource)
        .unwrap();
    s.add_root(root).unwrap();
    assert_eq!(
        s.expanded_leaf_count(CsgPolicy::Descend).unwrap(),
        usize::MAX
    );
    assert!(s.validate().is_ok());
}
#[test]
fn count_ignores_mesh_resolution_but_walk_does_not() {
    let mut s = Scene::new();
    let a = leaf(&mut s);
    s.add_root(a).unwrap();
    let missing = s.allocate_unbound_mesh_resource_id();
    s.nodes[a.index()].as_mut().unwrap().kind = NodeKind::Leaf(LeafPayload::MeshResource(missing));
    assert_eq!(s.expanded_leaf_count(CsgPolicy::Descend), Ok(1));
    assert_eq!(
        s.for_each_expanded_leaf::<SceneError, _>(CsgPolicy::Descend, |_, _, _| Ok(())),
        Err(SceneError::DanglingMeshResource { id: missing })
    );
}
#[test]
fn transform_composition_includes_detached_ancestors() {
    let mut s = Scene::new();
    let parent = s.add_group(
        None,
        Transform3D::Affine(AffineTransform3D::from_uniform_scale(2.)),
    );
    let child = leaf(&mut s);
    s.set_local_transform(child, shift(3., 0., 0.)).unwrap();
    s.add_child(parent, child).unwrap();
    assert_eq!(
        s.world_transform_of(child)
            .unwrap()
            .apply_to_vertex(Vertex::new([1., 0., 0.]))
            .coords(),
        [8., 0., 0.]
    );
}
#[test]
fn bake_identity_mirror_zero_normal_and_singular() {
    let mut m = mesh();
    m.meta = Some(MeshMetadata::default());
    assert_eq!(
        bake_mesh_to_world(&m, &Transform3D::Identity).unwrap(),
        mesh()
    );
    let t = Transform3D::Affine(AffineTransform3D::from_non_uniform_scale([-1., 2., 1.]));
    let b = bake_mesh_to_world(&m, &t).unwrap();
    assert_eq!(b.faces[0].vertices.map(|i| i.get()), [0, 2, 1]);
    assert_eq!(
        b.faces[0].normal,
        t.apply_to_normal(m.faces[0].normal).unwrap()
    );
    let singular = Transform3D::Affine(AffineTransform3D::from_uniform_scale(0.));
    assert!(matches!(
        bake_mesh_to_world(&m, &singular),
        Err(SceneError::Transform(_))
    ));
    m.faces[0].normal = Normal::new([-0., 0., 0.]);
    let baked = bake_mesh_to_world(&m, &singular).unwrap();
    assert_eq!(baked.faces[0].normal[0].to_bits(), (-0f32).to_bits());
}
#[test]
fn merge_occurrences_shifts_indices_and_discards_metadata() {
    let mut s = Scene::from(mesh());
    let first = s.roots()[0];
    let a = leaf(&mut s);
    s.add_root(a).unwrap();
    s.set_local_transform(a, shift(10., 0., 0.)).unwrap();
    let merged = s.merge_meshes().unwrap().unwrap();
    assert_eq!(merged.vertices.len(), 6);
    assert_eq!(merged.faces[1].vertices.map(|i| i.get()), [3, 4, 5]);
    assert_eq!(merged.vertices[3].coords(), [10., 0., 0.]);
    assert!(merged.meta.is_none());
    assert_eq!(first.get(), 1);
    let mut empty = mesh();
    empty.faces.clear();
    let scene = Scene::from(empty);
    assert!(scene.merge_meshes().unwrap().is_none());
    assert!(scene.aabb().unwrap().is_some());
}
#[test]
fn tight_bounds_and_drop_use_unused_vertices() {
    let mut m = mesh();
    m.vertices.push(Vertex::new([5., 5., 1.]));
    let mut s = Scene::from(m);
    let id = s.roots()[0];
    s.set_local_transform(
        id,
        Transform3D::Affine(AffineTransform3D::from_rotation_z(
            std::f64::consts::FRAC_PI_4,
        )),
    )
    .unwrap();
    let loose = s.aabb().unwrap().unwrap();
    let tight = s.tight_aabb().unwrap().unwrap();
    assert!(loose.max()[0] > tight.max()[0]);
    assert!(s.drop_at([0., 0.], 0.).unwrap());
    let b = s.tight_aabb().unwrap().unwrap();
    assert!(b.min()[2].abs() < 1e-5);
    assert!((b.min()[0] + b.max()[0]).abs() < 1e-5);
}
#[test]
fn unchanged_drop_preserves_identity() {
    let mut s = Scene::from(IndexedMesh::new(
        None,
        vec![Vertex::new([-1., -1., 0.]), Vertex::new([1., 1., 1.])],
        vec![],
    ));
    assert!(!s.drop_at([0., 0.], 0.).unwrap());
    assert_eq!(
        s.node(s.roots()[0]).unwrap().local_transform,
        Transform3D::Identity
    );
    assert!(!Scene::new().drop_at([0., 0.], 0.).unwrap());
}
#[test]
fn plate_reconstruction_allocation_and_validation() {
    let mut p = Plates::default();
    p.order[0].settings = Some("opaque settings".into());
    assert_eq!(p.add(), 2);
    assert_eq!(p.order[1].settings, p.order[0].settings);
    p.order.pop();
    assert_eq!(p.add(), 3);
    let p = Plates::from_parts(
        vec![
            Plate {
                id: 9,
                locked: true,
                settings: None,
            },
            Plate {
                id: 2,
                locked: false,
                settings: None,
            },
        ],
        BTreeMap::new(),
    )
    .unwrap();
    let mut p = p;
    assert_eq!(p.add(), 10);
    for order in [
        vec![],
        vec![Plate {
            id: 0,
            locked: false,
            settings: None,
        }],
        vec![Plate {
            id: u32::MAX,
            locked: false,
            settings: None,
        }],
        vec![p.order[0].clone(), p.order[0].clone()],
    ] {
        assert!(Plates::from_parts(order, BTreeMap::new()).is_none());
    }
    let mut s = Scene::new();
    let before = s.clone();
    p.order.clear();
    assert!(!s.set_plates(p));
    assert_eq!(s, before);
}
#[test]
fn plate_grid_exact_formulas() {
    for (index, count, expected) in [
        (0, 1, [0., 0.]),
        (1, 2, [140., 0.]),
        (2, 4, [0., -90.]),
        (3, 4, [140., -90.]),
        (2, 5, [280., 0.]),
        (3, 5, [0., -90.]),
        (3, 0, [0., -270.]),
    ] {
        assert_eq!(plate_grid_offset(index, count, 100., 50.), expected);
    }
    assert_eq!(PLATE_GAP_MM, 40.);
}
#[test]
fn support_plate_ownership_is_one_level_and_live() {
    let mut s = Scene::new();
    let owner = leaf(&mut s);
    let support = leaf(&mut s);
    let raft = leaf(&mut s);
    s.set_node_meta(
        support,
        Some(SceneMetadata {
            name: None,
            comment: Some(format!(
                "sc-generated-support\nowner = bad\nowner = {}",
                owner.get()
            )),
        }),
    )
    .unwrap();
    s.set_node_meta(
        raft,
        Some(SceneMetadata {
            name: None,
            comment: Some("sc-generated-support-raft".into()),
        }),
    )
    .unwrap();
    let mut p = Plates::default();
    let second = p.add();
    p.assignments.insert(owner, second);
    p.assignments.insert(support, 1);
    s.set_plates(p);
    assert_eq!(s.plate_of(support), second);
    assert_eq!(s.plate_of(raft), second);
    assert_eq!(s.plate_of(NodeId(number(99, "test"))), 1);
    s.set_node_meta(
        owner,
        Some(SceneMetadata {
            name: None,
            comment: Some(format!("sc-generated-support\nowner = {}", support.get())),
        }),
    )
    .unwrap();
    assert_eq!(s.plate_of(support), second);
}
#[test]
fn plate_isolation_retains_resources_references_and_detached() {
    let mut s = Scene::new();
    let a = leaf(&mut s);
    let b = leaf(&mut s);
    let detached = leaf(&mut s);
    s.add_root(a).unwrap();
    s.add_root(b).unwrap();
    let subtree = leaf(&mut s);
    let resource = s.register_subtree_resource(subtree).unwrap();
    let r = s
        .add_reference(None, Transform3D::Identity, resource)
        .unwrap();
    s.add_root(r).unwrap();
    let mut p = Plates::default();
    let second = p.add();
    p.assignments.insert(b, second);
    s.set_plates(p);
    let isolated = s.plate_scene(second).unwrap();
    assert!(isolated.node(a).is_none());
    for id in [b, detached, subtree, r] {
        assert!(isolated.node(id).is_some());
    }
    assert_eq!(isolated.plates(), s.plates());
    assert!(isolated.validate().is_ok());
}
#[test]
fn stored_targets_include_detached_and_unreferenced() {
    let mut s = Scene::new();
    let a = leaf(&mut s);
    let r = s.add_mesh_resource(mesh());
    let b = s
        .add_leaf(None, Transform3D::Identity, LeafPayload::MeshResource(r))
        .unwrap();
    assert_eq!(
        s.mesh_targets(),
        vec![MeshTarget::Resource(r), MeshTarget::InlineLeaf(a)]
    );
    assert_eq!(s.leaves_referencing(MeshTarget::Resource(r)), vec![b]);
    assert_eq!(s.leaves_referencing(MeshTarget::InlineLeaf(a)), vec![a]);
    assert_eq!(s.diagnose_meshes().len(), 2);
    assert_eq!(s.non_edge_manifold_leaves(), vec![a, b]);
}
#[test]
fn exact_shared_coordinates_zero_nan_and_self_duplicates() {
    let mut m = IndexedMesh::new(
        None,
        vec![Vertex::new([0., 1., 2.]), Vertex::new([0., 1., 2.])],
        vec![],
    );
    let mut s = Scene::from(m.clone());
    assert!(s.detect_shared_vertices().is_empty());
    m.vertices = vec![Vertex::new([-0., 1., 2.])];
    let b = s
        .add_leaf(None, shift(100., 0., 0.), LeafPayload::Mesh(Arc::new(m)))
        .unwrap();
    assert_eq!(s.detect_shared_vertices(), vec![s.roots()[0], b]);
    let nan = f32::from_bits(0x7fc00001);
    let mut s = Scene::from(IndexedMesh::new(
        None,
        vec![Vertex::new([nan, 0., 0.])],
        vec![],
    ));
    let a = s.roots()[0];
    let b = s
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(s.leaf_mesh_arc(a).unwrap().clone()),
        )
        .unwrap();
    assert_eq!(s.detect_shared_vertices(), vec![a, b]);
}
#[test]
fn clean_and_refused_repair_keep_exact_allocations() {
    let mut s = Scene::from(IndexedMesh::new(None, vec![], vec![]));
    let id = s.roots()[0];
    let before = s.leaf_mesh_arc(id).unwrap().clone();
    let result = s.repair_meshes(RepairPasses::default_repair());
    assert!(result[0].fixes.is_empty());
    assert!(Arc::ptr_eq(&before, s.leaf_mesh_arc(id).unwrap()));
    let bad = IndexedMesh::new(None, vec![Vertex::new([f32::NAN, 0., 0.])], vec![]);
    let b = s
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(bad)),
        )
        .unwrap();
    let before = s.leaf_mesh_arc(b).unwrap().clone();
    let result = s.repair_meshes(RepairPasses::default_repair());
    assert!(result[1].refusal.is_some());
    assert!(Arc::ptr_eq(&before, s.leaf_mesh_arc(b).unwrap()));
}
#[test]
fn mesh_mutation_isolates_only_one_stored_target() {
    let arc = Arc::new(mesh());
    let mut s = Scene::new();
    let r = s.add_mesh_resource_arc(arc.clone());
    let a = s
        .add_leaf(None, Transform3D::Identity, LeafPayload::Mesh(arc.clone()))
        .unwrap();
    let b = s
        .add_leaf(None, Transform3D::Identity, LeafPayload::MeshResource(r))
        .unwrap();
    s.target_mesh_mut(MeshTarget::Resource(r)).unwrap().vertices[0] = Vertex::new([9.; 3]);
    assert_eq!(s.mesh_at_leaf(b).unwrap().vertices[0].coords(), [9.; 3]);
    assert_eq!(s.mesh_at_leaf(a).unwrap().vertices[0].coords(), [0.; 3]);
    assert_eq!(arc.vertices[0].coords(), [0.; 3]);
}
#[test]
fn validator_precedence_and_cycle_witness() {
    let mut s = Scene::new();
    let a = s.add_group(None, Transform3D::Identity);
    let b = s.add_group(None, Transform3D::Identity);
    s.nodes[a.index()].as_mut().unwrap().kind = NodeKind::Csg {
        op: CsgOp::Intersection,
        children: vec![],
    };
    let missing = s.allocate_unbound_node_id();
    s.nodes[b.index()].as_mut().unwrap().kind = NodeKind::Group {
        children: vec![missing],
    };
    assert_eq!(
        s.validate(),
        Err(SceneError::EmptyIntersection { node_id: a })
    );
    s.nodes[a.index()].as_mut().unwrap().kind = NodeKind::Group { children: vec![b] };
    assert_eq!(
        s.validate(),
        Err(SceneError::DanglingNodeId { id: missing })
    );
    s.nodes[b.index()].as_mut().unwrap().kind = NodeKind::Group { children: vec![a] };
    s.parents[a.index()] = Some(b);
    s.parents[b.index()] = Some(a);
    assert_eq!(s.validate(), Err(SceneError::ExpansionCycle { node_id: a }));
}
#[test]
fn validator_checks_detached_parent_mismatch_and_duplicates() {
    let mut s = Scene::new();
    let a = s.add_group(None, Transform3D::Identity);
    let b = leaf(&mut s);
    s.nodes[a.index()]
        .as_mut()
        .unwrap()
        .children_mut()
        .unwrap()
        .extend([b, b]);
    assert_eq!(
        s.validate(),
        Err(SceneError::DuplicateChild {
            parent: a,
            child: b
        })
    );
    s.nodes[a.index()]
        .as_mut()
        .unwrap()
        .children_mut()
        .unwrap()
        .pop();
    assert_eq!(
        s.validate(),
        Err(SceneError::ParentMismatch {
            node_id: b,
            recorded: None
        })
    );
}
#[test]
fn intersection_removal_is_allowed_then_invalid() {
    let mut s = Scene::new();
    let a = leaf(&mut s);
    let c = s
        .add_csg(None, Transform3D::Identity, CsgOp::Intersection, vec![a])
        .unwrap();
    s.remove_leaf(a).unwrap();
    assert_eq!(
        s.validate(),
        Err(SceneError::EmptyIntersection { node_id: c })
    );
}
#[test]
fn metadata_and_singular_transform_are_not_validated() {
    let mut s = Scene::new();
    let a = leaf(&mut s);
    s.set_node_meta(a, Some(SceneMetadata::default())).unwrap();
    assert_ne!(s.node(a).unwrap().meta, None);
    s.set_local_transform(
        a,
        Transform3D::Affine(AffineTransform3D::from_uniform_scale(0.)),
    )
    .unwrap();
    assert!(s.validate().is_ok());
}
#[test]
fn exact_error_display_and_sources() {
    let a = NodeId(number(0, "test"));
    let b = NodeId(number(1, "test"));
    assert_eq!(
        SceneError::DuplicateChild {
            parent: a,
            child: b
        }
        .to_string(),
        "duplicate child 2 under parent 1"
    );
    assert_eq!(
        SceneError::ParentMismatch {
            node_id: a,
            recorded: Some(b)
        }
        .to_string(),
        "parent mismatch at node 1: recorded Some(2)"
    );
    assert_eq!(
        SceneError::ParentMismatch {
            node_id: a,
            recorded: None
        }
        .to_string(),
        "parent mismatch at node 1: recorded None"
    );
    assert_eq!(
        SceneError::InvalidPlates.to_string(),
        "invalid project plate table"
    );
    assert!(std::error::Error::source(&SceneError::InvalidPlates).is_none());
}

#[test]
fn malformed_expansion_errors_and_structural_skip() {
    let mut s = Scene::new();
    let a = leaf(&mut s);
    s.add_root(a).unwrap();
    let missing = s.allocate_unbound_node_id();
    s.roots.insert(0, missing);
    assert_eq!(
        s.reachable_leaves(CsgPolicy::Descend, ReferencePolicy::Skip)
            .unwrap(),
        vec![a]
    );
    assert_eq!(
        s.expanded_leaf_count(CsgPolicy::Descend),
        Err(SceneError::DanglingNodeId { id: missing })
    );
    assert_eq!(
        s.for_each_expanded_leaf::<SceneError, _>(CsgPolicy::Descend, |_, _, _| Ok(())),
        Err(SceneError::DanglingNodeId { id: missing })
    );
    s.roots.remove(0);
    let g = s.add_group(None, Transform3D::Identity);
    let resource = s.register_subtree_resource(g).unwrap();
    let r = s
        .add_reference(None, Transform3D::Identity, resource)
        .unwrap();
    s.add_root(r).unwrap();
    s.nodes[g.index()].as_mut().unwrap().kind = NodeKind::Group { children: vec![r] };
    assert_eq!(
        s.expanded_leaf_count(CsgPolicy::Descend),
        Err(SceneError::ExpansionCycle { node_id: r })
    );
    assert_eq!(
        s.for_each_expanded_leaf::<SceneError, _>(CsgPolicy::Descend, |_, _, _| Ok(())),
        Err(SceneError::ExpansionCycle { node_id: r })
    );
}
#[test]
fn deep_traversal_avoids_call_stack_recursion() {
    let mut s = Scene::new();
    let root = s.add_group(None, Transform3D::Identity);
    let mut parent = root;
    // Build from private slots here so this test measures walks rather than checked edge insertion.
    for _ in 0..20_000 {
        let child = s.add_group(None, Transform3D::Identity);
        s.nodes[parent.index()]
            .as_mut()
            .unwrap()
            .children_mut()
            .unwrap()
            .push(child);
        s.parents[child.index()] = Some(parent);
        parent = child;
    }
    let a = leaf(&mut s);
    s.nodes[parent.index()]
        .as_mut()
        .unwrap()
        .children_mut()
        .unwrap()
        .push(a);
    s.parents[a.index()] = Some(parent);
    s.add_root(root).unwrap();
    assert_eq!(s.reachable_leaf_count(), 1);
    assert_eq!(s.expanded_leaf_count(CsgPolicy::Descend), Ok(1));
    assert!(s.validate().is_ok());
    assert_eq!(s.world_transform_of(a), Ok(Transform3D::Identity));
    let mut count = 0;
    s.for_each_expanded_leaf::<SceneError, _>(CsgPolicy::Descend, |_, _, _| {
        count += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(count, 1);
}
#[test]
fn plate_owner_uses_first_parsed_line_even_if_not_live() {
    let mut s = Scene::new();
    let a = leaf(&mut s);
    let b = leaf(&mut s);
    let support = leaf(&mut s);
    s.set_node_meta(
        support,
        Some(SceneMetadata {
            name: None,
            comment: Some(format!(
                "sc-generated-support\nowner = 99999\nowner = {}",
                a.get()
            )),
        }),
    )
    .unwrap();
    let mut p = Plates::default();
    let second = p.add();
    p.assignments.insert(a, second);
    p.assignments.insert(support, second);
    s.set_plates(p);
    assert_eq!(s.plate_of(support), second);
    s.set_node_meta(
        support,
        Some(SceneMetadata {
            name: None,
            comment: Some(format!("sc-generated-support\nowner = {}", b.get())),
        }),
    )
    .unwrap();
    assert_eq!(s.plate_of(support), 1);
}
#[test]
fn repair_work_isolated_from_external_snapshot_and_inline_alias() {
    let mut m = mesh();
    m.faces[0].normal = Normal::new([0.; 3]);
    let arc = Arc::new(m);
    let mut s = Scene::new();
    let r = s.add_mesh_resource_arc(arc.clone());
    let inline = s
        .add_leaf(None, Transform3D::Identity, LeafPayload::Mesh(arc.clone()))
        .unwrap();
    let resource_leaf = s
        .add_leaf(None, Transform3D::Identity, LeafPayload::MeshResource(r))
        .unwrap();
    let snapshot = s.clone();
    let out = s.repair_meshes(RepairPasses {
        face_normals: true,
        ..RepairPasses::default()
    });
    assert_eq!(out.len(), 2);
    assert!(out.iter().all(|o| !o.fixes.is_empty()), "{out:?}");
    assert_eq!(arc.faces[0].normal.coords(), [0.; 3]);
    assert!(Arc::ptr_eq(snapshot.leaf_mesh_arc(inline).unwrap(), &arc));
    assert!(!Arc::ptr_eq(s.leaf_mesh_arc(resource_leaf).unwrap(), &arc));
    assert!(!Arc::ptr_eq(
        s.leaf_mesh_arc(inline).unwrap(),
        s.leaf_mesh_arc(resource_leaf).unwrap()
    ));
}

#[test]
fn every_error_display_template() {
    assert_eq!(
        SceneError::InvalidPlates.to_string(),
        "invalid project plate table"
    );
    assert_eq!(
        SceneError::DanglingNodeId {
            id: NodeId(number(2, "test"))
        }
        .to_string(),
        "dangling node id 3"
    );
    assert_eq!(
        SceneError::DanglingMeshResource {
            id: MeshResourceId(number(2, "test"))
        }
        .to_string(),
        "dangling mesh resource id 3"
    );
    assert_eq!(
        SceneError::DanglingSubtreeResource {
            id: SubtreeResourceId(number(2, "test"))
        }
        .to_string(),
        "dangling subtree resource id 3"
    );
    assert_eq!(
        SceneError::EmptyIntersection {
            node_id: NodeId(number(2, "test"))
        }
        .to_string(),
        "empty intersection at node 3"
    );
    assert_eq!(
        SceneError::ExpansionCycle {
            node_id: NodeId(number(2, "test"))
        }
        .to_string(),
        "cycle in the expansion graph reachable from node 3"
    );
    assert_eq!(
        SceneError::AlreadyHasParent {
            node_id: NodeId(number(2, "test")),
            parent: NodeId(number(3, "test"))
        }
        .to_string(),
        "node 3 already has parent 4"
    );
    assert_eq!(
        SceneError::AlreadyARoot {
            node_id: NodeId(number(2, "test"))
        }
        .to_string(),
        "node 3 is already a scene root"
    );
    assert_eq!(
        SceneError::AlreadyASubtreeResourceRoot {
            node_id: NodeId(number(2, "test"))
        }
        .to_string(),
        "node 3 is already a subtree resource root"
    );
    assert_eq!(
        SceneError::DuplicateChild {
            parent: NodeId(number(2, "test")),
            child: NodeId(number(3, "test"))
        }
        .to_string(),
        "duplicate child 4 under parent 3"
    );
    assert_eq!(
        SceneError::NotABranch {
            node_id: NodeId(number(2, "test"))
        }
        .to_string(),
        "node 3 is not a Group or Csg branch"
    );
    assert_eq!(
        SceneError::NotALeaf {
            node_id: NodeId(number(2, "test"))
        }
        .to_string(),
        "node 3 is not a Leaf"
    );
    assert_eq!(
        SceneError::NodeSlotOccupied {
            id: NodeId(number(2, "test"))
        }
        .to_string(),
        "node slot 3 is already occupied"
    );
    assert_eq!(
        SceneError::NotARoot {
            node_id: NodeId(number(2, "test"))
        }
        .to_string(),
        "node 3 is not a scene root"
    );
    assert_eq!(
        SceneError::AttachmentPositionOutOfRange {
            position: 3,
            len: 4
        }
        .to_string(),
        "attachment position 3 is past the end of a list of 4"
    );
    assert_eq!(
        SceneError::NotAnInlineMeshLeaf {
            node_id: NodeId(number(2, "test"))
        }
        .to_string(),
        "node 3 is a leaf but not an inline-mesh leaf"
    );
    assert_eq!(
        SceneError::MeshResourceSlotOccupied {
            id: MeshResourceId(number(2, "test"))
        }
        .to_string(),
        "mesh resource slot 3 is already occupied"
    );
    assert_eq!(
        SceneError::MeshResourceInUse {
            id: MeshResourceId(number(2, "test"))
        }
        .to_string(),
        "mesh resource 3 is still referenced by at least one leaf"
    );
    assert_eq!(
        SceneError::MeshResourceMismatch {
            id: MeshResourceId(number(2, "test"))
        }
        .to_string(),
        "mesh resource 3 does not hold the mesh the swap expected"
    );
    assert_eq!(
        SceneError::InlineMeshMismatch {
            node_id: NodeId(number(2, "test"))
        }
        .to_string(),
        "node 3 does not hold the inline mesh the swap expected"
    );
    assert_eq!(
        SceneError::UnevaluatedCsg {
            node_id: NodeId(number(2, "test"))
        }
        .to_string(),
        "unevaluated CSG node 3"
    );
    assert_eq!(
        SceneError::UnexpandedReference {
            node_id: NodeId(number(2, "test"))
        }
        .to_string(),
        "unexpanded Reference node 3"
    );
}

#[test]
fn unique_and_shared_repair_paths_have_identical_reports_and_bytes() {
    let clean = IndexedMesh::new(None, vec![], vec![]);
    let invalid = IndexedMesh::new(None, vec![Vertex::new([f32::INFINITY, 0., 0.])], vec![]);
    let mut normals = mesh();
    normals.faces[0].normal = Normal::new([0.; 3]);
    let mut degenerate = mesh();
    degenerate.faces.push(IndexedTriangle::from_raw_indices(
        Normal::new([0.; 3]),
        [0, 0, 1],
    ));
    for input in [clean, invalid, normals, degenerate] {
        for passes in [
            RepairPasses::default(),
            RepairPasses::default_repair(),
            RepairPasses {
                face_normals: true,
                ..RepairPasses::default()
            },
        ] {
            let mut unique = Scene::from(input.clone());
            let mut shared = Scene::from(input.clone());
            let id = unique.roots()[0];
            let pointer = Arc::as_ptr(unique.leaf_mesh_arc(id).unwrap());
            let held = shared.leaf_mesh_arc(id).unwrap().clone();
            let u = unique.repair_meshes(passes);
            let s = shared.repair_meshes(passes);
            assert_eq!(u, s);
            let a = unique.mesh_at_leaf(id).unwrap();
            let b = shared.mesh_at_leaf(id).unwrap();
            assert_eq!(a.faces, b.faces);
            assert_eq!(
                a.vertices
                    .iter()
                    .map(|v| v.coords().map(f32::to_bits))
                    .collect::<Vec<_>>(),
                b.vertices
                    .iter()
                    .map(|v| v.coords().map(f32::to_bits))
                    .collect::<Vec<_>>()
            );
            assert_eq!(pointer, Arc::as_ptr(unique.leaf_mesh_arc(id).unwrap()));
            if u[0].fixes.is_empty() {
                assert!(Arc::ptr_eq(&held, shared.leaf_mesh_arc(id).unwrap()));
            }
        }
    }
}

#[test]
fn large_parallel_bake_preserves_order_and_dependency_transform_values() {
    let mut m = mesh();
    m.vertices = (0..20_000)
        .map(|i| Vertex::new([i as f32, (i % 13) as f32, -2.]))
        .collect();
    m.faces = vec![m.faces[0]; 20_000];
    m.faces[4096].normal = Normal::new([-0., 0., 0.]);
    let t = Transform3D::Affine(AffineTransform3D::from_non_uniform_scale([-2., 3., 4.]));
    let baked = bake_mesh_to_world(&m, &t).unwrap();
    for (input, output) in m.vertices.iter().zip(&baked.vertices) {
        assert_eq!(*output, t.apply_to_vertex(*input));
    }
    for (input, output) in m.faces.iter().zip(&baked.faces) {
        assert_eq!(output.vertices.map(|i| i.get()), [0, 2, 1]);
        let normal = if input.normal.coords() == [0.; 3] {
            input.normal
        } else {
            t.apply_to_normal(input.normal).unwrap()
        };
        assert_eq!(
            output.normal.coords().map(f32::to_bits),
            normal.coords().map(f32::to_bits)
        );
    }
    let singular = Transform3D::Affine(AffineTransform3D::from_uniform_scale(0.));
    assert!(matches!(
        bake_mesh_to_world(&m, &singular),
        Err(SceneError::Transform(_))
    ));
}

#[test]
fn target_mesh_selects_only_the_requested_stored_mesh() {
    let mut scene = Scene::new();
    let inline = leaf(&mut scene);
    let resource = scene.add_mesh_resource(mesh());
    let reference = scene
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::MeshResource(resource),
        )
        .unwrap();
    let group = scene.add_group(None, Transform3D::Identity);
    let missing = scene.allocate_unbound_node_id();
    assert_eq!(
        scene.target_mesh(MeshTarget::InlineLeaf(inline)),
        Some(&mesh())
    );
    assert_eq!(
        scene.target_mesh(MeshTarget::Resource(resource)),
        Some(&mesh())
    );
    for id in [reference, group, missing] {
        assert!(scene.target_mesh(MeshTarget::InlineLeaf(id)).is_none());
    }
}

#[test]
fn drop_at_reports_empty_stationary_and_moved_scenes() {
    let mut empty = Scene::new();
    assert!(!empty.drop_at([4., 7.], 3.).unwrap());
    let mut scene = Scene::from(mesh());
    assert!(!scene.drop_at([1., 1.], 0.).unwrap());
    assert!(scene.drop_at([4., 5.], 2.).unwrap());
    let bounds = scene.tight_aabb().unwrap().unwrap();
    assert_eq!(bounds.min().coords(), [3., 4., 2.]);
    assert_eq!(bounds.max().coords(), [5., 6., 2.]);
    assert!(!scene.drop_at([4., 5.], 2.).unwrap());
}

#[test]
fn expanded_csg_policy_preserves_order_and_stops_before_children() {
    let mut scene = Scene::new();
    let first = leaf(&mut scene);
    let second = leaf(&mut scene);
    let branch = scene
        .add_csg(
            None,
            Transform3D::Identity,
            CsgOp::Difference,
            vec![second, first],
        )
        .unwrap();
    scene.add_root(branch).unwrap();
    let mut seen = Vec::new();
    assert_eq!(
        scene.for_each_expanded_leaf::<SceneError, _>(CsgPolicy::Refuse, |id, _, _| {
            seen.push(id);
            Ok(())
        }),
        Err(SceneError::UnevaluatedCsg { node_id: branch })
    );
    assert!(seen.is_empty());
    assert_eq!(
        scene.expanded_leaf_count(CsgPolicy::Refuse),
        Err(SceneError::UnevaluatedCsg { node_id: branch })
    );
    scene
        .for_each_expanded_leaf::<SceneError, _>(CsgPolicy::Descend, |id, _, _| {
            seen.push(id);
            Ok(())
        })
        .unwrap();
    assert_eq!(seen, [second, first]);
    assert_eq!(scene.expanded_leaf_count(CsgPolicy::Descend), Ok(2));
}

#[test]
fn node_role_diagnostics_keep_their_exact_text() {
    let node_id = NodeId(number(6, "test"));
    let cases = [
        (SceneError::AlreadyARoot { node_id }, "node 7 is already a scene root"),
        (SceneError::NotABranch { node_id }, "node 7 is not a Group or Csg branch"),
        (SceneError::NotALeaf { node_id }, "node 7 is not a Leaf"),
        (SceneError::NotARoot { node_id }, "node 7 is not a scene root"),
    ];
    for (error, text) in cases {
        assert_eq!(error.to_string(), text);
    }
}

#[test]
fn repair_reports_each_stored_mesh_target_in_order() {
    let mut scene = Scene::new();
    let resource = scene.add_mesh_resource_arc(Arc::new(mesh()));
    let inline = scene
        .add_leaf(None, Transform3D::Identity, LeafPayload::Mesh(Arc::new(mesh())))
        .unwrap();
    scene
        .add_leaf(None, Transform3D::Identity, LeafPayload::MeshResource(resource))
        .unwrap();
    let reports = scene.repair_meshes(RepairPasses::default());
    assert_eq!(
        reports.iter().map(|r| r.target).collect::<Vec<_>>(),
        vec![MeshTarget::Resource(resource), MeshTarget::InlineLeaf(inline)]
    );
    assert!(Scene::new().repair_meshes(RepairPasses::default()).is_empty());
}

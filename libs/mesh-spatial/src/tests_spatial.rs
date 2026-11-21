// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
use sc_mesh_core::{IndexedTriangle, Normal, transform::AffineTransform3D};
use sc_scene::node::LeafAttachment;
use std::sync::Arc;
fn mesh() -> Arc<IndexedMesh> {
    Arc::new(IndexedMesh::new(
        None,
        vec![
            Vertex::new([0., 0., 0.]),
            Vertex::new([2., 0., 0.]),
            Vertex::new([0., 3., 1.]),
            Vertex::new([7., 8., 9.]),
        ],
        vec![IndexedTriangle::from_raw_indices(
            Normal::new([0., 0., 1.]),
            [0, 1, 2],
        )],
    ))
}
fn add(s: &mut Scene, p: LeafPayload, t: Transform3D) -> NodeId {
    let id = s.add_leaf(None, t, p).unwrap();
    s.add_root(id).unwrap();
    id
}
fn shift(x: f64) -> Transform3D {
    Transform3D::Affine(AffineTransform3D::from_translation([x, 0., 0.]))
}
#[test]
fn local_and_clusters_have_different_vertex_sets() {
    let mut s = Scene::new();
    let m = mesh();
    let id = add(&mut s, LeafPayload::Mesh(m.clone()), shift(2.));
    let spatial = SceneSpatial::from_graph(s);
    let q = spatial.leaf(id).unwrap();
    assert!(std::ptr::eq(q.mesh(), m.as_ref()));
    assert_eq!(q.local_aabb(), m.local_aabb());
    assert_eq!(q.local_aabb().unwrap().max().coords(), [7., 8., 9.]);
    let c = q.clusters().unwrap();
    assert_eq!(c.aabb(0).max().coords(), [2., 3., 1.]);
    assert!(std::ptr::eq(c, q.clusters().unwrap()));
    assert_eq!(q.local_transform(), shift(2.));
}
#[test]
fn clusters_partition_and_reject_any_bad_face() {
    let m = mesh();
    for count in [1, 63, 64, 65, 128, 129] {
        let faces = vec![m.faces()[0]; count];
        let c = TriangleClusters::build(m.vertices(), &faces).unwrap();
        assert_eq!(c.len(), count.div_ceil(64));
        assert_eq!(c.face_count(), count as u32);
        assert!(!c.is_empty());
        let ranges: Vec<_> = c.iter().map(|(r, _)| r).collect();
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges.last().unwrap().end, count as u32);
        for w in ranges.windows(2) {
            assert_eq!(w[0].end, w[1].start);
        }
    }
    assert!(TriangleClusters::build(m.vertices(), &[]).is_none());
    let mut faces = vec![m.faces()[0]; 65];
    faces[64] = IndexedTriangle::from_raw_indices(Normal::new([0.; 3]), [0, 0, 999]);
    assert!(TriangleClusters::build(m.vertices(), &faces).is_none());
    faces[64] = IndexedTriangle::from_raw_indices(Normal::new([0.; 3]), [0, 0, 0]);
    assert!(TriangleClusters::build(m.vertices(), &faces).is_some());
}
#[test]
fn eager_progress_counts_each_reference_in_order() {
    let mut s = Scene::new();
    let m = mesh();
    let rid = s.add_mesh_resource_arc(m);
    let a = add(
        &mut s,
        LeafPayload::MeshResource(rid),
        Transform3D::Identity,
    );
    let b = add(&mut s, LeafPayload::MeshResource(rid), shift(1.));
    add(
        &mut s,
        LeafPayload::Mesh(Arc::new(IndexedMesh::new(None, vec![], vec![]))),
        Transform3D::Identity,
    );
    let mut ticks = vec![];
    let spatial = SceneSpatial::primed_graph_with_progress(s, |p| ticks.push(p));
    assert_eq!(
        ticks,
        vec![
            PrimingProgress {
                primed_triangles: 1,
                total_triangles: 2
            },
            PrimingProgress {
                primed_triangles: 2,
                total_triangles: 2
            },
            PrimingProgress {
                primed_triangles: 2,
                total_triangles: 2
            }
        ]
    );
    assert!(std::ptr::eq(
        spatial.leaf(a).unwrap().clusters().unwrap(),
        spatial.leaf(b).unwrap().clusters().unwrap()
    ));
    let mut called = false;
    SceneSpatial::primed_graph_with_progress(Scene::new(), |_| called = true);
    assert!(!called);
}
#[test]
fn malformed_faces_still_prime_and_vertices_without_faces_have_bounds() {
    let mut m = (*mesh()).clone();
    m.faces[0] = IndexedTriangle::from_raw_indices(Normal::new([0.; 3]), [999, 0, 0]);
    let mut s = Scene::new();
    let id = add(
        &mut s,
        LeafPayload::Mesh(Arc::new(m)),
        Transform3D::Identity,
    );
    let mut ticks = vec![];
    let spatial = SceneSpatial::primed_graph_with_progress(s, |p| ticks.push(p));
    assert_eq!(ticks.len(), 1);
    assert!(spatial.leaf(id).unwrap().clusters().is_none());
    assert!(spatial.leaf(id).unwrap().local_aabb().is_some());
}
#[test]
fn structural_order_and_ancestor_composition() {
    let mut s = Scene::new();
    let root = s.add_group(None, shift(10.));
    s.add_root(root).unwrap();
    let nested = s.add_group(None, shift(5.));
    s.add_child(root, nested).unwrap();
    let a = s
        .add_leaf(None, shift(2.), LeafPayload::Mesh(mesh()))
        .unwrap();
    s.add_child(nested, a).unwrap();
    let b = add(&mut s, LeafPayload::Mesh(mesh()), shift(3.));
    let hidden = s
        .add_leaf(None, shift(0.), LeafPayload::Mesh(mesh()))
        .unwrap();
    let rid = s.add_mesh_resource_arc(mesh());
    let unlisted = s
        .add_leaf(None, shift(4.), LeafPayload::MeshResource(rid))
        .unwrap();
    let spatial = SceneSpatial::from_graph(s);
    assert_eq!(spatial.leaf_ids(), vec![a, b]);
    assert_eq!(spatial.leaf_index_of(root), None);
    assert_eq!(spatial.leaf_index_of(hidden), None);
    assert!(spatial.leaf(hidden).is_none());
    assert!(spatial.leaf(unlisted).is_some());
    for (i, id) in spatial.leaf_ids().iter().enumerate() {
        assert_eq!(spatial.leaf_id_at_index(i), Some(*id));
        assert_eq!(spatial.leaf_index_of(*id), Some(i));
    }
    assert!(spatial.leaf_id_at_index(2).is_none());
    let q = spatial.leaf(a).unwrap();
    assert_eq!(
        q.parent_world()
            .apply_to_vertex(Vertex::new([0.; 3]))
            .coords(),
        [15., 0., 0.]
    );
    assert_eq!(
        q.world_transform()
            .apply_to_vertex(Vertex::new([0.; 3]))
            .coords(),
        [17., 0., 0.]
    );
}
#[test]
fn bounds_use_supplied_transform_and_separate_translation_rounding() {
    let mut s = Scene::new();
    let id = add(&mut s, LeafPayload::Mesh(mesh()), shift(500.));
    let spatial = SceneSpatial::from_graph(s);
    let q = spatial.leaf(id).unwrap();
    assert_eq!(q.transformed_aabb(Transform3D::Identity), q.local_aabb());
    for a in [
        AffineTransform3D::from_rotation_z(0.7),
        AffineTransform3D::from_non_uniform_scale([-2., 0., 3.]),
        AffineTransform3D::from_rotation_x(1.2),
    ] {
        let mut expected = Aabb::empty();
        for v in q.mesh().vertices() {
            expected.extend(a.apply_to_vertex(*v));
        }
        assert_eq!(q.transformed_aabb(Transform3D::Affine(a)), Some(expected));
        for x in [1., 123456789.25, -10.] {
            let t = AffineTransform3D::from_translation([x, 0., 0.]).compose(&a);
            let got = q.transformed_aabb(Transform3D::Affine(t)).unwrap();
            let mut min = expected.min().coords();
            let mut max = expected.max().coords();
            min[0] += x as f32;
            max[0] += x as f32;
            assert_eq!(got, Aabb::from_min_max(Vertex::new(min), Vertex::new(max)));
        }
    }
}
#[test]
fn transform_edits_preserve_cluster_identity_and_failed_edits_preserve_queries() {
    let mut s = Scene::new();
    let id = add(&mut s, LeafPayload::Mesh(mesh()), Transform3D::Identity);
    let mut spatial = SceneSpatial::primed_graph(s);
    let address = spatial.leaf(id).unwrap().clusters().unwrap() as *const _;
    spatial
        .apply_edit(SceneEdit::SetLocalTransform {
            node_id: id,
            before: Transform3D::Identity,
            after: shift(4.),
        })
        .unwrap();
    assert_eq!(
        spatial.leaf(id).unwrap().clusters().unwrap() as *const _,
        address
    );
    let empty = spatial.allocate_unbound_node_id();
    let before = spatial.graph().clone();
    let edit = SceneEdit::Compound(vec![
        SceneEdit::SetLocalTransform {
            node_id: id,
            before: shift(4.),
            after: shift(9.),
        },
        SceneEdit::SetLocalTransform {
            node_id: empty,
            before: shift(0.),
            after: shift(1.),
        },
    ]);
    assert!(spatial.apply_edit(edit).is_err());
    assert_eq!(spatial.graph(), &before);
    assert_eq!(
        spatial.leaf(id).unwrap().clusters().unwrap() as *const _,
        address
    );
}
#[test]
fn replacements_refresh_all_users_and_inline_installations() {
    let mut s = Scene::new();
    let old = mesh();
    let rid = s.add_mesh_resource_arc(old.clone());
    let a = add(
        &mut s,
        LeafPayload::MeshResource(rid),
        Transform3D::Identity,
    );
    let b = add(&mut s, LeafPayload::MeshResource(rid), shift(1.));
    let mut spatial = SceneSpatial::primed_graph(s);
    let new = Arc::new(IndexedMesh::new(None, vec![Vertex::new([100.; 3])], vec![]));
    spatial
        .apply_edit(SceneEdit::ReplaceMeshResource {
            rid,
            before: old,
            after: new.clone(),
        })
        .unwrap();
    for id in [a, b] {
        let q = spatial.leaf(id).unwrap();
        assert!(std::ptr::eq(q.mesh(), new.as_ref()));
        assert_eq!(q.local_aabb().unwrap().min().coords(), [100.; 3]);
        assert!(q.clusters().is_none());
    }
    let remove = SceneEdit::remove_leaf_with_gc(spatial.graph(), a).unwrap();
    spatial.apply_edit(remove).unwrap();
    assert!(spatial.leaf(a).is_none());
    spatial
        .apply_edit(SceneEdit::AddLeaf(sc_scene_edit::LeafRecord {
            node_id: a,
            attachment: LeafAttachment::Root { position: 0 },
            payload: LeafPayload::Mesh(mesh()),
            local_transform: shift(2.),
            meta: None,
        }))
        .unwrap();
    assert!(spatial.leaf(a).unwrap().clusters().is_some());
    let before = spatial.graph().leaf_mesh_arc(a).unwrap().clone();
    spatial
        .apply_edit(SceneEdit::ReplaceLeafMesh {
            node_id: a,
            before,
            after: new,
        })
        .unwrap();
    assert!(spatial.leaf(a).unwrap().clusters().is_none());
}
#[test]
fn whole_scene_bounds_agree_and_empty_is_none() {
    assert!(scene_aabb(&SceneSpatial::from_graph(Scene::new())).is_none());
    assert!(scene_tight_aabb(&SceneSpatial::from_graph(Scene::new())).is_none());
    let mut s = Scene::new();
    add(
        &mut s,
        LeafPayload::Mesh(mesh()),
        Transform3D::Affine(AffineTransform3D::from_rotation_z(0.7)),
    );
    add(
        &mut s,
        LeafPayload::Mesh(mesh()),
        Transform3D::Affine(AffineTransform3D::from_non_uniform_scale([-2., 3., 1.])),
    );
    let expected = s.aabb().unwrap();
    let spatial = SceneSpatial::from_graph(s);
    assert_eq!(scene_aabb(&spatial), expected);
    let mut union = Aabb::empty();
    for id in spatial.leaf_ids() {
        let q = spatial.leaf(id).unwrap();
        union.union(&q.transformed_aabb(q.world_transform()).unwrap());
    }
    assert_eq!(scene_tight_aabb(&spatial), Some(union));
}
#[test]
fn supported_types_are_send_and_sync() {
    fn check<T: Send + Sync>() {}
    check::<SceneSpatial>();
}

#[cfg(debug_assertions)]
#[test]
fn constructor_reference_diagnostics_and_precedence() {
    for constructor in [SceneSpatial::from_graph, SceneSpatial::primed_graph] {
        let mut s = Scene::new();
        let root = s.add_group(None, Transform3D::Identity);
        let rid = s.register_subtree_resource(root).unwrap();
        let id = s.add_reference(None, Transform3D::Identity, rid).unwrap();
        let panic = std::panic::catch_unwind(|| constructor(s)).unwrap_err();
        let text = panic.downcast_ref::<String>().unwrap();
        assert_eq!(
            text,
            &format!(
                "SceneSpatial cannot hold subtree references: node {} references a subtree",
                id.get()
            )
        );
    }
    let mut s = Scene::new();
    let root = s.add_group(None, Transform3D::Identity);
    s.register_subtree_resource(root).unwrap();
    let panic = std::panic::catch_unwind(|| SceneSpatial::from_graph(s)).unwrap_err();
    assert_eq!(
        panic.downcast_ref::<&str>(),
        Some(&"SceneSpatial requires a scene without registered subtree resources")
    );
}
#[test]
fn csg_operands_are_not_evaluated_or_plate_filtered() {
    let mut s = Scene::new();
    let mut children = vec![];
    for x in [0., 10.] {
        children.push(
            s.add_leaf(None, shift(x), LeafPayload::Mesh(mesh()))
                .unwrap(),
        );
    }
    let parent = s
        .add_csg(
            None,
            Transform3D::Identity,
            sc_scene::CsgOp::Difference,
            children.clone(),
        )
        .unwrap();
    s.add_root(parent).unwrap();
    let spatial = SceneSpatial::from_graph(s);
    assert_eq!(spatial.leaf_ids(), children);
    assert_eq!(
        scene_tight_aabb(&spatial).unwrap().max().coords(),
        [17., 8., 9.]
    );
}
#[test]
fn resource_cache_identity_survives_resource_arena_growth_and_plates() {
    let mut s = Scene::new();
    let rid = s.add_mesh_resource_arc(mesh());
    let id = add(
        &mut s,
        LeafPayload::MeshResource(rid),
        Transform3D::Identity,
    );
    let mut spatial = SceneSpatial::primed_graph(s);
    let address = spatial.leaf(id).unwrap().clusters().unwrap() as *const _;
    for _ in 0..30 {
        spatial.allocate_unbound_mesh_resource_id();
    }
    let before = spatial.graph().plates().clone();
    let mut after = before.clone();
    after.add();
    spatial
        .apply_edit(SceneEdit::SetPlates { before, after })
        .unwrap();
    assert_eq!(
        spatial.leaf(id).unwrap().clusters().unwrap() as *const _,
        address
    );
}
#[test]
fn empty_vertices_and_debug_contracts() {
    let mut s = Scene::new();
    let id = add(
        &mut s,
        LeafPayload::Mesh(Arc::new(IndexedMesh::new(None, vec![], vec![]))),
        Transform3D::Identity,
    );
    let spatial = SceneSpatial::primed_graph(s);
    let q = spatial.leaf(id).unwrap();
    assert!(q.local_aabb().is_none());
    assert!(q.transformed_aabb(shift(4.)).is_none());
    assert!(q.clusters().is_none());
    assert_eq!(spatial.leaf_ids(), vec![id]);
    let debug = format!("{q:?}");
    for field in [
        "ObjectQuery",
        "node",
        "world_transform",
        "parent_world",
        "..",
    ] {
        assert!(debug.contains(field));
    }
    assert!(format!("{spatial:?}").contains("graph"));
}

#[test]
fn cluster_bounds_follow_each_face_chunk() {
    let vertices = [
        Vertex::new([-2., 1., 4.]),
        Vertex::new([3., 5., 6.]),
        Vertex::new([10., 20., 30.]),
    ];
    let first = IndexedTriangle::from_raw_indices(Normal::new([0.; 3]), [0, 1, 0]);
    let last = IndexedTriangle::from_raw_indices(Normal::new([0.; 3]), [2, 2, 2]);
    let mut faces = vec![first; 64];
    faces.push(last);
    let clusters = TriangleClusters::build(&vertices, &faces).unwrap();
    let chunks: Vec<_> = clusters.iter().collect();
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].0, 0..64);
    assert_eq!(chunks[0].1.min().coords(), [-2., 1., 4.]);
    assert_eq!(chunks[0].1.max().coords(), [3., 5., 6.]);
    assert_eq!(chunks[1].0, 64..65);
    assert_eq!(chunks[1].1.min().coords(), [10., 20., 30.]);
    assert!(TriangleClusters::build(&vertices, &[]).is_none());
    faces[64] = IndexedTriangle::from_raw_indices(Normal::new([0.; 3]), [2, 3, 2]);
    assert!(TriangleClusters::build(&vertices, &faces).is_none());
}

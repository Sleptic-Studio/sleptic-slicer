// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use std::path::PathBuf;
use std::sync::Arc;

use sc_mesh_core::IndexedMesh;
use sc_mesh_core::primitives::{IndexedTriangle, Normal, Vertex};
use sc_scene::node::SceneMetadata;
use sc_scene::{NodeId, Scene};

use super::*;
use crate::scene_document::support_owner;
use crate::selection::{LeafId, Selection};

/// A right triangle with legs of 1 along X and Y.
fn triangle() -> Arc<IndexedMesh> {
    let corners = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
        .map(Vertex::new)
        .to_vec();
    let face = IndexedTriangle::from_raw_indices(Normal::new([0.0, 0.0, 1.0]), [0, 1, 2]);
    Arc::new(IndexedMesh {
        meta: None,
        vertices: corners,
        faces: vec![face],
    })
}

fn moved(by: [f64; 3]) -> Transform3D {
    Transform3D::Affine(AffineTransform3D::from_translation(by))
}

/// A scene as a loaded file has it: parts under one group root.
fn scene_of(parts: &[(Option<SceneMetadata>, Transform3D, LeafPayload)]) -> (Scene, Vec<NodeId>) {
    let mut scene = Scene::new();
    let root = scene.add_group(None, Transform3D::Identity);
    scene.add_root(root).unwrap();
    let ids = parts
        .iter()
        .map(|(meta, placed, payload)| {
            let id = scene
                .add_leaf(meta.clone(), *placed, payload.clone())
                .unwrap();
            scene.add_child(root, id).unwrap();
            id
        })
        .collect();
    (scene, ids)
}

fn opened(scene: Scene, selected: Option<NodeId>) -> std::rc::Rc<SceneDocument> {
    let doc = SceneDocument::new();
    doc.take_mesh_scene(scene, PathBuf::from("/tmp/clipboard.obj"));
    if let Some(id) = selected {
        doc.choose(Selection::One(LeafId(id)));
    }
    doc
}

fn first_mesh(fragment: &SceneFragment) -> *const IndexedMesh {
    fragment
        .scene()
        .mesh_resources_iter()
        .next()
        .map(|(_, mesh)| mesh as *const IndexedMesh)
        .unwrap()
}

fn root_transform(fragment: &SceneFragment) -> Transform3D {
    let scene = fragment.scene();
    scene.node(scene.roots()[0]).unwrap().local_transform
}

#[test]
fn nothing_to_copy_without_a_scene_a_selection_or_a_mesh() {
    assert!(copy_of_selection(&SceneDocument::new()).is_none());
    let (scene, _) = scene_of(&[(None, Transform3D::Identity, LeafPayload::Mesh(triangle()))]);
    assert!(copy_of_selection(&opened(scene, None)).is_none());
    // A group passed off as a part has no mesh to copy.
    let mut scene = Scene::new();
    let group = scene.add_group(None, Transform3D::Identity);
    scene.add_root(group).unwrap();
    assert!(copy_of_selection(&opened(scene, Some(group))).is_none());
}

#[test]
fn a_copy_shares_the_mesh_whether_inline_or_a_resource() {
    let mesh = triangle();
    let (scene, ids) = scene_of(&[(None, Transform3D::Identity, LeafPayload::Mesh(mesh.clone()))]);
    let fragment = copy_of_selection(&opened(scene, Some(ids[0]))).unwrap();
    assert_eq!(first_mesh(&fragment), Arc::as_ptr(&mesh));
    let mut scene = Scene::new();
    let resource = scene.add_mesh_resource_arc(mesh.clone());
    let part = scene
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::MeshResource(resource),
        )
        .unwrap();
    scene.add_root(part).unwrap();
    let fragment = copy_of_selection(&opened(scene, Some(part))).unwrap();
    assert_eq!(first_mesh(&fragment), Arc::as_ptr(&mesh));
}

#[test]
fn a_copy_keeps_the_turn_and_drops_the_place() {
    let quarter_turn = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    let placed = Transform3D::Affine(AffineTransform3D::from_linear_and_translation(
        quarter_turn,
        [7.0, 8.0, 9.0],
    ));
    let (scene, ids) = scene_of(&[(None, placed, LeafPayload::Mesh(triangle()))]);
    let fragment = copy_of_selection(&opened(scene, Some(ids[0]))).unwrap();
    let Transform3D::Affine(copied) = root_transform(&fragment) else {
        panic!("the turn is kept");
    };
    assert_eq!(
        (*copied.linear(), *copied.translation()),
        (quarter_turn, [0.0; 3])
    );
    // A part not moved at all stays plain identity.
    let (scene, ids) = scene_of(&[(None, Transform3D::Identity, LeafPayload::Mesh(triangle()))]);
    let fragment = copy_of_selection(&opened(scene, Some(ids[0]))).unwrap();
    assert_eq!(root_transform(&fragment), Transform3D::Identity);
}

fn support_note(owner: NodeId) -> Option<SceneMetadata> {
    let comment = format!(
        "sc-generated-support\nkind = column\nowner = {}\n",
        owner.get()
    );
    Some(SceneMetadata {
        name: None,
        comment: Some(comment),
    })
}

#[test]
fn supports_come_along_pointing_at_their_copied_model() {
    let mut scene = Scene::new();
    let root = scene.add_group(None, Transform3D::Identity);
    scene.add_root(root).unwrap();
    let model = scene
        .add_leaf(None, moved([10.0, 0.0, 0.0]), LeafPayload::Mesh(triangle()))
        .unwrap();
    scene.add_child(root, model).unwrap();
    let support = scene
        .add_leaf(
            support_note(model),
            moved([12.0, 0.0, 0.0]),
            LeafPayload::Mesh(triangle()),
        )
        .unwrap();
    scene.add_child(root, support).unwrap();
    let fragment = copy_of_selection(&opened(scene, Some(model))).unwrap();
    let copy = fragment.scene();
    let roots = copy.roots().to_vec();
    assert_eq!(roots.len(), 2, "the model and its support");
    // Placed relative to the model, which lands at the origin.
    let Transform3D::Affine(at_origin) = copy.node(roots[0]).unwrap().local_transform else {
        panic!("the model was placed");
    };
    assert_eq!(*at_origin.translation(), [0.0; 3]);
    assert_eq!(
        copy.node(roots[1]).unwrap().local_transform,
        moved([2.0, 0.0, 0.0])
    );
    let note = copy
        .node(roots[1])
        .unwrap()
        .meta
        .clone()
        .unwrap()
        .comment
        .unwrap();
    assert_eq!(
        note,
        format!(
            "sc-generated-support\nkind = column\nowner = {}\n",
            roots[0].get()
        )
    );
    assert_eq!(support_owner(copy, roots[1]), Some(roots[0]));
}

#[test]
fn the_owner_line_is_replaced_and_moved_last() {
    assert_eq!(with_owner("a\nowner = 3\nb", 9), "a\nb\nowner = 9\n");
    assert_eq!(with_owner("owner = 3", 9), "owner = 9\n");
    assert_eq!(with_owner("", 9), "owner = 9\n");
}

#[test]
fn the_anchor_is_the_plate_and_centre_of_the_parts_and_supports() {
    let (scene, ids) = scene_of(&[(None, moved([4.0, -2.0, 0.0]), LeafPayload::Mesh(triangle()))]);
    let model = ids[0];
    assert_eq!(
        selection_anchor(&opened(scene, Some(model))),
        Some((1, [4.5, -1.5]))
    );
    let (scene, _) = scene_of(&[(None, moved([4.0, -2.0, 0.0]), LeafPayload::Mesh(triangle()))]);
    assert_eq!(
        selection_anchor(&opened(scene, None)),
        None,
        "nothing selected"
    );
    // A support stretches the footprint.
    let mut scene = Scene::new();
    let model = scene
        .add_leaf(None, Transform3D::Identity, LeafPayload::Mesh(triangle()))
        .unwrap();
    scene.add_root(model).unwrap();
    let support = scene
        .add_leaf(
            support_note(model),
            moved([3.0, 1.0, 0.0]),
            LeafPayload::Mesh(triangle()),
        )
        .unwrap();
    scene.add_root(support).unwrap();
    assert_eq!(
        selection_anchor(&opened(scene, Some(model))),
        Some((1, [2.0, 1.0]))
    );
}

#[test]
fn the_clipboard_keeps_its_source() {
    let clipboard = Clipboard::new(SceneFragment::from_leaf_snapshot(
        triangle(),
        Transform3D::Identity,
    ));
    assert_eq!(clipboard.source(), None);
    assert_eq!(
        clipboard.with_source(Some((2, [4.0, -3.0]))).source(),
        Some((2, [4.0, -3.0]))
    );
}

#[test]
fn fragment_bounds_follow_the_placement() {
    let mesh = triangle();
    let plain = copied_box(&SceneFragment::from_leaf_snapshot(
        mesh.clone(),
        Transform3D::Identity,
    ))
    .unwrap();
    let local = mesh.local_aabb().unwrap();
    assert_eq!(
        (plain.min().coords(), plain.max().coords()),
        (local.min().coords(), local.max().coords())
    );
    // A quarter turn about Z moves the triangle to x in [-1, 0].
    let turned = Transform3D::Affine(AffineTransform3D::from_rotation_z(
        std::f64::consts::FRAC_PI_2,
    ));
    let aabb = copied_box(&SceneFragment::from_leaf_snapshot(mesh, turned)).unwrap();
    let close = |a: f32, b: f32| (a - b).abs() <= 1e-5;
    let (min, max) = (aabb.min().coords(), aabb.max().coords());
    assert!(
        close(min[0], -1.0) && close(max[0], 0.0) && close(min[1], 0.0) && close(max[1], 1.0),
        "{min:?} {max:?}"
    );
    let empty = Arc::new(IndexedMesh::new(None, Vec::new(), Vec::new()));
    assert!(
        copied_box(&SceneFragment::from_leaf_snapshot(
            empty,
            Transform3D::Identity
        ))
        .is_none()
    );
}

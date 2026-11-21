// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The document's held slice: keeping it, dropping it with the scene, and
//! telling whether the geometry moved on since it was made (undo back to
//! it counts as unchanged; history lost past the undo limit does not).
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use sc_mesh_core::IndexedMesh;
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_print_primitives::{AaLevel, MaterialId, PrinterId};
use sc_scene::{LeafPayload, NodeId, Scene};
use sc_sl1cer::contour::{ContourLayer, ContourStack};
use sc_sl1cer::profile::{
    BuildVolume, LayerSchedule, Mirror, PackingMode, PixelResolution, SliceProfile,
};

use crate::scene_document::{SceneDocument, SceneEdit, Revision, UNDO_DEPTH};
use crate::slice_result::{SliceParams, SliceResult};

fn one_part() -> Scene {
    let mut scene = Scene::new();
    let root = scene.add_group(None, Transform3D::Identity);
    scene.add_root(root).unwrap();
    let empty = Arc::new(IndexedMesh::new(None, Vec::new(), Vec::new()));
    let part = scene
        .add_leaf(None, Transform3D::Identity, LeafPayload::Mesh(empty))
        .unwrap();
    scene.add_child(root, part).unwrap();
    scene
}

fn open(doc: &SceneDocument) {
    doc.take_mesh_scene(one_part(), PathBuf::from("/tmp/slice_result.obj"));
}

fn opened() -> (Rc<SceneDocument>, NodeId) {
    let doc = SceneDocument::new();
    open(&doc);
    let part = doc.read_scene(|s| s.leaf_id_at_index(0)).flatten().unwrap();
    (doc, part)
}

fn at_x(x: f64) -> Transform3D {
    if x == 0.0 {
        Transform3D::Identity
    } else {
        Transform3D::Affine(AffineTransform3D::from_translation([x, 0.0, 0.0]))
    }
}

/// Moves the part from x - 1 to x, as one undoable edit.
fn move_to(doc: &SceneDocument, part: NodeId, x: usize) {
    let x = x as f64;
    let edit = SceneEdit::SetLocalTransform {
        node_id: part,
        before: at_x(x - 1.0),
        after: at_x(x),
    };
    doc.commit_edit(edit.clone()).unwrap();
    doc.record_edit(edit);
}

fn layers(count: u32, height: f64) -> LayerSchedule {
    LayerSchedule::uniform(height, 0.05, count).unwrap()
}

/// A slice of `count` layers made at `geometry`.
fn slice(geometry: Revision, count: u32) -> SliceResult {
    let profile = SliceProfile::new(
        BuildVolume::new(60.0, 60.0, 100.0).unwrap(),
        PixelResolution::new(1920, 1080).unwrap(),
        layers(1, 0.05),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    SliceResult {
        merged: ContourStack::new(vec![ContourLayer::new(0.05, vec![])]).unwrap(),
        schedule: layers(count, 0.05),
        token: "sample-token".into(),
        params: SliceParams {
            printer: PrinterId::new("elegoo-saturn-4-ultra").unwrap(),
            resin: MaterialId::new("standard-405").unwrap(),
            first_layer_height: None,
            aa_level: AaLevel::Off,
            geometry,
        },
        profile,
    }
}

/// Slices the document as it is now.
fn slice_now(doc: &SceneDocument) -> Revision {
    let now = doc.revision();
    doc.keep_slice(slice(now, 1));
    now
}

#[test]
fn a_slice_is_held_until_the_scene_is_replaced_or_closed() {
    let (doc, _) = opened();
    assert!(doc.slicing_inputs().is_none() && doc.layer_total().is_none());
    let made_at = slice_now(&doc);
    let held = doc.slicing_inputs().unwrap();
    assert_eq!((held.geometry, held.aa_level), (made_at, AaLevel::Off));
    open(&doc);
    assert!(doc.slicing_inputs().is_none());
    slice_now(&doc);
    doc.empty();
    assert!(doc.slicing_inputs().is_none() && doc.layer_total().is_none());
}

#[test]
fn a_new_or_closed_scene_counts_as_changed_with_no_history() {
    // Both scenes have no edits; only the scene itself tells them apart.
    let (doc, _) = opened();
    let before = doc.revision();
    open(&doc);
    assert_ne!(doc.revision(), before);
    assert!(doc.slice_outdated_by_geometry(before));
    let (doc, _) = opened();
    let before = doc.revision();
    doc.empty();
    assert!(doc.slice_outdated_by_geometry(before));
}

#[test]
fn edits_change_the_geometry_and_undo_back_to_the_slice_restores_it() {
    let (doc, part) = opened();
    let unedited = doc.revision();
    move_to(&doc, part, 1);
    assert_ne!(doc.revision(), unedited);
    let made_at = slice_now(&doc);
    assert!(!doc.slice_outdated_by_geometry(made_at) && !doc.slice_is_geometry_stale());
    move_to(&doc, part, 2);
    assert!(doc.slice_outdated_by_geometry(made_at) && doc.slice_is_geometry_stale());
    assert!(doc.step_back().unwrap());
    assert!(!doc.slice_outdated_by_geometry(made_at));
    assert!(doc.step_forward().unwrap());
    assert!(doc.slice_outdated_by_geometry(made_at));
    // Slicing again makes the current geometry the reference.
    slice_now(&doc);
    assert!(!doc.slice_is_geometry_stale());
}

#[test]
fn a_slice_of_the_unedited_scene_survives_undoing_everything_unless_history_was_lost() {
    let (doc, part) = opened();
    let made_at = slice_now(&doc);
    move_to(&doc, part, 1);
    assert!(doc.slice_outdated_by_geometry(made_at));
    assert!(doc.step_back().unwrap());
    assert!(!doc.slice_outdated_by_geometry(made_at));
    // One edit past the undo limit drops the oldest: undoing all that is
    // left does not get back to the sliced geometry.
    let (doc, part) = opened();
    let made_at = slice_now(&doc);
    for x in 1..=UNDO_DEPTH + 1 {
        move_to(&doc, part, x);
    }
    for _ in 0..UNDO_DEPTH {
        assert!(doc.step_back().unwrap());
    }
    assert!(!doc.has_past());
    assert!(doc.slice_outdated_by_geometry(made_at));
}

#[test]
fn nothing_is_stale_without_a_slice() {
    let (doc, part) = opened();
    move_to(&doc, part, 1);
    assert!(!doc.slice_is_geometry_stale());
}

#[test]
fn layers_are_counted_and_their_tops_read_from_the_held_slice() {
    let (doc, _) = opened();
    assert!(doc.layer_top_mm(0).is_none());
    doc.keep_slice(slice(doc.revision(), 3));
    assert_eq!(doc.layer_total(), Some(3));
    let tops: Vec<f64> = layers(3, 0.05).iter_slab_tops().collect();
    assert_eq!(
        (doc.layer_top_mm(0), doc.layer_top_mm(2)),
        (Some(tops[0]), Some(tops[2]))
    );
    assert!(doc.layer_top_mm(3).is_none());
}

#[test]
fn slices_are_saved_only_while_they_match_the_scene_and_the_layers() {
    let (doc, part) = opened();
    let one_layer = layers(1, 0.05);
    assert!(doc.slices_to_save(&one_layer).is_none(), "nothing held");
    slice_now(&doc);
    let (plate, token) = doc.slices_to_save(&one_layer).unwrap();
    assert_eq!(token, "sample-token");
    assert_eq!(
        plate.slab_tops,
        one_layer.iter_slab_tops().collect::<Vec<_>>()
    );
    // A thicker layer is a different schedule, whatever the profile says.
    assert!(doc.slices_to_save(&layers(1, 0.10)).is_none());
    move_to(&doc, part, 1);
    assert!(
        doc.slices_to_save(&one_layer).is_none(),
        "the scene moved on"
    );
}

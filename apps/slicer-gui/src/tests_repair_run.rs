// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use std::path::PathBuf;

use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_mesh_fixtures::{
    cuboid_with_y_tunnel_and_flipped_faces, cuboid_with_z_tunnel_and_flipped_faces, unit_cube_mesh,
    unit_cube_with_flipped_faces,
};
use sc_scene::node::LeafPayload;

use super::*;
use crate::mesh_health::{MeshHealth, scene_mesh_health};

/// Two broken parts of their own, a broken mesh shared by two parts, and a
/// clean part: four meshes, three to repair.
fn broken_scene() -> Scene {
    let mut scene = Scene::new();
    let root = scene.add_group(None, Transform3D::Identity);
    scene.add_root(root).unwrap();
    let add = |scene: &mut Scene, payload| {
        let id = scene.add_leaf(None, Transform3D::Identity, payload).unwrap();
        scene.add_child(root, id).unwrap();
    };
    for mesh in [unit_cube_with_flipped_faces(), cuboid_with_z_tunnel_and_flipped_faces()] {
        add(&mut scene, LeafPayload::Mesh(Arc::new(mesh)));
    }
    let shared = scene.add_mesh_resource(cuboid_with_y_tunnel_and_flipped_faces());
    add(&mut scene, LeafPayload::MeshResource(shared));
    add(&mut scene, LeafPayload::MeshResource(shared));
    add(&mut scene, LeafPayload::Mesh(Arc::new(unit_cube_mesh())));
    scene
}

fn open(scene: Scene, path: &str) -> Rc<SceneDocument> {
    let doc = SceneDocument::new();
    doc.take_mesh_scene(scene, PathBuf::from(path));
    doc
}

/// Every stored mesh of the document's scene, by where it is stored.
fn meshes(doc: &SceneDocument) -> Vec<(MeshTarget, Arc<IndexedMesh>)> {
    doc.read_scene(|spatial| {
        let scene = spatial.graph();
        scene.mesh_targets().into_iter().filter_map(|t| Some((t, current_mesh_of(scene, t)?))).collect()
    })
    .unwrap()
}

fn health(doc: &SceneDocument) -> MeshHealth {
    doc.read_scene(|s| scene_mesh_health(s.graph())).unwrap()
}

fn repaired(doc: &SceneDocument) -> FinishedRepair {
    doc.read_scene(|spatial| repair_copy(spatial.graph().clone())).unwrap()
}

#[test]
fn only_the_broken_meshes_are_replaced() {
    let scene = broken_scene();
    let completed = repair_copy(scene.clone());
    assert_eq!((completed.repaired_parts.len(), completed.finished_swaps.len()), (4, 3));
    for change in &completed.finished_swaps {
        assert!(Arc::ptr_eq(&current_mesh_of(&scene, change.mesh_slot).unwrap(), &change.old_mesh));
        assert!(!Arc::ptr_eq(&change.old_mesh, &change.new_mesh));
    }
    // A line for each repaired mesh, naming its fixes; none for the clean one.
    let lines = repair_report_lines(&completed.repaired_parts, &scene);
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|line| line.contains("reoriented") && line.contains("face")), "{lines:?}");
}

#[test]
fn a_clean_scene_needs_nothing() {
    let mut scene = Scene::new();
    let id = scene.add_leaf(None, Transform3D::Identity, LeafPayload::Mesh(Arc::new(unit_cube_mesh()))).unwrap();
    scene.add_root(id).unwrap();
    let completed = repair_copy(scene.clone());
    assert!(completed.finished_swaps.is_empty());
    assert!(completed.repaired_parts.iter().all(|o| o.fixes.is_empty() && o.refusal.is_none() && o.edge_manifold_after));
    assert_eq!(repair_summary_text(&completed.repaired_parts), "No problems found.");
    assert!(repair_report_lines(&completed.repaired_parts, &scene).is_empty());
    let doc = open(scene, "/tmp/clean.obj");
    let started = doc.revision();
    assert_eq!(decide_repair(&doc, &repaired(&doc), started), RepairVerdict::NoChange);
    assert!(!doc.has_past());
}

#[test]
fn a_repair_is_one_undo_step_that_brings_back_the_same_meshes() {
    let doc = open(broken_scene(), "/tmp/repair-run-fixture.obj");
    let (started, before) = (doc.revision(), meshes(&doc));
    assert_eq!(health(&doc), MeshHealth::HasProblems);
    let completed = repaired(&doc);
    assert_eq!(repair_summary_text(&completed.repaired_parts), "3 mesh(es) repaired.");
    assert_eq!(decide_repair(&doc, &completed, started), RepairVerdict::Committed);
    assert_eq!(health(&doc), MeshHealth::ProblemFree);
    // A slice made before the repair is now out of date.
    assert!(doc.slice_outdated_by_geometry(started));
    assert!(doc.step_back().unwrap());
    assert_eq!(health(&doc), MeshHealth::HasProblems);
    for ((target, original), (_, now)) in before.iter().zip(meshes(&doc)) {
        assert!(Arc::ptr_eq(original, &now), "{target:?}");
    }
}

#[test]
fn a_result_for_a_changed_or_replaced_scene_is_dropped() {
    // Moved during the run.
    let moved = open(broken_scene(), "/tmp/repair-run-fixture.obj");
    let started = moved.revision();
    let completed = repaired(&moved);
    let part = moved.read_scene(|s| s.leaf_id_at_index(0).unwrap()).unwrap();
    let after = Transform3D::Affine(AffineTransform3D::from_translation([1.0, 0.0, 0.0]));
    let edit = SceneEdit::SetLocalTransform { node_id: part, before: Transform3D::Identity, after };
    moved.commit_edit(edit.clone()).unwrap();
    moved.record_edit(edit);
    assert_eq!(decide_repair(&moved, &completed, started), RepairVerdict::DroppedOutdated);
    assert_eq!(health(&moved), MeshHealth::HasProblems);
    // Another file opened during the run, its meshes numbered exactly as
    // before: only the scene's identity tells them apart.
    let replaced = open(broken_scene(), "/tmp/repair-run-fixture.obj");
    let started = replaced.revision();
    let completed = repaired(&replaced);
    replaced.take_mesh_scene(broken_scene(), PathBuf::from("/tmp/repair-run-other.obj"));
    let fresh = meshes(&replaced);
    assert_eq!(decide_repair(&replaced, &completed, started), RepairVerdict::DroppedOutdated);
    assert_eq!(health(&replaced), MeshHealth::HasProblems);
    for ((target, installed), (_, now)) in fresh.iter().zip(meshes(&replaced)) {
        assert!(Arc::ptr_eq(installed, &now), "{target:?}");
    }
    assert!(!replaced.has_past());
}

/// One part with `mesh`, the part named `part` and the mesh `mesh_name`.
fn named_part(mesh: IndexedMesh, part: Option<&str>, mesh_name: Option<&str>) -> (Scene, MeshTarget) {
    let mut mesh = mesh;
    mesh.meta = mesh_name.map(|name| sc_mesh_core::indexed_mesh::MeshMetadata {
        name: Some(name.to_string()),
        header_comment: None,
    });
    let mut scene = Scene::new();
    let root = scene.add_group(None, Transform3D::Identity);
    scene.add_root(root).unwrap();
    let resource = scene.add_mesh_resource(mesh);
    let leaf = scene.add_leaf(None, Transform3D::Identity, LeafPayload::MeshResource(resource)).unwrap();
    scene.add_child(root, leaf).unwrap();
    let meta = part.map(|name| sc_scene::node::SceneMetadata { name: Some(name.to_string()), comment: None });
    scene.set_node_meta(leaf, meta).unwrap();
    (scene, MeshTarget::Resource(resource))
}

#[test]
fn a_mesh_is_called_by_its_part_then_its_own_name_then_its_number() {
    let (scene, target) = named_part(unit_cube_mesh(), Some("bracket"), Some("cube.stl"));
    assert_eq!(part_label(&scene, target, 0), "bracket");
    let (scene, target) = named_part(unit_cube_mesh(), None, Some("cube.stl"));
    assert_eq!(part_label(&scene, target, 0), "cube.stl");
    let (scene, target) = named_part(unit_cube_mesh(), None, None);
    assert_eq!(part_label(&scene, target, 2), "Mesh 3");
}

#[test]
fn a_single_repaired_mesh_is_counted_in_the_summary() {
    let (scene, _) = named_part(unit_cube_with_flipped_faces(), None, None);
    let completed = repair_copy(scene);
    assert_eq!(repair_summary_text(&completed.repaired_parts), "1 mesh(es) repaired.");
}

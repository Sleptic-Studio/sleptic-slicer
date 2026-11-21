// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

use super::{MeshHealth, opening_section, scene_mesh_health};
use crate::WorkspaceSection::{Prepare, Transform};
use sc_mesh_core::{IndexedMesh, transform::Transform3D};
use sc_scene::{LeafPayload, Scene};
use std::sync::Arc;

fn add_mesh(scene: &mut Scene, mesh: IndexedMesh) {
    let group = scene.add_group(None, Transform3D::Identity);
    scene.add_root(group).unwrap();
    let leaf = scene
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(mesh)),
        )
        .unwrap();
    scene.add_child(group, leaf).unwrap();
}

#[test]
fn classification_handles_empty_sound_broken_and_mixed_scenes_without_mutation() {
    let mut scene = Scene::new();
    assert_eq!(scene_mesh_health(&scene), MeshHealth::ProblemFree);
    add_mesh(&mut scene, sc_mesh_fixtures::unit_cube_mesh());
    assert_eq!(scene_mesh_health(&scene), MeshHealth::ProblemFree);
    let mut broken = sc_mesh_fixtures::unit_cube_mesh();
    broken.faces.pop();
    add_mesh(&mut scene, broken);
    let before = scene.reachable_leaf_count();
    assert_eq!(scene_mesh_health(&scene), MeshHealth::HasProblems);
    assert_eq!(scene_mesh_health(&scene), MeshHealth::HasProblems);
    assert_eq!(scene.reachable_leaf_count(), before);
}

#[test]
fn only_a_confirmed_sound_scene_starts_in_transform() {
    assert_eq!(MeshHealth::default(), MeshHealth::Unknown);
    assert_eq!(opening_section(MeshHealth::Unknown), Prepare);
    assert_eq!(opening_section(MeshHealth::HasProblems), Prepare);
    assert_eq!(opening_section(MeshHealth::ProblemFree), Transform);
}

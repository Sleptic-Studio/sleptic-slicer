// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_mesh_core::{IndexedMesh, transform::Transform3D};
use sc_scene::{LeafPayload, Scene, node::LeafAttachment};
use std::sync::Arc;
/// An independently owned scene snapshot that shares immutable mesh buffers.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneFragment {
    scene: Scene,
}
impl SceneFragment {
    pub fn from_leaf_snapshot(mesh: Arc<IndexedMesh>, transform: Transform3D) -> Self {
        Self::from_leaf_snapshots([(mesh, transform)])
    }
    pub fn from_leaf_snapshots(
        leaves: impl IntoIterator<Item = (Arc<IndexedMesh>, Transform3D)>,
    ) -> Self {
        let mut scene = Scene::new();
        for (mesh, transform) in leaves {
            let rid = match scene.find_mesh_resource_by_arc(&mesh) {
                Some(rid) => rid,
                None => scene.add_mesh_resource_arc(mesh),
            };
            let id = scene.allocate_unbound_node_id();
            // Fresh slots, installed resources and append positions satisfy installation requirements.
            let _ = scene.install_leaf_at(
                id,
                LeafAttachment::Root {
                    position: scene.roots().len(),
                },
                LeafPayload::MeshResource(rid),
                transform,
                None,
            );
        }
        Self { scene }
    }
    pub fn from_scene(scene: Scene) -> Self {
        Self { scene }
    }
    pub fn scene(&self) -> &Scene {
        &self.scene
    }
    pub fn into_scene(self) -> Scene {
        self.scene
    }
}

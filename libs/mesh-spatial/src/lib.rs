// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Scene queries with shared resource preparation and translation-independent bounds caching.

pub mod clusters;
pub use clusters::TriangleClusters;
use sc_mesh_core::{Aabb, IndexedMesh, Vertex, transform::Transform3D};
use sc_scene::{LeafPayload, MeshResourceId, NodeId, NodeKind, Scene, SceneError};
use sc_scene_edit::SceneEdit;
use std::{
    fmt,
    sync::{Mutex, OnceLock},
};

// Only the most recent linear transform is retained: translations reuse these extrema.
type LinearBounds = Option<([[f64; 3]; 3], Option<Aabb>)>;
#[derive(Debug, Default)]
struct Prepared {
    local: OnceLock<Option<Aabb>>,
    clusters: OnceLock<Option<TriangleClusters>>,
    linear: Mutex<LinearBounds>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimingProgress {
    pub primed_triangles: u64,
    pub total_triangles: u64,
}
/// An owned scene with stable-address preparation for each resource and supported inline leaf.
pub struct SceneSpatial {
    graph: Scene,
    inline: Vec<Option<Box<Prepared>>>,
    // Stable addresses preserve cluster identity when the resource arena grows.
    #[allow(clippy::vec_box)]
    resources: Vec<Box<Prepared>>,
}
impl fmt::Debug for SceneSpatial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SceneSpatial")
            .field("graph", &self.graph)
            .finish_non_exhaustive()
    }
}
impl SceneSpatial {
    pub fn from_graph(graph: Scene) -> Self {
        if cfg!(debug_assertions) {
            if let Some(id) = graph.first_reference_node() {
                panic!(
                    "SceneSpatial cannot hold subtree references: node {} references a subtree",
                    id.get()
                );
            }
            assert!(
                !graph.has_subtree_resources(),
                "SceneSpatial requires a scene without registered subtree resources"
            );
        }
        let mut result = Self {
            inline: (0..graph.nodes_len()).map(|_| None).collect(),
            resources: (0..graph.mesh_resources_len())
                .map(|_| Box::<Prepared>::default())
                .collect(),
            graph,
        };
        for id in result.leaf_ids() {
            if matches!(
                result.graph.node(id).map(|n| &n.kind),
                Some(NodeKind::Leaf(LeafPayload::Mesh(_)))
            ) {
                result.inline[id.index()] = Some(Box::<Prepared>::default());
            }
        }
        result
    }
    pub fn primed_graph(graph: Scene) -> Self {
        Self::primed_graph_with_progress(graph, |_| {})
    }
    pub fn primed_graph_with_progress(
        graph: Scene,
        mut progress: impl FnMut(PrimingProgress),
    ) -> Self {
        let result = Self::from_graph(graph);
        let ids = result.leaf_ids();
        let total_triangles = ids
            .iter()
            .filter_map(|id| result.graph.leaf_mesh_arc(*id))
            .map(|m| m.faces().len() as u64)
            .sum();
        let mut primed_triangles = 0;
        for id in ids {
            if let Some(mesh) = result.graph.leaf_mesh_arc(id) {
                let prepared = match result.graph.node(id).map(|node| &node.kind) {
                    Some(NodeKind::Leaf(LeafPayload::MeshResource(rid))) => {
                        result.resources.get(rid.index())
                    }
                    _ => result.inline.get(id.index()).and_then(Option::as_ref),
                };
                if let Some(prepared) = prepared {
                    prepared.local.get_or_init(|| mesh.local_aabb());
                    prepared
                        .clusters
                        .get_or_init(|| TriangleClusters::for_mesh(mesh));
                }
                primed_triangles += mesh.faces().len() as u64;
                progress(PrimingProgress {
                    primed_triangles,
                    total_triangles,
                });
            }
        }
        result
    }
    pub fn graph(&self) -> &Scene {
        &self.graph
    }
    pub fn into_graph(self) -> Scene {
        self.graph
    }
    pub fn allocate_unbound_node_id(&mut self) -> NodeId {
        let id = self.graph.allocate_unbound_node_id();
        self.inline.push(None);
        id
    }
    pub fn allocate_unbound_mesh_resource_id(&mut self) -> MeshResourceId {
        let id = self.graph.allocate_unbound_mesh_resource_id();
        self.resources.push(Box::<Prepared>::default());
        id
    }
    pub fn leaf_ids(&self) -> Vec<NodeId> {
        let mut ids = Vec::new();
        self.graph.for_each_reachable_node(|id, node| {
            if matches!(node.kind, NodeKind::Leaf(_)) {
                ids.push(id)
            }
        });
        ids
    }
    pub fn leaf_id_at_index(&self, index: usize) -> Option<NodeId> {
        self.leaf_ids().get(index).copied()
    }
    pub fn leaf_index_of(&self, id: NodeId) -> Option<usize> {
        self.leaf_ids().iter().position(|x| *x == id)
    }
    pub fn leaf(&self, id: NodeId) -> Option<ObjectQuery<'_>> {
        let node = self.graph.node(id)?;
        let prepared = match &node.kind {
            NodeKind::Leaf(LeafPayload::Mesh(_)) => self.inline.get(id.index())?.as_ref()?,
            NodeKind::Leaf(LeafPayload::MeshResource(rid)) => self.resources.get(rid.index())?,
            _ => return None,
        };
        let mesh = self.graph.leaf_mesh_arc(id)?.as_ref();
        let parent_world = match self.graph.parent_of(id) {
            Some(p) => self.graph.world_transform_of(p).ok()?,
            None => Transform3D::Identity,
        };
        Some(ObjectQuery {
            node: id,
            mesh,
            prepared,
            local_transform: node.local_transform,
            world_transform: parent_world.compose(&node.local_transform),
            parent_world,
        })
    }
    /// Update preparation only after application succeeds, preserving caches on rollback.
    pub fn apply_edit(&mut self, edit: SceneEdit) -> Result<(), SceneError> {
        edit.apply_to_scene(&mut self.graph)?;
        self.refresh(&edit);
        Ok(())
    }
    fn refresh(&mut self, edit: &SceneEdit) {
        match edit {
            SceneEdit::Compound(edits) => {
                for edit in edits {
                    self.refresh(edit)
                }
            }
            SceneEdit::ReplaceMeshResource { rid, .. } => self.invalidate_mesh_resource(*rid),
            SceneEdit::ReplaceLeafMesh { node_id, .. } | SceneEdit::AddLeaf(sc_scene_edit::LeafRecord { node_id, .. }) => {
                if let Some(slot) = self.inline.get_mut(node_id.index()) {
                    *slot = Some(Box::<Prepared>::default());
                }
            }
            SceneEdit::RemoveLeaf(sc_scene_edit::LeafRecord { node_id, .. }) => {
                if let Some(slot) = self.inline.get_mut(node_id.index()) {
                    *slot = None;
                }
            }
            _ => {}
        }
    }
    pub fn invalidate_leaf(&mut self, id: NodeId) {
        if matches!(
            self.graph.node(id).map(|n| &n.kind),
            Some(NodeKind::Leaf(LeafPayload::Mesh(_)))
        ) && let Some(slot) = self.inline.get_mut(id.index())
        {
            *slot = Some(Box::<Prepared>::default());
        }
    }
    pub fn invalidate_mesh_resource(&mut self, id: MeshResourceId) {
        if let Some(slot) = self.resources.get_mut(id.index()) {
            **slot = Prepared::default();
        }
    }
}
/// A resolved mesh and ancestor transforms, borrowing the owned scene and its caches.
pub struct ObjectQuery<'a> {
    node: NodeId,
    mesh: &'a IndexedMesh,
    prepared: &'a Prepared,
    local_transform: Transform3D,
    world_transform: Transform3D,
    parent_world: Transform3D,
}
impl fmt::Debug for ObjectQuery<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObjectQuery")
            .field("node", &self.node)
            .field("world_transform", &self.world_transform)
            .field("parent_world", &self.parent_world)
            .finish_non_exhaustive()
    }
}
impl<'a> ObjectQuery<'a> {
    pub fn mesh(&self) -> &'a IndexedMesh {
        self.mesh
    }
    pub fn local_transform(&self) -> Transform3D {
        self.local_transform
    }
    pub fn world_transform(&self) -> Transform3D {
        self.world_transform
    }
    pub fn parent_world(&self) -> Transform3D {
        self.parent_world
    }
    pub fn local_aabb(&self) -> Option<Aabb> {
        *self.prepared.local.get_or_init(|| self.mesh.local_aabb())
    }
    pub fn clusters(&self) -> Option<&'a TriangleClusters> {
        self.prepared
            .clusters
            .get_or_init(|| TriangleClusters::for_mesh(self.mesh))
            .as_ref()
    }
    pub fn transformed_aabb(&self, transform: Transform3D) -> Option<Aabb> {
        let Transform3D::Affine(affine) = transform else {
            return self.local_aabb();
        };
        let linear = *affine.linear();
        let mut cached = match self.prepared.linear.lock() {
            Ok(lock) => lock,
            Err(poison) => poison.into_inner(),
        };
        let bounds = match *cached {
            Some((previous, bounds)) if previous == linear => bounds,
            _ => {
                let mut bounds = Aabb::empty();
                for vertex in self.mesh.vertices() {
                    let [x, y, z] = vertex.coords().map(f64::from);
                    let point = linear.map(|r| r[0].mul_add(x, r[1].mul_add(y, r[2] * z)) as f32);
                    bounds.extend(Vertex::new(point));
                }
                let bounds = if self.mesh.vertices().is_empty() {
                    None
                } else {
                    Some(bounds)
                };
                *cached = Some((linear, bounds));
                bounds
            }
        }?;
        let translation = affine.translation().map(|v| v as f32);
        let min = bounds.min().coords();
        let max = bounds.max().coords();
        Some(Aabb::from_min_max(
            Vertex::new(std::array::from_fn(|i| min[i] + translation[i])),
            Vertex::new(std::array::from_fn(|i| max[i] + translation[i])),
        ))
    }
}
pub fn scene_tight_aabb(scene: &SceneSpatial) -> Option<Aabb> {
    collect_bounds(scene, true)
}
pub fn scene_aabb(scene: &SceneSpatial) -> Option<Aabb> {
    collect_bounds(scene, false)
}
fn collect_bounds(scene: &SceneSpatial, tight: bool) -> Option<Aabb> {
    let mut result = None;
    for id in scene.leaf_ids() {
        if let Some(query) = scene.leaf(id) {
            let bounds = if tight {
                query.transformed_aabb(query.world_transform())
            } else {
                query.local_aabb().map(|b| match query.world_transform() {
                    Transform3D::Identity => b,
                    Transform3D::Affine(a) => b.transformed_arvo(&a),
                })
            };
            if let Some(bounds) = bounds {
                match &mut result {
                    Some(total) => Aabb::union(total, &bounds),
                    None => result = Some(bounds),
                }
            }
        }
    }
    result
}
#[cfg(test)]
mod tests_spatial;

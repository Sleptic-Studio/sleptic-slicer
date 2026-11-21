// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Stable identities and shared geometry for editable resin-print scenes.
pub mod aabb;
pub mod bake;
pub mod error;
pub mod expand;
pub mod mesh_health;
pub mod mesh_targets;
pub mod node;
pub mod plates;
pub mod repair;
pub mod shared_vertices;
pub mod traverse;
pub mod validate;
pub use bake::{bake_leaf_to_world, bake_mesh_to_world};
pub use error::SceneError;
pub use expand::MAX_EXPANDED_LEAF_COUNT;
use node::LeafAttachment;
pub use node::{
    CsgOp, LeafPayload, MeshResourceId, Node, NodeId, NodeKind, RemovedLeaf, SceneMetadata,
    SubtreeResourceId,
};
pub use plates::{PLATE_GAP_MM, plate_grid_offset};
use sc_mesh_core::{IndexedMesh, transform::Transform3D};
use std::{collections::HashSet, num::NonZeroU32, sync::Arc};
pub use traverse::{CsgPolicy, ReferencePolicy};
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Scene {
    nodes: Vec<Option<Node>>,
    parents: Vec<Option<NodeId>>,
    roots: Vec<NodeId>,
    meshes: Vec<Option<Arc<IndexedMesh>>>,
    subtrees: Vec<NodeId>,
    meta: Option<SceneMetadata>,
    plates: plates::Plates,
}
fn number(index: usize, message: &str) -> NonZeroU32 {
    match u32::try_from(index)
        .ok()
        .and_then(|n| n.checked_add(1))
        .and_then(NonZeroU32::new)
    {
        Some(n) => n,
        None => panic!("{message}"),
    }
}
impl Scene {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn nodes_len(&self) -> usize {
        self.nodes.len()
    }
    pub fn mesh_resources_len(&self) -> usize {
        self.meshes.len()
    }
    pub fn meta(&self) -> &Option<SceneMetadata> {
        &self.meta
    }
    pub fn set_meta(&mut self, m: Option<SceneMetadata>) {
        self.meta = m;
    }
    pub fn roots(&self) -> &[NodeId] {
        &self.roots
    }
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.index())?.as_ref()
    }
    fn node_mut(&mut self, id: NodeId) -> Result<&mut Node, SceneError> {
        self.nodes
            .get_mut(id.index())
            .and_then(Option::as_mut)
            .ok_or(SceneError::DanglingNodeId { id })
    }
    pub fn parent_of(&self, id: NodeId) -> Option<NodeId> {
        self.parents.get(id.index()).copied().flatten()
    }
    pub fn mesh_resource_arc(&self, id: MeshResourceId) -> Option<&Arc<IndexedMesh>> {
        self.meshes.get(id.index())?.as_ref()
    }
    pub fn mesh_resource(&self, id: MeshResourceId) -> Option<&IndexedMesh> {
        self.mesh_resource_arc(id).map(Arc::as_ref)
    }
    pub fn leaf_mesh_arc(&self, id: NodeId) -> Option<&Arc<IndexedMesh>> {
        match &self.node(id)?.kind {
            NodeKind::Leaf(LeafPayload::Mesh(m)) => Some(m),
            NodeKind::Leaf(LeafPayload::MeshResource(r)) => self.mesh_resource_arc(*r),
            _ => None,
        }
    }
    pub fn subtree_resource_root(&self, id: SubtreeResourceId) -> Option<NodeId> {
        self.subtrees.get(id.get() as usize - 1).copied()
    }
    pub fn has_subtree_resources(&self) -> bool {
        !self.subtrees.is_empty()
    }
    pub fn first_reference_node(&self) -> Option<NodeId> {
        self.live()
            .find_map(|(id, n)| matches!(n.kind, NodeKind::Reference { .. }).then_some(id))
    }
    fn live(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.nodes.iter().enumerate().filter_map(|(i, n)| {
            n.as_ref()
                .map(|n| (NodeId(number(i, "node id overflow")), n))
        })
    }
    pub fn mesh_resources_iter(&self) -> impl Iterator<Item = (MeshResourceId, &IndexedMesh)> {
        self.meshes.iter().enumerate().filter_map(|(i, m)| {
            m.as_ref().map(|m| {
                (
                    MeshResourceId(number(i, "mesh resource id overflow")),
                    m.as_ref(),
                )
            })
        })
    }
    pub fn find_mesh_resource_by_arc(&self, m: &Arc<IndexedMesh>) -> Option<MeshResourceId> {
        self.meshes
            .iter()
            .position(|v| v.as_ref().is_some_and(|v| Arc::ptr_eq(v, m)))
            .map(|i| MeshResourceId(number(i, "mesh resource id overflow")))
    }
    pub fn set_node_meta(
        &mut self,
        id: NodeId,
        m: Option<SceneMetadata>,
    ) -> Result<(), SceneError> {
        self.node_mut(id)?.meta = m;
        Ok(())
    }
    pub fn set_local_transform(&mut self, id: NodeId, t: Transform3D) -> Result<(), SceneError> {
        self.node_mut(id)?.local_transform = t;
        Ok(())
    }
    pub fn allocate_unbound_node_id(&mut self) -> NodeId {
        let id = NodeId(number(self.nodes.len(), "node id overflow"));
        self.nodes.push(None);
        self.parents.push(None);
        id
    }
    pub fn allocate_unbound_mesh_resource_id(&mut self) -> MeshResourceId {
        let id = MeshResourceId(number(self.meshes.len(), "mesh resource id overflow"));
        self.meshes.push(None);
        id
    }
    fn insert(
        &mut self,
        meta: Option<SceneMetadata>,
        local_transform: Transform3D,
        kind: NodeKind,
    ) -> NodeId {
        let id = self.allocate_unbound_node_id();
        self.nodes[id.index()] = Some(Node {
            meta,
            local_transform,
            kind,
        });
        id
    }
    pub fn add_group(&mut self, m: Option<SceneMetadata>, t: Transform3D) -> NodeId {
        self.insert(m, t, NodeKind::Group { children: vec![] })
    }
    fn check_payload(&self, p: &LeafPayload) -> Result<(), SceneError> {
        if let LeafPayload::MeshResource(id) = p
            && self.mesh_resource(*id).is_none()
        {
            return Err(SceneError::DanglingMeshResource { id: *id });
        }
        Ok(())
    }
    pub fn add_leaf(
        &mut self,
        m: Option<SceneMetadata>,
        t: Transform3D,
        p: LeafPayload,
    ) -> Result<NodeId, SceneError> {
        self.check_payload(&p)?;
        Ok(self.insert(m, t, NodeKind::Leaf(p)))
    }
    pub fn add_mesh_resource(&mut self, m: IndexedMesh) -> MeshResourceId {
        self.add_mesh_resource_arc(Arc::new(m))
    }
    pub fn add_mesh_resource_arc(&mut self, m: Arc<IndexedMesh>) -> MeshResourceId {
        let id = self.allocate_unbound_mesh_resource_id();
        self.meshes[id.index()] = Some(m);
        id
    }
    fn unattached(&self, id: NodeId) -> Result<(), SceneError> {
        self.node(id).ok_or(SceneError::DanglingNodeId { id })?;
        if let Some(parent) = self.parent_of(id) {
            return Err(SceneError::AlreadyHasParent {
                node_id: id,
                parent,
            });
        }
        if self.roots.contains(&id) {
            return Err(SceneError::AlreadyARoot { node_id: id });
        }
        if self.subtrees.contains(&id) {
            return Err(SceneError::AlreadyASubtreeResourceRoot { node_id: id });
        }
        Ok(())
    }
    pub fn add_root(&mut self, id: NodeId) -> Result<(), SceneError> {
        self.unattached(id)?;
        self.roots.push(id);
        Ok(())
    }
    pub fn register_subtree_resource(
        &mut self,
        id: NodeId,
    ) -> Result<SubtreeResourceId, SceneError> {
        self.unattached(id)?;
        let r = SubtreeResourceId(number(self.subtrees.len(), "subtree resource id overflow"));
        self.subtrees.push(id);
        Ok(r)
    }
    pub fn add_reference(
        &mut self,
        m: Option<SceneMetadata>,
        t: Transform3D,
        r: SubtreeResourceId,
    ) -> Result<NodeId, SceneError> {
        if self.subtree_resource_root(r).is_none() {
            return Err(SceneError::DanglingSubtreeResource { id: r });
        }
        Ok(self.insert(m, t, NodeKind::Reference { resource: r }))
    }
    pub fn add_csg(
        &mut self,
        m: Option<SceneMetadata>,
        t: Transform3D,
        op: CsgOp,
        children: Vec<NodeId>,
    ) -> Result<NodeId, SceneError> {
        let id = NodeId(number(self.nodes.len(), "node id overflow"));
        if op == CsgOp::Intersection && children.is_empty() {
            return Err(SceneError::EmptyIntersection { node_id: id });
        }
        let mut seen = HashSet::new();
        for &c in &children {
            self.node(c).ok_or(SceneError::DanglingNodeId { id: c })?;
            if !seen.insert(c) {
                return Err(SceneError::DuplicateChild {
                    parent: id,
                    child: c,
                });
            }
            self.unattached(c)?;
        }
        for &c in &children {
            self.parents[c.index()] = Some(id);
        }
        Ok(self.insert(m, t, NodeKind::Csg { op, children }))
    }
    fn edges(&self, id: NodeId) -> Result<Vec<NodeId>, SceneError> {
        let n = self.node(id).ok_or(SceneError::DanglingNodeId { id })?;
        match &n.kind {
            NodeKind::Reference { resource } => {
                Ok(vec![self.subtree_resource_root(*resource).ok_or(
                    SceneError::DanglingSubtreeResource { id: *resource },
                )?])
            }
            _ => Ok(n.children().cloned().unwrap_or_default()),
        }
    }
    pub fn add_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), SceneError> {
        self.node(parent)
            .ok_or(SceneError::DanglingNodeId { id: parent })?;
        self.node(child)
            .ok_or(SceneError::DanglingNodeId { id: child })?;
        if self.node(parent).and_then(Node::children).is_none() {
            return Err(SceneError::NotABranch { node_id: parent });
        }
        self.unattached(child)?;
        let mut stack = vec![child];
        let mut seen = HashSet::new();
        while let Some(id) = stack.pop() {
            if id == parent {
                return Err(SceneError::ExpansionCycle { node_id: parent });
            }
            if seen.insert(id) {
                stack.extend(self.edges(id)?);
            }
        }
        if let Some(c) = self.node_mut(parent)?.children_mut() {
            c.push(child);
        }
        self.parents[child.index()] = Some(parent);
        Ok(())
    }
    pub fn attachment_of(&self, id: NodeId) -> Option<LeafAttachment> {
        if !matches!(self.node(id)?.kind, NodeKind::Leaf(_)) {
            return None;
        }
        if let Some(parent) = self.parent_of(id) {
            let position = self
                .node(parent)?
                .children()?
                .iter()
                .position(|c| *c == id)?;
            Some(LeafAttachment::Parent { parent, position })
        } else {
            self.roots
                .iter()
                .position(|c| *c == id)
                .map(|position| LeafAttachment::Root { position })
        }
    }
    pub fn remove_leaf(&mut self, id: NodeId) -> Result<RemovedLeaf, SceneError> {
        let n = self.node(id).ok_or(SceneError::DanglingNodeId { id })?;
        if !matches!(n.kind, NodeKind::Leaf(_)) {
            return Err(SceneError::NotALeaf { node_id: id });
        }
        let attachment = self
            .attachment_of(id)
            .ok_or(SceneError::NotARoot { node_id: id })?;
        match attachment {
            LeafAttachment::Root { position } => {
                self.roots.remove(position);
            }
            LeafAttachment::Parent { parent, position } => {
                if let Some(c) = self.node_mut(parent)?.children_mut() {
                    c.remove(position);
                }
            }
        }
        let Some(n) = self.nodes[id.index()].take() else {
            return Err(SceneError::DanglingNodeId { id });
        };
        self.parents[id.index()] = None;
        let NodeKind::Leaf(payload) = n.kind else {
            return Err(SceneError::NotALeaf { node_id: id });
        };
        Ok(RemovedLeaf {
            attachment,
            payload,
            local_transform: n.local_transform,
            meta: n.meta,
        })
    }
    pub fn install_leaf_at(
        &mut self,
        id: NodeId,
        a: LeafAttachment,
        p: LeafPayload,
        t: Transform3D,
        m: Option<SceneMetadata>,
    ) -> Result<(), SceneError> {
        if id.index() >= self.nodes.len() {
            return Err(SceneError::DanglingNodeId { id });
        }
        if self.node(id).is_some() {
            return Err(SceneError::NodeSlotOccupied { id });
        }
        self.check_payload(&p)?;
        let (position, len) = match a {
            LeafAttachment::Root { position } => (position, self.roots.len()),
            LeafAttachment::Parent { parent, position } => {
                let n = self
                    .node(parent)
                    .ok_or(SceneError::DanglingNodeId { id: parent })?;
                let c = n
                    .children()
                    .ok_or(SceneError::NotABranch { node_id: parent })?;
                (position, c.len())
            }
        };
        if position > len {
            return Err(SceneError::AttachmentPositionOutOfRange { position, len });
        }
        match a {
            LeafAttachment::Root { position } => self.roots.insert(position, id),
            LeafAttachment::Parent { parent, position } => {
                if let Some(c) = self.node_mut(parent)?.children_mut() {
                    c.insert(position, id);
                }
                self.parents[id.index()] = Some(parent);
            }
        }
        self.nodes[id.index()] = Some(Node {
            meta: m,
            local_transform: t,
            kind: NodeKind::Leaf(p),
        });
        Ok(())
    }
    pub fn install_mesh_resource_at(
        &mut self,
        id: MeshResourceId,
        m: Arc<IndexedMesh>,
    ) -> Result<(), SceneError> {
        let slot = self
            .meshes
            .get_mut(id.index())
            .ok_or(SceneError::DanglingMeshResource { id })?;
        if slot.is_some() {
            return Err(SceneError::MeshResourceSlotOccupied { id });
        }
        *slot = Some(m);
        Ok(())
    }
    pub fn other_leaf_references_resource(&self, r: MeshResourceId, excluded: NodeId) -> bool {
        self.live().any(|(id, n)| {
            id != excluded && matches!(n.kind,NodeKind::Leaf(LeafPayload::MeshResource(v)) if v==r)
        })
    }
    pub fn remove_mesh_resource(
        &mut self,
        id: MeshResourceId,
    ) -> Result<Arc<IndexedMesh>, SceneError> {
        self.mesh_resource(id)
            .ok_or(SceneError::DanglingMeshResource { id })?;
        if self
            .live()
            .any(|(_, n)| matches!(n.kind,NodeKind::Leaf(LeafPayload::MeshResource(r)) if r==id))
        {
            return Err(SceneError::MeshResourceInUse { id });
        }
        self.meshes[id.index()]
            .take()
            .ok_or(SceneError::DanglingMeshResource { id })
    }
    pub fn replace_mesh_resource(
        &mut self,
        id: MeshResourceId,
        expected: &Arc<IndexedMesh>,
        replacement: Arc<IndexedMesh>,
    ) -> Result<(), SceneError> {
        let slot = self
            .meshes
            .get_mut(id.index())
            .and_then(Option::as_mut)
            .ok_or(SceneError::DanglingMeshResource { id })?;
        if !Arc::ptr_eq(slot, expected) {
            return Err(SceneError::MeshResourceMismatch { id });
        }
        *slot = replacement;
        Ok(())
    }
    pub fn replace_inline_leaf_mesh(
        &mut self,
        id: NodeId,
        expected: &Arc<IndexedMesh>,
        replacement: Arc<IndexedMesh>,
    ) -> Result<(), SceneError> {
        match &mut self.node_mut(id)?.kind {
            NodeKind::Leaf(LeafPayload::Mesh(m)) => {
                if !Arc::ptr_eq(m, expected) {
                    return Err(SceneError::InlineMeshMismatch { node_id: id });
                }
                *m = replacement;
                Ok(())
            }
            NodeKind::Leaf(_) => Err(SceneError::NotAnInlineMeshLeaf { node_id: id }),
            _ => Err(SceneError::NotALeaf { node_id: id }),
        }
    }
    pub fn mesh_at_leaf(&self, id: NodeId) -> Result<&IndexedMesh, SceneError> {
        match &self.node(id).ok_or(SceneError::DanglingNodeId { id })?.kind {
            NodeKind::Leaf(LeafPayload::Mesh(m)) => Ok(m),
            NodeKind::Leaf(LeafPayload::MeshResource(r)) => self
                .mesh_resource(*r)
                .ok_or(SceneError::DanglingMeshResource { id: *r }),
            _ => Err(SceneError::NotALeaf { node_id: id }),
        }
    }
}
impl From<IndexedMesh> for Scene {
    fn from(m: IndexedMesh) -> Self {
        let mut s = Self::new();
        let id = s.insert(
            None,
            Transform3D::Identity,
            NodeKind::Leaf(LeafPayload::Mesh(Arc::new(m))),
        );
        s.roots.push(id);
        s
    }
}

#[cfg(test)]
mod tests_scene;
#[cfg(test)]
mod tests_error_messages;

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Stored targets include detached geometry and unreferenced resources.
use crate::*;
use sc_mesh_core::IndexedMesh;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MeshTarget {
    Resource(MeshResourceId),
    InlineLeaf(NodeId),
}
impl Scene {
    pub fn mesh_targets(&self) -> Vec<MeshTarget> {
        self.mesh_resources_iter()
            .map(|(id, _)| MeshTarget::Resource(id))
            .chain(self.live().filter_map(|(id, n)| {
                matches!(n.kind, NodeKind::Leaf(LeafPayload::Mesh(_)))
                    .then_some(MeshTarget::InlineLeaf(id))
            }))
            .collect()
    }
    pub fn leaves_referencing(&self, target: MeshTarget) -> Vec<NodeId> {
        self.live()
            .filter_map(|(id, n)| match (&n.kind, target) {
                (NodeKind::Leaf(LeafPayload::Mesh(_)), MeshTarget::InlineLeaf(v)) if v == id => {
                    Some(id)
                }
                (NodeKind::Leaf(LeafPayload::MeshResource(r)), MeshTarget::Resource(v))
                    if *r == v =>
                {
                    Some(id)
                }
                _ => None,
            })
            .collect()
    }
    pub fn target_mesh(&self, t: MeshTarget) -> Option<&IndexedMesh> {
        match t {
            MeshTarget::Resource(r) => self.mesh_resource(r),
            MeshTarget::InlineLeaf(id) => self.node(id).and_then(|node| {
                if let NodeKind::Leaf(LeafPayload::Mesh(mesh)) = &node.kind {
                    Some(mesh.as_ref())
                } else {
                    None
                }
            }),
        }
    }
    pub fn target_mesh_mut(&mut self, t: MeshTarget) -> Option<&mut IndexedMesh> {
        let arc = match t {
            MeshTarget::Resource(r) => self.meshes.get_mut(r.index())?.as_mut()?,
            MeshTarget::InlineLeaf(id) => match &mut self.nodes.get_mut(id.index())?.as_mut()?.kind
            {
                NodeKind::Leaf(LeafPayload::Mesh(m)) => m,
                _ => return None,
            },
        };
        Some(Arc::make_mut(arc))
    }
}

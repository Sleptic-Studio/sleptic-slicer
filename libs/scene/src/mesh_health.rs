// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Resource health is assessed once before mapping back to leaves.
use crate::mesh_targets::MeshTarget;
use crate::*;
use sc_mesh_core::{IndexedMesh, indexed_mesh::MeshDiagnosis};
pub(crate) fn sound(m: &IndexedMesh) -> bool {
    m.validated_faces()
        .is_ok_and(|v| v.validate_edge_manifold().is_ok())
}
impl Scene {
    pub fn non_edge_manifold_leaves(&self) -> Vec<NodeId> {
        let bad: Vec<_> = self
            .meshes
            .iter()
            .map(|m| m.as_ref().is_some_and(|m| !sound(m)))
            .collect();
        self.live()
            .filter_map(|(id, n)| match &n.kind {
                NodeKind::Leaf(LeafPayload::Mesh(m)) if !sound(m) => Some(id),
                NodeKind::Leaf(LeafPayload::MeshResource(r))
                    if bad.get(r.index()).copied().unwrap_or(false) =>
                {
                    Some(id)
                }
                _ => None,
            })
            .collect()
    }

    /// Each mesh's diagnosis, by the target that reaches it, borrowing the
    /// meshes rather than copying them.
    pub fn diagnose_meshes(&self) -> Vec<(MeshTarget, MeshDiagnosis)> {
        let mut found = Vec::new();
        for target in self.mesh_targets() {
            if let Some(mesh) = self.target_mesh(target) {
                found.push((target, mesh.diagnose()));
            }
        }
        found
    }
}

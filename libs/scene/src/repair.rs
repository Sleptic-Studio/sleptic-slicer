// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Shared targets are assessed before copy-on-write; unique targets repair in place.
use crate::{LeafPayload, NodeKind, Scene, mesh_health::sound, mesh_targets::MeshTarget};
use sc_mesh_core::indexed_mesh::{MeshFix, RepairAssessment, RepairPasses, RepairRefusal, assess_repair};
use std::sync::Arc;
#[derive(Clone, Debug, PartialEq)]
pub struct SceneRepairOutcome {
    pub target: MeshTarget,
    pub fixes: Vec<MeshFix>,
    pub refusal: Option<RepairRefusal>,
    pub edge_manifold_after: bool,
}
impl Scene {
    pub fn repair_meshes(&mut self, passes: RepairPasses) -> Vec<SceneRepairOutcome> {
        let mut pending = self.mesh_targets().into_iter();
        let mut outcomes = Vec::with_capacity(pending.len());
        while let Some(target) = pending.next() {
            // A unique allocation needs no assessment to avoid a copy. Repair's
            // own policy preserves clean/refused bytes and supplies the same report.
            let arc = match target {
                MeshTarget::Resource(id) => {
                    self.meshes.get_mut(id.index()).and_then(Option::as_mut)
                }
                MeshTarget::InlineLeaf(id) => self
                    .nodes
                    .get_mut(id.index())
                    .and_then(Option::as_mut)
                    .and_then(|node| match &mut node.kind {
                        NodeKind::Leaf(LeafPayload::Mesh(arc)) => Some(arc),
                        _ => None,
                    }),
            };
            if let Some(mesh) = arc.and_then(Arc::get_mut) {
                let report = mesh.repair(passes);
                outcomes.push(SceneRepairOutcome {
                    target,
                    fixes: report.fixes,
                    refusal: report.refusal,
                    edge_manifold_after: report.edge_manifold_after,
                });
                continue;
            }
            let Some(mesh) = self.target_mesh(target) else {
                continue;
            };
            let mut outcome = SceneRepairOutcome {
                target,
                fixes: vec![],
                refusal: None,
                edge_manifold_after: false,
            };
            match assess_repair(mesh, passes) {
                RepairAssessment::Clean => outcome.edge_manifold_after = sound(mesh),
                RepairAssessment::Refused(r) => {
                    outcome.edge_manifold_after = sound(mesh);
                    outcome.refusal = Some(r);
                }
                RepairAssessment::Work => {
                    if let Some(m) = self.target_mesh_mut(target) {
                        let r = m.repair(passes);
                        outcome.fixes = r.fixes;
                        outcome.refusal = r.refusal;
                        outcome.edge_manifold_after = r.edge_manifold_after;
                    }
                }
            }
            outcomes.push(outcome);
        }
        outcomes
    }
}

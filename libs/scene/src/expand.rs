// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Occurrence walks and memoized counts over shared subtrees.
pub use crate::traverse::CsgPolicy;
use crate::*;
use sc_mesh_core::{IndexedMesh, transform::Transform3D};
use std::collections::{HashMap, HashSet};
pub const MAX_EXPANDED_LEAF_COUNT: usize = 100_000;
impl Scene {
    pub fn for_each_expanded_leaf<
        E: From<SceneError>,
        F: FnMut(NodeId, &Transform3D, &IndexedMesh) -> Result<(), E>,
    >(
        &self,
        policy: CsgPolicy,
        mut f: F,
    ) -> Result<(), E> {
        let mut active = HashSet::new();
        let mut stack: Vec<_> = self
            .roots
            .iter()
            .rev()
            .map(|&id| (id, Transform3D::Identity, false))
            .collect();
        while let Some((id, parent, exit)) = stack.pop() {
            if exit {
                active.remove(&id);
                continue;
            }
            let n = self.node(id).ok_or(SceneError::DanglingNodeId { id })?;
            if active.contains(&id) {
                return Err(SceneError::ExpansionCycle { node_id: id }.into());
            }
            let t = parent.compose(&n.local_transform);
            match &n.kind {
                NodeKind::Leaf(_) => f(id, &t, self.mesh_at_leaf(id)?)?,
                _ => {
                    let permitted =
                        policy != CsgPolicy::Refuse || !matches!(n.kind, NodeKind::Csg { .. });
                    permitted
                        .then_some(())
                        .ok_or_else(|| E::from(SceneError::UnevaluatedCsg { node_id: id }))?;
                    active.insert(id);
                    stack.push((id, t, true));
                    for child in self.edges(id)?.into_iter().rev() {
                        stack.push((child, t, false));
                    }
                }
            }
        }
        Ok(())
    }
    pub fn expanded_leaf_count(&self, policy: CsgPolicy) -> Result<usize, SceneError> {
        // Cache by reached identity: detached allocation history must not inflate this walk.
        let mut memo = HashMap::new();
        let mut active = HashSet::new();
        let mut stack: Vec<_> = self.roots.iter().rev().map(|&id| (id, false)).collect();
        while let Some((id, exit)) = stack.pop() {
            let n = self.node(id).ok_or(SceneError::DanglingNodeId { id })?;
            if memo.get(&id).is_some() {
                continue;
            }
            if exit {
                let mut count = 0usize;
                for c in self.edges(id)? {
                    count = count.saturating_add(memo.get(&c).copied().unwrap_or(0));
                }
                memo.insert(id, count);
                active.remove(&id);
                continue;
            }
            if active.contains(&id) {
                return Err(SceneError::ExpansionCycle { node_id: id });
            }
            if matches!(n.kind, NodeKind::Csg { .. }) && policy == CsgPolicy::Refuse {
                return Err(SceneError::UnevaluatedCsg { node_id: id });
            }
            if matches!(n.kind, NodeKind::Leaf(_)) {
                memo.insert(id, 1);
                continue;
            }
            active.insert(id);
            stack.push((id, true));
            for c in self.edges(id)?.into_iter().rev() {
                stack.push((c, false));
            }
        }
        Ok(self.roots.iter().fold(0usize, |sum, id| {
            sum.saturating_add(memo.get(id).copied().unwrap_or(0))
        }))
    }
}

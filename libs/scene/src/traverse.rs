// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Structural walks preserve child order and never expand references.
use crate::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsgPolicy {
    Refuse,
    Descend,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferencePolicy {
    Skip,
    Refuse,
}
impl Scene {
    pub fn for_each_reachable_node(&self, mut f: impl FnMut(NodeId, &Node)) {
        let mut stack: Vec<_> = self.roots.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            if let Some(n) = self.node(id) {
                f(id, n);
                if let Some(c) = n.children() {
                    stack.extend(c.iter().rev().copied());
                }
            }
        }
    }
    pub fn for_each_reachable_leaf<E: From<SceneError>, F: FnMut(NodeId) -> Result<(), E>>(
        &self,
        c: CsgPolicy,
        r: ReferencePolicy,
        mut f: F,
    ) -> Result<(), E> {
        let mut stack: Vec<_> = self.roots.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            let Some(n) = self.node(id) else {
                continue;
            };
            match &n.kind {
                NodeKind::Leaf(_) => f(id)?,
                NodeKind::Reference { .. } => {
                    if r == ReferencePolicy::Refuse {
                        return Err(SceneError::UnexpandedReference { node_id: id }.into());
                    }
                }
                NodeKind::Csg { .. } if c == CsgPolicy::Refuse => {
                    return Err(SceneError::UnevaluatedCsg { node_id: id }.into());
                }
                _ => {
                    if let Some(c) = n.children() {
                        stack.extend(c.iter().rev().copied());
                    }
                }
            }
        }
        Ok(())
    }
    pub fn reachable_leaves(
        &self,
        c: CsgPolicy,
        r: ReferencePolicy,
    ) -> Result<Vec<NodeId>, SceneError> {
        let mut v = vec![];
        self.for_each_reachable_leaf(c, r, |id| {
            v.push(id);
            Ok::<_, SceneError>(())
        })?;
        Ok(v)
    }
    pub fn reachable_leaf_count(&self) -> usize {
        let mut count = 0;
        self.for_each_reachable_node(|_, n| {
            if matches!(n.kind, NodeKind::Leaf(_)) {
                count += 1;
            }
        });
        count
    }
    pub fn first_leaf_id(&self) -> Option<NodeId> {
        let mut first = None;
        let _: Result<(), SceneError> =
            self.for_each_reachable_leaf(CsgPolicy::Descend, ReferencePolicy::Skip, |id| {
                first = Some(id);
                Err(SceneError::NotALeaf { node_id: id })
            });
        first
    }
    pub fn world_transform_of(&self, id: NodeId) -> Result<Transform3D, SceneError> {
        let mut chain = vec![];
        let mut cursor = Some(id);
        while let Some(id) = cursor {
            chain.push(
                self.node(id)
                    .ok_or(SceneError::DanglingNodeId { id })?
                    .local_transform,
            );
            cursor = self.parent_of(id);
        }
        let mut t = Transform3D::Identity;
        for v in chain.into_iter().rev() {
            t = t.compose(&v);
        }
        Ok(t)
    }
}

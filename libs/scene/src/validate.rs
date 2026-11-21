// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Structural validation follows deterministic identity and edge order.
use crate::*;
use std::collections::HashSet;
impl Scene {
    pub fn validate(&self) -> Result<(), SceneError> {
        for (id, n) in self.live() {
            match &n.kind {
                NodeKind::Csg {
                    op: CsgOp::Intersection,
                    children,
                } if children.is_empty() => {
                    return Err(SceneError::EmptyIntersection { node_id: id });
                }
                NodeKind::Leaf(p) => self.check_payload(p)?,
                NodeKind::Reference { resource } => {
                    self.subtree_resource_root(*resource)
                        .ok_or(SceneError::DanglingSubtreeResource { id: *resource })?;
                }
                _ => {}
            }
            if let Some(children) = n.children() {
                for &id in children {
                    self.node(id).ok_or(SceneError::DanglingNodeId { id })?;
                }
            }
        }
        for &id in &self.subtrees {
            self.node(id).ok_or(SceneError::DanglingNodeId { id })?;
        }
        for &id in &self.roots {
            self.node(id).ok_or(SceneError::DanglingNodeId { id })?;
            if self.subtrees.contains(&id) {
                return Err(SceneError::AlreadyASubtreeResourceRoot { node_id: id });
            }
        }
        let mut parents = vec![None; self.nodes.len()];
        for (parent, n) in self.live() {
            let mut seen = HashSet::new();
            if let Some(children) = n.children() {
                for &child in children {
                    if !seen.insert(child) {
                        return Err(SceneError::DuplicateChild { parent, child });
                    }
                    if parents[child.index()].is_some() {
                        return Err(SceneError::ParentMismatch {
                            node_id: child,
                            recorded: self.parent_of(child),
                        });
                    }
                    parents[child.index()] = Some(parent);
                }
            }
        }
        for (id, _) in self.live() {
            if parents[id.index()] != self.parent_of(id) {
                return Err(SceneError::ParentMismatch {
                    node_id: id,
                    recorded: self.parent_of(id),
                });
            }
        }
        let mut colors = vec![0u8; self.nodes.len()];
        for (start, _) in self.live() {
            let mut stack = vec![(start, false)];
            while let Some((id, exit)) = stack.pop() {
                if exit {
                    colors[id.index()] = 2;
                    continue;
                }
                match colors[id.index()] {
                    2 => continue,
                    1 => return Err(SceneError::ExpansionCycle { node_id: id }),
                    _ => {}
                }
                colors[id.index()] = 1;
                stack.push((id, true));
                for c in self.edges(id)?.into_iter().rev() {
                    stack.push((c, false));
                }
            }
        }
        Ok(())
    }
}

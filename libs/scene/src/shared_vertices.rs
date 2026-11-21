// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Exact local coordinates, with signed zero canonicalized for matching.
use crate::*;
use std::collections::{BTreeSet, HashMap};
impl Scene {
    pub fn detect_shared_vertices(&self) -> Vec<NodeId> {
        let leaves: Vec<_> = self
            .live()
            .filter_map(|(id, _)| {
                self.leaf_mesh_arc(id)
                    .filter(|m| !m.vertices.is_empty())
                    .map(|m| (id, m))
            })
            .collect();
        if leaves.len() < 2 {
            return vec![];
        }
        let mut first = HashMap::new();
        let mut shared = BTreeSet::new();
        for (id, m) in leaves {
            for v in &m.vertices {
                let key = v.coords().map(|v| if v == 0. { 0 } else { v.to_bits() });
                if let Some(&other) = first.get(&key) {
                    if other != id {
                        shared.insert(id);
                        shared.insert(other);
                    }
                } else {
                    first.insert(key, id);
                }
            }
        }
        shared.into_iter().collect()
    }
}

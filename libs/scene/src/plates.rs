// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Opaque project settings and presentation-only plate placement.
use crate::*;
use std::collections::{BTreeMap, HashSet};
pub type PlateId = u32;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plate {
    pub id: PlateId,
    pub locked: bool,
    pub settings: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plates {
    pub order: Vec<Plate>,
    pub assignments: BTreeMap<NodeId, PlateId>,
    next: u32,
}
impl Default for Plates {
    fn default() -> Self {
        Self {
            order: vec![Plate {
                id: 1,
                locked: false,
                settings: None,
            }],
            assignments: BTreeMap::new(),
            next: 2,
        }
    }
}
impl Plates {
    pub fn from_parts(order: Vec<Plate>, assignments: BTreeMap<NodeId, PlateId>) -> Option<Self> {
        let next = order.iter().map(|p| p.id).max()?.checked_add(1)?;
        let p = Self {
            order,
            assignments,
            next,
        };
        p.is_valid().then_some(p)
    }
    pub fn is_valid(&self) -> bool {
        let mut seen = HashSet::new();
        !self.order.is_empty()
            && self
                .order
                .iter()
                .all(|p| p.id > 0 && p.id < self.next && seen.insert(p.id))
            && self.assignments.values().all(|p| seen.contains(p))
    }
    pub fn add(&mut self) -> PlateId {
        let id = self.next;
        self.next = match id.checked_add(1) {
            Some(n) => n,
            None => panic!("plate identity overflow"),
        };
        self.order.push(Plate {
            id,
            locked: false,
            settings: self.order.last().and_then(|p| p.settings.clone()),
        });
        id
    }
}
pub const PLATE_GAP_MM: f32 = 40.0;
pub fn plate_grid_offset(index: usize, count: usize, width: f32, depth: f32) -> [f32; 2] {
    let columns = (count.max(1) as f64).sqrt().ceil() as usize;
    [
        (index % columns) as f32 * (width + PLATE_GAP_MM),
        -((index / columns) as f32) * (depth + PLATE_GAP_MM),
    ]
}
impl Scene {
    pub fn plates(&self) -> &Plates {
        &self.plates
    }
    pub fn set_plates(&mut self, p: Plates) -> bool {
        if !p.is_valid() {
            return false;
        }
        self.plates = p;
        true
    }
    pub fn plate_of(&self, id: NodeId) -> PlateId {
        let mut owner = None;
        let comment = self
            .node(id)
            .and_then(|n| n.meta.as_ref())
            .and_then(|m| m.comment.as_deref());
        if let Some(c) = comment {
            if c.starts_with("sc-generated-support") {
                let parsed = c
                    .lines()
                    .filter_map(|l| {
                        l.strip_prefix("owner = ")
                            .and_then(|v| v.parse::<u32>().ok())
                    })
                    .next();
                owner = parsed
                    .and_then(NonZeroU32::new)
                    .map(NodeId)
                    .filter(|id| self.node(*id).is_some());
            }
            if owner.is_none() && c == "sc-generated-support-raft" {
                let mut models = self
                    .live()
                    .filter(|(_, n)| {
                        matches!(n.kind, NodeKind::Leaf(_))
                            && !n
                                .meta
                                .as_ref()
                                .and_then(|m| m.comment.as_deref())
                                .is_some_and(|c| c.starts_with("sc-generated-support"))
                    })
                    .map(|(id, _)| id);
                let first = models.next();
                if models.next().is_none() {
                    owner = first;
                }
            }
        }
        self.plates
            .assignments
            .get(&owner.unwrap_or(id))
            .copied()
            .unwrap_or_else(|| {
                if self.plates.order.iter().any(|p| p.id == 1) {
                    1
                } else {
                    self.plates.order.first().map_or(1, |p| p.id)
                }
            })
    }
    pub fn plate_scene(&self, id: PlateId) -> Result<Self, SceneError> {
        let leaves = self.reachable_leaves(CsgPolicy::Refuse, ReferencePolicy::Skip)?;
        let mut s = self.clone();
        for leaf in leaves {
            if self.plate_of(leaf) != id {
                s.remove_leaf(leaf)?;
            }
        }
        Ok(s)
    }
    pub fn plate_offset(&self, id: PlateId, width: f32, depth: f32) -> [f32; 2] {
        plate_grid_offset(
            self.plates
                .order
                .iter()
                .position(|p| p.id == id)
                .unwrap_or(0),
            self.plates.order.len(),
            width,
            depth,
        )
    }
}

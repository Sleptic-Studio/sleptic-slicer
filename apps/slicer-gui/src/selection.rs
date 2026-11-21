// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Ordered object identities; the final member is the active tool target.

use sc_scene::NodeId;
use std::cell::RefCell;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) struct LeafId(pub(crate) NodeId);

impl LeafId {
    pub(crate) fn node_id(self) -> NodeId {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Selection {
    None,
    One(LeafId),
    Many(Vec<LeafId>),
}

impl Selection {
    fn members(&self) -> &[LeafId] {
        match self {
            Self::None => &[],
            Self::One(member) => std::slice::from_ref(member),
            Self::Many(members) => members,
        }
    }

    pub(crate) fn ids(&self) -> Vec<NodeId> {
        self.members()
            .iter()
            .copied()
            .map(LeafId::node_id)
            .collect()
    }

    pub(crate) fn primary(&self) -> Option<LeafId> {
        self.members().last().copied()
    }

    pub(crate) fn from_ids(ids: Vec<NodeId>) -> Self {
        match ids.as_slice() {
            [] => Self::None,
            [only] => Self::One(LeafId(*only)),
            _ => Self::Many(ids.into_iter().map(LeafId).collect()),
        }
    }
}

pub(crate) struct SelectionState(RefCell<Selection>);

impl SelectionState {
    pub(crate) fn new() -> Self {
        Self(RefCell::new(Selection::None))
    }

    pub(crate) fn current(&self) -> Selection {
        self.0.borrow().clone()
    }

    pub(crate) fn set(&self, selection: Selection) {
        self.0.replace(selection);
    }

    pub(crate) fn main_node(&self) -> Option<NodeId> {
        self.0.borrow().primary().map(LeafId::node_id)
    }
}

#[cfg(test)]
#[path = "tests_selection.rs"]
mod tests_selection;

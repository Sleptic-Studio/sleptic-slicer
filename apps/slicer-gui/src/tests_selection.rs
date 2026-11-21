// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

use super::{LeafId, Selection, SelectionState};
use sc_scene::{NodeId, Scene};

fn identities(count: usize) -> Vec<NodeId> {
    let mut scene = Scene::new();
    (0..count)
        .map(|_| scene.allocate_unbound_node_id())
        .collect()
}

#[test]
fn cardinality_order_and_duplicates_survive_conversion() {
    let ids = identities(4);
    assert_eq!(Selection::from_ids(vec![]), Selection::None);
    assert_eq!(
        Selection::from_ids(vec![ids[1]]),
        Selection::One(LeafId(ids[1]))
    );
    let ordered = vec![ids[2], ids[0], ids[2], ids[3]];
    let selection = Selection::from_ids(ordered.clone());
    assert!(matches!(&selection, Selection::Many(members) if members.len() == 4));
    assert_eq!(selection.ids(), ordered);
    assert_eq!(selection.primary().map(LeafId::node_id), Some(ids[3]));
    assert_eq!(
        Selection::from_ids(vec![ids[1], ids[1]]).ids(),
        vec![ids[1], ids[1]]
    );
}

#[test]
fn queries_accept_noncanonical_many_and_return_owned_ids() {
    let id = identities(1)[0];
    let empty = Selection::Many(vec![]);
    assert!(empty.ids().is_empty());
    assert_eq!(empty.primary(), None);
    let one = Selection::Many(vec![LeafId(id)]);
    assert_eq!(one.ids(), vec![id]);
    assert_eq!(one.primary(), Some(LeafId(id)));
    let mut detached = one.ids();
    detached.clear();
    assert_eq!(one.ids(), vec![id]);
    assert_eq!(Selection::None.primary(), None);
}

#[test]
fn stored_snapshots_are_independent_and_primary_tracks_replacements() {
    let ids = identities(3);
    let state = SelectionState::new();
    assert_eq!(state.current(), Selection::None);
    assert_eq!(state.main_node(), None);
    state.set(Selection::from_ids(ids.clone()));
    let mut snapshot = state.current();
    if let Selection::Many(members) = &mut snapshot {
        members.clear();
    }
    assert_eq!(state.current().ids(), ids);
    assert_eq!(state.main_node(), Some(ids[2]));
    state.set(Selection::One(LeafId(ids[0])));
    assert_eq!(state.main_node(), Some(ids[0]));
    state.set(Selection::Many(vec![]));
    assert_eq!(state.current(), Selection::Many(vec![]));
    assert_eq!(state.main_node(), None);
    state.set(Selection::None);
    assert_eq!(state.current(), Selection::None);
}

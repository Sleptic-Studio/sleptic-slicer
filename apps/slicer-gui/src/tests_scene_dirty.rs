// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Saved-state comparisons across history and asynchronous writes.
use crate::scene_document::{SaveCheck, SceneDocument, SceneEdit, DocumentEvent, UNDO_DEPTH};
use sc_mesh_core::{
    IndexedMesh,
    transform::{AffineTransform3D, Transform3D},
};
use sc_scene::{LeafPayload, NodeId, Scene};
use std::{cell::RefCell, path::PathBuf, rc::Rc, sync::Arc};
fn loaded() -> (Rc<SceneDocument>, NodeId) {
    let document = SceneDocument::new();
    let mut graph = Scene::new();
    let id = graph
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(IndexedMesh::new(None, vec![], vec![]))),
        )
        .unwrap();
    graph.add_root(id).unwrap();
    document.take_mesh_scene(graph, PathBuf::from("dirty.obj"));
    (document, id)
}
fn change(document: &SceneDocument, id: NodeId, x: f64) {
    let before = document
        .read_scene(|s| s.graph().node(id).unwrap().local_transform)
        .unwrap();
    let edit = SceneEdit::SetLocalTransform {
        node_id: id,
        before,
        after: Transform3D::Affine(AffineTransform3D::from_translation([x, 0.0, 0.0])),
    };
    document.commit_edit(edit.clone()).unwrap();
    document.record_edit(edit);
}
#[test]
fn dirty_follows_the_saved_head_in_both_directions() {
    for save_after in 0..=3 {
        let (document, id) = loaded();
        for step in 1..=3 {
            if step - 1 == save_after {
                document.note_saved();
            }
            change(&document, id, step as f64);
        }
        if save_after == 3 {
            document.note_saved();
        }
        for position in (0..=3).rev() {
            assert_eq!(document.has_unsaved_changes(), position != save_after);
            if position > 0 {
                assert!(document.step_back().unwrap());
            }
        }
        for position in 1..=3 {
            assert!(document.step_forward().unwrap());
            assert_eq!(document.has_unsaved_changes(), position != save_after);
        }
    }
}
#[test]
fn forgetting_applied_history_never_claims_the_loaded_content_was_restored() {
    let (document, id) = loaded();
    for step in 1..=UNDO_DEPTH + 1 {
        change(&document, id, step as f64);
    }
    for _ in 0..UNDO_DEPTH {
        assert!(document.step_back().unwrap());
    }
    assert!(!document.step_back().unwrap());
    assert!(document.has_unsaved_changes());
    document.note_saved();
    assert!(!document.has_unsaved_changes());
    change(&document, id, 20.0);
    assert!(document.has_unsaved_changes());
    document.step_back().unwrap();
    assert!(!document.has_unsaved_changes());
}
#[test]
fn a_nonempty_saved_head_remains_recognizable_after_eviction() {
    let (document, id) = loaded();
    for step in 1..=UNDO_DEPTH + 1 {
        change(&document, id, step as f64);
    }
    document.note_saved();
    document.step_back().unwrap();
    assert!(document.has_unsaved_changes());
    document.step_forward().unwrap();
    assert!(!document.has_unsaved_changes());
}
#[test]
fn project_choices_are_independent_of_geometry_history() {
    let (document, id) = loaded();
    assert!(document.fill_default_printer(Some("test-printer".parse().unwrap())));
    assert!(document.fill_default_resin(Some("test-resin".parse().unwrap())));
    assert!(!document.has_unsaved_changes());
    assert!(!document.fill_default_printer(Some("unused".parse().unwrap())));
    assert!(document.choose_printer(Some("changed".parse().unwrap())));
    change(&document, id, 5.0);
    document.step_back().unwrap();
    assert!(document.has_unsaved_changes());
    assert!(!document.opened_from_project());
    document.note_saved();
    assert!(!document.choose_printer(Some("changed".parse().unwrap())));
    assert!(!document.has_unsaved_changes());
}
#[test]
fn choice_setters_report_only_changes() {
    for field in 0..3 {
        let (document, _) = loaded();
        let set = || match field {
            0 => document.choose_printer(Some("printer-a".parse().unwrap())),
            1 => document.choose_resin(Some("resin-a".parse().unwrap())),
            _ => document.choose_first_layer(Some(
                sc_print_primitives::PositiveFiniteMm::new(0.08).unwrap(),
            )),
        };
        assert!(set());
        assert!(document.has_unsaved_changes());
        document.note_saved();
        assert!(!set());
        assert!(!document.has_unsaved_changes());
        assert!(!document.fill_default_printer(None));
        assert!(!document.fill_default_resin(None));
    }
}
#[test]
fn dirty_notifications_count_transitions_and_follow_history_notifications() {
    let (document, id) = loaded();
    let events = Rc::new(RefCell::new(vec![]));
    let sink = events.clone();
    let _subscription = document.watch(move |event| sink.borrow_mut().push(event));
    change(&document, id, 1.0);
    change(&document, id, 2.0);
    document.note_saved();
    document.note_saved();
    document.step_back().unwrap();
    document.step_forward().unwrap();
    let events = events.borrow();
    assert_eq!(
        events
            .iter()
            .filter(|&&e| e == DocumentEvent::UnsavedChanged)
            .count(),
        4
    );
    assert_eq!(
        &events[..3],
        [
            DocumentEvent::Edited,
            DocumentEvent::HistoryChanged,
            DocumentEvent::UnsavedChanged
        ]
    );
}
#[test]
fn resetting_a_document_retires_unsaved_state_and_eviction() {
    let (document, id) = loaded();
    for step in 1..=UNDO_DEPTH + 1 {
        change(&document, id, step as f64);
    }
    document.choose_resin(Some("resin-a".parse().unwrap()));
    document.take_mesh_scene(Scene::new(), PathBuf::from("empty.obj"));
    assert!(!document.has_unsaved_changes());
    assert!(document.chosen_resin().is_none());
    assert!(!document.has_past());
    document.choose_printer(Some("printer-a".parse().unwrap()));
    document.empty();
    assert!(!document.has_unsaved_changes());
    assert!(document.chosen_printer().is_none());
}
#[test]
fn completion_of_a_write_distinguishes_unchanged_drifted_and_replaced_documents() {
    for intervening in 0..4 {
        let (document, id) = loaded();
        change(&document, id, 1.0);
        let stamp = document.stamp_for_save();
        let payload = document.project_payload(None, None);
        match intervening {
            1 => change(&document, id, 2.0),
            2 => {
                document.choose_printer(Some("later".parse().unwrap()));
            }
            3 => document.take_mesh_scene(Scene::new(), PathBuf::from("replacement.obj")),
            _ => {}
        }
        let target = PathBuf::from("saved.3mf");
        let result = document.note_project_saved(payload, target.clone(), stamp);
        assert_eq!(
            result,
            match intervening {
                0 => SaveCheck::Current,
                3 => SaveCheck::Superseded,
                _ => SaveCheck::ChangedSince,
            }
        );
        assert_eq!(document.has_unsaved_changes(), matches!(intervening, 1 | 2));
        assert_eq!(document.opened_from_project(), intervening != 3);
        assert_eq!(
            document.file_path(),
            Some(if intervening == 3 {
                PathBuf::from("replacement.obj")
            } else {
                target
            })
        );
        if intervening == 2 {
            assert_eq!(document.chosen_printer(), Some("later".parse().unwrap()));
        }
    }
}
#[test]
fn payload_defaults_and_embedded_documents_roundtrip() {
    use sc_project_formats::{EmbeddedProfile, ProjectPayload};
    let (document, _) = loaded();
    let embedded = vec![EmbeddedProfile { profile: sc_project_formats::NamedProfile::Printer("local-printer".parse().unwrap()), extension: false, bytes: b"opaque profile".to_vec() }];
    let payload = ProjectPayload {
        resin: Some("local-resin".parse().unwrap()),
        first_layer_height: Some(sc_print_primitives::PositiveFiniteMm::new(0.1).unwrap()),
        embedded: embedded.clone(),
        ..Default::default()
    };
    document.take_project_scene(Scene::new(), PathBuf::from("input.3mf"), payload);
    let composed = document.project_payload(
        Some("fallback".parse().unwrap()),
        Some("ignored".parse().unwrap()),
    );
    assert_eq!(composed.printer, Some("fallback".parse().unwrap()));
    assert_eq!(composed.resin, Some("local-resin".parse().unwrap()));
    assert_eq!(composed.embedded, embedded);
    assert_eq!(document.chosen_resin(), Some("local-resin".parse().unwrap()));
    assert_eq!(composed.first_layer_height, document.chosen_first_layer());
    assert!(document.opened_from_project());
    assert!(!document.has_unsaved_changes());
    document.empty();
    assert_eq!(document.chosen_resin(), None);
    assert_eq!(document.chosen_first_layer(), None);
    assert!(
        document
            .project_payload(None, None)
            .embedded
            .is_empty()
    );
}

#[test]
fn fresh_choices_and_noop_saves_remain_clean() {
    let document = SceneDocument::new();
    assert!(!document.has_unsaved_changes());
    assert!(!document.choose_printer(None));
    assert!(!document.choose_first_layer(None));
    assert_eq!(document.chosen_printer(), None);
    assert_eq!(document.chosen_first_layer(), None);
    document.note_saved();
    assert!(!document.has_unsaved_changes());
    let (document, id) = loaded();
    document.record_edit(SceneEdit::SetLocalTransform {
        node_id: id, before: Transform3D::Identity, after: Transform3D::Identity,
    });
    assert!(!document.has_unsaved_changes());
    document.note_saved();
    assert!(!document.has_unsaved_changes());
}

#[test]
fn choice_values_and_dirty_events_survive_until_saved() {
    let (document, _) = loaded();
    let events = Rc::new(RefCell::new(vec![]));
    let sink = events.clone();
    let _subscription = document.watch(move |event| sink.borrow_mut().push(event));
    let printer = Some("printer-a".parse().unwrap());
    let resin = Some("resin-a".parse().unwrap());
    let height = Some(sc_print_primitives::PositiveFiniteMm::new(0.08).unwrap());
    assert!(document.choose_printer(printer.clone()));
    assert!(document.choose_resin(resin.clone()));
    assert!(document.choose_first_layer(height));
    assert_eq!(document.chosen_printer(), printer);
    assert_eq!(document.chosen_resin(), resin);
    assert_eq!(document.chosen_first_layer(), height);
    assert!(document.has_unsaved_changes());
    assert_eq!(events.borrow().iter().filter(|&&e| e == DocumentEvent::UnsavedChanged).count(), 1);
    document.note_saved();
    assert!(!document.has_unsaved_changes());
    assert!(!document.choose_printer(printer));
    assert!(!document.choose_first_layer(height));
    assert_eq!(events.borrow().iter().filter(|&&e| e == DocumentEvent::UnsavedChanged).count(), 2);
}

#[test]
fn clearing_dirty_geometry_notifies_and_resets() {
    let (document, id) = loaded();
    change(&document, id, 2.0);
    let events = Rc::new(RefCell::new(vec![]));
    let sink = events.clone();
    let _subscription = document.watch(move |event| sink.borrow_mut().push(event));
    document.empty();
    assert!(!document.has_unsaved_changes());
    assert!(events.borrow().contains(&DocumentEvent::UnsavedChanged));
}

#[test]
fn defaults_and_replacement_reset_choices_and_saved_history() {
    let (document, id) = loaded();
    assert!(document.fill_default_printer(Some("first".parse().unwrap())));
    assert!(document.fill_default_resin(Some("resin".parse().unwrap())));
    assert!(!document.has_unsaved_changes());
    document.choose_printer(Some("chosen".parse().unwrap()));
    assert!(!document.fill_default_printer(Some("other".parse().unwrap())));
    assert_eq!(document.chosen_printer(), Some("chosen".parse().unwrap()));
    assert!(document.has_unsaved_changes());
    for step in 1..=UNDO_DEPTH + 1 { change(&document, id, step as f64); }
    let mut graph = Scene::new();
    let new_id = graph.add_leaf(None, Transform3D::Identity, LeafPayload::Mesh(Arc::new(IndexedMesh::new(None, vec![], vec![])))).unwrap();
    graph.add_root(new_id).unwrap();
    document.take_mesh_scene(graph, PathBuf::from("replacement.obj"));
    assert!(!document.has_unsaved_changes());
    assert_eq!(document.chosen_printer(), None);
    change(&document, new_id, 1.0);
    document.step_back().unwrap();
    assert!(!document.has_unsaved_changes());
}

#[test]
fn project_identity_survives_choice_changes() {
    use sc_project_formats::ProjectPayload;
    let (document, _) = loaded();
    document.choose_printer(Some("chosen".parse().unwrap()));
    assert!(!document.opened_from_project());
    let payload = document.project_payload(None, None);
    assert_eq!(payload, ProjectPayload { printer: Some("chosen".parse().unwrap()), ..Default::default() });
    document.take_project_scene(Scene::new(), PathBuf::from("project.3mf"), ProjectPayload::default());
    document.choose_printer(Some("another".parse().unwrap()));
    assert!(document.opened_from_project());
}

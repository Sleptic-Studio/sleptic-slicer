// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Observable document transactions and session state.

use super::*;
use sc_mesh_fixtures::unit_cube_mesh;

fn sample(count: usize) -> Scene {
    let mut graph = Scene::new();
    for index in 0..count {
        let id = graph
            .add_leaf(
                None,
                shifted(index as f64),
                LeafPayload::Mesh(Arc::new(unit_cube_mesh())),
            )
            .unwrap();
        graph.add_root(id).unwrap();
    }
    graph
}

fn shifted(x: f64) -> Transform3D {
    Transform3D::Affine(AffineTransform3D::from_translation([x, 0.0, 0.0]))
}

fn opened() -> (Rc<SceneDocument>, NodeId) {
    let document = SceneDocument::new();
    document.take_mesh_scene(sample(1), PathBuf::from("part.obj"));
    let id = document
        .read_scene(|s| s.leaf_id_at_index(0))
        .flatten()
        .unwrap();
    (document, id)
}

fn move_to(document: &SceneDocument, id: NodeId, x: f64) -> SceneEdit {
    let before = document
        .read_scene(|s| s.graph().node(id).unwrap().local_transform)
        .unwrap();
    SceneEdit::SetLocalTransform {
        node_id: id,
        before,
        after: shifted(x),
    }
}

fn record(document: &SceneDocument, edit: SceneEdit) {
    document.commit_edit(edit.clone()).unwrap();
    document.record_edit(edit);
}

fn watch(document: &Rc<SceneDocument>) -> (Rc<RefCell<Vec<DocumentEvent>>>, Watch) {
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    let subscription = document.watch(move |e| sink.borrow_mut().push(e));
    (events, subscription)
}

#[test]
fn lifecycle_invalidates_identity_without_inventing_history() {
    let document = SceneDocument::new();
    let revision = document.revision();
    let (events, _subscription) = watch(&document);
    assert!(!document.has_scene());
    assert!(document.read_scene(|_| ()).is_none());
    assert!(document.file_path().is_none());
    assert_eq!(document.read_file_path(|_| ()), None);
    document.take_mesh_scene(sample(1), PathBuf::from("folder/part.obj"));
    assert_eq!(document.file_name().as_deref(), Some("part.obj"));
    assert!(document.slice_outdated_by_geometry(revision));
    document.select_sole_object();
    assert!(document.main_chosen_node().is_some());
    document.empty();
    assert!(!document.has_scene());
    assert_eq!(document.chosen(), Selection::None);
    assert!(!document.has_past());
    assert_eq!(
        *events.borrow(),
        [
            DocumentEvent::Opened,
            DocumentEvent::ChoiceChanged,
            DocumentEvent::Emptied
        ]
    );
}

#[test]
fn live_motion_and_commits_have_distinct_notifications() {
    let (document, id) = opened();
    let (events, _subscription) = watch(&document);
    let initial = document.revision();
    let edit = move_to(&document, id, 3.0);
    document.commit_edit(edit.clone()).unwrap();
    assert!(!document.has_unsaved_changes());
    assert_eq!(document.revision(), initial);
    document.record_edit(edit);
    assert_eq!(
        *events.borrow(),
        [
            DocumentEvent::Edited,
            DocumentEvent::HistoryChanged,
            DocumentEvent::UnsavedChanged
        ]
    );
    events.borrow_mut().clear();
    assert!(document.step_back().unwrap());
    assert_eq!(document.revision(), initial);
    assert!(!document.has_unsaved_changes());
    assert_eq!(
        *events.borrow(),
        [
            DocumentEvent::Edited,
            DocumentEvent::HistoryChanged,
            DocumentEvent::UnsavedChanged
        ]
    );
    assert!(document.step_forward().unwrap());
    assert!(document.slice_outdated_by_geometry(initial));
}

#[test]
fn branching_cannot_reuse_a_saved_history_identity() {
    let (document, id) = opened();
    record(&document, move_to(&document, id, 2.0));
    document.note_saved();
    let saved = document.revision();
    document.step_back().unwrap();
    document.step_forward().unwrap();
    assert_eq!(saved, document.revision());
    document.step_back().unwrap();
    record(&document, move_to(&document, id, 2.0));
    assert!(document.has_unsaved_changes());
    assert_ne!(saved, document.revision());
    assert!(!document.has_future());
    assert!(document.step_back().unwrap());
    assert!(document.has_unsaved_changes());
}

#[test]
fn invalid_edits_leave_selection_and_observers_alone() {
    let (document, id) = opened();
    document.select_sole_object();
    let (events, _subscription) = watch(&document);
    let absent = sample(8).allocate_unbound_node_id();
    let bad = SceneEdit::SetLocalTransform {
        node_id: absent,
        before: shifted(90.0),
        after: shifted(1.0),
    };
    assert!(matches!(document.commit_edit(bad), Err(EditRefused::Refused(SceneError::DanglingNodeId { .. }))));
    assert_eq!(document.main_chosen_node(), Some(id));
    assert!(events.borrow().is_empty());
    assert!(!document.has_past());
}

#[test]
fn empty_history_operations_and_noop_commits_are_silent() {
    let (document, id) = opened();
    let (events, _subscription) = watch(&document);
    assert!(!document.step_back().unwrap());
    assert!(!document.step_forward().unwrap());
    document.record_edit(move_to(&document, id, 0.0));
    assert!(events.borrow().is_empty());
}

#[test]
fn removing_multiple_selected_parts_is_one_reversible_transaction() {
    let document = SceneDocument::new();
    document.take_mesh_scene(sample(3), PathBuf::from("parts.obj"));
    document.select_all();
    assert_eq!(document.chosen().ids().len(), 3);
    assert!(document.delete_chosen().unwrap());
    assert_eq!(document.read_scene(|s| s.graph().roots().len()), Some(0));
    assert_eq!(document.chosen(), Selection::None);
    document.step_back().unwrap();
    assert_eq!(document.read_scene(|s| s.graph().roots().len()), Some(3));
    assert!(!document.has_past());
}

#[test]
fn selection_order_and_subscription_lifetime_are_observable() {
    let document = SceneDocument::new();
    document.take_mesh_scene(sample(2), PathBuf::from("pair.obj"));
    let ids = document.read_scene(|s| s.graph().roots().to_vec()).unwrap();
    let (events, subscription) = watch(&document);
    document.toggle_selection(ids[0]);
    document.toggle_selection(ids[1]);
    assert_eq!(document.main_chosen_node(), Some(ids[1]));
    document.toggle_selection(ids[1]);
    assert_eq!(document.main_chosen_node(), Some(ids[0]));
    document.choose(document.chosen());
    assert_eq!(events.borrow().len(), 3);
    drop(subscription);
    document.choose(Selection::None);
    assert_eq!(events.borrow().len(), 3);
}

#[test]
fn observers_can_read_the_completed_mutation() {
    let (document, id) = opened();
    let weak = Rc::downgrade(&document);
    let _subscription = document.watch(move |event| {
        let document = weak.upgrade().unwrap();
        if event == DocumentEvent::Edited {
            assert_eq!(
                document.read_scene(|s| s.graph().node(id).unwrap().local_transform),
                Some(shifted(4.0))
            );
        }
    });
    record(&document, move_to(&document, id, 4.0));
}

#[test]
fn plate_names_membership_and_settings_survive_history_replay() {
    let (document, id) = opened();
    document.select_sole_object();
    document.rename_plate("  primary  ");
    assert_eq!(document.plate_name().as_deref(), Some("primary"));
    document.add_plate();
    let second = document.active_plate();
    assert_ne!(second, 1);
    document.move_selection_to_plate(second);
    assert_eq!(
        document.read_scene(|s| s.graph().plate_of(id)),
        Some(second)
    );
    document.step_back().unwrap();
    assert_eq!(document.read_scene(|s| s.graph().plate_of(id)), Some(1));
    document.set_active_plate(1);
    assert_eq!(document.plate_name().as_deref(), Some("primary"));
    document.rename_plate(" \t ");
    assert_eq!(document.plate_name(), None);
}

#[test]
fn missing_slice_queries_have_consistent_empty_results() {
    let (document, _) = opened();
    assert!(document.slicing_inputs().is_none());
    assert!(document.slice_layer_schedule().is_none());
    assert!(document.slice_export_data().is_none());
    assert!(document.layer_total().is_none());
    assert!(document.layer_top_mm(0).is_none());
    assert!(document.read_layer(0, |_| ()).is_none());
    assert!(document.slice_island_spans(0).is_none());
    assert!(document.slice_island_spans(1).is_none());
    assert!(document.slice_thin_spans(0, 0.3).is_none());
    assert!(!document.slice_is_geometry_stale());
    assert!(document.all_plate_slices().is_empty());
}

#[test]
fn pasted_resources_share_storage_and_a_whole_paste_undoes_at_once() {
    let document = SceneDocument::new();
    document.take_mesh_scene(Scene::new(), PathBuf::from("empty.obj"));
    let mesh = Arc::new(unit_cube_mesh());
    let fragment = SceneFragment::from_leaf_snapshots([
        (mesh.clone(), shifted(2.0)),
        (mesh.clone(), shifted(8.0)),
    ]);
    document
        .paste_parts(&fragment, [10.0, 0.0, 0.0])
        .unwrap()
        .unwrap();
    let snapshot = document
        .read_scene(|s| {
            let roots = s.graph().roots();
            assert_eq!(roots.len(), 2);
            assert_eq!(
                s.graph().node(roots[0]).unwrap().local_transform,
                shifted(12.0)
            );
            assert_eq!(
                s.graph().node(roots[1]).unwrap().local_transform,
                shifted(18.0)
            );
            assert_eq!(s.graph().mesh_resources_iter().count(), 1);
            assert!(
                s.graph()
                    .mesh_resources_iter()
                    .all(|(rid, _)| Arc::ptr_eq(s.graph().mesh_resource_arc(rid).unwrap(), &mesh))
            );
            roots.to_vec()
        })
        .unwrap();
    assert_eq!(document.chosen().ids(), snapshot);
    document.step_back().unwrap();
    assert_eq!(document.read_scene(|s| s.graph().roots().len()), Some(0));
    document.step_forward().unwrap();
    assert_eq!(
        document.read_scene(|s| s.graph().roots().to_vec()),
        Some(snapshot)
    );
    document.paste_parts(&fragment, [0.0; 3]).unwrap();
    assert_eq!(
        document.read_scene(|s| s.graph().mesh_resources_iter().count()),
        Some(1)
    );
}

#[test]
fn deleting_the_last_resource_user_collects_it_and_undo_reinstates_it() {
    let document = SceneDocument::new();
    let mut graph = Scene::new();
    let resource = graph.add_mesh_resource(unit_cube_mesh());
    document.take_mesh_scene(graph, PathBuf::from("empty.obj"));
    let id = document
        .add_model(resource, shifted(3.0))
        .unwrap()
        .unwrap();
    document.choose(Selection::from_ids(vec![id]));
    document.delete_chosen().unwrap();
    assert_eq!(
        document.read_scene(|s| s.graph().mesh_resources_iter().count()),
        Some(0)
    );
    assert_eq!(document.main_chosen_node(), None);
    assert_eq!(document.read_scene(|s| s.graph().node(id).is_none()), Some(true));
    document.step_back().unwrap();
    assert_eq!(document.read_scene(|s| matches!(s.graph().node(id).unwrap().kind, NodeKind::Leaf(LeafPayload::MeshResource(r)) if r == resource)), Some(true));
    assert_eq!(
        document.read_scene(|s| s.graph().mesh_resources_iter().count()),
        Some(1)
    );
    assert_eq!(
        document.read_scene(|s| s.graph().node(id).unwrap().local_transform),
        Some(shifted(3.0))
    );
}

#[test]
fn supports_follow_translation_and_warn_after_scale_without_moving() {
    let mut graph = sample(1);
    let owner = graph.roots()[0];
    let metadata = sc_scene::node::SceneMetadata {
        name: None,
        comment: Some(format!(
            "sc-generated-support\nowner = {}\nowner_linear = [[1,0,0],[0,1,0],[0,0,1]]\n",
            owner.get()
        )),
    };
    let support = graph
        .add_leaf(
            Some(metadata),
            shifted(1.0),
            LeafPayload::Mesh(Arc::new(unit_cube_mesh())),
        )
        .unwrap();
    graph.add_root(support).unwrap();
    let document = SceneDocument::new();
    document.take_mesh_scene(graph, PathBuf::from("supported.obj"));
    record(&document, move_to(&document, owner, 5.0));
    assert_eq!(
        document.read_scene(|s| s.graph().node(support).unwrap().local_transform),
        Some(shifted(6.0))
    );
    assert_eq!(
        document.read_scene(|s| owned_support_ids(s.graph(), owner)),
        Some(vec![support])
    );
    document.step_back().unwrap();
    assert_eq!(
        document.read_scene(|s| s.graph().node(support).unwrap().local_transform),
        Some(shifted(1.0))
    );
    let after = Transform3D::Affine(AffineTransform3D::from_linear_and_translation(
        [[2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        [0.0; 3],
    ));
    record(
        &document,
        SceneEdit::SetLocalTransform {
            node_id: owner,
            before: shifted(0.0),
            after,
        },
    );
    assert_eq!(
        document.read_scene(|s| support_is_stale(s.graph(), support)),
        Some(true)
    );
    assert_eq!(
        document.read_scene(|s| s.graph().node(support).unwrap().local_transform),
        Some(shifted(1.0))
    );
    document.step_back().unwrap();
    assert_eq!(
        document.read_scene(|s| support_is_stale(s.graph(), support)),
        Some(false)
    );
}

fn sliced(document: &SceneDocument) -> SliceResult {
    use sc_print_primitives::{AaLevel, BuildVolume, Mirror, PixelResolution};
    let schedule = LayerSchedule::uniform(0.1, 0.2, 2).unwrap();
    SliceResult {
        merged: ContourStack::new(vec![
            ContourLayer::new(0.2, vec![]),
            ContourLayer::new(0.3, vec![]),
        ])
        .unwrap(),
        schedule,
        token: "document-test-token".into(),
        params: SliceParams {
            printer: "printer-a".parse().unwrap(),
            resin: "resin-a".parse().unwrap(),
            first_layer_height: None,
            aa_level: AaLevel::Off,
            geometry: document.revision(),
        },
        profile: SliceProfile::new(
            BuildVolume::new(10.0, 10.0, 10.0).unwrap(),
            PixelResolution::new(10, 10).unwrap(),
            schedule,
            Mirror::default(),
            AaLevel::Off,
            sc_sl1cer::profile::PackingMode::None,
        ),
    }
}

#[test]
fn held_slices_are_projected_persisted_and_invalidated_by_committed_geometry() {
    let (document, id) = opened();
    let result = sliced(&document);
    let schedule = result.schedule;
    let generation = document.slice_serial_now();
    document.keep_slice(result);
    assert_eq!(document.slice_serial_now(), generation.wrapping_add(1));
    assert!(!document.has_unsaved_changes());
    assert_eq!(document.layer_total(), Some(2));
    assert_eq!(document.layer_top_mm(1), Some(0.30000000000000004));
    assert_eq!(document.layer_top_mm(2), None);
    assert_eq!(document.slice_layer_schedule(), Some(schedule));
    assert_eq!(document.read_layer(1, |_| 7), Some(7));
    assert!(document.slice_export_data().is_some());
    assert_eq!(
        document.slices_to_save(&schedule).unwrap().1,
        "document-test-token"
    );
    assert!(
        document
            .slices_to_save(&LayerSchedule::uniform(0.2, 0.2, 2).unwrap())
            .is_none()
    );
    assert_eq!(document.slice_thin_spans(0, 0.3), Some((vec![], 10, 10)));
    assert_eq!(document.slice_island_spans(1), Some((vec![], 10, 10)));
    record(&document, move_to(&document, id, 2.0));
    assert!(document.slice_is_geometry_stale());
    assert!(document.slices_to_save(&schedule).is_none());
    document.step_back().unwrap();
    assert!(!document.slice_is_geometry_stale());
    assert!(document.slices_to_save(&schedule).is_some());
    document.empty();
    assert_eq!(document.slice_serial_now(), generation.wrapping_add(1));
    assert!(document.slicing_inputs().is_none());
}

#[test]
fn plate_switches_cache_slices_without_changing_geometry() {
    let (document, _) = opened();
    document.add_plate();
    let second = document.active_plate();
    document.set_active_plate(1);
    document.keep_slice(sliced(&document));
    let revision = document.revision();
    document.set_active_plate(second);
    assert!(document.slicing_inputs().is_none());
    document.keep_slice(sliced(&document));
    assert_eq!(document.all_plate_slices().len(), 2);
    document.set_active_plate(1);
    assert!(document.slicing_inputs().is_some());
    assert_eq!(document.revision(), revision);
}

#[test]
fn dispatch_keeps_its_snapshot_when_a_callback_replaces_an_observer() {
    let (document, _) = opened();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let later = Rc::new(RefCell::new(None));
    let replacement = Rc::new(RefCell::new(None));
    let weak = Rc::downgrade(&document);
    let (sink, retired, installed) = (calls.clone(), later.clone(), replacement.clone());
    let _first = document.watch(move |_| {
        sink.borrow_mut().push(1);
        retired.borrow_mut().take();
        if installed.borrow().is_none() {
            let sink = sink.clone();
            *installed.borrow_mut() = Some(weak.upgrade().unwrap().watch(move |_| {
                sink.borrow_mut().push(3);
            }));
        }
    });
    let sink = calls.clone();
    *later.borrow_mut() = Some(document.watch(move |_| sink.borrow_mut().push(2)));
    document.select_sole_object();
    assert_eq!(*calls.borrow(), [1, 2]);
    calls.borrow_mut().clear();
    document.choose(Selection::None);
    assert_eq!(*calls.borrow(), [1, 3]);
}

#[test]
fn rejected_replay_keeps_the_entry_available_for_another_attempt() {
    let (document, id) = opened();
    let bad_id = sample(5).allocate_unbound_node_id();
    document.record_edit(SceneEdit::SetLocalTransform {
        node_id: bad_id,
        before: shifted(0.0),
        after: shifted(1.0),
    });
    let revision = document.revision();
    let (events, _subscription) = watch(&document);
    for _ in 0..2 {
        assert!(document.step_back().is_err());
        assert_eq!(document.revision(), revision);
        assert!(document.has_past());
        assert!(!document.has_future());
    }
    assert!(events.borrow().is_empty());
    document.take_mesh_scene(sample(1), PathBuf::from("retry.obj"));
    record(&document, move_to(&document, id, 4.0));
    document.step_back().unwrap();
    let removal = document
        .read_scene(|s| SceneEdit::remove_leaf_with_gc(s.graph(), id))
        .flatten()
        .unwrap();
    document.commit_edit(removal).unwrap();
    events.borrow_mut().clear();
    for _ in 0..2 {
        assert!(document.step_forward().is_err());
        assert!(document.has_future());
        assert!(!document.has_past());
    }
    assert!(events.borrow().is_empty());
}

#[test]
fn evicted_slice_history_does_not_match_an_empty_head_after_full_undo() {
    let (document, id) = opened();
    document.keep_slice(sliced(&document));
    for x in 1..=UNDO_DEPTH + 1 {
        record(&document, move_to(&document, id, x as f64));
    }
    while document.step_back().unwrap() {}
    assert!(document.slice_is_geometry_stale());
    document.keep_slice(sliced(&document));
    assert!(!document.slice_is_geometry_stale());
}

#[test]
fn recovery_payload_keeps_choices_but_requires_a_fresh_save_target() {
    let Some(directory) = crate::recovery::directory() else {
        return;
    };
    let document = SceneDocument::new();
    let payload = ProjectPayload {
        printer: Some("recovery-printer".parse().unwrap()),
        aa_level: Some(sc_print_primitives::AaLevel::X4),
        ..Default::default()
    };
    document.take_project_scene(sample(1), directory.join("recovered.3mf"), payload);
    assert!(document.has_unsaved_changes());
    assert!(!document.opened_from_project());
    assert_eq!(
        document.chosen_printer(),
        Some("recovery-printer".parse().unwrap())
    );
    assert_eq!(
        document.scene_aa.get(),
        Some(sc_print_primitives::AaLevel::X4)
    );
    assert!(!document.has_past());
    document.note_saved();
    assert!(!document.has_unsaved_changes());
}

#[test]
fn paste_preserves_relative_offsets_and_rebinds_support_metadata() {
    let mut graph = Scene::new();
    let resource = graph.add_mesh_resource(unit_cube_mesh());
    let owner = graph
        .add_leaf(None, shifted(2.0), LeafPayload::MeshResource(resource))
        .unwrap();
    graph.add_root(owner).unwrap();
    let support = graph
        .add_leaf(
            Some(sc_scene::node::SceneMetadata {
                name: Some("support label".into()),
                comment: Some(format!(
                    "sc-generated-support\nowner = {}\nextra = true\n",
                    owner.get()
                )),
            }),
            shifted(7.0),
            LeafPayload::MeshResource(resource),
        )
        .unwrap();
    graph.add_root(support).unwrap();
    let (document, _) = opened();
    let pasted = document
        .paste_parts(&SceneFragment::from_scene(graph), [10.0, 0.0, 0.0])
        .unwrap()
        .unwrap();
    let snapshot = document.read_scene(|s| s.graph().clone()).unwrap();
    let supports = owned_support_ids(&snapshot, pasted);
    assert_eq!(supports.len(), 1);
    assert_eq!(
        snapshot.node(pasted).unwrap().local_transform,
        shifted(12.0)
    );
    assert_eq!(
        snapshot.node(supports[0]).unwrap().local_transform,
        shifted(17.0)
    );
    assert_eq!(
        node_comment(&snapshot, supports[0]),
        Some(
            format!(
                "sc-generated-support\nextra = true\nowner = {}\n",
                pasted.get()
            )
            .as_str()
        )
    );
    assert_eq!(document.chosen().ids(), [pasted]);
    document.step_back().unwrap();
    assert_eq!(document.read_scene(|s| s.graph().roots().len()), Some(1));
    document.step_forward().unwrap();
    assert_eq!(
        document.read_scene(|s| owned_support_ids(s.graph(), pasted)),
        Some(supports)
    );
}

#[test]
fn support_motion_respects_explicit_moves_and_history() {
    let mut graph = sample(1);
    let owner = graph.roots()[0];
    let mesh = || LeafPayload::Mesh(Arc::new(unit_cube_mesh()));
    let add = |graph: &mut Scene, comment: String| {
        let id = graph.add_leaf(
            Some(sc_scene::node::SceneMetadata { name: None, comment: Some(comment) }),
            Transform3D::Identity, mesh(),
        ).unwrap();
        graph.add_root(id).unwrap();
        id
    };
    let column = add(&mut graph, format!("sc-generated-support\nowner = {}\n", owner.get()));
    let raft = add(&mut graph, "sc-generated-support-raft".into());
    let document = SceneDocument::new();
    document.take_mesh_scene(graph, PathBuf::from("supported.obj"));
    let local = |id| document.read_scene(|s| s.graph().node(id).unwrap().local_transform).unwrap();
    document.read_scene(|s| {
        assert_eq!(support_owner(s.graph(), column), Some(owner));
        assert_eq!(support_owner(s.graph(), raft), Some(owner));
        assert_eq!(support_owner(s.graph(), owner), None);
    }).unwrap();
    record(&document, move_to(&document, owner, 4.0));
    assert_eq!(local(column), shifted(4.0));
    assert_eq!(local(raft), shifted(4.0));
    assert!(document.step_back().unwrap());
    assert_eq!(local(owner), shifted(0.0));
    assert_eq!(local(column), shifted(0.0));
    assert!(document.step_forward().unwrap());
    assert_eq!(local(column), shifted(4.0));
    assert!(document.step_back().unwrap());
    let compound = SceneEdit::Compound(vec![move_to(&document, owner, 2.0)]);
    record(&document, compound);
    assert_eq!(local(column), shifted(2.0));
    assert!(document.step_back().unwrap());
    assert_eq!(local(column), shifted(0.0));
    let edits = [owner, column, raft].map(|id| move_to(&document, id, 3.0));
    document.commit_edit(SceneEdit::Compound(edits.to_vec())).unwrap();
    assert_eq!(local(column), shifted(3.0));
    assert_eq!(local(raft), shifted(3.0));
    document.commit_edit(SceneEdit::Compound([owner, column, raft].map(|id| move_to(&document, id, 0.0)).to_vec())).unwrap();
    document.commit_edit(move_to(&document, column, 2.0)).unwrap();
    assert_eq!(local(raft), shifted(0.0));
}

#[test]
fn scene_sources_and_selection_reset_across_loads() {
    let document = SceneDocument::new();
    assert_eq!(document.chosen(), Selection::None);
    assert_eq!(document.main_chosen_node(), None);
    assert_eq!(document.file_name(), None);
    let absent = sample(1).roots()[0];
    assert!(matches!(document.commit_edit(SceneEdit::SetLocalTransform { node_id: absent, before: Transform3D::Identity, after: shifted(1.0) }), Err(EditRefused::Empty)));
    assert!(!document.has_past());
    assert!(!document.has_future());
    let (events, _subscription) = watch(&document);
    document.take_mesh_scene(Scene::new(), PathBuf::from("first.obj"));
    assert!(document.has_scene());
    assert_eq!(document.read_scene(|s| s.leaf_id_at_index(0)), Some(None));
    assert_eq!(document.file_path(), Some(PathBuf::from("first.obj")));
    assert_eq!(document.file_name().as_deref(), Some("first.obj"));
    assert_eq!(document.read_file_path(|p| p.file_stem().unwrap().to_owned()).as_deref(), Some("first".as_ref()));
    document.empty();
    assert!(document.read_scene(|_| ()).is_none());
    assert!(document.file_path().is_none());
    assert_eq!(*events.borrow(), [DocumentEvent::Opened, DocumentEvent::Emptied]);
    document.take_mesh_scene(sample(1), PathBuf::from("first.obj"));
    document.select_sole_object();
    let selection = document.chosen();
    assert_eq!(document.main_chosen_node(), selection.ids().last().copied());
    document.take_mesh_scene(sample(1), PathBuf::from("second.obj"));
    assert!(document.has_scene());
    assert_eq!(document.file_path(), Some(PathBuf::from("second.obj")));
    assert_eq!(document.chosen(), Selection::None);
    document.take_spatial(SceneSpatial::primed_graph(Scene::new()), PathBuf::from("spatial.obj"), None);
    assert!(document.has_scene());
    assert_eq!(document.file_path(), Some(PathBuf::from("spatial.obj")));
    let stamp = document.stamp_for_save();
    document.note_project_saved(ProjectPayload::default(), PathBuf::from("saved.3mf"), stamp);
    assert_eq!(document.file_name().as_deref(), Some("saved.3mf"));
}

#[test]
fn history_tracks_branches_caps_and_scene_replacement() {
    let fresh = SceneDocument::new();
    assert!(!fresh.step_back().unwrap());
    assert!(!fresh.step_forward().unwrap());
    let (document, id) = opened();
    document.record_edit(move_to(&document, id, 0.0));
    assert!(!document.has_past());
    record(&document, move_to(&document, id, 1.0));
    assert!(document.has_past());
    assert!(!document.has_future());
    assert!(document.step_back().unwrap());
    assert_eq!(document.read_scene(|s| s.graph().node(id).unwrap().local_transform), Some(shifted(0.0)));
    assert!(!document.has_past());
    assert!(document.has_future());
    assert!(document.step_forward().unwrap());
    assert_eq!(document.read_scene(|s| s.graph().node(id).unwrap().local_transform), Some(shifted(1.0)));
    assert!(document.has_past());
    assert!(!document.has_future());
    document.step_back().unwrap();
    record(&document, move_to(&document, id, 2.0));
    assert!(document.has_past());
    assert!(!document.has_future());
    document.take_mesh_scene(sample(1), PathBuf::from("new.obj"));
    assert!(!document.has_past());
    assert!(!document.has_future());
    let id = document.read_scene(|s| s.graph().roots()[0]).unwrap();
    for x in 1..=UNDO_DEPTH + 5 {
        record(&document, move_to(&document, id, x as f64));
    }
    for _ in 0..UNDO_DEPTH { assert!(document.step_back().unwrap()); }
    assert!(!document.step_back().unwrap());
    assert_eq!(document.read_scene(|s| s.graph().node(id).unwrap().local_transform), Some(shifted(5.0)));
    document.empty();
    assert!(!document.has_past());
    assert!(!document.has_future());
}

#[test]
fn adding_roots_and_pruning_selection_are_reversible() {
    let document = SceneDocument::new();
    let mut graph = Scene::new();
    let rid = graph.add_mesh_resource(unit_cube_mesh());
    document.take_mesh_scene(graph, PathBuf::from("empty.obj"));
    let first = document.add_model(rid, shifted(3.0)).unwrap().unwrap();
    assert!(document.has_past());
    document.read_scene(|s| {
        let node = s.graph().node(first).unwrap();
        assert_eq!(node.local_transform, shifted(3.0));
        assert!(matches!(node.kind, NodeKind::Leaf(LeafPayload::MeshResource(r)) if r == rid));
        assert!(s.graph().roots().contains(&first));
    }).unwrap();
    let second = document.add_model(rid, shifted(5.0)).unwrap().unwrap();
    document.choose(Selection::from_ids(vec![first]));
    document.step_back().unwrap();
    assert_eq!(document.main_chosen_node(), Some(first));
    document.step_forward().unwrap();
    document.choose(Selection::from_ids(vec![second]));
    let (events, _subscription) = watch(&document);
    document.step_back().unwrap();
    assert_eq!(document.chosen(), Selection::None);
    assert_eq!(*events.borrow(), [DocumentEvent::Regrouped, DocumentEvent::ChoiceChanged, DocumentEvent::HistoryChanged]);
    assert_eq!(document.read_scene(|s| s.graph().node(second).is_some()), Some(false));
    document.step_forward().unwrap();
    assert_eq!(document.read_scene(|s| matches!(s.graph().node(second).unwrap().kind, NodeKind::Leaf(LeafPayload::MeshResource(r)) if r == rid)), Some(true));
    document.empty();
    assert!(document.add_model(rid, shifted(0.0)).unwrap().is_none());
    assert!(!document.has_past());
}

#[test]
fn selection_changes_and_install_notifications_are_exact() {
    let document = SceneDocument::new();
    let (events, _subscription) = watch(&document);
    document.take_mesh_scene(sample(1), PathBuf::from("one.obj"));
    assert_eq!(*events.borrow(), [DocumentEvent::Opened]);
    let id = document.read_scene(|s| s.graph().roots()[0]).unwrap();
    events.borrow_mut().clear();
    let selection = Selection::from_ids(vec![id]);
    document.choose(selection.clone());
    assert_eq!(document.chosen(), selection);
    assert_eq!(document.main_chosen_node(), Some(id));
    document.choose(selection);
    document.choose(Selection::None);
    document.choose(Selection::None);
    assert_eq!(*events.borrow(), [DocumentEvent::ChoiceChanged, DocumentEvent::ChoiceChanged]);
    document.record_edit(move_to(&document, id, 1.0));
    events.borrow_mut().clear();
    document.take_mesh_scene(sample(1), PathBuf::from("two.obj"));
    assert_eq!(*events.borrow(), [DocumentEvent::Opened, DocumentEvent::HistoryChanged, DocumentEvent::UnsavedChanged]);
}

#[test]
fn reanchoring_replaces_only_translation() {
    let linear = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    for original in [Transform3D::Identity, Transform3D::Affine(AffineTransform3D::from_linear_and_translation(linear, [9.0; 3]))] {
        let Transform3D::Affine(result) = reanchor(original, [1.0, 2.0, 3.0]) else { panic!() };
        assert_eq!(*result.translation(), [1.0, 2.0, 3.0]);
        if original != Transform3D::Identity { assert_eq!(*result.linear(), linear); }
    }
}

#[test]
fn paste_preserves_root_order_resources_and_notifications() {
    let document = SceneDocument::new();
    let mesh = Arc::new(unit_cube_mesh());
    let fragment = SceneFragment::from_leaf_snapshots([(mesh.clone(), Transform3D::Identity)]);
    assert!(document.paste_parts(&fragment, [1.0, 2.0, 3.0]).unwrap().is_none());
    assert!(!document.has_past());
    let mut graph = Scene::new();
    let rid = graph.add_mesh_resource_arc(mesh);
    let existing = graph.add_leaf(None, Transform3D::Identity, LeafPayload::MeshResource(rid)).unwrap();
    graph.add_root(existing).unwrap();
    document.take_mesh_scene(graph, PathBuf::from("paste.obj"));
    let (events, _subscription) = watch(&document);
    let pasted = document.paste_parts(&fragment, [1.0, 2.0, 3.0]).unwrap().unwrap();
    assert_eq!(document.read_scene(|s| s.graph().roots().to_vec()), Some(vec![existing, pasted]));
    assert_eq!(document.read_scene(|s| s.graph().mesh_resources_iter().count()), Some(1));
    assert!(events.borrow().contains(&DocumentEvent::Regrouped));
    assert!(!events.borrow().contains(&DocumentEvent::Edited));
    document.step_back().unwrap();
    assert_eq!(document.read_scene(|s| s.graph().roots().to_vec()), Some(vec![existing]));
    assert!(!document.has_past());
    document.step_forward().unwrap();
    document.read_scene(|s| {
        let node = s.graph().node(pasted).unwrap();
        assert!(matches!(node.kind, NodeKind::Leaf(LeafPayload::MeshResource(r)) if r == rid));
        assert_eq!(node.local_transform, Transform3D::Affine(AffineTransform3D::from_translation([1.0, 2.0, 3.0])));
    }).unwrap();
}

#[test]
fn deleting_named_children_and_shared_resources_restores_them() {
    let document = SceneDocument::new();
    assert!(!document.delete_chosen().unwrap());
    assert!(!document.has_past());
    let mut graph = Scene::new();
    let parent = graph.add_group(None, Transform3D::Identity);
    graph.add_root(parent).unwrap();
    let child = graph.add_leaf(Some(sc_scene::SceneMetadata { name: Some("part".into()), comment: None }), Transform3D::Identity, LeafPayload::Mesh(Arc::new(unit_cube_mesh()))).unwrap();
    graph.add_child(parent, child).unwrap();
    let rid = graph.add_mesh_resource(unit_cube_mesh());
    document.take_mesh_scene(graph, PathBuf::from("parts.obj"));
    let first = document.add_model(rid, shifted(1.0)).unwrap().unwrap();
    let second = document.add_model(rid, shifted(2.0)).unwrap().unwrap();
    assert!(!document.delete_chosen().unwrap());
    assert!(document.has_past());
    document.choose(Selection::from_ids(vec![second]));
    let (events, _subscription) = watch(&document);
    assert!(document.delete_chosen().unwrap());
    assert_eq!(*events.borrow(), [DocumentEvent::Regrouped, DocumentEvent::ChoiceChanged, DocumentEvent::HistoryChanged]);
    assert_eq!(document.read_scene(|s| (s.graph().node(second).is_some(), s.graph().node(first).is_some(), s.graph().mesh_resources_iter().count())), Some((false, true, 1)));
    document.step_back().unwrap();
    assert_eq!(document.read_scene(|s| (s.graph().node(second).is_some(), s.graph().mesh_resources_iter().count())), Some((true, 1)));
    document.choose(Selection::from_ids(vec![child]));
    assert!(document.delete_chosen().unwrap());
    assert_eq!(document.chosen(), Selection::None);
    assert_eq!(document.read_scene(|s| s.graph().node(child).is_none()), Some(true));
    document.step_back().unwrap();
    document.read_scene(|s| {
        assert_eq!(s.graph().parent_of(child), Some(parent));
        assert_eq!(s.graph().node(child).unwrap().meta.as_ref().unwrap().name.as_deref(), Some("part"));
    }).unwrap();
}

#[test]
fn undoing_a_selected_add_notifies_a_clean_transition() {
    let mut graph = Scene::new();
    let rid = graph.add_mesh_resource(unit_cube_mesh());
    let document = SceneDocument::new();
    document.take_mesh_scene(graph, PathBuf::from("empty.obj"));
    let id = document.add_model(rid, shifted(1.0)).unwrap().unwrap();
    document.choose(Selection::from_ids(vec![id]));
    let (events, _subscription) = watch(&document);
    document.step_back().unwrap();
    assert_eq!(*events.borrow(), [DocumentEvent::Regrouped, DocumentEvent::ChoiceChanged, DocumentEvent::HistoryChanged, DocumentEvent::UnsavedChanged]);
}

#[test]
fn install_then_edit_and_occupied_undo_are_observable() {
    let document = SceneDocument::new();
    let (events, _subscription) = watch(&document);
    document.take_mesh_scene(sample(1), PathBuf::from("part.obj"));
    let id = document.read_scene(|s| s.graph().roots()[0]).unwrap();
    document.commit_edit(move_to(&document, id, 1.0)).unwrap();
    assert_eq!(*events.borrow(), [DocumentEvent::Opened, DocumentEvent::Edited]);
    let rid = Scene::new().allocate_unbound_mesh_resource_id();
    document.record_edit(SceneEdit::RemoveLeaf(sc_scene_edit::LeafRecord { node_id: id, attachment: LeafAttachment::Root { position: 0 }, payload: LeafPayload::MeshResource(rid), local_transform: shifted(1.0), meta: None }));
    assert!(matches!(document.step_back(), Err(SceneError::NodeSlotOccupied { id: occupied }) if occupied == id));
    assert!(document.has_past());
    assert!(!document.has_future());
}

#[test]
fn replacing_a_mesh_invalidates_slices_and_restores_arc_on_undo() {
    let mut graph = Scene::new();
    let original = Arc::new(unit_cube_mesh());
    let rid = graph.add_mesh_resource_arc(original.clone());
    let id = graph.add_leaf(None, Transform3D::Identity, LeafPayload::MeshResource(rid)).unwrap();
    graph.add_root(id).unwrap();
    let document = SceneDocument::new();
    document.take_mesh_scene(graph, PathBuf::from("part.obj"));
    let revision = document.revision();
    let after = Arc::new(unit_cube_mesh());
    record(&document, SceneEdit::ReplaceMeshResource { rid, before: original.clone(), after });
    assert!(document.slice_outdated_by_geometry(revision));
    document.step_back().unwrap();
    assert!(document.read_scene(|s| Arc::ptr_eq(s.graph().mesh_resource_arc(rid).unwrap(), &original)).unwrap());
}

#[test]
fn supported_parts_delete_copy_and_rotate_together() {
    let mut graph = sample(1);
    let owner = graph.roots()[0];
    for comment in [format!("sc-generated-support\nowner = {}\n", owner.get()), "sc-generated-support-raft".into()] {
        let id = graph.add_leaf(Some(sc_scene::SceneMetadata { name: None, comment: Some(comment) }), Transform3D::Identity, LeafPayload::Mesh(Arc::new(unit_cube_mesh()))).unwrap();
        graph.add_root(id).unwrap();
    }
    let doc = SceneDocument::new();
    doc.take_mesh_scene(graph, PathBuf::from("supported.obj"));
    let supports = doc.read_scene(|s| owned_support_ids(s.graph(), owner)).unwrap();
    assert_eq!(supports.len(), 2);
    let rotation = Transform3D::Affine(AffineTransform3D::from_rotation_z(0.5));
    record(&doc, SceneEdit::SetLocalTransform { node_id: owner, before: shifted(0.0), after: rotation });
    doc.read_scene(|s| for &id in &supports {
        assert_eq!(s.graph().node(id).unwrap().local_transform, Transform3D::Identity);
        assert!(support_is_stale(s.graph(), id));
    }).unwrap();
    doc.step_back().unwrap();
    assert!(!doc.read_scene(|s| support_is_stale(s.graph(), supports[0])).unwrap());
    doc.choose(Selection::from_ids(vec![owner]));
    let fragment = crate::clipboard::copy_of_selection(&doc).unwrap();
    assert_eq!(fragment.scene().roots().len(), 3);
    let copy = doc.paste_parts(&fragment, [15.0, 0.0, 0.0]).unwrap().unwrap();
    let check = || doc.read_scene(|s| {
        let copied = owned_support_ids(s.graph(), copy);
        assert_eq!(copied.len(), 2);
        for id in copied { assert_eq!(support_owner(s.graph(), id), Some(copy)); assert!(node_comment(s.graph(), id).unwrap().contains(&format!("owner = {}", copy.get()))); }
        assert_eq!(owned_support_ids(s.graph(), owner).len(), 2);
    }).unwrap();
    check(); doc.step_back().unwrap(); doc.step_forward().unwrap(); check();
    doc.choose(Selection::from_ids(vec![owner]));
    doc.delete_chosen().unwrap();
    assert!(doc.read_scene(|s| [owner, supports[0], supports[1]].iter().all(|&id| s.graph().node(id).is_none())).unwrap());
    doc.step_back().unwrap();
    assert!(doc.read_scene(|s| [owner, supports[0], supports[1]].iter().all(|&id| s.graph().node(id).is_some())).unwrap());
}

#[test]
fn generated_supports_replace_old_ones_and_track_linear_changes() {
    let mut graph = sample(1);
    let owner = graph.roots()[0];
    let old = graph.add_leaf(Some(sc_scene::SceneMetadata { name: None, comment: Some(format!("sc-generated-support\nowner = {}\n", owner.get())) }), Transform3D::Identity, LeafPayload::Mesh(Arc::new(unit_cube_mesh()))).unwrap();
    graph.add_root(old).unwrap();
    let doc = SceneDocument::new();
    doc.take_mesh_scene(graph.clone(), PathBuf::from("part.obj"));
    doc.apply_support_scene(&graph, &[owner]).unwrap();
    let supports = doc.read_scene(|s| owned_support_ids(s.graph(), owner)).unwrap();
    assert!(!supports.is_empty());
    assert!(!supports.contains(&old));
    for linear in [AffineTransform3D::from_rotation_z(0.5), AffineTransform3D::from_uniform_scale(1.5), AffineTransform3D::from_linear_and_translation([[-1.0,0.0,0.0],[0.0,1.0,0.0],[0.0,0.0,1.0]], [0.0;3])] {
        record(&doc, SceneEdit::SetLocalTransform { node_id: owner, before: Transform3D::Identity, after: Transform3D::Affine(linear) });
        assert!(doc.read_scene(|s| supports.iter().all(|&id| support_is_stale(s.graph(), id))).unwrap());
        doc.step_back().unwrap();
        assert!(doc.read_scene(|s| supports.iter().all(|&id| !support_is_stale(s.graph(), id))).unwrap());
    }
}

#[test]
fn generated_scene_replacement_replays_as_single_steps() {
    let original = sample(1);
    let root = original.roots()[0];
    let doc = SceneDocument::new();
    doc.take_mesh_scene(original.clone(), PathBuf::from("part.obj"));
    let mut generated = original.clone();
    generated.set_local_transform(root, shifted(5.0)).unwrap();
    let extra = generated.add_leaf(None, Transform3D::Identity, LeafPayload::Mesh(Arc::new(unit_cube_mesh()))).unwrap();
    generated.add_root(extra).unwrap();
    doc.apply_support_scene(&generated, &[root]).unwrap();
    doc.apply_support_scene(&generated, &[root]).unwrap();
    assert_eq!(doc.read_scene(|s| s.graph().roots().len()), Some(2));
    doc.step_back().unwrap();
    assert_eq!(doc.read_scene(|s| s.graph().roots().len()), Some(2));
    doc.step_back().unwrap();
    assert_eq!(doc.read_scene(|s| s.graph().roots().to_vec()), Some(vec![root]));
    assert_eq!(doc.read_scene(|s| s.graph().node(root).unwrap().local_transform), Some(shifted(0.0)));
    doc.step_forward().unwrap();
    assert_eq!(doc.read_scene(|s| (s.graph().roots().len(), s.graph().node(root).unwrap().local_transform)), Some((2, shifted(5.0))));
}

#[test]
fn replacing_nested_supports_preserves_the_other_contact() {
    let mut graph = Scene::new();
    let group = graph.add_group(None, Transform3D::Identity);
    graph.add_root(group).unwrap();
    let mut contacts = vec![];
    for _ in 0..2 {
        let id = graph.add_leaf(Some(sc_scene::SceneMetadata { name: None, comment: Some("sc-generated-support".into()) }), Transform3D::Identity, LeafPayload::Mesh(Arc::new(unit_cube_mesh()))).unwrap();
        graph.add_child(group, id).unwrap(); contacts.push(id);
    }
    let doc = SceneDocument::new();
    doc.take_mesh_scene(graph.clone(), PathBuf::from("part.obj"));
    graph.remove_leaf(contacts[0]).unwrap();
    doc.apply_support_scene(&graph, &[group]).unwrap();
    assert_eq!(doc.read_scene(|s| crate::auto_support_run::support_ids(s.graph()).len()), Some(1));
    doc.step_back().unwrap();
    assert_eq!(doc.read_scene(|s| crate::auto_support_run::support_ids(s.graph()).len()), Some(2));
}

#[test]
fn filling_plate_from_a_selected_support_duplicates_its_owner() {
    let mut graph = Scene::new();
    let transform = Transform3D::Affine(AffineTransform3D::from_translation([0.0, 0.0, 5.0]))
        .compose(&Transform3D::Affine(AffineTransform3D::from_uniform_scale(10.0)));
    let owner = graph.add_leaf(None, transform, LeafPayload::Mesh(Arc::new(unit_cube_mesh()))).unwrap();
    graph.add_root(owner).unwrap();
    let column = graph.add_leaf(
        Some(sc_scene::node::SceneMetadata { name: None, comment: Some(format!("sc-generated-support\nowner = {}\n", owner.get())) }),
        shifted(0.0), LeafPayload::Mesh(Arc::new(unit_cube_mesh())),
    ).unwrap();
    graph.add_root(column).unwrap();
    let document = SceneDocument::new();
    document.take_mesh_scene(graph, PathBuf::from("supported.obj"));
    document.choose(Selection::from_ids(vec![column]));
    let copies = document.fill_plate(crate::build_volume::BuildVolume::new(60.0, 60.0, 60.0), 3.0).unwrap();
    assert!(copies > 0);
    document.read_scene(|scene| {
        let supports = crate::auto_support_run::support_ids(scene.graph()).len();
        assert_eq!(supports, copies + 1);
        assert_eq!(scene.leaf_ids().len() - supports, copies + 1);
    }).unwrap();
}

#[test]
fn undo_history_limit_is_256_entries() {
    assert_eq!(UNDO_DEPTH, 256);
}

#[test]
fn undo_addition_prunes_only_the_removed_selection() {
    let document = SceneDocument::new();
    let mut graph = Scene::new();
    let resource = graph.add_mesh_resource(unit_cube_mesh());
    document.take_mesh_scene(graph, PathBuf::from("empty.obj"));
    let first = document.add_model(resource, shifted(0.0)).unwrap().unwrap();
    let second = document.add_model(resource, shifted(1.0)).unwrap().unwrap();
    document.choose(Selection::from_ids(vec![first, second]));
    document.step_back().unwrap();
    assert_eq!(document.chosen().ids(), vec![first]);
}

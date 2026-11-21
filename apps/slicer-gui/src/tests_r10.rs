// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{
    WorkspaceSection, scene_document::SceneDocument, selection::Selection,
    viewport::ViewController,
};
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_scene::{Scene, node::LeafPayload};
use std::sync::Arc;

fn cube() -> Scene {
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_translation([0.0, 0.0, 10.0])).compose(
                &Transform3D::Affine(AffineTransform3D::from_uniform_scale(20.0)),
            ),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    scene
}
fn count(doc: &SceneDocument) -> usize {
    doc.read_scene(|s| {
        s.graph()
            .reachable_leaves(
                sc_scene::CsgPolicy::Descend,
                sc_scene::ReferencePolicy::Skip,
            )
            .unwrap()
            .len()
    })
    .unwrap()
}
fn controller() -> ViewController {
    let ctrl = ViewController::for_test();
    ctrl.put_graph_for_test(cube());
    ctrl.switch_section(WorkspaceSection::Transform);
    ctrl
}
#[test]
fn import_batch_keeps_existing_transforms_and_undoes_once() {
    let ctrl = controller();
    let doc = ctrl.scene_doc();
    let before = doc.read_scene(|s| s.graph().clone()).unwrap();
    doc.import_geometry(&[cube(), cube()], ctrl.printer_volume())
        .unwrap();
    assert_eq!(count(&doc), 3);
    assert_eq!(doc.chosen().ids().len(), 2);
    for &id in before.roots() {
        assert_eq!(
            before.node(id).unwrap().local_transform,
            doc.read_scene(|s| s.graph().node(id).unwrap().local_transform)
                .unwrap()
        );
    }
    let boxes = doc
        .read_scene(crate::clipboard::placement::collect_scene_placement_bounds)
        .unwrap();
    for (i, a) in boxes.iter().enumerate() {
        for b in &boxes[i + 1..] {
            let a0 = a.min().coords();
            let a1 = a.max().coords();
            let b0 = b.min().coords();
            let b1 = b.max().coords();
            assert!(a1[0] <= b0[0] || b1[0] <= a0[0] || a1[1] <= b0[1] || b1[1] <= a0[1]);
        }
    }
    assert!(doc.step_back().unwrap());
    assert_eq!(count(&doc), 1);
    assert!(!doc.has_past());
    assert!(doc.step_forward().unwrap());
    assert_eq!(count(&doc), 3);
}
#[test]
fn multi_select_toggle_delete_and_single_undo() {
    let ctrl = controller();
    let doc = ctrl.scene_doc();
    doc.import_geometry(&[cube()], ctrl.printer_volume()).unwrap();
    doc.select_all();
    let ids = doc.chosen().ids();
    assert_eq!(ids.len(), 2);
    doc.toggle_selection(ids[0]);
    assert_eq!(doc.chosen().ids(), vec![ids[1]]);
    doc.toggle_selection(ids[0]);
    assert_eq!(doc.chosen().ids().len(), 2);
    assert!(doc.delete_chosen().unwrap());
    assert_eq!(count(&doc), 0);
    assert_eq!(doc.chosen(), Selection::None);
    assert!(doc.step_back().unwrap());
    assert_eq!(count(&doc), 2);
}
#[test]
fn multi_duplicate_preserves_relative_positions_and_undoes_once() {
    let ctrl = controller();
    let doc = ctrl.scene_doc();
    doc.import_geometry(&[cube()], ctrl.printer_volume()).unwrap();
    doc.select_all();
    let fragment = crate::clipboard::copy_of_selection(&doc).unwrap();
    assert_eq!(fragment.scene().roots().len(), 2);
    let a = fragment
        .scene()
        .world_transform_of(fragment.scene().roots()[0])
        .unwrap();
    let b = fragment
        .scene()
        .world_transform_of(fragment.scene().roots()[1])
        .unwrap();
    let relative = a.inverse().unwrap().compose(&b);
    doc.paste_parts(&fragment, [0.0, 40.0, 10.0]).unwrap();
    assert_eq!(count(&doc), 4);
    let ids = doc.chosen().ids();
    assert_eq!(ids.len(), 2);
    let pasted = doc
        .read_scene(|s| {
            s.graph()
                .world_transform_of(ids[0])
                .unwrap()
                .inverse()
                .unwrap()
                .compose(&s.graph().world_transform_of(ids[1]).unwrap())
        })
        .unwrap();
    for point in [[0.0, 0.0, 0.0], [1.0, 2.0, 3.0]] {
        let vertex = sc_mesh_core::Vertex::new(point);
        let a = relative.apply_to_vertex(vertex).coords();
        let b = pasted.apply_to_vertex(vertex).coords();
        for axis in 0..3 {
            assert!((a[axis] - b[axis]).abs() < 1e-5);
        }
    }
    doc.step_back().unwrap();
    assert_eq!(count(&doc), 2);
}
#[test]
fn menu_delete_all_and_reset_use_history() {
    let ctrl = controller();
    let doc = ctrl.scene_doc();
    doc.import_geometry(&[cube()], ctrl.printer_volume()).unwrap();
    ctrl.context_action(1);
    assert_eq!(doc.chosen().ids().len(), 2);
    let id = doc.main_chosen_node().unwrap();
    let before = doc
        .read_scene(|s| s.graph().node(id).unwrap().local_transform)
        .unwrap();
    ctrl.context_action(20);
    assert_eq!(
        doc.read_scene(|s| s.graph().node(id).unwrap().local_transform)
            .unwrap(),
        Transform3D::Identity
    );
    doc.step_back().unwrap();
    assert_eq!(
        doc.read_scene(|s| s.graph().node(id).unwrap().local_transform)
            .unwrap(),
        before
    );
    ctrl.context_action(6);
    assert_eq!(count(&doc), 0);
    doc.step_back().unwrap();
    assert_eq!(count(&doc), 2);
}
#[test]
fn multi_mirror_and_arrange_are_single_commands() {
    let ctrl = controller();
    let doc = ctrl.scene_doc();
    doc.import_geometry(&[cube()], ctrl.printer_volume()).unwrap();
    doc.select_all();
    let ids = doc.chosen().ids();
    let before = doc.read_scene(|s| s.graph().clone()).unwrap();
    ctrl.mirror_action(0);
    for id in ids {
        assert_ne!(
            before.node(id).unwrap().local_transform,
            doc.read_scene(|s| s.graph().node(id).unwrap().local_transform)
                .unwrap()
        );
    }
    doc.step_back().unwrap();
    ctrl.layout_action(true);
    doc.step_back().unwrap();
    for &id in before.roots() {
        assert_eq!(
            before.node(id).unwrap().local_transform,
            doc.read_scene(|s| s.graph().node(id).unwrap().local_transform)
                .unwrap()
        );
    }
}
#[test]
fn import_into_empty_document_remains_dirty_and_can_undo_to_empty_plate() {
    let ctrl = ViewController::for_test();
    let doc = ctrl.scene_doc();
    doc.import_geometry(&[cube(), cube()], ctrl.printer_volume())
        .unwrap();
    assert!(doc.has_unsaved_changes());
    assert_eq!(count(&doc), 2);
    doc.step_back().unwrap();
    assert_eq!(count(&doc), 0);
}
#[test]
fn failed_import_is_atomic() {
    let ctrl = controller();
    let doc = ctrl.scene_doc();
    let before = doc.read_scene(|s| s.graph().clone()).unwrap();
    let huge = sc_scene_edit::SceneFragment::from_leaf_snapshot(
        Arc::new(sc_mesh_fixtures::unit_cube_mesh()),
        Transform3D::Affine(AffineTransform3D::from_uniform_scale(10000.0)),
    )
    .into_scene();
    assert!(
        doc.import_geometry(&[cube(), huge], ctrl.printer_volume())
            .is_err()
    );
    assert_eq!(count(&doc), 1);
    assert!(!doc.has_past());
    assert_eq!(
        doc.read_scene(|s| s.graph().roots().to_vec()).unwrap(),
        before.roots()
    );
}
#[test]
fn remove_hollow_restores_original_and_undo_restores_shell() {
    let ctrl = controller();
    let doc = ctrl.scene_doc();
    doc.select_all();
    let id = doc.main_chosen_node().unwrap();
    let original = doc
        .read_scene(|s| s.graph().leaf_mesh_arc(id).unwrap().clone())
        .unwrap();
    let hollow = doc
        .read_scene(|s| {
            crate::hollow_tools::edit(s.graph(), id, 2.0, crate::hollow_tools::HoleChange::Keep)
                .unwrap()
        })
        .unwrap();
    doc.commit_edit(hollow.clone()).unwrap();
    doc.record_edit(hollow);
    ctrl.context_action(18);
    assert!(
        doc.read_scene(|s| crate::hollow_tools::wall_thickness(s.graph(), id))
            .flatten()
            .is_none()
    );
    assert_eq!(
        original.vertices,
        doc.read_scene(|s| s.graph().leaf_mesh_arc(id).unwrap().vertices.clone())
            .unwrap()
    );
    doc.step_back().unwrap();
    assert_eq!(
        doc.read_scene(|s| crate::hollow_tools::wall_thickness(s.graph(), id))
            .flatten(),
        Some(2.0)
    );
}
#[test]
fn shared_primary_button_preserves_keyboard_activation_and_theme() {
    let button = include_str!("../ui/primary-button.slint");
    assert!(button.contains("ShellTheme.accent"));
    assert!(button.contains("Key.Return"));
    assert!(button.contains("forward-focus: focus"));
    for dialog in [
        include_str!("../ui/error-dialog.slint"),
        include_str!("../ui/info-dialog.slint"),
        include_str!("../ui/printer-editor.slint"),
    ] {
        assert!(dialog.contains("PrimaryButton"));
        assert!(!dialog.contains("primary: true"));
    }
}

#[test]
fn object_support_apply_leaves_other_objects_and_supports_untouched() {
    let ctrl = controller();
    let doc = ctrl.scene_doc();
    doc.import_geometry(&[cube()], ctrl.printer_volume()).unwrap();
    let selected = doc.main_chosen_node().unwrap();
    let mut generated = doc.read_scene(|s| s.graph().clone()).unwrap();
    let before = generated.clone();
    let old = generated.node(selected).unwrap().local_transform;
    generated
        .set_local_transform(
            selected,
            Transform3D::Affine(AffineTransform3D::from_translation([0., 0., 5.])).compose(&old),
        )
        .unwrap();
    let roots = generated.roots().to_vec();
    doc.apply_support_scene_scoped(&generated, &roots, Some(selected))
        .unwrap();
    for &id in before.roots() {
        if id != selected {
            assert_eq!(
                before.node(id).unwrap().local_transform,
                doc.read_scene(|s| s.graph().node(id).unwrap().local_transform)
                    .unwrap()
            );
        }
    }
    assert_ne!(
        before.node(selected).unwrap().local_transform,
        doc.read_scene(|s| s.graph().node(selected).unwrap().local_transform)
            .unwrap()
    );
    doc.step_back().unwrap();
    assert_eq!(
        before.node(selected).unwrap().local_transform,
        doc.read_scene(|s| s.graph().node(selected).unwrap().local_transform)
            .unwrap()
    );
}

#[test]
fn duplicate_uses_object_position_without_a_viewport_or_mouse_anchor() {
    let ctrl = controller();
    let doc = ctrl.scene_doc();
    doc.select_all();
    let clipboard = crate::clipboard::Clipboard::new(
        crate::clipboard::copy_of_selection(&doc).unwrap(),
    );
    assert!(ctrl.duplicate_clipboard(&clipboard));
    assert_eq!(count(&doc), 2);
    let boxes = doc
        .read_scene(crate::clipboard::placement::collect_scene_placement_bounds)
        .unwrap();
    for a in boxes {
        assert!(ctrl.printer_volume().holds_box(a));
    }
    doc.step_back().unwrap();
    assert_eq!(count(&doc), 1);
}

/// Sleptic: Ctrl+C then Ctrl+V dropped the copy "in a random position,
/// usually way off the build plate"; every paste should take the most
/// compact free spot. Five pastes with no cursor anywhere: all on the
/// plate, none overlapping, packed around the original.
#[test]
fn pasted_copies_land_compactly_on_the_plate() {
    let ctrl = controller();
    let doc = ctrl.scene_doc();
    doc.select_all();
    let clipboard = crate::clipboard::Clipboard::new(
        crate::clipboard::copy_of_selection(&doc).unwrap(),
    )
    .with_source(crate::clipboard::selection_anchor(&doc));
    for _ in 0..5 {
        assert!(ctrl.paste_at_cursor(&clipboard));
    }
    assert_eq!(count(&doc), 6);
    let boxes = doc
        .read_scene(crate::clipboard::placement::collect_scene_placement_bounds)
        .unwrap();
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for (i, a) in boxes.iter().enumerate() {
        assert!(ctrl.printer_volume().holds_box(*a), "copy {i} off the plate");
        for b in &boxes[i + 1..] {
            let apart = (0..2).any(|k| {
                a.max().coords()[k] <= b.min().coords()[k] || b.max().coords()[k] <= a.min().coords()[k]
            });
            assert!(apart, "copies overlap");
        }
        for k in 0..2 {
            lo[k] = lo[k].min(a.min().coords()[k]);
            hi[k] = hi[k].max(a.max().coords()[k]);
        }
    }
    // Six 20 mm cubes 3 mm apart fit a 3 x 2 block (66 x 43 mm).
    assert!(hi[0] - lo[0] <= 66.01 && hi[1] - lo[1] <= 66.01, "{lo:?} {hi:?}");
    // One undo per paste.
    doc.step_back().unwrap();
    assert_eq!(count(&doc), 5);
}

/// Sleptic: the Scale tool gets Orca's Scale to fit.
#[test]
fn scale_to_fit_fills_the_build_volume_centred_on_the_plate() {
    let ctrl = controller();
    let doc = ctrl.scene_doc();
    doc.select_all();
    ctrl.scale_to_fit_action();
    let volume = ctrl.printer_volume();
    let a = doc
        .read_scene(crate::clipboard::placement::collect_scene_placement_bounds)
        .unwrap()[0];
    let (min, max) = (a.min().coords(), a.max().coords());
    let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    // A cube is held by the volume's smallest side, at 98 %.
    let side = volume
        .width_mm()
        .min(volume.depth_mm())
        .min(volume.height_mm())
        * 0.98;
    for s in size {
        assert!((s - side).abs() < 0.05, "{size:?} vs {side}");
    }
    assert!(min[2].abs() < 1e-3, "rests on the plate: {min:?}");
    assert!((min[0] + max[0]).abs() < 1e-3 && (min[1] + max[1]).abs() < 1e-3);
    assert!(volume.holds_box(a));
    doc.step_back().unwrap();
    let b = doc
        .read_scene(crate::clipboard::placement::collect_scene_placement_bounds)
        .unwrap()[0];
    assert!((b.max().coords()[0] - b.min().coords()[0] - 20.0).abs() < 1e-3);
}

#[test]
fn plate_lock_skips_arrange_orient_fill_and_unlock_restores_edits() {
    let ctrl = controller();
    let doc = ctrl.scene_doc();
    doc.select_all();
    let before = doc.read_scene(|s| s.graph().clone()).unwrap();
    ctrl.set_plate_locked(true);
    ctrl.layout_action(true);
    ctrl.auto_orient_action();
    ctrl.context_action(3);
    ctrl.fill_plate_action();
    // The lock itself is now a persisted, undoable plate edit.
    assert!(doc.has_past());
    assert!(doc.step_back().unwrap());
    assert!(!doc.has_past());
    assert!(doc.step_forward().unwrap());
    assert_eq!(count(&doc), 1);
    for &id in before.roots() {
        assert_eq!(
            before.node(id).unwrap().local_transform,
            doc.read_scene(|s| s.graph().node(id).unwrap().local_transform)
                .unwrap()
        );
    }
    ctrl.set_plate_locked(false);
    ctrl.fill_plate_action();
    assert!(count(&doc) > 1);
    assert!(doc.step_back().unwrap());
    assert_eq!(count(&doc), 1);
}

/// Fill plate re-orients the part first (FILL_PLATE_WEIGHTS) and copies it in
/// the same step: one undo must restore both the original pose and count.
#[test]
fn fill_plate_reorientation_and_copies_undo_as_one_step() {
    let ctrl = controller();
    let doc = ctrl.scene_doc();
    doc.select_all();
    let id = doc.main_chosen_node().unwrap();
    let before = doc
        .read_scene(|s| s.graph().node(id).unwrap().local_transform)
        .unwrap();
    let turn = doc
        .read_scene(|s| {
            crate::layout_tools::world_edit(
                s.graph(),
                id,
                Transform3D::Affine(AffineTransform3D::from_rotation_x(0.5)),
            )
        })
        .flatten()
        .unwrap();
    let added = doc
        .fill_plate_after(Some(turn), ctrl.printer_volume(), 3.0)
        .unwrap();
    assert!(added > 0);
    assert_ne!(
        before,
        doc.read_scene(|s| s.graph().node(id).unwrap().local_transform)
            .unwrap()
    );
    assert!(doc.step_back().unwrap());
    assert!(!doc.has_past());
    assert_eq!(count(&doc), 1);
    assert_eq!(
        before,
        doc.read_scene(|s| s.graph().node(id).unwrap().local_transform)
            .unwrap()
    );
}

/// With nothing selected no fill can be planned, so a supplied
/// re-orientation must not be left behind either.
#[test]
fn fill_plate_rolls_back_the_reorientation_when_it_cannot_fill() {
    let ctrl = controller();
    let doc = ctrl.scene_doc();
    doc.select_all();
    let id = doc.main_chosen_node().unwrap();
    let before = doc
        .read_scene(|s| s.graph().node(id).unwrap().local_transform)
        .unwrap();
    let turn = doc
        .read_scene(|s| {
            crate::layout_tools::world_edit(
                s.graph(),
                id,
                Transform3D::Affine(AffineTransform3D::from_rotation_x(0.5)),
            )
        })
        .flatten()
        .unwrap();
    doc.choose(Selection::None);
    assert_eq!(
        doc.fill_plate_after(Some(turn), ctrl.printer_volume(), 3.0),
        None
    );
    assert!(!doc.has_past());
    assert_eq!(
        before,
        doc.read_scene(|s| s.graph().node(id).unwrap().local_transform)
            .unwrap()
    );
}

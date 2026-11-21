// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
use glam::Vec3;
use sc_scene::{Scene, node::LeafPayload};
use std::sync::Arc;

fn fixture() -> (ViewController, ScreenPoint) {
    let ctrl = ViewController::for_test();
    let mut graph = Scene::new();
    let id = graph
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_translation([0.0, 0.0, 10.0])).compose(
                &Transform3D::Affine(AffineTransform3D::from_uniform_scale(20.0)),
            ),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    graph.add_root(id).unwrap();
    ctrl.put_graph_for_test(graph);
    let mut camera = crate::camera::ViewCamera::new(Vec3::ZERO, 100.0);
    camera.snap_to_axis(ViewAxis::Top);
    let (x, y) = picking::screen_point_of(
        &camera,
        crate::mesh_render::starting_fov_y(),
        800.0,
        600.0,
        Vec3::new(0.0, 0.0, 20.0),
    )
    .unwrap();
    *ctrl.orbit.view_cell().borrow_mut() = Some(camera);
    ctrl.put_viewport_for_test(800.0, 600.0);
    (ctrl, ScreenPoint::new(x, y))
}

#[test]
fn object_drag_updates_live_and_commits_exactly_one_edit() {
    let (ctrl, press) = fixture();
    assert!(ctrl.on_pointer_down(press, PointerModifiers::default()));
    let id = ctrl.scene_doc().read_scene(|s| s.leaf_ids()[0]).unwrap();
    let original = ctrl
        .scene_doc()
        .read_scene(|s| s.leaf(id).unwrap().local_transform())
        .unwrap();
    for dx in [10.0, 20.0, 40.0] {
        ctrl.on_pointer_move(
            ScreenPoint::new(press.x + dx, press.y),
            PointerModifiers::default(),
        );
        assert!(!ctrl.scene_doc().has_past());
    }
    let moved = ctrl
        .scene_doc()
        .read_scene(|s| s.leaf(id).unwrap().local_transform())
        .unwrap();
    assert_ne!(moved, original);
    ctrl.on_pointer_up(
        ScreenPoint::new(press.x + 40.0, press.y),
        PointerModifiers::default(),
    );
    assert!(ctrl.scene_doc().step_back().unwrap());
    assert!(!ctrl.scene_doc().has_past());
    assert_eq!(
        ctrl.scene_doc()
            .read_scene(|s| s.leaf(id).unwrap().local_transform())
            .unwrap(),
        original
    );
    assert!(ctrl.scene_doc().step_forward().unwrap());
    assert_eq!(
        ctrl.scene_doc()
            .read_scene(|s| s.leaf(id).unwrap().local_transform())
            .unwrap(),
        moved
    );
}

#[test]
fn click_jitter_locked_plate_and_empty_press_do_not_edit() {
    let (ctrl, press) = fixture();
    assert!(!ctrl.on_pointer_down(ScreenPoint::new(0.0, 0.0), PointerModifiers::default()));
    ctrl.on_pointer_up(ScreenPoint::new(0.0, 0.0), PointerModifiers::default());
    assert!(ctrl.on_pointer_down(press, PointerModifiers::default()));
    ctrl.on_pointer_move(
        ScreenPoint::new(press.x + 2.0, press.y),
        PointerModifiers::default(),
    );
    ctrl.on_pointer_up(
        ScreenPoint::new(press.x + 2.0, press.y),
        PointerModifiers::default(),
    );
    assert!(!ctrl.scene_doc().has_past());
    ctrl.set_plate_locked(true);
    assert!(!ctrl.on_pointer_down(press, PointerModifiers::default()));
    ctrl.on_pointer_move(
        ScreenPoint::new(press.x + 40.0, press.y),
        PointerModifiers::default(),
    );
    ctrl.on_pointer_up(
        ScreenPoint::new(press.x + 40.0, press.y),
        PointerModifiers::default(),
    );
    assert!(ctrl.scene_doc().step_back().unwrap());
    assert!(!ctrl.scene_doc().has_past()); // only the lock was recorded
}

#[test]
fn locking_during_drag_reverts_without_undo() {
    let (ctrl, press) = fixture();
    ctrl.on_pointer_down(press, PointerModifiers::default());
    let id = ctrl.scene_doc().read_scene(|s| s.leaf_ids()[0]).unwrap();
    let before = ctrl
        .scene_doc()
        .read_scene(|s| s.leaf(id).unwrap().local_transform())
        .unwrap();
    ctrl.on_pointer_move(
        ScreenPoint::new(press.x + 40.0, press.y),
        PointerModifiers::default(),
    );
    ctrl.set_plate_locked(true);
    assert_eq!(
        ctrl.scene_doc()
            .read_scene(|s| s.leaf(id).unwrap().local_transform())
            .unwrap(),
        before
    );
    assert!(ctrl.scene_doc().step_back().unwrap());
    assert!(!ctrl.scene_doc().has_past()); // only the lock was recorded
}

/// Two cubes side by side; returns the controller and both cubes' top-face screen points.
fn two_object_fixture() -> (ViewController, ScreenPoint, ScreenPoint) {
    let ctrl = ViewController::for_test();
    let mut graph = Scene::new();
    for x in [0.0, 60.0] {
        let id = graph
            .add_leaf(
                None,
                Transform3D::Affine(AffineTransform3D::from_translation([x, 0.0, 10.0])).compose(
                    &Transform3D::Affine(AffineTransform3D::from_uniform_scale(20.0)),
                ),
                LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
            )
            .unwrap();
        graph.add_root(id).unwrap();
    }
    ctrl.put_graph_for_test(graph);
    let mut camera = crate::camera::ViewCamera::new(Vec3::new(30.0, 0.0, 0.0), 160.0);
    camera.snap_to_axis(ViewAxis::Top);
    let project = |x: f32| {
        let (sx, sy) = picking::screen_point_of(
            &camera,
            crate::mesh_render::starting_fov_y(),
            800.0,
            600.0,
            Vec3::new(x, 0.0, 20.0),
        )
        .unwrap();
        ScreenPoint::new(sx, sy)
    };
    let (a, b) = (project(0.0), project(60.0));
    *ctrl.orbit.view_cell().borrow_mut() = Some(camera);
    ctrl.put_viewport_for_test(800.0, 600.0);
    (ctrl, a, b)
}

fn leaf(ctrl: &ViewController, index: usize) -> sc_scene::NodeId {
    ctrl.scene_doc().read_scene(|s| s.leaf_ids()[index]).unwrap()
}

// Regression: a press on an object armed a plate move that set the selection
// and swallowed the release, so plain clicks never reached the pick path (no
// Ctrl+click toggling, no-op undo entries). Visible once a plate holds several
// objects, e.g. after Fill plate.
#[test]
fn click_on_a_second_object_selects_it_without_an_undo_entry() {
    let (ctrl, a, b) = two_object_fixture();
    let (first, second) = (leaf(&ctrl, 0), leaf(&ctrl, 1));
    for (point, expected) in [(a, first), (b, second), (a, first)] {
        ctrl.on_pointer_down(point, PointerModifiers::default());
        ctrl.on_pointer_up(point, PointerModifiers::default());
        assert_eq!(ctrl.scene_doc().main_chosen_node(), Some(expected));
    }
    assert!(!ctrl.scene_doc().has_past());
}

#[test]
fn press_alone_does_not_change_selection_but_a_real_drag_selects_the_object() {
    let (ctrl, a, b) = two_object_fixture();
    let (first, second) = (leaf(&ctrl, 0), leaf(&ctrl, 1));
    ctrl.scene_doc().choose(Selection::One(LeafId(first)));
    ctrl.on_pointer_down(b, PointerModifiers::default());
    assert_eq!(ctrl.scene_doc().main_chosen_node(), Some(first));
    ctrl.on_pointer_move(
        ScreenPoint::new(b.x + 30.0, b.y),
        PointerModifiers::default(),
    );
    assert_eq!(ctrl.scene_doc().main_chosen_node(), Some(second));
    ctrl.on_pointer_up(
        ScreenPoint::new(b.x + 30.0, b.y),
        PointerModifiers::default(),
    );
    assert!(ctrl.scene_doc().has_past());
    let _ = a;
}

#[test]
fn ctrl_click_on_objects_builds_a_multi_selection() {
    let (ctrl, a, b) = two_object_fixture();
    let ctrl_mods = PointerModifiers {
        control: true,
        ..PointerModifiers::default()
    };
    ctrl.on_pointer_down(a, PointerModifiers::default());
    ctrl.on_pointer_up(a, PointerModifiers::default());
    ctrl.on_pointer_down(b, ctrl_mods.clone());
    ctrl.on_pointer_up(b, ctrl_mods);
    let mut ids = ctrl.scene_doc().chosen().ids();
    ids.sort();
    let mut expected = vec![leaf(&ctrl, 0), leaf(&ctrl, 1)];
    expected.sort();
    assert_eq!(ids, expected);
}

#[test]
fn dragging_one_object_of_a_multi_selection_moves_the_group_in_one_undo_step() {
    let (ctrl, a, b) = two_object_fixture();
    let (first, second) = (leaf(&ctrl, 0), leaf(&ctrl, 1));
    ctrl.scene_doc()
        .choose(Selection::Many(vec![LeafId(first), LeafId(second)]));
    let local = |id| {
        ctrl.scene_doc()
            .read_scene(|s| s.leaf(id).unwrap().local_transform())
            .unwrap()
    };
    let x = |t: Transform3D| match t {
        Transform3D::Affine(a) => a.translation()[0],
        Transform3D::Identity => 0.0,
    };
    let (a0, b0) = (local(first), local(second));
    ctrl.on_pointer_down(b, PointerModifiers::default());
    ctrl.on_pointer_move(
        ScreenPoint::new(b.x + 40.0, b.y),
        PointerModifiers::default(),
    );
    ctrl.on_pointer_up(
        ScreenPoint::new(b.x + 40.0, b.y),
        PointerModifiers::default(),
    );
    let (da, db) = (x(local(first)) - x(a0), x(local(second)) - x(b0));
    assert!(db > 1.0, "lead moved {db}");
    assert!((da - db).abs() < 1e-6, "companion moved {da}, lead {db}");
    assert_eq!(ctrl.scene_doc().chosen().ids().len(), 2);
    assert!(ctrl.scene_doc().step_back().unwrap());
    assert_eq!(local(first), a0);
    assert_eq!(local(second), b0);
    assert!(!ctrl.scene_doc().has_past());
    let _ = a;
}

// Regression: pasting with plate 2 active used the cursor's world
// position as a plate-local one, dropping the copy far off plate 2.
#[test]
fn paste_at_cursor_on_plate_two_lands_on_plate_two() {
    let (ctrl, _) = fixture();
    ctrl.scene_doc().add_plate();
    ctrl.scene_doc().set_active_plate(2);
    let volume = ctrl.printer_volume();
    let [ox, oy] = sc_scene::plate_grid_offset(1, 2, volume.width_mm(), volume.depth_mm());
    let (x, y) = {
        let camera = ctrl.orbit.view_cell().borrow();
        picking::screen_point_of(
            camera.as_ref().unwrap(),
            crate::mesh_render::starting_fov_y(),
            800.0,
            600.0,
            Vec3::new(ox, oy, 0.0),
        )
        .unwrap()
    };
    let clipboard = Clipboard::new(sc_scene_edit::SceneFragment::from_leaf_snapshot(
        Arc::new(sc_mesh_fixtures::unit_cube_mesh()),
        Transform3D::Identity,
    ));
    assert!(ctrl.paste_at_pixel(&clipboard, x, y));
    let pasted = ctrl
        .scene_doc()
        .read_scene(|s| *s.leaf_ids().last().unwrap())
        .unwrap();
    let (plate, centre) = ctrl
        .scene_doc()
        .read_scene(|s| {
            let q = s.leaf(pasted).unwrap();
            let b = q.transformed_aabb(q.world_transform()).unwrap();
            (
                s.graph().plate_of(pasted),
                (b.min().coords()[0] + b.max().coords()[0]) * 0.5,
            )
        })
        .unwrap();
    assert_eq!(plate, 2);
    assert!(
        centre.abs() < 0.5 * volume.width_mm(),
        "plate-local x {centre}"
    );
}

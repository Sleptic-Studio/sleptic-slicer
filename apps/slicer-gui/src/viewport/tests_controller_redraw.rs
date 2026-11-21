// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The view only repaints when a frame is owed. Every camera gesture and
//! arrow key that changes what is seen must owe one, and one that changes
//! nothing must not (or the window would repaint for nothing).

use std::sync::Arc;

use sc_mesh_core::transform::Transform3D;
use sc_scene::{LeafPayload, Scene};

use super::super::arrow_keys::KEY_ARROW_POSITIVE_X;
use super::*;
use crate::camera::ViewCamera;

/// A controller with one cube selected, a camera and a 400 x 300 view.
fn viewing_a_cube() -> ViewController {
    let ctrl = ViewController::for_test();
    let mut scene = Scene::new();
    let root = scene.add_group(None, Transform3D::Identity);
    scene.add_root(root).expect("root group");
    let cube = LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh()));
    let leaf = scene.add_leaf(None, Transform3D::Identity, cube).expect("cube leaf");
    scene.add_child(root, leaf).expect("cube under the root");
    ctrl.put_graph_for_test(scene);
    *ctrl.orbit.view_cell().borrow_mut() = Some(ViewCamera::new(glam::Vec3::ZERO, 2.0));
    ctrl.put_viewport_for_test(400.0, 300.0);
    ctrl.on_select_by_index(0);
    ctrl
}

/// Runs `gesture` on a clean frame and says whether it now owes one.
fn owes_after(ctrl: &ViewController, gesture: impl FnOnce(&ViewController)) -> bool {
    ctrl.forget_frame_for_test();
    assert!(!ctrl.redraw_due_for_test(), "the frame starts clean");
    gesture(ctrl);
    ctrl.redraw_due_for_test()
}

#[test]
fn every_camera_gesture_owes_a_frame() {
    let ctrl = viewing_a_cube();
    let centre = ScreenPoint::new(200.0, 150.0);
    let gestures: [(&str, &dyn Fn(&ViewController)); 5] = [
        ("orbit", &|c| c.orbit_by(ScreenDelta::new(30.0, 10.0))),
        ("pan", &|c| c.pan_by(ScreenDelta::new(12.0, -8.0))),
        ("zoom", &|c| c.zoom_by(120.0, centre)),
        ("frame", &|c| c.frame_models()),
        ("axis view", &|c| c.look_along_axis(crate::AxisView::Left)),
    ];
    for (name, gesture) in gestures {
        assert!(owes_after(&ctrl, gesture), "{name} must owe a frame");
    }
}

#[test]
fn camera_gestures_without_a_camera_owe_nothing() {
    let ctrl = ViewController::for_test();
    ctrl.put_viewport_for_test(400.0, 300.0);
    assert!(!owes_after(&ctrl, |c| c.orbit_by(ScreenDelta::new(30.0, 10.0))));
    assert!(!owes_after(&ctrl, |c| c.pan_by(ScreenDelta::new(12.0, -8.0))));
    assert!(!owes_after(&ctrl, |c| c.zoom_by(120.0, ScreenPoint::new(1.0, 1.0))));
}

#[test]
fn an_arrow_key_owes_a_frame_only_while_it_drives_the_gizmo() {
    let ctrl = viewing_a_cube();
    assert!(
        !owes_after(&ctrl, |c| c.gizmo_key_down(KEY_ARROW_POSITIVE_X)),
        "without keyboard focus the arrow does nothing"
    );
    ctrl.gizmo_key_up(KEY_ARROW_POSITIVE_X);
    ctrl.keys_move();
    assert!(owes_after(&ctrl, |c| c.gizmo_key_down(KEY_ARROW_POSITIVE_X)));
    assert!(owes_after(&ctrl, |c| c.gizmo_key_up(KEY_ARROW_POSITIVE_X)));
}

#[test]
fn view_cube_faces_reach_the_camera_unchanged() {
    use crate::AxisView as Face;
    let pairs = [
        (Face::Top, ViewAxis::Top),
        (Face::Bottom, ViewAxis::Bottom),
        (Face::Front, ViewAxis::Front),
        (Face::Back, ViewAxis::Back),
        (Face::Left, ViewAxis::Left),
        (Face::Right, ViewAxis::Right),
    ];
    for (face, expected) in pairs {
        let got = ViewAxis::from(face);
        assert_eq!(format!("{got:?}"), format!("{expected:?}"));
    }
}

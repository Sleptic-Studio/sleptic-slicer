// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;

fn at(x: f32, y: f32) -> ScreenPoint {
    ScreenPoint::new(x, y)
}

#[test]
fn with_nothing_loaded_there_is_nothing_to_focus_nudge_or_pick() {
    let input = ViewportInputRouter::new();
    let empty = SceneDocument::new();
    let no_camera = RefCell::new(None);
    assert_eq!(input.viewport_keyboard_motion().keyboard_transform_focus(), KeyboardTransformFocus::NoTransformFocus);
    assert!(
        input
            .change_viewport_keyboard_focus(&empty, KeyboardTransformFocus::MoveFocus)
            .is_none(),
        "no part to focus"
    );
    assert!(!input.press_viewport_gizmo_arrow(&empty, 0), "no part to nudge");
    for viewport in [None, ViewportSize::new(1920.0, 1080.0)] {
        assert!(
            input
                .pick_viewport_position(&empty, &no_camera, viewport, at(100.0, 100.0))
                .is_none()
        );
    }
    input.clear_viewport_input();
    assert_eq!(input.viewport_keyboard_motion().keyboard_transform_focus(), KeyboardTransformFocus::NoTransformFocus);
}

/// Sleptic: "any object should be easy to select with a simple click".
/// A thin pin a few pixels wide is selected by a click just beside it
/// that the exact ray misses; a click well away still selects nothing.
#[test]
fn click_just_beside_a_thin_part_still_selects_it() {
    use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
    let mut scene = sc_scene::Scene::new();
    let pin = sc_scene::bake_mesh_to_world(
        &sc_mesh_fixtures::unit_cube_mesh(),
        &Transform3D::Affine(AffineTransform3D::from_non_uniform_scale([0.3, 0.3, 30.0])),
    )
    .unwrap();
    let leaf = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_translation([0.0, 0.0, 15.0])),
            sc_scene::node::LeafPayload::Mesh(std::sync::Arc::new(pin)),
        )
        .unwrap();
    scene.add_root(leaf).unwrap();
    let doc = SceneDocument::new();
    doc.take_mesh_scene(scene, "pin.stl".into());
    let camera = ViewCamera::new(glam::Vec3::new(0.0, 0.0, 15.0), 120.0);
    let (w, h) = (1280.0, 800.0);
    let vp = ViewportSize::new(w, h);
    let (cx, cy) = picking::screen_point_of(
        &camera,
        starting_fov_y(),
        w,
        h,
        glam::Vec3::new(0.0, 0.0, 15.0),
    )
    .unwrap();
    let camera = RefCell::new(Some(camera));
    let dispatch = ViewportInputRouter::new();
    let at = |x: f32, y: f32| dispatch.pick_viewport_position(&doc, &camera, vp, ScreenPoint::new(x, y));
    assert_eq!(at(cx, cy), Some(Some(leaf)));
    // 6 px to the side: outside the pin, so the exact ray misses ...
    let exact = doc
        .read_scene(|spatial| {
            let ray = picking::ray_through_pixel(
                camera.borrow().as_ref().unwrap(),
                starting_fov_y(),
                w,
                h,
                cx + 6.0,
                cy,
            );
            picking::part_under_ray_on_plates(
                &ray,
                spatial,
                Some((doc.plate_width.get(), doc.plate_depth.get())),
            )
        })
        .unwrap();
    assert_eq!(exact, None, "fixture: the pin must be narrower than 6 px");
    // ... but the click still selects it.
    assert_eq!(at(cx + 6.0, cy), Some(Some(leaf)));
    assert_eq!(at(cx + 40.0, cy), Some(None));
}

#[test]
fn the_hover_point_is_kept_until_the_pointer_leaves_or_input_resets() {
    let input = ViewportInputRouter::new();
    assert_eq!(input.last_viewport_hover_position(), None);
    for (point, end) in [(at(12.0, 34.0), 0), (at(7.0, 9.0), 1)] {
        input.record_viewport_hover_position(point);
        assert_eq!(input.last_viewport_hover_position(), Some(point));
        if end == 0 {
            input.leave_viewport_pointer();
        } else {
            input.clear_viewport_input();
        }
        assert_eq!(input.last_viewport_hover_position(), None);
    }
}

#[test]
fn a_right_click_answers_once_with_where_it_went_down() {
    let input = ViewportInputRouter::new();
    let down = at(20.0, 30.0);
    assert_eq!(input.finish_context_pointer_press(down), None, "no press");
    input.begin_context_pointer_press(down);
    assert_eq!(input.finish_context_pointer_press(down), Some(down));
    assert_eq!(input.finish_context_pointer_press(down), None, "used up");
    // Released far away: a drag, not a click.
    input.begin_context_pointer_press(down);
    assert_eq!(input.finish_context_pointer_press(at(120.0, 30.0)), None);
}

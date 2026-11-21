// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

use super::*;
use float_cmp::assert_approx_eq;

fn step(focus: KeyboardTransformFocus, keys: HeldArrowDirections) -> Option<Transform3D> {
    compute_keyboard_transform(
        focus,
        keys,
        1.0,
        Vec3::ZERO,
        &Transform3D::Identity,
        &Transform3D::Identity,
        0.1,
    )
}

#[test]
fn opposing_keys_cancel_and_axes_are_independent() {
    let keys = HeldArrowDirections {
        positive_x_held: true,
        negative_x_held: true,
        positive_y_held: true,
        negative_z_held: true,
        ..Default::default()
    };
    assert_eq!(keys.direction_signs(), (0.0, 1.0, -1.0));
    assert_eq!(
        HeldArrowDirections {
            positive_x_held: true,
            negative_x_held: true,
            ..Default::default()
        }
        .direction_signs(),
        (0.0, 0.0, 0.0)
    );
    assert_eq!(
        HeldArrowDirections {
            positive_y_held: true,
            ..Default::default()
        }
        .direction_signs(),
        (0.0, 1.0, 0.0)
    );
    assert_eq!(
        HeldArrowDirections {
            negative_x_held: true,
            positive_z_held: true,
            ..Default::default()
        }
        .direction_signs(),
        (-1.0, 0.0, 1.0)
    );
    assert!(keys.has_held_arrow());
    assert!(!HeldArrowDirections::default().has_held_arrow());
}

#[test]
fn inactive_or_cancelled_input_does_not_move() {
    let positive = HeldArrowDirections {
        positive_x_held: true,
        ..Default::default()
    };
    let cancelled = HeldArrowDirections {
        positive_x_held: true,
        negative_x_held: true,
        ..Default::default()
    };
    assert!(step(KeyboardTransformFocus::NoTransformFocus, positive).is_none());
    assert!(step(KeyboardTransformFocus::MoveFocus, HeldArrowDirections::default()).is_none());
    assert!(step(KeyboardTransformFocus::MoveFocus, cancelled).is_none());
}

#[test]
fn translation_uses_screen_speed() {
    let keys = HeldArrowDirections {
        positive_x_held: true,
        ..Default::default()
    };
    let Transform3D::Affine(a) = step(KeyboardTransformFocus::MoveFocus, keys).unwrap() else {
        panic!("expected affine transform")
    };
    for (actual, expected) in a.translation().iter().zip([3.0, 0.0, 0.0]) {
        assert_approx_eq!(f64, *actual, expected, epsilon = 1e-6);
    }
    assert_approx_eq!(f64, a.linear()[0][0], 1.0, epsilon = 1e-6);
}

#[test]
fn rotation_maps_y_towards_z() {
    let keys = HeldArrowDirections {
        positive_x_held: true,
        ..Default::default()
    };
    let Transform3D::Affine(a) = step(KeyboardTransformFocus::RotationFocus, keys).unwrap() else {
        panic!("expected affine transform")
    };
    for (actual, expected) in [a.linear()[0][1], a.linear()[1][1], a.linear()[2][1]]
        .into_iter()
        .zip([
            0.0,
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
        ])
    {
        assert_approx_eq!(f64, actual, expected, epsilon = 1e-6);
    }
}

#[test]
fn scale_direction_selects_factor_or_reciprocal() {
    for (keys, expected) in [
        (
            HeldArrowDirections {
                positive_x_held: true,
                ..Default::default()
            },
            1.5,
        ),
        (
            HeldArrowDirections {
                negative_x_held: true,
                ..Default::default()
            },
            1.0 / 1.5,
        ),
    ] {
        let Transform3D::Affine(a) = step(KeyboardTransformFocus::ScalingFocus, keys).unwrap() else {
            panic!("expected affine transform")
        };
        for (actual, expected) in [a.linear()[0][0], a.linear()[1][1], a.linear()[2][2]]
            .into_iter()
            .zip([expected, 1.0, 1.0])
        {
            assert_approx_eq!(f64, actual, expected, epsilon = 1e-5);
        }
    }
}

#[test]
fn focus_and_arrow_state_reset_at_mode_boundaries() {
    let document = SceneDocument::new();
    let state = KeyboardMotionState::new();
    assert!(!state.set_keyboard_transform_focus(&document, KeyboardTransformFocus::MoveFocus, false));
    assert!(state.set_keyboard_transform_focus(&document, KeyboardTransformFocus::MoveFocus, true));
    assert!(!state.set_keyboard_transform_focus(&document, KeyboardTransformFocus::MoveFocus, true));
    for code in [
        KEY_ARROW_POSITIVE_X,
        KEY_ARROW_NEGATIVE_X,
        KEY_ARROW_POSITIVE_Y,
        KEY_ARROW_NEGATIVE_Y,
        KEY_ARROW_POSITIVE_Z,
        KEY_ARROW_NEGATIVE_Z,
    ] {
        assert!(state.update_held_arrow(&document, None, code, true));
    }
    assert_eq!(state.held_keyboard_arrows().direction_signs(), (0.0, 0.0, 0.0));
    assert!(state.held_keyboard_arrows().has_held_arrow());
    assert!(!state.update_held_arrow(&document, None, 99, false));
    assert!(state.set_keyboard_transform_focus(&document, KeyboardTransformFocus::RotationFocus, true));
    assert!(!state.held_keyboard_arrows().has_held_arrow());
    assert!(state.update_held_arrow(&document, None, KEY_ARROW_NEGATIVE_Z, true));
    assert_eq!(state.held_keyboard_arrows().direction_signs(), (0.0, 0.0, -1.0));
    state.reset_keyboard_motion();
    assert_eq!(state.keyboard_transform_focus(), KeyboardTransformFocus::NoTransformFocus);
    assert!(!state.held_keyboard_arrows().has_held_arrow());
}

#[test]
fn arrow_updates_accept_repeats_and_ignore_invalid_codes() {
    let document = SceneDocument::new();
    let state = KeyboardMotionState::new();
    for code in [
        KEY_ARROW_POSITIVE_X,
        KEY_ARROW_NEGATIVE_X,
        KEY_ARROW_POSITIVE_Y,
        KEY_ARROW_NEGATIVE_Y,
        KEY_ARROW_POSITIVE_Z,
        KEY_ARROW_NEGATIVE_Z,
    ] {
        assert!(state.update_held_arrow(&document, None, code, true));
        assert!(state.update_held_arrow(&document, None, code, true));
        assert!(state.held_keyboard_arrows().has_held_arrow());
        assert!(!state.update_held_arrow(&document, None, -1, false));
        assert!(state.held_keyboard_arrows().has_held_arrow());
        assert!(state.update_held_arrow(&document, None, code, false));
        assert!(state.update_held_arrow(&document, None, code, false));
        assert!(!state.held_keyboard_arrows().has_held_arrow());
    }
}

#[test]
fn integration_waits_for_input_and_viewport() {
    let document = SceneDocument::new();
    let camera = RefCell::new(None);
    let state = KeyboardMotionState::new();
    let context = || KeyboardIntegrationContext {
        keyboard_document: &document,
        keyboard_camera: &camera,
        keyboard_viewport: None,
        keyboard_selection_id: None,
        keyboard_build_volume: BuildVolume::new(100.0, 100.0, 100.0),
    };
    assert!(!state.integrate_keyboard_motion(context()));
    assert!(state.set_keyboard_transform_focus(&document, KeyboardTransformFocus::MoveFocus, true));
    assert!(!state.integrate_keyboard_motion(context()));
    assert!(state.update_held_arrow(&document, None, KEY_ARROW_POSITIVE_X, true));
    assert!(!state.integrate_keyboard_motion(context()));
    assert!(state.held_keyboard_arrows().has_held_arrow());
    assert!(state.update_held_arrow(&document, None, KEY_ARROW_POSITIVE_X, false));
    assert!(!state.integrate_keyboard_motion(context()));
}

#[test]
fn held_keys_share_one_undo_until_last_release() {
    use std::sync::Arc;
    let mut scene = sc_scene::Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Identity,
            sc_scene::LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let document = SceneDocument::new();
    document.take_mesh_scene(scene, "cube.stl".into());
    let state = KeyboardMotionState::new();
    assert!(state.set_keyboard_transform_focus(&document, KeyboardTransformFocus::MoveFocus, true));
    assert!(state.update_held_arrow(&document, Some(id), KEY_ARROW_POSITIVE_X, true));
    assert!(state.update_held_arrow(&document, Some(id), KEY_ARROW_POSITIVE_Y, true));
    let moved = Transform3D::Affine(AffineTransform3D::from_translation([2.0, 0.0, 0.0]));
    document
        .commit_edit(SceneEdit::SetLocalTransform {
            node_id: id,
            before: Transform3D::Identity,
            after: moved,
        })
        .unwrap();
    assert!(state.update_held_arrow(&document, Some(id), KEY_ARROW_POSITIVE_X, false));
    assert!(!document.has_past());
    assert!(state.update_held_arrow(&document, Some(id), KEY_ARROW_POSITIVE_Y, false));
    assert!(document.step_back().unwrap());
    assert_eq!(
        document.read_scene(|s| s.leaf(id).unwrap().local_transform()),
        Some(Transform3D::Identity)
    );
    assert!(!document.has_past());
}

#[test]
fn every_held_key_combination_has_independent_signs() {
    for mask in 0u8..64 {
        let held = |bit| mask & (1u8 << bit) != 0u8;
        let keys = HeldArrowDirections {
            positive_x_held: held(0),
            negative_x_held: held(1),
            positive_y_held: held(2),
            negative_y_held: held(3),
            positive_z_held: held(4),
            negative_z_held: held(5),
        };
        let expected = |positive, negative| (i32::from(positive) - i32::from(negative)) as f32;
        assert_eq!(
            keys.direction_signs(),
            (
                expected(held(0), held(1)),
                expected(held(2), held(3)),
                expected(held(4), held(5)),
            ),
            "mask {mask}"
        );
        assert_eq!(keys.has_held_arrow(), mask != 0, "mask {mask}");
    }
}

#[test]
fn a_long_stall_moves_by_one_capped_step_at_most() {
    let drive = KeyboardMotionState::new();
    // The first tick has no earlier one: nothing has elapsed.
    assert_eq!(drive.tick_delta(), 0.0);
    // Two seconds without a frame (a hitch) still move one capped step,
    // so the part does not jump across the plate.
    let stalled = Instant::now() - std::time::Duration::from_secs(2);
    drive.last_keyboard_tick.set(Some(stalled));
    assert_eq!(drive.tick_delta(), KEYBOARD_MAX_FRAME_SECONDS);
    assert!(drive.tick_delta() < KEYBOARD_MAX_FRAME_SECONDS, "the clock restarted");
}

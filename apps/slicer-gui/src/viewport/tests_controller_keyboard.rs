// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The arrow keys and the keyboard gizmo, driven through the controller:
//! which tool the keys drive, how a run of held keys becomes one undo
//! step, Escape's order, and the build volume's walls.
use std::sync::Arc;

use glam::Vec3;
use sc_mesh_core::IndexedMesh;
use sc_mesh_core::primitives::Vertex;
use sc_mesh_core::transform::Transform3D;
use sc_scene::{LeafPayload, NodeId, Scene};

use super::super::arrow_keys::{KEY_ARROW_POSITIVE_X, KEY_ARROW_POSITIVE_Y, KeyboardIntegrationContext};
use super::*;
use crate::camera::ViewCamera;
use crate::gizmo::axis::Axis;
use crate::scene_document::SceneEdit;

/// A part made of `corners` only (no faces: nothing here draws or picks),
/// under a root group as a loaded file has it.
fn part_spanning(corners: &[[f32; 3]]) -> Scene {
    let mut scene = Scene::new();
    let root = scene.add_group(None, Transform3D::Identity);
    scene.add_root(root).unwrap();
    let outline = IndexedMesh {
        meta: None,
        vertices: corners.iter().copied().map(Vertex::new).collect(),
        faces: Vec::new(),
    };
    let part = LeafPayload::Mesh(Arc::new(outline));
    let leaf = scene.add_leaf(None, Transform3D::Identity, part).unwrap();
    scene.add_child(root, leaf).unwrap();
    scene
}

fn a_point() -> Scene {
    part_spanning(&[[0.0; 3]])
}

/// A box from (-1, -1, 0) to (1, 1, 2): a volume 2 wide puts its +X wall
/// right against the box's +X side.
fn a_box() -> Scene {
    part_spanning(&[[-1.0, -1.0, 0.0], [1.0, 1.0, 2.0]])
}

fn moved_along_x(distance: f64) -> Transform3D {
    Transform3D::Affine(AffineTransform3D::from_translation([distance, 0.0, 0.0]))
}

/// A unit cube moved `x` along X, with the given metadata, as a root.
fn add_cube(scene: &mut Scene, x: f64, meta: Option<sc_scene::SceneMetadata>) -> NodeId {
    let cube = LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh()));
    let leaf = scene.add_leaf(meta, moved_along_x(x), cube).unwrap();
    scene.add_root(leaf).unwrap();
    leaf
}

/// A controller holding a scene, for pressing keys at.
struct Desk(Rc<ViewController>);

impl std::ops::Deref for Desk {
    type Target = ViewController;
    fn deref(&self) -> &ViewController {
        &self.0
    }
}

impl Desk {
    fn holding(scene: Scene) -> Self {
        let desk = Self(Rc::new(ViewController::for_test()));
        desk.put_graph_for_test(scene);
        desk
    }

    /// The scene's first part selected and the Move tool on the keys.
    fn moving(scene: Scene) -> Self {
        let desk = Self::holding(scene);
        desk.on_select_by_index(0);
        desk.keys_move();
        desk
    }

    /// As [`Desk::holding`], with a camera from the front and a 400 x 200
    /// view, which the arrow integrator needs to move anything.
    fn in_view(scene: Scene) -> Self {
        let desk = Self::holding(scene);
        let mut front = ViewCamera::new(Vec3::ZERO, 1.0);
        front.snap_to_axis(ViewAxis::Front);
        *desk.orbit.view_cell().borrow_mut() = Some(front);
        desk.put_viewport_for_test(400.0, 200.0);
        desk
    }

    fn hold(&self, key: i32) {
        self.gizmo_key_down(key);
    }

    fn let_go(&self, key: i32) {
        self.gizmo_key_up(key);
    }

    fn keys_down(&self) -> bool {
        self.pointer.viewport_keyboard_motion().held_keyboard_arrows().has_held_arrow()
    }

    fn focus(&self) -> KeyboardTransformFocus {
        self.pointer.viewport_keyboard_motion().keyboard_transform_focus()
    }

    fn doc(&self) -> &Rc<SceneDocument> {
        &self.state.document
    }

    /// Stands in for the integrator: puts the selected part `distance`
    /// along X from where it began, as a held key would.
    fn nudge(&self, distance: f64) {
        let node_id = self.doc().main_chosen_node().unwrap();
        let after = moved_along_x(distance);
        let edit = SceneEdit::SetLocalTransform {
            node_id,
            before: Transform3D::Identity,
            after,
        };
        self.doc().commit_edit(edit).unwrap();
    }

    fn placement_of(&self, part: NodeId) -> Transform3D {
        self.doc()
            .read_scene(|scene| scene.leaf(part).unwrap().local_transform())
            .unwrap()
    }

    fn first_part(&self) -> NodeId {
        self.doc()
            .read_scene(|scene| scene.leaf_id_at_index(0))
            .flatten()
            .unwrap()
    }

    fn first_placement(&self) -> Transform3D {
        self.placement_of(self.first_part())
    }

    fn x_of(&self, part: NodeId) -> f64 {
        match self.placement_of(part) {
            Transform3D::Affine(placed) => placed.translation()[0],
            Transform3D::Identity => 0.0,
        }
    }

    /// Two integrator ticks 50 ms apart; the first only starts its clock.
    fn run_the_integrator(&self) {
        for starting in [true, false] {
            let context = KeyboardIntegrationContext {
                keyboard_document: self.doc(),
                keyboard_camera: self.orbit.view_cell(),
                keyboard_viewport: self.renderer.view_extent(),
                keyboard_selection_id: self.doc().main_chosen_node(),
                keyboard_build_volume: self.state.place.volume(),
            };
            let _ = self.pointer.viewport_keyboard_motion().integrate_keyboard_motion(context);
            if starting {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
}

#[test]
fn the_keys_need_a_selection_and_follow_the_chosen_tool() {
    // Without a selection T does nothing: the arrows would move nothing.
    let desk = Desk::holding(a_point());
    desk.keys_move();
    assert_eq!(desk.focus(), KeyboardTransformFocus::NoTransformFocus);
    desk.on_select_by_index(0);
    let choices: [(fn(&ViewController), KeyboardTransformFocus); 3] = [
        (
            ViewController::keys_move,
            KeyboardTransformFocus::MoveFocus,
        ),
        (ViewController::keys_rotate, KeyboardTransformFocus::RotationFocus),
        (ViewController::pick_scale_tool, KeyboardTransformFocus::ScalingFocus),
    ];
    for (choose, expected) in choices {
        choose(&desk);
        assert_eq!(desk.focus(), expected);
    }
}

#[test]
fn held_keys_are_dropped_by_a_tool_change_and_ignored_without_one() {
    // A key held under one tool does not carry over to the next.
    let switching = Desk::moving(a_point());
    switching.hold(KEY_ARROW_POSITIVE_X);
    assert!(switching.keys_down());
    switching.keys_rotate();
    assert!(!switching.keys_down());

    // With no tool the key is ignored (the window filters it as well).
    let no_tool = Desk::holding(a_point());
    no_tool.on_select_by_index(0);
    no_tool.hold(KEY_ARROW_POSITIVE_X);
    assert!(!no_tool.keys_down());

    // Escape takes the tool away while the key is down; letting the key
    // go afterwards still counts, so none is stuck down next time.
    let escaped = Desk::moving(a_point());
    escaped.hold(KEY_ARROW_POSITIVE_X);
    escaped.select_nothing();
    escaped.let_go(KEY_ARROW_POSITIVE_X);
    assert!(!escaped.keys_down());
}

#[test]
fn a_run_of_held_keys_is_one_undo_step() {
    // Down, moved, up: one step, which puts the part back when undone.
    let moved = Desk::moving(a_point());
    moved.hold(KEY_ARROW_POSITIVE_X);
    moved.nudge(2.0);
    moved.let_go(KEY_ARROW_POSITIVE_X);
    assert!(moved.doc().step_back().unwrap());
    assert_eq!(moved.first_placement(), Transform3D::Identity);

    // Down and up again without a move: nothing to undo.
    let unmoved = Desk::moving(a_point());
    unmoved.hold(KEY_ARROW_POSITIVE_X);
    unmoved.let_go(KEY_ARROW_POSITIVE_X);
    assert!(!unmoved.doc().has_past());

    // Two keys: the step closes when the last one comes up, as one step.
    let two_keys = Desk::moving(a_point());
    two_keys.hold(KEY_ARROW_POSITIVE_X);
    two_keys.hold(KEY_ARROW_POSITIVE_Y);
    two_keys.nudge(1.0);
    two_keys.let_go(KEY_ARROW_POSITIVE_X);
    assert!(!two_keys.doc().has_past(), "one key is still down");
    two_keys.let_go(KEY_ARROW_POSITIVE_Y);
    assert!(two_keys.doc().step_back().unwrap());
    assert!(!two_keys.doc().has_past());
}

#[test]
fn a_tool_change_keeps_the_run_and_closing_the_scene_drops_it() {
    // A tool change with a key down closes the run so far as a step.
    let switching = Desk::moving(a_point());
    switching.hold(KEY_ARROW_POSITIVE_X);
    switching.nudge(3.0);
    switching.keys_rotate();
    assert!(switching.doc().step_back().unwrap());
    assert_eq!(switching.first_placement(), Transform3D::Identity);

    // Closing the scene throws the run away with its part.
    let closing = Desk(Rc::new(ViewController::for_test()));
    closing.0.listen_to_document();
    closing.put_graph_for_test(a_point());
    closing.on_select_by_index(0);
    closing.keys_move();
    closing.hold(KEY_ARROW_POSITIVE_X);
    closing.nudge(1.0);
    closing.close_scene();
    assert!(!closing.doc().has_past());
}

#[test]
fn esc_drops_the_drag_then_the_tool_then_the_selection() {
    let desk = Desk::moving(a_point());
    // There is no camera to drag with, so the drag is set up directly.
    let node_id = desk.doc().main_chosen_node().unwrap();
    desk.pointer.viewport_gizmo_machine().force_drag_state(HandleDragState::HandleHeld {
        op: DragKind::Translate {
            axis: Axis::X,
            anchor: Vec3::ZERO,
        },
        original: Transform3D::Identity,
        pivot: Vec3::ZERO,
        node_id,
    });
    // After each Escape: the tool left on the keys, and the selection.
    let steps = [
        (KeyboardTransformFocus::MoveFocus, Some(0)),
        (KeyboardTransformFocus::NoTransformFocus, Some(0)),
        (KeyboardTransformFocus::NoTransformFocus, None),
    ];
    for (step, (focus, selection)) in steps.into_iter().enumerate() {
        desk.select_nothing();
        let drag_gone = matches!(desk.pointer.viewport_gizmo_machine().drag_snapshot(), HandleDragState::Resting);
        assert!(drag_gone, "Escape {step}");
        assert_eq!(desk.focus(), focus, "Escape {step}");
        assert_eq!(desk.chosen(), selection, "Escape {step}");
    }
}

/// With the build volume's +X wall against the box, moving right does
/// nothing while turning still turns: only moves are kept inside.
#[test]
fn a_part_at_the_wall_cannot_move_out_but_still_turns() {
    let tools: [(fn(&ViewController), bool); 2] = [
        (ViewController::keys_move, false),
        (ViewController::keys_rotate, true),
    ];
    for (choose, turns) in tools {
        let desk = Desk::in_view(a_box());
        desk.on_select_by_index(0);
        desk.state
            .place.set_volume(BuildVolume::new(2.0, 4.0, 4.0));
        choose(&desk);
        desk.hold(KEY_ARROW_POSITIVE_X);
        desk.run_the_integrator();
        match desk.first_placement() {
            // Turning about X carries Y into Z.
            Transform3D::Affine(placed) if turns => {
                assert!(placed.linear()[2][1].abs() > 1e-6, "{:?}", placed.linear());
            }
            Transform3D::Identity if turns => panic!("a turn makes an affine placement"),
            Transform3D::Affine(placed) => {
                let x = placed.translation()[0];
                assert!(x <= f64::EPSILON, "moved {x} past the wall");
            }
            Transform3D::Identity => {}
        }
    }
}

#[test]
fn arrow_burst_translates_all_selected_models_and_supports_with_one_undo() {
    let desk = Desk::in_view(a_box());
    let mut scene = a_box();
    let model = add_cube(&mut scene, 10.0, None);
    // A support of `model`, as generation tags it.
    let tag = sc_scene::SceneMetadata {
        name: None,
        comment: Some(format!("sc-generated-support\nowner = {}\n", model.get())),
    };
    let support = add_cube(&mut scene, 10.0, Some(tag));
    desk.put_graph_for_test(scene);
    let first = desk.first_part();
    desk.doc()
        .choose(Selection::from_ids(vec![model, first]));
    desk.keys_move();
    desk.hold(KEY_ARROW_POSITIVE_X);
    desk.run_the_integrator();
    desk.let_go(KEY_ARROW_POSITIVE_X);

    // Both selected parts moved by the same amount, the support with its model.
    let step = desk.x_of(first);
    assert!(step > 0.0);
    assert!((desk.x_of(model) - (10.0 + step)).abs() < 1e-8);
    assert!((desk.x_of(support) - desk.x_of(model)).abs() < 1e-8);
    // All of it is one undo step.
    let document = desk.doc();
    assert!(document.step_back().unwrap());
    assert!(!document.has_past());
    assert!(desk.x_of(first).abs() < 1e-8);
    assert!((desk.x_of(model) - 10.0).abs() < 1e-8);
    assert!((desk.x_of(support) - 10.0).abs() < 1e-8);
    assert!(document.step_forward().unwrap());
    assert!((desk.x_of(support) - desk.x_of(model)).abs() < 1e-8);
}

#[test]
fn arrow_group_clamps_when_a_companion_reaches_the_wall() {
    // The cube at 9.5 already touches the +X wall of a 20 wide volume, so
    // the group cannot move right at all, not even the box.
    let desk = Desk::in_view(a_box());
    let mut scene = a_box();
    let companion = add_cube(&mut scene, 9.5, None);
    desk.put_graph_for_test(scene);
    desk.state
        .place.set_volume(BuildVolume::new(20.0, 20.0, 20.0));
    let first = desk.first_part();
    desk.doc()
        .choose(Selection::from_ids(vec![companion, first]));
    desk.keys_move();
    desk.hold(KEY_ARROW_POSITIVE_X);
    desk.run_the_integrator();
    desk.let_go(KEY_ARROW_POSITIVE_X);
    assert!(desk.placement_of(first).is_identity());
    assert_eq!(desk.placement_of(companion), moved_along_x(9.5));
    assert!(!desk.doc().has_past(), "nothing moved, nothing to undo");
}

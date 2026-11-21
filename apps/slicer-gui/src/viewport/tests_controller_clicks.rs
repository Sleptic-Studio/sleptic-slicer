// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Pointer input on the 3D view: picking parts, grabbing the transform
//! handles, what a drag does to the part and to the history, and the
//! scene events and frames a drag causes.

use std::cell::Cell;
use std::sync::Arc;

use float_cmp::assert_approx_eq;
use glam::Vec3;
use sc_mesh_core::IndexedMesh;
use sc_mesh_core::primitives::{IndexedTriangle, Normal, Vertex};
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_scene::{LeafPayload, Scene};

use super::*;
use crate::camera::ViewCamera;
use crate::gizmo::axis::Axis;
use crate::mesh_render::starting_fov_y;
use crate::picking;
use crate::gizmo::SIZES;

/// The view the handle tests use: wide enough that the whole X arrow,
/// cone included, is on screen with the part filling the view.
const VIEW: (f32, f32) = (400.0, 200.0);

/// Where the X arrow's cone sits on screen for [`Stage::one_part`]: with the
/// camera looking along +Y the arrow points right along the view's middle
/// row, and its cone covers x 320 to 350.
const X_CONE: (f32, f32) = (335.0, 100.0);

fn plain() -> PointerModifiers {
    PointerModifiers::default()
}

fn with_shift() -> PointerModifiers {
    PointerModifiers {
        shift: true,
        ..PointerModifiers::default()
    }
}

fn at((x, y): (f32, f32)) -> ScreenPoint {
    ScreenPoint::new(x, y)
}

/// A flat triangle facing +Z, centred on its own origin, one unit across.
fn flat_triangle() -> IndexedMesh {
    let corners = [[-0.5, -0.5, 0.0], [0.5, -0.5, 0.0], [0.0, 0.5, 0.0]];
    IndexedMesh {
        meta: None,
        vertices: corners.into_iter().map(Vertex::new).collect(),
        faces: vec![IndexedTriangle::from_raw_indices(
            Normal::new([0.0, 0.0, 1.0]),
            [0, 1, 2],
        )],
    }
}

/// One triangle part per position, under a single root group (as loading
/// builds a scene).
fn triangles_at(positions: &[[f64; 3]]) -> Scene {
    let mut scene = Scene::new();
    let root = scene.add_group(None, Transform3D::Identity);
    scene.add_root(root).expect("root group");
    for &offset in positions {
        let placed = Transform3D::Affine(AffineTransform3D::from_translation(offset));
        let part = LeafPayload::Mesh(Arc::new(flat_triangle()));
        let leaf = scene.add_leaf(None, placed, part).expect("part");
        scene.add_child(root, leaf).expect("part under the root");
    }
    scene
}

/// A camera at `distance` looking at `target` from `side`.
fn camera(side: ViewAxis, target: Vec3, distance: f32) -> ViewCamera {
    let mut camera = ViewCamera::new(target, distance);
    camera.snap_to_axis(side);
    camera
}

/// A controller with parts in view, for driving the pointer.
struct Stage(Rc<ViewController>);

impl std::ops::Deref for Stage {
    type Target = ViewController;
    fn deref(&self) -> &ViewController {
        &self.0
    }
}

impl Stage {
    fn new(parts: &[[f64; 3]], view: ViewAxis, size: (f32, f32)) -> Self {
        let controller = Rc::new(ViewController::for_test());
        controller.put_graph_for_test(triangles_at(parts));
        *controller.orbit.view_cell().borrow_mut() = Some(camera(view, Vec3::ZERO, 1.0));
        controller.put_viewport_for_test(size.0, size.1);
        Self(controller)
    }

    /// One part at the origin, selected, seen from the front: the transform
    /// handles are on it and the X arrow points right.
    fn one_part() -> Self {
        let stage = Self::new(&[[0.0; 3]], ViewAxis::Front, VIEW);
        stage.on_select_by_index(0);
        stage
    }

    /// As [`Stage::one_part`], with the controller answering its document's
    /// events (the frame owed, the close reset), as in the app.
    fn one_part_listening() -> Self {
        let stage = Self(Rc::new(ViewController::for_test()));
        stage.0.listen_to_document();
        stage.put_graph_for_test(triangles_at(&[[0.0; 3]]));
        *stage.orbit.view_cell().borrow_mut() = Some(camera(ViewAxis::Front, Vec3::ZERO, 1.0));
        stage.put_viewport_for_test(VIEW.0, VIEW.1);
        stage.on_select_by_index(0);
        stage
    }

    fn press(&self, point: (f32, f32), keys: PointerModifiers) -> bool {
        self.on_pointer_down(at(point), keys)
    }

    fn drag(&self, point: (f32, f32), keys: PointerModifiers) {
        self.on_pointer_move(at(point), keys);
    }

    fn release(&self, point: (f32, f32), keys: PointerModifiers) {
        self.on_pointer_up(at(point), keys);
    }

    /// The first part's placement.
    fn placement(&self) -> Transform3D {
        self.state
            .document
            .read_scene(|spatial| {
                let first = spatial.leaf_id_at_index(0).expect("a part");
                spatial.leaf(first).expect("its leaf").local_transform()
            })
            .expect("a scene")
    }

    /// Where the first part has been moved to.
    fn offset(&self) -> Vec3 {
        match self.placement() {
            Transform3D::Affine(placed) => {
                let moved = placed.translation();
                Vec3::new(moved[0] as f32, moved[1] as f32, moved[2] as f32)
            }
            Transform3D::Identity => Vec3::ZERO,
        }
    }

    /// The first part's rotation and scale, rows of a 3x3 matrix.
    fn linear(&self) -> [[f64; 3]; 3] {
        match self.placement() {
            Transform3D::Affine(placed) => *placed.linear(),
            Transform3D::Identity => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }

    fn dragging(&self) -> bool {
        matches!(self.pointer.viewport_gizmo_machine().drag_snapshot(), HandleDragState::HandleHeld { .. })
    }

    fn idle(&self) -> bool {
        matches!(self.pointer.viewport_gizmo_machine().drag_snapshot(), HandleDragState::Resting)
    }

    /// Picks the Scale tool and gives the screen point of the X axis's
    /// scale cube, which sits partway along the arrow.
    fn x_scale_cube(&self) -> (f32, f32) {
        self.pick_scale_tool();
        let camera = self.orbit.view_cell().borrow().clone().expect("camera");
        let (width, height) = self.viewport_size_for_test().expect("viewport");
        let reach = gizmo::gizmo_scale(&camera, starting_fov_y(), height);
        let cube = Vec3::X * reach * SIZES.cube_at;
        picking::screen_point_of(&camera, starting_fov_y(), width, height, cube)
            .expect("the cube is in view")
    }

    /// Counts the document's `Mutated` events from now on (selection and
    /// install events are left out).
    fn count_moves(&self) -> (Rc<Cell<usize>>, crate::scene_document::Watch) {
        let moves = Rc::new(Cell::new(0));
        let counter = Rc::clone(&moves);
        let subscription = self.state.document.watch(move |event| {
            if matches!(event, DocumentEvent::Edited) {
                counter.set(counter.get() + 1);
            }
        });
        (moves, subscription)
    }
}

fn assert_near(actual: Vec3, expected: Vec3, tolerance: f32) {
    for (got, want) in actual.to_array().into_iter().zip(expected.to_array()) {
        assert_approx_eq!(f32, got, want, epsilon = tolerance);
    }
}

fn is_uniform(scale: [[f64; 3]; 3]) -> bool {
    (scale[0][0] - scale[1][1]).abs() < 1e-5 && (scale[0][0] - scale[2][2]).abs() < 1e-5
}

#[test]
fn picking_a_part_then_the_background_updates_selection() {
    let stage = Stage::new(&[[0.0; 3], [10.0, 0.0, 0.0]], ViewAxis::Top, (800.0, 600.0));
    for (point, expected) in [((400.0, 300.0), Some(0)), ((0.0, 0.0), None)] {
        stage.press(point, plain());
        stage.release(point, plain());
        assert_eq!(stage.chosen(), expected);
    }
}

#[test]
fn control_click_toggles_membership_without_starting_a_gizmo_drag() {
    let stage = Stage::new(
        &[[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        ViewAxis::Top,
        (800.0, 600.0),
    );
    *stage.orbit.view_cell().borrow_mut() = Some(camera(ViewAxis::Top, Vec3::ZERO, 2.0));
    stage.switch_section(WorkspaceSection::Transform);
    let seen_from = stage.orbit.view_cell().borrow().clone().unwrap();
    let on_screen = [-1.0, 1.0].map(|x| {
        let centre = Vec3::new(x, 0.0, 0.0);
        picking::screen_point_of(&seen_from, starting_fov_y(), 800.0, 600.0, centre).unwrap()
    });
    let control = PointerModifiers {
        control: true,
        ..PointerModifiers::default()
    };
    // First part, second part, first part again: one, two, then one selected.
    for (point, selected) in [(on_screen[0], 1), (on_screen[1], 2), (on_screen[0], 1)] {
        assert!(!stage.press(point, control.clone()), "no gizmo drag starts");
        stage.release(point, control.clone());
        assert_eq!(stage.scene_doc().chosen().ids().len(), selected);
    }
    assert!(!stage.scene_doc().has_past(), "selecting is not an edit");
}

#[test]
fn dragging_a_part_selects_it_once_the_drag_starts_and_keeps_it() {
    // A press may still become a click, so it selects nothing yet.
    let stage = Stage::new(&[[0.0; 3]], ViewAxis::Top, (200.0, 100.0));
    stage.press((100.0, 50.0), plain());
    assert_eq!(stage.chosen(), None);
    stage.drag((150.0, 50.0), plain());
    assert_eq!(stage.chosen(), Some(0), "selected as the drag starts");
    // Released over empty space, the dragged part stays selected.
    stage.release((150.0, 50.0), plain());
    assert_eq!(stage.chosen(), Some(0));
    assert!(stage.scene_doc().has_past(), "the move is an edit");
}

#[test]
fn handle_capture_depends_on_hit_and_workspace() {
    let off_the_handles = Stage::one_part();
    off_the_handles.press((0.0, 0.0), plain());
    assert!(off_the_handles.idle());

    let outside_transform = Stage::one_part();
    outside_transform.switch_section(WorkspaceSection::Prepare);
    outside_transform.press(X_CONE, plain());
    assert!(outside_transform.idle());

    let on_the_arrow = Stage::one_part();
    on_the_arrow.press(X_CONE, plain());
    assert!(matches!(
        on_the_arrow.pointer.viewport_gizmo_machine().drag_snapshot(),
        HandleDragState::HandleHeld {
            op: DragKind::Translate { axis: Axis::X, .. },
            ..
        }
    ));
}

#[test]
fn axis_gesture_commits_once_and_section_change_cancels() {
    let stage = Stage::one_part();
    let end = (X_CONE.0 + 5.0, X_CONE.1);
    // The move expected: how far the point under the cursor slides along
    // the X axis between the press and the end of the drag.
    let seen_from = stage.orbit.view_cell().borrow().clone().unwrap();
    let on_axis = |(x, y): (f32, f32)| {
        let ray = picking::ray_through_pixel(&seen_from, starting_fov_y(), VIEW.0, VIEW.1, x, y);
        gizmo::drag_math::closest_point_on_axis(&ray, Vec3::ZERO, Axis::X).unwrap()
    };
    let expected = on_axis(end) - on_axis(X_CONE);

    stage.press(X_CONE, plain());
    assert!(stage.dragging());
    stage.drag((X_CONE.0 + 3.0, X_CONE.1), plain());
    stage.drag(end, plain());
    let moved = stage.offset();
    assert_near(moved, expected, 1e-4);
    assert!(moved.x.abs() > 1e-4, "the part moved");
    stage.release(end, plain());
    assert!(stage.idle());
    assert_near(stage.offset(), moved, 1e-6);

    // One undo step, which undoes and redoes the whole drag.
    let document = &stage.state.document;
    assert!(document.has_past() && !document.has_future());
    assert!(document.step_back().unwrap());
    assert_near(stage.offset(), Vec3::ZERO, 1e-6);
    assert!(!document.has_past() && document.has_future());
    assert!(document.step_forward().unwrap());
    assert_near(stage.offset(), moved, 1e-6);

    // Leaving Transform mid-drag puts the part back.
    let interrupted = Stage::one_part();
    interrupted.press(X_CONE, plain());
    assert!(interrupted.dragging());
    interrupted.drag(end, plain());
    interrupted.switch_section(WorkspaceSection::Slice);
    assert!(interrupted.idle());
    assert_approx_eq!(f32, interrupted.offset().x, 0.0, epsilon = 1e-4);
}

#[test]
fn cancelled_or_stationary_gestures_do_not_enter_history() {
    let still = Stage::one_part();
    still.press(X_CONE, plain());
    still.release(X_CONE, plain());
    assert!(
        !still.state.document.has_past(),
        "a press without a move"
    );

    let escaped = Stage::one_part();
    escaped.press(X_CONE, plain());
    escaped.drag((X_CONE.0 + 5.0, X_CONE.1), plain());
    assert!(escaped.offset().x.abs() > 1e-4);
    // The first Escape cancels the drag only: the part goes back, the
    // selection stays, and there is nothing to undo.
    escaped.select_nothing();
    assert_near(escaped.offset(), Vec3::ZERO, 1e-6);
    assert!(escaped.idle());
    assert_eq!(escaped.chosen(), Some(0));
    assert!(!escaped.state.document.has_past());
    // The next one clears the selection.
    escaped.select_nothing();
    assert_eq!(escaped.chosen(), None);
}

#[test]
fn closing_the_scene_mid_drag_drops_the_drag_and_the_selection() {
    // The drag holds the part it moves; with the scene gone there is
    // nothing to put back, and nothing may try.
    let stage = Stage::one_part_listening();
    stage.press(X_CONE, plain());
    stage.drag((X_CONE.0 + 5.0, X_CONE.1), plain());
    stage.close_scene();
    assert!(stage.idle());
    assert_eq!(stage.chosen(), None);
}

#[test]
fn a_few_pixels_of_wobble_between_press_and_release_is_still_a_click() {
    // Trackpads and lifting fingers wobble; under the drag threshold
    // (4 px) the release still picks.
    let stage = Stage::new(&[[0.0; 3]], ViewAxis::Top, (200.0, 100.0));
    stage.press((100.0, 50.0), plain());
    stage.release((101.5, 51.5), plain());
    assert_eq!(stage.chosen(), Some(0));
}

#[test]
fn scale_tool_pointer_drag_changes_only_x_or_all_axes() {
    for uniform in [false, true] {
        let stage = Stage::one_part();
        let cube = stage.x_scale_cube();
        let keys = if uniform { with_shift() } else { plain() };
        assert!(stage.press(cube, keys.clone()));
        let pulled = (cube.0 + 35.0, cube.1);
        stage.drag(pulled, keys.clone());
        let scale = stage.linear();
        assert!((scale[0][0] - 1.0).abs() > 0.01, "X scaled");
        if uniform {
            assert!(is_uniform(scale));
        } else {
            assert!((scale[1][1] - 1.0).abs() < 1e-5 && (scale[2][2] - 1.0).abs() < 1e-5);
        }
        stage.release(pulled, keys);
    }
}

#[test]
fn active_rotate_and_scale_toolbar_buttons_toggle_off_to_no_tool() {
    // No tool is the default, so pressing the active tool's button puts
    // it away (it does not fall back to Move), and then the handles
    // cannot be grabbed.
    let stage = Stage::one_part();
    for (button, mode) in [
        (1, crate::mesh_render::HandleShowing::Turn),
        (2, crate::mesh_render::HandleShowing::Resize),
    ] {
        stage.on_gizmo_toolbar(button);
        assert_eq!(stage.pointer.viewport_gizmo_machine().mode(), mode);
        assert!(stage.pointer.viewport_gizmo_machine().handles());
        stage.on_gizmo_toolbar(button);
        assert!(!stage.pointer.viewport_gizmo_machine().handles());
        let centre = (VIEW.0 / 2.0, VIEW.1 / 2.0);
        stage.press(centre, plain());
        assert!(
            !matches!(
                stage.pointer.viewport_gizmo_machine().drag_snapshot(),
                HandleDragState::HandleHeld {
                    op: DragKind::ScaleUniform { .. } | DragKind::Rotate { .. },
                    ..
                }
            ),
            "the hidden handles cannot be grabbed"
        );
        stage.release(centre, plain());
        // That click may have picked empty space: select the part again.
        stage.scene_doc().select_all();
    }
}

#[test]
fn centre_scale_pointer_drag_is_uniform() {
    let stage = Stage::one_part();
    stage.pick_scale_tool();
    assert!(stage.press((VIEW.0 / 2.0, VIEW.1 / 2.0), plain()));
    assert!(matches!(
        stage.pointer.viewport_gizmo_machine().drag_snapshot(),
        HandleDragState::HandleHeld {
            op: DragKind::ScaleUniform { .. },
            ..
        }
    ));
    stage.drag((235.0, 90.0), plain());
    let scale = stage.linear();
    assert!(scale[0][0] > 1.1, "grew");
    assert!(is_uniform(scale));
}

#[test]
fn planar_move_pointer_drag_changes_xy_and_preserves_z() {
    let stage = Stage::one_part();
    stage.keys_move();
    *stage.orbit.view_cell().borrow_mut() = Some(camera(ViewAxis::Top, Vec3::ZERO, 1.0));
    let seen_from = stage.orbit.view_cell().borrow().clone().unwrap();
    // The XY plane handle sits about a third of the way along both arrows.
    let reach = gizmo::gizmo_scale(&seen_from, starting_fov_y(), VIEW.1);
    let corner = Vec3::new(0.32 * reach, 0.32 * reach, 0.0);
    let press =
        picking::screen_point_of(&seen_from, starting_fov_y(), VIEW.0, VIEW.1, corner)
            .unwrap();
    assert!(stage.press(press, plain()));
    assert!(matches!(
        stage.pointer.viewport_gizmo_machine().drag_snapshot(),
        HandleDragState::HandleHeld {
            op: DragKind::TranslatePlane {
                normal: Axis::Z,
                ..
            },
            ..
        }
    ));
    stage.drag((press.0 + 20.0, press.1 - 15.0), plain());
    let moved = stage.offset();
    assert!(
        moved.x.abs() > 0.01 && moved.y.abs() > 0.01,
        "moved in the plane"
    );
    assert!(moved.z.abs() < 1e-5, "not off it");
}

#[test]
fn axis_scale_captures_modifier_at_press() {
    for (keys, proportional_expected) in [(plain(), false), (with_shift(), true)] {
        let stage = Stage::one_part();
        let cube = stage.x_scale_cube();
        assert_eq!(
            stage.pointer.viewport_gizmo_machine().mode(),
            crate::mesh_render::HandleShowing::Resize
        );
        stage.press(cube, keys);
        match stage.pointer.viewport_gizmo_machine().drag_snapshot() {
            HandleDragState::HandleHeld {
                op: DragKind::ScaleAxis {
                    axis, proportional, ..
                },
                ..
            } => {
                assert_eq!(axis, Axis::X);
                assert_eq!(proportional, proportional_expected);
            }
            state => panic!("expected an X scale drag, got {state:?}"),
        }
    }
}

#[test]
fn shift_mid_drag_switches_an_axis_scale_to_uniform_from_where_it_is() {
    // Without Shift, pulling the X cube stretches X alone.
    let stage = Stage::one_part();
    let cube = stage.x_scale_cube();
    stage.press(cube, plain());
    let stretched_at = (cube.0 + 20.0, cube.1);
    stage.drag(stretched_at, plain());
    let stretched = stage.linear();
    assert!(stretched[0][0] > 1.001, "X grew: {}", stretched[0][0]);
    assert_approx_eq!(f64, stretched[1][1], 1.0, epsilon = 1e-6);
    assert_approx_eq!(f64, stretched[2][2], 1.0, epsilon = 1e-6);

    // Shift goes down without the cursor moving: the drag starts again
    // from here, and pulling further now scales every axis by one factor
    // on top of the stretch so far.
    stage.drag(stretched_at, with_shift());
    stage.drag((stretched_at.0 + 20.0, stretched_at.1), with_shift());
    let both = stage.linear();
    assert_approx_eq!(f64, both[1][1], both[2][2], epsilon = 1e-4);
    assert!(
        (both[1][1] - 1.0).abs() > 1e-3,
        "Y and Z scaled too: {}",
        both[1][1]
    );
    assert!(
        (both[0][0] - both[1][1]).abs() > 1e-3,
        "X keeps its extra stretch: {} against {}",
        both[0][0],
        both[1][1]
    );
}

#[test]
fn pressing_or_letting_go_of_shift_mid_drag_never_jumps_the_part() {
    let stage = Stage::one_part();
    let cube = stage.x_scale_cube();
    stage.press(cube, plain());
    let there = (cube.0 + 20.0, cube.1);
    stage.drag(there, plain());
    let (scale_before, offset_before) = (stage.linear(), stage.offset());
    // Same spot, Shift now held: the drag re-anchors, the part stays put.
    stage.drag(there, with_shift());
    let scale_after = stage.linear();
    for (row_after, row_before) in scale_after.iter().zip(&scale_before) {
        for (after, before) in row_after.iter().zip(row_before) {
            assert_approx_eq!(f64, *after, *before, epsilon = 1e-4);
        }
    }
    assert_near(stage.offset(), offset_before, 1e-4);
}

#[test]
fn letting_go_of_shift_mid_drag_goes_back_to_stretching_x_alone() {
    // Begun uniform, then Shift released: from there on only X changes,
    // Y and Z keeping what they had reached.
    let stage = Stage::one_part();
    let cube = stage.x_scale_cube();
    stage.press(cube, with_shift());
    let there = (cube.0 + 20.0, cube.1);
    stage.drag(there, with_shift());
    stage.drag(there, plain());
    let held = stage.linear();
    stage.drag((there.0 + 10.0, there.1), plain());
    let after = stage.linear();
    assert!(
        (after[0][0] - held[0][0]).abs() > 1e-3,
        "X went on growing: {} then {}",
        held[0][0],
        after[0][0]
    );
    assert_approx_eq!(f64, after[1][1], held[1][1], epsilon = 1e-4);
    assert_approx_eq!(f64, after[2][2], held[2][2], epsilon = 1e-4);
}

#[test]
fn each_drag_step_is_one_scene_change_and_hovering_is_none() {
    let dragged = Stage::one_part();
    dragged.press(X_CONE, plain());
    let (moves, _listening) = dragged.count_moves();
    dragged.drag((X_CONE.0 + 5.0, X_CONE.1), plain());
    assert_eq!(moves.get(), 1, "one pointer move, one change");

    // Moving over the handles without a press changes the highlight only.
    let hovered = Stage::one_part();
    let (moves, _listening) = hovered.count_moves();
    hovered.drag(X_CONE, plain());
    hovered.drag((X_CONE.0 + 50.0, X_CONE.1), plain());
    assert_eq!(moves.get(), 0);
}

#[test]
fn abandoning_a_drag_is_one_scene_change_and_abandoning_none_is_nothing() {
    let dragging = Stage::one_part();
    dragging.press(X_CONE, plain());
    assert!(dragging.dragging());
    let (moves, _listening) = dragging.count_moves();
    assert!(
        dragging
            .pointer
            .viewport_gizmo_machine()
            .cancel_drag_edit(&dragging.state.document)
    );
    assert_eq!(moves.get(), 1, "putting the part back is one change");

    let resting = Stage::one_part();
    assert!(resting.idle());
    let (moves, _listening) = resting.count_moves();
    assert!(!resting.pointer.viewport_gizmo_machine().cancel_drag_edit(&resting.state.document));
    assert_eq!(moves.get(), 0, "no drag, nothing to put back");
}

#[test]
fn a_drag_step_owes_a_frame_through_the_scene_change() {
    // The controller is told of the change by its document, not by the
    // drag code itself, and that alone owes the frame.
    let stage = Stage::one_part_listening();
    stage.press(X_CONE, plain());
    stage.forget_frame_for_test();
    assert!(!stage.redraw_due_for_test());
    stage.drag((X_CONE.0 + 5.0, X_CONE.1), plain());
    assert!(stage.redraw_due_for_test());
}

#[test]
fn a_new_build_volume_owes_a_frame_and_the_same_one_does_not() {
    // Without a frame owed the next paint draws nothing, and the new
    // plate would only show once something else moved.
    let controller = ViewController::for_test();
    let first = BuildVolume::new(80.0, 80.0, 120.0);
    assert_ne!(controller.state.place.volume(), first);
    controller.forget_frame_for_test();
    controller.set_build_volume(first);
    assert_eq!(controller.state.place.volume(), first);
    assert!(controller.redraw_due_for_test());

    controller.forget_frame_for_test();
    controller.set_build_volume(first);
    assert!(!controller.redraw_due_for_test(), "nothing changed");

    let wider = BuildVolume::new(150.0, 80.0, 120.0);
    controller.set_build_volume(wider);
    assert_eq!(controller.state.place.volume(), wider);
    assert!(controller.redraw_due_for_test());
}

#[test]
fn an_arrow_drag_stops_at_the_build_volume_wall() {
    // A 1 mm wide volume puts its +X wall at 0.5, exactly where the part's
    // right corner already is, so no drag to the right may move it.
    let stage = Stage::one_part();
    stage
        .state
        .place.set_volume(BuildVolume::new(1.0, 4.0, 4.0));
    stage.press(X_CONE, plain());
    stage.drag((X_CONE.0 + 50.0, X_CONE.1), plain());
    assert_near(stage.offset(), Vec3::ZERO, 1e-6);
}

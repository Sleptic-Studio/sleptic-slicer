// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use glam::Vec3;
use sc_mesh_core::transform::Transform3D;
use sc_scene::Scene;

use super::*;
use crate::gizmo::axis::Axis;

fn dragging(op: DragKind) -> HandleDragState {
    let node_id = Scene::new().add_group(None, Transform3D::Identity);
    HandleDragState::HandleHeld {
        op,
        original: Transform3D::Identity,
        pivot: Vec3::new(1.0, 2.0, 3.0),
        node_id,
    }
}

fn scale_axis(axis: Axis, current_anchor: Vec3, proportional: bool) -> DragKind {
    let start_anchor = Vec3::ZERO;
    DragKind::ScaleAxis {
        axis,
        start_anchor,
        current_anchor,
        proportional,
        press_screen: (0.0, 0.0),
        pivot_screen: (0.0, 0.0),
    }
}

#[test]
fn at_rest_the_gizmo_shows_its_mode() {
    for mode in [
        HandleShowing::Resting,
        HandleShowing::Move,
        HandleShowing::Turn,
        HandleShowing::Resize,
    ] {
        let (shown, cube) = gizmo_look(&HandleDragState::Resting, mode);
        assert_eq!(shown, mode);
        assert!(cube.is_none());
    }
}

#[test]
fn a_drag_shows_only_its_handle() {
    let anchor = Vec3::ONE;
    let drags = [
        (
            DragKind::Plate {
                anchor,
                snap: false,
                press: (0.0, 0.0),
                activated: true,
            },
            HandleShowing::Resting,
        ),
        (
            DragKind::TranslatePlane {
                normal: Axis::Z,
                anchor,
            },
            HandleShowing::Move,
        ),
        (
            DragKind::ScaleUniform {
                press_screen: (0.0, 0.0),
            },
            HandleShowing::Resize,
        ),
        (
            DragKind::Translate {
                axis: Axis::Y,
                anchor,
            },
            HandleShowing::Moving { axis: Axis::Y },
        ),
        (
            DragKind::Rotate {
                axis: Axis::X,
                start_angle: 0.0,
                last_raw: 0.5,
                current_unwrapped: 0.5,
            },
            HandleShowing::Turning { axis: Axis::X },
        ),
    ];
    for (op, expected) in drags {
        // The mode shown at rest does not matter while dragging.
        let (shown, cube) = gizmo_look(&dragging(op), HandleShowing::Turn);
        assert_eq!(shown, expected);
        assert!(cube.is_none(), "{expected:?}");
    }
}

#[test]
fn an_axis_scale_moves_its_cube_to_the_pointer_unless_proportional() {
    // Pivot (1, 2, 3); the pointer is 4.5 along Y from it.
    let op = scale_axis(Axis::Y, Vec3::new(9.0, 6.5, -2.0), false);
    let (shown, cube) = gizmo_look(&dragging(op), HandleShowing::Resize);
    assert_eq!(shown, HandleShowing::Stretching { axis: Axis::Y });
    let Some(LiveDrag::Stretch {
        axis,
        reach,
    }) = cube
    else {
        panic!("the cube follows the pointer");
    };
    assert_eq!((axis, reach), (Axis::Y, 4.5));
    let proportional = scale_axis(Axis::Y, Vec3::new(9.0, 6.5, -2.0), true);
    let (shown, cube) = gizmo_look(&dragging(proportional), HandleShowing::Resize);
    assert_eq!(shown, HandleShowing::Stretching { axis: Axis::Y });
    assert!(cube.is_none());
}

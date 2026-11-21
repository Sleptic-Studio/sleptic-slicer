// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Gesture state and transform calculations for viewport handles.
//! The controller stores the state; these functions calculate each move
//! from the captured transform so repeated pointer events do not accumulate
//! numerical drift.

use glam::Vec3;
use sc_mesh_core::transform::AffineTransform3D;
use sc_mesh_core::transform::Transform3D;
use sc_scene::NodeId;

use crate::camera::ViewCamera;
use crate::gizmo::axis::Axis;
use crate::gizmo::{self};
use crate::picking;

/// A gesture retains its starting transform and pivot until it ends.
#[derive(Copy, Clone, Debug)]
pub(super) enum HandleDragState {
    Resting,
    HandleHeld {
        op: DragKind,
        original: Transform3D,
        pivot: Vec3,
        node_id: NodeId,
    },
}

/// Inputs captured for each handle, including the running rotation angle and
/// both scale anchors needed when Shift changes the scale mode.
#[derive(Copy, Clone, Debug)]
pub(super) enum DragKind {
    Plate {
        anchor: Vec3,
        snap: bool,
        press: (f32, f32),
        activated: bool,
    },
    TranslatePlane {
        normal: Axis,
        anchor: Vec3,
    },
    ScaleUniform {
        press_screen: (f32, f32),
    },
    Translate {
        axis: Axis,
        anchor: Vec3,
    },
    Rotate {
        axis: Axis,
        start_angle: f32,
        last_raw: f32,
        current_unwrapped: f32,
    },
    ScaleAxis {
        axis: Axis,
        start_anchor: Vec3,
        /// Updated on each move so the rendered handle follows the cursor.
        current_anchor: Vec3,
        proportional: bool,
        press_screen: (f32, f32),
        pivot_screen: (f32, f32),
    },
}

/// The handle chosen by the shared press and hover hit test.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum GrabbedHandle {
    Translate(Axis),
    TranslatePlane(Axis),
    ScaleUniform,
    Rotate(Axis),
    ScaleAxis(Axis),
}

/// View parameters needed to capture a new scale anchor.
pub(super) struct ReanchorView<'a> {
    pub lens: &'a ViewCamera,
    pub pivot_point: Vec3,
    pub fov_y: f32,
    pub view_px: (f32, f32),
    pub cursor_px: (f32, f32),
}

/// Calculate a move from the captured transform, updating the operation's
/// running state. An unusable ray leaves the current transform unchanged.
pub(super) fn drag_to_transform(
    op: DragKind,
    ray: &picking::PickRay,
    pivot: Vec3,
    parent_world: &Transform3D,
    original: &Transform3D,
    cursor_x: f32,
    cursor_y: f32,
) -> Option<(Transform3D, DragKind)> {
    match op {
        DragKind::Plate { anchor, snap, .. } => {
            let delta = plate_drag_offset(ray, anchor, snap, parent_world)?;
            let translation = shift_by(delta);
            Some((
                about_pivot(&translation, Vec3::ZERO, parent_world, original),
                op,
            ))
        }
        DragKind::TranslatePlane { normal, anchor } => {
            let movement = gizmo::drag_math::ray_hits_axis_plane(ray, pivot, normal)? - anchor;
            Some((
                about_pivot(
                    &shift_by(movement),
                    Vec3::ZERO,
                    parent_world,
                    original,
                ),
                op,
            ))
        }
        DragKind::ScaleUniform { press_screen } => {
            let factor = centre_drag_scale(press_screen, (cursor_x, cursor_y));
            let scale = AffineTransform3D::from_uniform_scale(f64::from(factor));
            Some((
                about_pivot(&scale, pivot, parent_world, original),
                op,
            ))
        }
        DragKind::Translate { axis, anchor } => {
            let hit = gizmo::drag_math::closest_point_on_axis(ray, pivot, axis)?;
            let offset = Transform3D::Affine(shift_by(hit - anchor));
            Some((offset.compose(original), op))
        }
        DragKind::Rotate {
            axis,
            start_angle,
            last_raw,
            current_unwrapped,
        } => {
            let angle = gizmo::drag_math::rotate_drag_angle(ray, pivot, axis)?;
            let accumulated = continue_angle(last_raw, current_unwrapped, angle);
            let rotation = rotation_about(axis, snap_rotation(accumulated - start_angle))?;
            let transformed = about_pivot(&rotation, pivot, parent_world, original);
            let next = DragKind::Rotate {
                axis,
                start_angle,
                last_raw: angle,
                current_unwrapped: accumulated,
            };
            Some((transformed, next))
        }
        DragKind::ScaleAxis {
            axis,
            start_anchor,
            current_anchor: _,
            proportional,
            press_screen,
            pivot_screen,
        } => {
            // Keep the handle position current in both scale modes.
            let current_anchor = gizmo::drag_math::closest_point_on_axis(ray, pivot, axis)?;
            let scale_mat = axis_scale_factor(
                axis,
                start_anchor,
                current_anchor,
                pivot,
                proportional,
                press_screen,
                pivot_screen,
                (cursor_x, cursor_y),
            );
            let new_op = DragKind::ScaleAxis {
                axis,
                start_anchor,
                current_anchor,
                proportional,
                press_screen,
                pivot_screen,
            };
            Some((
                about_pivot(&scale_mat, pivot, parent_world, original),
                new_op,
            ))
        }
    }
}

// Both scale modes share the same anchor update, but derive their factors
// from different coordinates.
fn axis_scale_factor(
    axis: Axis,
    start: Vec3,
    current: Vec3,
    pivot: Vec3,
    proportional: bool,
    press: (f32, f32),
    pivot_screen: (f32, f32),
    cursor: (f32, f32),
) -> AffineTransform3D {
    if proportional {
        let factor = gizmo::drag_math::even_stretch(
            pivot_screen.0,
            pivot_screen.1,
            press.0,
            press.1,
            cursor.0,
            cursor.1,
        );
        AffineTransform3D::from_uniform_scale(f64::from(factor))
    } else {
        let factor = gizmo::drag_math::axis_stretch(start, current, pivot, axis);
        stretch_along(axis, factor)
    }
}

// Screen displacement works at the centre, where a radial ratio is undefined.
fn centre_drag_scale(press: (f32, f32), cursor: (f32, f32)) -> f32 {
    let distance = cursor.0 - press.0 - cursor.1 + press.1;
    2.0_f32.powf(distance / 120.0).clamp(0.01, 100.0)
}

// Plate movement is measured in world space but applied in the parent frame.
fn plate_drag_offset(
    ray: &picking::PickRay,
    anchor: Vec3,
    snap: bool,
    parent_world: &Transform3D,
) -> Option<Vec3> {
    let (x, y) = picking::level_plane_crossing(ray, 0.0)?;
    let mut movement = Vec3::new(x - anchor.x, y - anchor.y, 0.0);
    if snap {
        movement = movement.round();
    }
    if let Transform3D::Affine(parent) = parent_world {
        let inverse = parent.inverse().ok()?;
        let point = |v: Vec3| {
            Vec3::from(
                inverse
                    .apply_to_vertex(sc_mesh_core::primitives::Vertex::new(v.to_array()))
                    .coords(),
            )
        };
        movement = point(movement) - point(Vec3::ZERO);
    }
    Some(movement)
}

// Translation construction is shared by all three drag paths so their
// conversion from the cursor's f32 coordinates remains consistent.
fn shift_by(delta: Vec3) -> AffineTransform3D {
    AffineTransform3D::from_translation(delta.to_array().map(f64::from))
}

#[cfg(test)]
#[path = "tests_handle_drag.rs"]
mod tests;

/// Capture the current position when Shift changes the scale mode, avoiding a jump.
pub(super) fn reanchor_axis_scale(
    op: DragKind,
    running_transform: Transform3D,
    ctx: &ReanchorView<'_>,
    new_proportional: bool,
) -> Option<(DragKind, Transform3D)> {
    let axis = match op {
        DragKind::ScaleAxis { axis, .. } => axis,
        _ => return None,
    };
    let (width, height) = ctx.view_px;
    let (x, y) = ctx.cursor_px;
    let projected =
        picking::screen_point_of(ctx.lens, ctx.fov_y, width, height, ctx.pivot_point)?;
    let cursor_ray = picking::ray_through_pixel(ctx.lens, ctx.fov_y, width, height, x, y);
    let anchor = gizmo::drag_math::closest_point_on_axis(&cursor_ray, ctx.pivot_point, axis)?;
    Some((
        DragKind::ScaleAxis {
            axis,
            start_anchor: anchor,
            current_anchor: anchor,
            proportional: new_proportional,
            press_screen: ctx.cursor_px,
            pivot_screen: projected,
        },
        running_transform,
    ))
}

/// Spacing of the marks displayed on the rotation ring.
pub(crate) const ROTATION_STEP_DEG: f32 = 15.0;
const SNAP_WINDOW_DEG: f32 = 3.0;
const RIGHT_ANGLE_SNAP_DEG: f32 = 5.0;

/// Hold near marked angles so precise common rotations are easier to select.
pub(crate) fn snap_rotation(angle: f32) -> f32 {
    let degrees = angle.to_degrees();
    let nearest = (degrees / ROTATION_STEP_DEG).round() * ROTATION_STEP_DEG;
    let tolerance = if (nearest / 90.0).fract().abs() < 1e-3 {
        RIGHT_ANGLE_SNAP_DEG
    } else {
        SNAP_WINDOW_DEG
    };
    if (degrees - nearest).abs() <= tolerance {
        nearest.to_radians()
    } else {
        angle
    }
}

/// Carry the angle across the atan2 discontinuity to preserve full turns.
pub(super) fn continue_angle(last_raw: f32, current_unwrapped: f32, raw: f32) -> f32 {
    let step = raw - last_raw;
    let step = if step > std::f32::consts::PI {
        step - std::f32::consts::TAU
    } else if step < -std::f32::consts::PI {
        step + std::f32::consts::TAU
    } else {
        step
    };
    current_unwrapped + step
}

/// Construct the rotation around one of the world axes.
pub(super) fn rotation_about(axis: Axis, angle: f32) -> Option<AffineTransform3D> {
    let radians = f64::from(angle);
    Some(match axis {
        Axis::X => AffineTransform3D::from_rotation_x(radians),
        Axis::Y => AffineTransform3D::from_rotation_y(radians),
        Axis::Z => AffineTransform3D::from_rotation_z(radians),
    })
}

/// Leave the other two dimensions unchanged when scaling an axis.
pub(super) fn stretch_along(axis: Axis, factor: f32) -> AffineTransform3D {
    let mut dimensions = [1.0; 3];
    dimensions[match axis {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
    }] = f64::from(factor);
    AffineTransform3D::from_non_uniform_scale(dimensions)
}

/// Use the parent-local pivot so an ancestor's placement does not move the centre.
pub(super) fn about_pivot(
    m: &AffineTransform3D,
    pivot: Vec3,
    parent_world: &Transform3D,
    original: &Transform3D,
) -> Transform3D {
    let local = pivot_in_parent(pivot, parent_world);
    let forward = shift_by(local);
    let backward = shift_by(-local);
    Transform3D::Affine(forward.compose(&m.compose(&backward))).compose(original)
}

// A singular parent has no inverse; retain the previous fallback in that case.
fn pivot_in_parent(pivot: Vec3, parent_world: &Transform3D) -> Vec3 {
    match parent_world {
        Transform3D::Identity => pivot,
        Transform3D::Affine(parent) => parent.inverse().ok().map_or(pivot, |inverse| {
            Vec3::from(
                inverse
                    .apply_to_vertex(sc_mesh_core::primitives::Vertex::new(pivot.to_array()))
                    .coords(),
            )
        }),
    }
}


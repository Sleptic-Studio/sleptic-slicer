// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Turning the mouse ray during a handle drag into a point on an axis, an
//! angle around one, or a scale factor.
use glam::Vec3;

use super::axis::{Axis, axis_basis};
use crate::picking::PickRay;
use crate::gizmo::LIMITS;

/// sin² of the 8° lock-out angle. Presses are refused inside it already
/// (`axis_pickable`), so this only bites when the view turns mid-drag.
const MIN_DENOMINATOR: f32 = 1.0 - LIMITS.axis_edge_on_cos * LIMITS.axis_edge_on_cos;

/// The point of the line `pivot + t·axis` nearest the mouse ray: where a
/// move drag puts the grabbed point. None while the ray runs within 8° of
/// the axis, where a pixel of mouse would throw the part across the plate;
/// the drag then waits for a usable view.
pub(crate) fn closest_point_on_axis(ray: &PickRay, pivot: Vec3, axis: Axis) -> Option<Vec3> {
    let along = axis.unit_vector();
    let to_pivot = pivot - ray.start;
    let cos = ray.heading.dot(along);
    let sin_sq = 1.0 - cos * cos;
    if sin_sq < MIN_DENOMINATOR {
        return None;
    }
    // Setting the connecting segment square to both lines gives
    // t·sin² = (q·d)(a·d) - q·a, with q from the ray origin to the pivot.
    let t = (to_pivot.dot(ray.heading) * cos - to_pivot.dot(along)) / sin_sq;
    Some(pivot + along * t)
}

/// Where the mouse ray crosses the plane through `pivot` square to `axis`;
/// None if it runs (nearly) parallel or the plane is behind the camera.
pub(crate) fn ray_hits_axis_plane(ray: &PickRay, pivot: Vec3, axis: Axis) -> Option<Vec3> {
    let normal = axis.unit_vector();
    let facing = ray.heading.dot(normal);
    if facing.abs() < 1e-6 {
        return None;
    }
    let t = (pivot - ray.start).dot(normal) / facing;
    (t > 0.0).then(|| ray.start + ray.heading * t)
}

/// The angle (-π to π) of the mouse around `axis`, from `axis_basis`'s
/// first direction towards its second. A rotate drag reads one at the
/// press and turns by the change; None only where the ring is locked.
pub(crate) fn rotate_drag_angle(ray: &PickRay, pivot: Vec3, axis: Axis) -> Option<f32> {
    let offset = ray_hits_axis_plane(ray, pivot, axis)? - pivot;
    let (u, v) = axis_basis(axis);
    Some(offset.dot(v).atan2(offset.dot(u)))
}

/// Scale along one axis: how far the grabbed point now lies from the pivot
/// along the axis, over how far it lay at the press. Kept within
/// [LIMITS.min_scale, 1/LIMITS.min_scale], which also stops mirroring. A press on the
/// pivot itself gives 1 (no ratio to take).
pub(crate) fn axis_stretch(
    start_anchor: Vec3,
    current_anchor: Vec3,
    pivot: Vec3,
    axis: Axis,
) -> f32 {
    let along = axis.unit_vector();
    let start = (start_anchor - pivot).dot(along);
    if start.abs() < 1e-6 {
        return 1.0;
    }
    ((current_anchor - pivot).dot(along) / start).clamp(LIMITS.min_scale, 1.0 / LIMITS.min_scale)
}

/// Uniform scale (Shift on a scale cube), on screen: how far the mouse has
/// travelled along the line from the pivot through the press point, as a
/// share of the press distance. Measuring along that line (not plain
/// distance) keeps a drag through the pivot from growing the part again
/// on the far side; there it stays at the minimum. All positions are in the
/// same logical pixels; a press on the pivot gives 1.
pub(crate) fn even_stretch(
    pivot_screen_x: f32,
    pivot_screen_y: f32,
    x_press: f32,
    y_press: f32,
    x_now: f32,
    y_now: f32,
) -> f32 {
    let (px, py) = (x_press - pivot_screen_x, y_press - pivot_screen_y);
    let reach_sq = px * px + py * py;
    if reach_sq < 1e-12 {
        return 1.0;
    }
    let (nx, ny) = (x_now - pivot_screen_x, y_now - pivot_screen_y);
    ((nx * px + ny * py) / reach_sq).clamp(LIMITS.min_scale, 1.0 / LIMITS.min_scale)
}

#[cfg(test)]
#[path = "tests_drag_math.rs"]
mod tests;

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Which handle the mouse ray hits: move arrows, rotate rings, scale cubes.
//! Each test returns the axis and the distance along the ray, so the
//! controller can pick the nearest handle across all three kinds.
use glam::Vec3;

use super::axis::{ALL_AXES, Axis};
use crate::picking::{PickRay, box_entry_distance};
use crate::gizmo::{SIZES, LIMITS};

/// A move arrow (and its scale cube) can be grabbed unless the view runs
/// within 8° of its axis; such an arrow is drawn grey.
pub(crate) fn axis_pickable(ray: &PickRay, axis: Axis) -> bool {
    axis.unit_vector().dot(ray.heading).abs() < LIMITS.axis_edge_on_cos
}

/// A rotate ring can be grabbed unless it is seen within 8° of edge-on.
pub(crate) fn arc_pickable(ray: &PickRay, axis: Axis) -> bool {
    axis.unit_vector().dot(ray.heading).abs() >= LIMITS.ring_edge_on_cos
}

/// The nearest hit over the axes that `usable` allows; on a tie the
/// earlier axis (X, then Y, then Z) wins.
fn nearest(usable: impl Fn(Axis) -> bool, hit: impl Fn(Axis) -> Option<f32>) -> Option<(Axis, f32)> {
    ALL_AXES
        .into_iter()
        .filter(|axis| usable(*axis))
        .filter_map(|axis| hit(axis).map(|t| (axis, t)))
        .fold(None, |best, (axis, t)| match best {
            Some((_, nearest)) if nearest <= t => best,
            _ => Some((axis, t)),
        })
}

/// Move arrows. Only the head takes clicks (from one length out to 1.25
/// lengths); the shaft is just drawn.
pub(crate) fn handle_under(
    ray: &PickRay,
    pivot: Vec3,
    length_world: f32,
    radius_world: f32,
) -> Option<(Axis, f32)> {
    let (from, to) = (length_world * SIZES.cone_grab.0, length_world * SIZES.cone_grab.1);
    nearest(
        |axis| axis_pickable(ray, axis),
        |axis| ray_enters_cylinder(ray, pivot, axis.unit_vector(), from, to, radius_world),
    )
}

/// Where the ray enters a capped cylinder around `centre + s·along` for
/// s in [s_min, s_max]. None if it misses, starts inside, or the
/// cylinder is behind it. The ray is inside where both the cap slab and
/// the wall allow it; entry is the later of the two entries.
fn ray_enters_cylinder(
    ray: &PickRay,
    centre: Vec3,
    along: Vec3,
    s_min: f32,
    s_max: f32,
    radius: f32,
) -> Option<f32> {
    let offset = ray.start - centre;
    let axial_speed = ray.heading.dot(along);
    let axial_start = offset.dot(along);
    let (cap_in, cap_out) = if axial_speed.abs() < 1e-10 {
        // Moving square to the axis: inside the caps everywhere or nowhere.
        if axial_start < s_min || axial_start > s_max {
            return None;
        }
        (f32::NEG_INFINITY, f32::INFINITY)
    } else {
        ordered(
            (s_min - axial_start) / axial_speed,
            (s_max - axial_start) / axial_speed,
        )
    };
    // Distance from the axis squared is a quadratic a·t² + b·t + c in t.
    let radial_speed = ray.heading - along * axial_speed;
    let radial_start = offset - along * axial_start;
    let a = radial_speed.dot(radial_speed);
    let b = 2.0 * radial_speed.dot(radial_start);
    let c = radial_start.dot(radial_start) - radius * radius;
    let (wall_in, wall_out) = if a.abs() < 1e-10 {
        // Moving along the axis: inside the wall everywhere or nowhere.
        if c > 0.0 {
            return None;
        }
        (f32::NEG_INFINITY, f32::INFINITY)
    } else {
        let discriminant = b * b - 4.0 * a * c;
        if discriminant < 0.0 {
            return None;
        }
        let root = discriminant.sqrt();
        let half_inverse = 1.0 / (2.0 * a);
        ordered((-b - root) * half_inverse, (-b + root) * half_inverse)
    };
    let (enter, leave) = (cap_in.max(wall_in), cap_out.min(wall_out));
    // A handle is grabbed from outside, in front of the camera.
    (enter <= leave && leave > 0.0 && enter > 0.0).then_some(enter)
}

fn ordered(a: f32, b: f32) -> (f32, f32) {
    if a < b { (a, b) } else { (b, a) }
}

/// One rotate ring: a circle of `radius` about `axis` through `pivot`,
/// lying in the plane square to the axis. The ray hits where it crosses
/// that plane within `band` of the circle; the band widens as the ring
/// tilts away (up to the 0.35 cap), so it keeps its width on screen.
pub(crate) fn ray_hits_rotate_ring(
    ray: &PickRay,
    pivot: Vec3,
    axis: Axis,
    radius: f32,
    band: f32,
) -> Option<f32> {
    let normal = axis.unit_vector();
    let facing = normal.dot(ray.heading);
    if facing.abs() < 1e-6 || radius <= 0.0 {
        return None;
    }
    let t = normal.dot(pivot - ray.start) / facing;
    if t <= 0.0 {
        return None;
    }
    let from_centre = (ray.start + ray.heading * t - pivot).length();
    ((from_centre - radius).abs() <= band / facing.abs().max(0.35)).then_some(t)
}

/// Rotate rings, nearest first; edge-on rings are skipped.
pub(crate) fn ring_under(
    ray: &PickRay,
    pivot: Vec3,
    radius: f32,
    band: f32,
) -> Option<(Axis, f32)> {
    nearest(
        |axis| arc_pickable(ray, axis),
        |axis| ray_hits_rotate_ring(ray, pivot, axis, radius, band),
    )
}

/// Scale cubes: world-aligned cubes of side `axis_cube_world` at the end
/// of each shaft. They share the move arrows' lock-out, since their drag
/// uses the same point-on-axis maths.
pub(crate) fn cube_under(
    ray: &PickRay,
    pivot: Vec3,
    shaft_length_world: f32,
    axis_cube_world: f32,
) -> Option<(Axis, f32)> {
    let half = Vec3::splat(axis_cube_world * 0.5);
    nearest(
        |axis| axis_pickable(ray, axis),
        |axis| {
            let centre = pivot + axis.unit_vector() * (shaft_length_world * SIZES.cube_at);
            box_entry_distance(ray, centre - half, centre + half)
        },
    )
}

#[cfg(test)]
#[path = "tests_hit_test.rs"]
mod tests;

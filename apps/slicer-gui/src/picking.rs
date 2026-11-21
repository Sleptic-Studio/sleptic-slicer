// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! From a pixel to what is under it: the mouse ray, the part it hits first
//! and where, the point on the plate, and screen positions of world points.
//!
//! Parts are tested in their own coordinates (the ray is carried into
//! them), box by box, then triangle by triangle, front faces only, as the
//! renderer culls. Plates after the first are drawn shifted; picks undo
//! that shift per part.
use glam::{DMat3, DVec3, Vec3};
use sc_mesh_core::Aabb;
use sc_mesh_core::aabb::arvo_transformed_aabb;
use sc_mesh_core::geometry::DEGENERACY_SIN_SQ_THRESHOLD;
use sc_mesh_core::transform::Transform3D;
use sc_mesh_spatial::{ObjectQuery, SceneSpatial};
use sc_scene::NodeId;

use crate::camera::ViewCamera;

/// Hits closer than this along the ray are the ray's own start.
const MIN_HIT_DISTANCE: f32 = 1e-7;

/// Triangles thinner than the mesh library's own degeneracy limit are
/// skipped: their winding (and so their facing) is noise.
const SLIVER_SIN_SQ: f32 = DEGENERACY_SIN_SQ_THRESHOLD as f32;

/// An orthographic ray starts this far behind the target, behind any part.
const ORTHO_START_BEHIND_MM: f32 = 10_000.0;

#[derive(Clone, Copy, Debug)]
pub(crate) struct PickRay {
    pub start: Vec3,
    /// Unit length.
    pub heading: Vec3,
}

/// The ray through logical pixel (`px`, `py`), y down, in a view `width` x
/// `height` pixels with vertical field of view `fov_y`. Perspective rays
/// leave the eye; orthographic rays run parallel to the line of sight from
/// well behind the scene.
pub(crate) fn ray_through_pixel(camera: &ViewCamera, fov_y: f32, width: f32, height: f32, px: f32, py: f32) -> PickRay {
    // The pixel in the view's [-1, 1] square, y up; the frustum's half
    // height at unit distance is `half_tall`.
    let shape = width / height;
    let half_tall = (fov_y / 2.0).tan();
    let across = (px / width) * 2.0 - 1.0;
    let upward = 1.0 - (py / height) * 2.0;
    if camera.is_perspective {
        let in_view = Vec3::new(across * shape * half_tall, upward * half_tall, -1.0).normalize();
        let in_world = camera.world_to_view().inverse() * in_view.extend(0.0);
        return PickRay { start: camera.eye_position(), heading: in_world.truncate().normalize() };
    }
    let half_high = camera.eye_distance * half_tall;
    let (right, up) = camera.right_and_up();
    let ahead = (camera.look_at - camera.eye_position()).normalize_or_zero();
    let on_target = camera.look_at + right * (across * shape * half_high) + up * (upward * half_high);
    PickRay { start: on_target - ahead * ORTHO_START_BEHIND_MM, heading: ahead }
}

/// Where `point` shows in a view `width` x `height` pixels, y down; none
/// when it is behind a perspective camera.
pub(crate) fn screen_point_of(
    camera: &ViewCamera,
    fov_y: f32,
    width: f32,
    height: f32,
    point: Vec3,
) -> Option<(f32, f32)> {
    let to_clip = camera.view_to_clip(fov_y, width / height.max(1.0), 0.01, 1.0e5) * camera.world_to_view();
    let clip = to_clip * point.extend(1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let (across, upward) = (clip.x / clip.w, clip.y / clip.w);
    Some(((across + 1.0) / 2.0 * width, (1.0 - upward) / 2.0 * height))
}

/// Where the ray meets the horizontal plane at height `z`, if ahead of it
/// (none for a ray running level).
pub(crate) fn level_plane_crossing(ray: &PickRay, z: f32) -> Option<(f32, f32)> {
    let climb = ray.heading.z;
    if climb.abs() < 1e-7 {
        return None;
    }
    let along = (z - ray.start.z) / climb;
    (along >= 0.0).then(|| {
        let at = ray.start + ray.heading * along;
        (at.x, at.y)
    })
}

/// Whether the ray lands on the plate (edges included).
pub(crate) fn lands_on_plate(ray: &PickRay, volume: crate::build_volume::BuildVolume) -> bool {
    level_plane_crossing(ray, 0.0).is_some_and(|(x, y)| {
        x.abs() <= volume.width_mm() * 0.5 && y.abs() <= volume.depth_mm() * 0.5
    })
}

/// Which right-click menu opens: a part's, the plate's, or the general one.
#[derive(Debug, PartialEq)]
pub(crate) enum ClickMenu {
    OnPart,
    OnPlate,
    Elsewhere,
}

pub(crate) fn menu_for_click(object_hit: bool, plate_hit: bool) -> ClickMenu {
    match (object_hit, plate_hit) {
        (true, _) => ClickMenu::OnPart,
        (false, true) => ClickMenu::OnPlate,
        (false, false) => ClickMenu::Elsewhere,
    }
}

/// The part under the ray on a single plate (tests only; the app always
/// knows its plate size).
#[cfg(test)]
pub(crate) fn part_under_ray(ray: &PickRay, spatial: &SceneSpatial) -> Option<NodeId> {
    part_under_ray_on_plates(ray, spatial, None)
}

pub(crate) fn part_under_ray_on_plates(
    ray: &PickRay,
    spatial: &SceneSpatial,
    plate_size: Option<(f32, f32)>,
) -> Option<NodeId> {
    first_part_hit(ray, spatial, plate_size).map(|(id, _)| id)
}

/// The ray as a part on its plate sees it: shifted back by that plate's
/// display offset.
fn seen_from_plate(ray: &PickRay, spatial: &SceneSpatial, id: NodeId, plate_size: (f32, f32)) -> PickRay {
    let graph = spatial.graph();
    let [x, y] = graph.plate_offset(graph.plate_of(id), plate_size.0, plate_size.1);
    let mut shifted = *ray;
    shifted.start.x -= x;
    shifted.start.y -= y;
    shifted
}

/// The first part the ray hits and the distance to it. Overlays use the
/// distance to hide behind parts. On equal distance the earlier part wins.
pub(crate) fn first_part_hit(
    ray: &PickRay,
    spatial: &SceneSpatial,
    plate_size: Option<(f32, f32)>,
) -> Option<(NodeId, f32)> {
    let mut nearest: Option<(NodeId, f32)> = None;
    for id in spatial.leaf_ids() {
        let ray = match plate_size {
            Some(size) => seen_from_plate(ray, spatial, id, size),
            None => *ray,
        };
        let Some(part) = spatial.leaf(id) else { continue };
        let Some(bounds) = part.local_aabb() else { continue };
        let (min, max) = part_box_in_world(bounds, &part.world_transform());
        if !crosses_box(&ray, min, max) {
            continue;
        }
        if let Some(t) = part_hit_distance(&ray, &part)
            && nearest.is_none_or(|(_, best)| t < best)
        {
            nearest = Some((id, t));
        }
    }
    nearest
}

/// The part hit and the hit point in its plate's own coordinates, as the
/// editing tools need it.
pub(crate) fn part_surface_point(
    ray: &PickRay,
    spatial: &SceneSpatial,
    plate_size: (f32, f32),
) -> Option<(NodeId, Vec3)> {
    let id = part_under_ray_on_plates(ray, spatial, Some(plate_size))?;
    let ray = seen_from_plate(ray, spatial, id, plate_size);
    let t = part_hit_distance(&ray, &spatial.leaf(id)?)?;
    Some((id, ray.start + ray.heading * t))
}

pub(crate) fn crosses_box(ray: &PickRay, min: Vec3, max: Vec3) -> bool {
    box_entry_distance(ray, min, max).is_some()
}

/// Where the ray enters a world-aligned box (0 when it starts inside), or
/// None if it misses or the box is behind it: the window of distances where
/// the ray is between each pair of opposite faces, narrowed axis by axis. A
/// zero heading component gives infinite distances, which the narrowing
/// copes with.
pub(crate) fn box_entry_distance(ray: &PickRay, min: Vec3, max: Vec3) -> Option<f32> {
    let (mut enter, mut leave) = (f32::NEG_INFINITY, f32::INFINITY);
    for axis in 0..3 {
        let per_unit = 1.0 / ray.heading[axis];
        let near_face = (min[axis] - ray.start[axis]) * per_unit;
        let far_face = (max[axis] - ray.start[axis]) * per_unit;
        enter = enter.max(near_face.min(far_face));
        leave = leave.min(near_face.max(far_face));
    }
    // A rejection, so a NaN window (a ray starting exactly on the face of a
    // flat box) still counts as a hit, as it always has.
    if leave < enter || leave < 0.0 {
        return None;
    }
    Some(enter.max(0.0))
}

/// A triangle as one corner and the two edges leaving it.
struct Triangle {
    corner: Vec3,
    to_b: Vec3,
    to_c: Vec3,
}

impl Triangle {
    /// Where `ray` crosses it, if it crosses the side `facing` calls the
    /// front (Moller-Trumbore with the side test first).
    fn crossing(&self, ray: &PickRay, facing: f32) -> Option<f32> {
        let p = ray.heading.cross(self.to_c);
        let det = self.to_b.dot(p);
        let front = det * facing;
        // Slivers are left out: their winding, and so their side, is noise.
        let area_scale = self.to_b.length_squared() * self.to_c.length_squared();
        if front <= 0.0 || front * front < SLIVER_SIN_SQ * area_scale {
            return None;
        }
        let inv = 1.0 / det;
        let offset = ray.start - self.corner;
        let u = inv * offset.dot(p);
        if !(0.0..=1.0).contains(&u) {
            return None;
        }
        let q = offset.cross(self.to_b);
        let v = inv * ray.heading.dot(q);
        if v < 0.0 || u + v > 1.0 {
            return None;
        }
        let t = inv * self.to_c.dot(q);
        (t > MIN_HIT_DISTANCE).then_some(t)
    }
}

/// How far along the ray it crosses the triangle `a b c`, front face only.
/// Front is counter-clockwise when `facing` is +1, as the renderer culls; a
/// part its transform mirrors shows its triangles' backs, so it is tested
/// with -1.
pub(crate) fn front_face_distance(ray: &PickRay, a: Vec3, b: Vec3, c: Vec3, facing: f32) -> Option<f32> {
    Triangle { corner: a, to_b: b - a, to_c: c - a }.crossing(ray, facing)
}

/// The nearest front-face hit on one part, as a world distance along the
/// ray. A singular transform, an empty mesh or a bad vertex index is a miss.
pub(crate) fn part_hit_distance(ray: &PickRay, query: &ObjectQuery<'_>) -> Option<f32> {
    let (local, scale, facing) = ray_in_part(ray, &query.world_transform())?;
    let clusters = query.clusters()?;
    let mesh = query.mesh();
    let (vertices, faces) = (mesh.vertices(), mesh.faces());
    let mut nearest: Option<f32> = None;
    for (range, bounds) in clusters.iter() {
        let (min, max) = (Vec3::from(bounds.min().coords()), Vec3::from(bounds.max().coords()));
        if !crosses_box(&local, min, max) {
            continue;
        }
        for face in &faces[range.start as usize..range.end as usize] {
            let mut corner = [Vec3::ZERO; 3];
            for (slot, index) in corner.iter_mut().zip(face.vertices) {
                *slot = Vec3::from(vertices.get(usize::from(index))?.coords());
            }
            if let Some(t) = front_face_distance(&local, corner[0], corner[1], corner[2], facing) {
                // Local distances are stretched by the transform's scale.
                let t = t / scale;
                if nearest.is_none_or(|best| t < best) {
                    nearest = Some(t);
                }
            }
        }
    }
    nearest
}

/// The ray in a part's own coordinates, with how much the inverse
/// transform stretches distances and which way the transform turns faces
/// (-1 when it mirrors). None for a singular transform.
fn ray_in_part(ray: &PickRay, transform: &Transform3D) -> Option<(PickRay, f32, f32)> {
    let Transform3D::Affine(affine) = transform else {
        return Some((*ray, 1.0, 1.0));
    };
    // glam matrices are built from columns; the stored rows go in transposed.
    let columns = |rows: &[[f64; 3]; 3]| DMat3::from_cols_array_2d(rows).transpose();
    let undo = affine.inverse().ok()?;
    let undo_linear = columns(undo.linear()).as_mat3();
    let undo_shift = DVec3::from_array(*undo.translation()).as_vec3();
    let heading = undo_linear * ray.heading;
    let stretch = heading.length();
    if !stretch.is_finite() || stretch <= MIN_HIT_DISTANCE {
        return None;
    }
    let turn = columns(affine.linear()).determinant() as f32;
    if turn == 0.0 {
        return None;
    }
    let start = undo_linear * ray.start + undo_shift;
    Some((PickRay { start, heading: heading / stretch }, stretch, turn.signum()))
}

/// A part's box in the world (Arvo's method in f32; the box only gates the
/// f32 triangle tests).
fn part_box_in_world(local: Aabb, transform: &Transform3D) -> (Vec3, Vec3) {
    let (min, max) = (Vec3::from(local.min().coords()), Vec3::from(local.max().coords()));
    let Transform3D::Affine(affine) = transform else {
        return (min, max);
    };
    let m = affine.linear().map(|row| row.map(|v| v as f32));
    let t = affine.translation().map(|v| v as f32);
    let (min, max) = arvo_transformed_aabb::<f32>(min.to_array(), max.to_array(), &m, &t);
    (Vec3::from(min), Vec3::from(max))
}

#[cfg(test)]
#[path = "tests_picking.rs"]
mod tests;

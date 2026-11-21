// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Camera-relative navigation cube geometry and face picking.
use crate::camera::{ViewAxis, ViewCamera};
use glam::{Vec2, Vec3};

pub(super) fn face_view(face: i32) -> Option<ViewAxis> {
    match face {
        0 => Some(ViewAxis::Top),
        1 => Some(ViewAxis::Bottom),
        2 => Some(ViewAxis::Front),
        3 => Some(ViewAxis::Back),
        4 => Some(ViewAxis::Left),
        5 => Some(ViewAxis::Right),
        _ => None,
    }
}

pub(super) struct Face {
    pub id: i32,
    pub points: [Vec2; 4],
    /// How squarely the face looks at the camera (0..1), for shading.
    pub facing: f32,
}

/// Cube widget box (logical px), its centre and the face half-size scale.
/// Sleptic (R54): smaller than the first Fusion-style cube (scale 34, box 150).
pub(crate) const CUBE_BOX: f32 = 120.0;
const CUBE_CENTRE: f32 = CUBE_BOX / 2.0;
const CUBE_SCALE: f32 = 27.0;
/// Fillet radius (fraction of the half-size): faces are flat out to
/// 1 - FILLET, then round over into their neighbours like Fusion's cube.
const FILLET: f32 = 0.28;
/// Facets per quarter-round edge.
const FILLET_STEPS: usize = 3;

/// Fraction of a face (from each edge) that belongs to the edge / corner
/// zones: exactly the rounded band, so a hovered face, edge or corner lights
/// its own flat face, fillet or rounded corner (Sleptic: with zones wider than
/// the fillet the highlight showed slivers and little squares).
const ZONE: f32 = FILLET;

/// The cube region under a widget point: a face, edge or corner, as the sum
/// of its outward unit axes (one, two or three non-zero components of +-1).
/// Looking from that direction shows the face / edge / corner head on.
pub(super) fn pick_target(camera: &ViewCamera, point: Vec2) -> Option<Vec3> {
    let view = camera.world_to_view();
    let inverse = glam::Mat3::from_mat4(view).transpose();
    let local = Vec3::new(
        (point.x - CUBE_CENTRE) / CUBE_SCALE,
        (CUBE_CENTRE - point.y) / CUBE_SCALE,
        10.0,
    );
    let origin = inverse * local;
    let dir = inverse * Vec3::new(0.0, 0.0, -1.0);
    // Slab test against the cube [-1, 1]^3; the entry point is the surface.
    let mut t_near = f32::NEG_INFINITY;
    let mut t_far = f32::INFINITY;
    for axis in 0..3 {
        if dir[axis].abs() < 1e-6 {
            if origin[axis].abs() > 1.0 {
                return None;
            }
            continue;
        }
        let a = (-1.0 - origin[axis]) / dir[axis];
        let b = (1.0 - origin[axis]) / dir[axis];
        t_near = t_near.max(a.min(b));
        t_far = t_far.min(a.max(b));
    }
    if t_near > t_far {
        return None;
    }
    let hit = origin + dir * t_near;
    let target = Vec3::new(
        zone(hit.x),
        zone(hit.y),
        zone(hit.z),
    );
    (target != Vec3::ZERO).then_some(target)
}

/// Widget point of a cube-space point (the inverse of [`pick_target`]'s
/// projection); verification hovers the cube through it.
pub(super) fn screen_point(camera: &ViewCamera, p: Vec3) -> Option<Vec2> {
    let v = camera.world_to_view().transform_vector3(p) * CUBE_SCALE;
    v.is_finite().then(|| Vec2::new(CUBE_CENTRE + v.x, CUBE_CENTRE - v.y))
}

fn zone(c: f32) -> f32 {
    if c >= 1.0 - ZONE {
        1.0
    } else if c <= -(1.0 - ZONE) {
        -1.0
    } else {
        0.0
    }
}

/// The highlight for a face / edge / corner target as one SVG path: the flat
/// face, or the fillet facets / rounded corner patch of that zone. One path
/// (union fill, no outline per facet) so neighbouring facets show no seams.
pub(super) fn target_path(camera: &ViewCamera, target: Vec3) -> String {
    let mut polygons: Vec<Vec<Vec2>> = Vec::new();
    if target.abs().element_sum() == 1.0 {
        let id = [Vec3::Z, -Vec3::Z, -Vec3::Y, Vec3::Y, -Vec3::X, Vec3::X]
            .iter()
            .position(|n| *n == target);
        polygons.extend(
            faces(camera)
                .into_iter()
                .filter(|f| Some(f.id as usize) == id)
                .map(|f| f.points.to_vec()),
        );
    }
    polygons.extend(
        rounding(camera)
            .into_iter()
            .filter(|piece| piece.zone == target)
            .map(|piece| piece.points),
    );
    let mut path = String::new();
    for polygon in polygons {
        for (k, p) in polygon.iter().enumerate() {
            path.push_str(&format!("{}{:.2} {:.2} ", if k == 0 { "M" } else { "L" }, p.x, p.y));
        }
        path.push_str("Z ");
    }
    path
}
/// One visible facet of the rounded edges and corners.
pub(super) struct Piece {
    pub points: Vec<Vec2>,
    pub facing: f32,
    /// The face / edge / corner zone it belongs to (see [`pick_target`]).
    pub zone: Vec3,
}

/// The fillets: each edge a quarter-round band of [`FILLET_STEPS`] facets,
/// split along its length where the corner zones begin; each corner a small
/// rounded patch. Facets facing away are left out (the cube is convex, so
/// what remains never overlaps).
pub(super) fn rounding(camera: &ViewCamera) -> Vec<Piece> {
    let view = camera.world_to_view();
    let toward_eye = (camera.eye_position() - camera.look_at).normalize();
    let project = |p: Vec3| {
        let p = view.transform_vector3(p) * CUBE_SCALE;
        Vec2::new(CUBE_CENTRE + p.x, CUBE_CENTRE - p.y)
    };
    let flat = 1.0 - FILLET;
    let mut pieces = Vec::new();
    let mut push = |points: Vec<Vec3>, normal: Vec3, zone: Vec3| {
        let facing = normal.normalize().dot(toward_eye);
        if facing > 0.001 {
            pieces.push(Piece {
                points: points.into_iter().map(project).collect(),
                facing: facing.clamp(0.0, 1.0),
                zone,
            });
        }
    };
    let axes = [Vec3::X, Vec3::Y, Vec3::Z];
    // Edges: along axis `e`, between faces s1*axes[i] and s2*axes[j].
    for e in 0..3 {
        let (i, j) = ((e + 1) % 3, (e + 2) % 3);
        for s1 in [-1.0f32, 1.0] {
            for s2 in [-1.0f32, 1.0] {
                let (n1, n2) = (axes[i] * s1, axes[j] * s2);
                let centre = (n1 + n2) * flat;
                let along = axes[e];
                let at = |k: usize, t: f32| {
                    let angle = k as f32 / FILLET_STEPS as f32 * std::f32::consts::FRAC_PI_2;
                    centre + (n1 * angle.cos() + n2 * angle.sin()) * FILLET + along * t
                };
                let cut = 1.0 - ZONE;
                for (t0, t1, end) in [(-flat, -cut, -1.0), (-cut, cut, 0.0), (cut, flat, 1.0)] {
                    // With the zones on the fillet the end pieces are empty.
                    if t1 - t0 < 1e-5 {
                        continue;
                    }
                    let zone = n1 + n2 + along * end;
                    for k in 0..FILLET_STEPS {
                        let mid = (k as f32 + 0.5) / FILLET_STEPS as f32 * std::f32::consts::FRAC_PI_2;
                        push(
                            vec![at(k, t0), at(k + 1, t0), at(k + 1, t1), at(k, t1)],
                            n1 * mid.cos() + n2 * mid.sin(),
                            zone,
                        );
                    }
                }
            }
        }
    }
    // Corners: the octant of a sphere of radius FILLET, as a small
    // triangulated patch (barycentric grid over the three axis directions).
    for sx in [-1.0f32, 1.0] {
        for sy in [-1.0f32, 1.0] {
            for sz in [-1.0f32, 1.0] {
                let s = Vec3::new(sx, sy, sz);
                let centre = s * flat;
                let n = FILLET_STEPS;
                let at = |a: usize, b: usize| {
                    let c = n - a - b;
                    let d = Vec3::new(a as f32 * sx, b as f32 * sy, c as f32 * sz).normalize();
                    (centre + d * FILLET, d)
                };
                for a in 0..n {
                    for b in 0..n - a {
                        let (p0, d0) = at(a, b);
                        let (p1, d1) = at(a + 1, b);
                        let (p2, d2) = at(a, b + 1);
                        push(vec![p0, p1, p2], d0 + d1 + d2, s);
                        if a + b + 1 < n {
                            let (p3, d3) = at(a + 1, b + 1);
                            push(vec![p1, p3, p2], d1 + d3 + d2, s);
                        }
                    }
                }
            }
        }
    }
    pieces
}

/// One world axis drawn along a cube edge (Fusion-style view cube).
pub(super) struct CubeAxis {
    pub from: Vec2,
    pub to: Vec2,
    /// Hidden when both faces along its edge face away from the camera.
    pub visible: bool,
}

/// X, Y and Z run from the cube corner at the world origin side (-1,-1,-1)
/// along the cube's edges, a little past the far corner.
pub(super) fn axes(camera: &ViewCamera) -> [CubeAxis; 3] {
    let view = camera.world_to_view();
    let toward_eye = (camera.eye_position() - camera.look_at).normalize();
    let project = |p: Vec3| {
        let p = view.transform_vector3(p) * CUBE_SCALE;
        Vec2::new(CUBE_CENTRE + p.x, CUBE_CENTRE - p.y)
    };
    let corner = Vec3::splat(-1.0);
    let faces_toward = |a: Vec3, b: Vec3| a.dot(toward_eye) > 0.001 || b.dot(toward_eye) > 0.001;
    [
        (Vec3::X, -Vec3::Y, -Vec3::Z),
        (Vec3::Y, -Vec3::X, -Vec3::Z),
        (Vec3::Z, -Vec3::X, -Vec3::Y),
    ]
    .map(|(axis, a, b)| CubeAxis {
        from: project(corner),
        to: project(corner + axis * 2.15),
        visible: faces_toward(a, b),
    })
}

pub(super) fn faces(camera: &ViewCamera) -> Vec<Face> {
    let view = camera.world_to_view();
    let toward_eye = (camera.eye_position() - camera.look_at).normalize();
    let normals = [Vec3::Z, -Vec3::Z, -Vec3::Y, Vec3::Y, -Vec3::X, Vec3::X];
    normals
        .into_iter()
        .enumerate()
        .filter_map(|(id, n)| {
            if n.dot(toward_eye) <= 0.001 {
                return None;
            }
            let a = if n.z.abs() > 0.5 { Vec3::X } else { Vec3::Z };
            let b = n.cross(a);
            let (a, b) = (a * (1.0 - FILLET), b * (1.0 - FILLET));
            let points = [n - a - b, n + a - b, n + a + b, n - a + b].map(|p| {
                let p = view.transform_vector3(p) * CUBE_SCALE;
                Vec2::new(CUBE_CENTRE + p.x, CUBE_CENTRE - p.y)
            });
            Some(Face {
                id: id as i32,
                points,
                facing: n.dot(toward_eye).clamp(0.0, 1.0),
            })
        })
        .collect()
}

/// SVG path for face `id`'s label, laid on the face itself rather than drawn
/// upright at its centre: it turns and foreshortens with the cube. Each label
/// is upright in that face's own axis view (side faces: world Z up; Top: +Y
/// up; Bottom, seen from below: -Y up), with right = up x normal so no face
/// shows mirrored text.
pub(super) fn face_label_path(
    camera: &ViewCamera,
    id: i32,
    text: &crate::surface_text::TextOutline,
) -> String {
    let normals = [Vec3::Z, -Vec3::Z, -Vec3::Y, Vec3::Y, -Vec3::X, Vec3::X];
    let Some(&n) = usize::try_from(id).ok().and_then(|i| normals.get(i)) else {
        return String::new();
    };
    let up = if n.z > 0.5 {
        Vec3::Y
    } else if n.z < -0.5 {
        -Vec3::Y
    } else {
        Vec3::Z
    };
    let right = up.cross(n);
    // The flat part spans +-(1 - FILLET); keep the label inside ~80% of it.
    let scale = (1.6 * (1.0 - FILLET) / text.width.max(0.01)).min(0.5);
    let view = camera.world_to_view();
    crate::surface_text::path_commands(text, |p| {
        let world = n + right * (p.x * scale) + up * (p.y * scale);
        let v = view.transform_vector3(world) * CUBE_SCALE;
        Vec2::new(CUBE_CENTRE + v.x, CUBE_CENTRE - v.y)
    })
}

#[cfg(test)]
pub(super) fn pick(camera: &ViewCamera, point: Vec2) -> Option<i32> {
    faces(camera)
        .into_iter()
        .find(|face| {
            let mut positive = false;
            let mut negative = false;
            for i in 0..4 {
                let a = face.points[i];
                let b = face.points[(i + 1) % 4];
                let cross = (b - a).perp_dot(point - a);
                positive |= cross > 0.001;
                negative |= cross < -0.001;
            }
            !(positive && negative)
        })
        .map(|f| f.id)
}

#[cfg(test)]
#[path = "tests_view_steps.rs"]
mod tests_view_steps;

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Frozen R17 implementation, used only by the opt-in comparative audit.
use super::{bounds, points, world_edit};
use glam::DVec3;
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_scene::{Scene, node::NodeId};
use sc_scene_edit::SceneEdit;
/// Deterministic resin orientation: 0.8 overhang + 1.0 peak section + 1.5 height.
/// Areas normalize by half the surface area, height by the original bounding diagonal.
/// Peak section is sampled at 24 evenly spaced planes using oriented triangle intersections.
pub(crate) fn auto_orient(scene: &Scene, id: NodeId, support_angle_deg: f64) -> Option<SceneEdit> {
    let mesh = sc_scene::bake_leaf_to_world(scene, id).ok()?;
    let vertices = points(scene, id)?;
    let (old_min, old_max) = bounds(&vertices)?;
    let triangles: Vec<_> = mesh
        .faces
        .iter()
        .filter_map(|f| {
            let [a, b, c] = f.vertices.map(|i| vertices.get(usize::from(i)).copied());
            let (a, b, c) = (a?, b?, c?);
            let n = (b - a).cross(c - a);
            (n.length_squared() > 1e-16).then_some(([a, b, c], n))
        })
        .collect();
    let area = triangles
        .iter()
        .map(|(_, n)| n.length() * 0.5)
        .sum::<f64>()
        .max(1e-12);
    let mut candidates = Vec::new();
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                if x != 0 || y != 0 || z != 0 {
                    candidates
                        .push(DVec3::new(f64::from(x), f64::from(y), f64::from(z)).normalize());
                }
            }
        }
    }
    let mut faces: Vec<_> = triangles.iter().map(|(_, n)| *n).collect();
    faces.sort_by(|a, b| b.length_squared().total_cmp(&a.length_squared()));
    let mut clusters: Vec<DVec3> = Vec::new();
    for n in faces {
        let n = n.normalize();
        if clusters.iter().all(|c| c.dot(n) < 0.985) {
            clusters.push(n);
            if clusters.len() == 12 {
                break;
            }
        }
    }
    for n in clusters {
        for d in [n, -n] {
            if candidates.iter().all(|c| c.dot(d) < 0.999999) {
                candidates.push(d);
            }
        }
    }
    let threshold = support_angle_deg.clamp(0.0, 90.0).to_radians().cos();
    let diagonal = (old_max - old_min).length().max(1e-12);
    let mut best = None;
    let mut best_score = f64::INFINITY;
    for up in candidates {
        let rotation = glam::DQuat::from_rotation_arc(up, DVec3::Z);
        let rotated: Vec<_> = vertices.iter().map(|v| rotation * *v).collect();
        let (min, max) = bounds(&rotated)?;
        let height = max.z - min.z;
        let overhang = triangles
            .iter()
            .map(|(_, n)| {
                let down = -n.normalize().dot(up);
                if down > threshold + 1e-10 {
                    0.5 * n.length() * down
                } else {
                    0.0
                }
            })
            .sum::<f64>();
        let mut sections = [0.0f64; 24];
        for (tri, _) in &triangles {
            let t = tri.map(|v| rotation * v);
            let lo = t.iter().map(|v| v.z).fold(f64::INFINITY, f64::min);
            let hi = t.iter().map(|v| v.z).fold(f64::NEG_INFINITY, f64::max);
            for (i, section) in sections.iter_mut().enumerate() {
                let z = min.z + height * (i as f64 + 0.5) / 24.0;
                if z < lo || z >= hi {
                    continue;
                }
                let mut crossings = [DVec3::ZERO; 2];
                let mut count = 0;
                for j in 0..3 {
                    let a = t[j];
                    let b = t[(j + 1) % 3];
                    if (a.z <= z && b.z > z) || (b.z <= z && a.z > z) {
                        if count < 2 {
                            crossings[count] = a + (b - a) * ((z - a.z) / (b.z - a.z));
                        }
                        count += 1;
                    }
                }
                if count == 2 {
                    let [a, b] = crossings;
                    let direction = DVec3::Z.cross((t[1] - t[0]).cross(t[2] - t[0]));
                    let sign = if (b - a).dot(direction) >= 0.0 {
                        1.0
                    } else {
                        -1.0
                    };
                    *section += sign * (a.x * b.y - b.x * a.y) * 0.5;
                }
            }
        }
        let peak = sections.into_iter().map(f64::abs).fold(0.0, f64::max);
        let score = 0.8 * overhang / area + peak / (area * 0.5) + 1.5 * height / diagonal;
        if up == DVec3::Z {
            eprintln!(
                "Auto orient baseline: weighted overhang={overhang:.3} mm2, sampled peak section={peak:.3} mm2, height={height:.3} mm"
            );
        }
        if score < best_score - 1e-12 {
            best_score = score;
            best = Some((up, min, max, overhang, peak));
        }
    }
    let (up, min, max, overhang, peak) = best?;
    eprintln!(
        "Auto orient result: up={:?}, weighted overhang={:.3} mm2, sampled peak section={:.3} mm2, height={:.3} mm",
        up,
        overhang,
        peak,
        max.z - min.z
    );
    let axis = up.cross(DVec3::Z);
    let affine = if axis.length_squared() < 1e-16 {
        if up.z > 0.0 {
            AffineTransform3D::identity()
        } else {
            AffineTransform3D::from_rotation_x(std::f64::consts::PI)
        }
    } else {
        AffineTransform3D::from_axis_angle(axis.to_array(), up.z.clamp(-1.0, 1.0).acos()).ok()?
    };
    let shift = (old_min + old_max - min - max) * 0.5;
    let delta = Transform3D::Affine(AffineTransform3D::from_translation([
        shift.x, shift.y, -min.z,
    ]))
    .compose(&Transform3D::Affine(affine));
    world_edit(scene, id, delta)
}

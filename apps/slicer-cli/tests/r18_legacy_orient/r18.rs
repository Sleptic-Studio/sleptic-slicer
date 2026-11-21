// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Deterministic resin orientation, shared by both frontends.
//!
//! Costs are normalized over fitting candidates. Balanced weights are support
//! 0.45, height 0.25, surface quality/peel 0.20 and footprint 0.10. Bed-contact
//! faces contribute support contacts because automatic supports lift the mesh.
//! Flat surfaces have zero stair-step penalty.
use glam::{DQuat, DVec2, DVec3};
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_print_primitives::BuildVolume;
use sc_scene::{Scene, node::NodeId};

/// Measured orientation costs (areas in square millimetres).
#[derive(Clone, Copy, Debug, Default)]
pub struct Metrics {
    /// Downward projected area above the build plate.
    pub support_area: f64,
    /// Estimated contact footprint for default 0.4 mm tips and 4 mm spacing.
    pub contact_area: f64,
    /// Curvature-weighted downward area; contacts on detailed surfaces cost more.
    pub detail_contact_area: f64,
    /// Topological local-minimum patches requiring at least one contact.
    pub minima: usize,
    /// Surface area exposed to stair stepping, weighted by slope.
    pub stair_area: f64,
    /// Largest of 24 sampled horizontal sections.
    pub peak_area: f64,
    /// Axis-aligned XY area after yaw optimization.
    pub footprint: f64,
    /// Height in millimetres.
    pub height: f64,
    /// XY dimensions after yaw.
    pub width: f64,
    /// Y extent after yaw.
    pub depth: f64,
    /// Whether this orientation fits the complete build envelope.
    pub fits: bool,
}
/// An orientation proposal in baked world coordinates.
#[derive(Clone, Debug)]
pub struct Proposal {
    /// World-space delta. Identity when retained.
    pub delta: Transform3D,
    /// Original costs.
    pub original: Metrics,
    /// Chosen costs.
    pub chosen: Metrics,
    /// Whether the original transform was retained.
    pub retained: bool,
}
#[derive(Clone)]
struct Candidate {
    rotation: DQuat,
    min: DVec3,
    max: DVec3,
    metrics: Metrics,
}
fn bounds(points: impl Iterator<Item = DVec3>) -> Option<(DVec3, DVec3)> {
    points.fold(None, |b, p| {
        Some(match b {
            None => (p, p),
            Some((a, b)) => (a.min(p), b.max(p)),
        })
    })
}
fn hull(mut points: Vec<DVec2>) -> Vec<DVec2> {
    points.sort_unstable_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let mut result = Vec::<DVec2>::new();
    for &p in &points {
        while result.len() >= 2
            && (result[result.len() - 1] - result[result.len() - 2])
                .perp_dot(p - result[result.len() - 1])
                <= 0.0
        {
            result.pop();
        }
        result.push(p);
    }
    let lower = result.len();
    for &p in points.iter().rev().skip(1) {
        while result.len() > lower
            && (result[result.len() - 1] - result[result.len() - 2])
                .perp_dot(p - result[result.len() - 1])
                <= 0.0
        {
            result.pop();
        }
        result.push(p);
    }
    result.pop();
    result
}
/// Evaluate hull-edge rectangles plus exact plate-boundary angles within every
/// caliper interval. A long part may fit diagonally even though its minimum-area
/// rectangle does not fit either plate-axis assignment.
fn yaw(points: &[DVec3], rotation: DQuat, volume: BuildVolume) -> DQuat {
    let polygon = hull(points.iter().map(|p| (rotation * *p).truncate()).collect());
    if polygon.len() < 2 {
        return rotation;
    }
    let mut breaks = vec![0.0, std::f64::consts::PI];
    for i in 0..polygon.len() {
        let edge = polygon[(i + 1) % polygon.len()] - polygon[i];
        let a = (-edge.y.atan2(edge.x)).rem_euclid(std::f64::consts::FRAC_PI_2);
        breaks.extend([a, a + std::f64::consts::FRAC_PI_2]);
    }
    breaks.sort_by(f64::total_cmp);
    breaks.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    let mut angles = breaks.clone();
    for interval in breaks.windows(2) {
        let mid = (interval[0] + interval[1]) * 0.5;
        let (s, c) = mid.sin_cos();
        let x = |p: &DVec2| c * p.x - s * p.y;
        let y = |p: &DVec2| s * p.x + c * p.y;
        let dx = *polygon.iter().max_by(|a, b| x(a).total_cmp(&x(b))).unwrap()
            - *polygon.iter().min_by(|a, b| x(a).total_cmp(&x(b))).unwrap();
        let dy = *polygon.iter().max_by(|a, b| y(a).total_cmp(&y(b))).unwrap()
            - *polygon.iter().min_by(|a, b| y(a).total_cmp(&y(b))).unwrap();
        for (a, b, limit) in [(dx.x, -dx.y, volume.x_mm()), (dy.y, dy.x, volume.y_mm())] {
            let radius = a.hypot(b);
            if radius < limit || radius <= 1e-12 {
                continue;
            }
            let phi = b.atan2(a);
            let offset = (limit / radius).clamp(-1.0, 1.0).acos();
            for root in [phi - offset, phi + offset] {
                for turn in [-std::f64::consts::TAU, 0.0, std::f64::consts::TAU] {
                    let angle = root + turn;
                    if angle >= interval[0] - 1e-12 && angle <= interval[1] + 1e-12 {
                        angles.push(angle);
                    }
                }
            }
        }
    }
    let mut best = (false, f64::INFINITY, 0.0);
    for a in angles {
        let (s, c) = a.sin_cos();
        let (lo, hi) = bounds(
            polygon
                .iter()
                .map(|p| DVec3::new(c * p.x - s * p.y, s * p.x + c * p.y, 0.0)),
        )
        .unwrap();
        let size = hi - lo;
        let fits = size.x <= volume.x_mm() + 1e-5 && size.y <= volume.y_mm() + 1e-5;
        let area = size.x * size.y;
        if (fits && !best.0) || (fits == best.0 && area < best.1 - 1e-9) {
            best = (fits, area, a);
        }
    }
    DQuat::from_rotation_z(best.2) * rotation
}

fn evaluate(
    points: &[DVec3],
    triangles: &[([DVec3; 3], DVec3, f64)],
    rotation: DQuat,
    volume: BuildVolume,
    threshold: f64,
    optimize_yaw: bool,
) -> Candidate {
    let rotation = if optimize_yaw {
        yaw(points, rotation, volume)
    } else {
        rotation
    };
    let (min, max) = bounds(points.iter().map(|p| rotation * *p)).unwrap();
    let size = max - min;
    let mut metrics = Metrics {
        height: size.z,
        width: size.x,
        depth: size.y,
        footprint: size.x * size.y,
        fits: size.x <= volume.x_mm() + 1e-5
            && size.y <= volume.y_mm() + 1e-5
            && size.z <= volume.z_mm() + 1e-5,
        ..Default::default()
    };
    let mut sections = [0.0f64; 24];
    for (tri, normal, detail) in triangles {
        let t = tri.map(|p| rotation * p);
        let normal = rotation * *normal;
        let area = normal.length() * 0.5;
        let sample_weight = normal.length() / (t[1] - t[0]).cross(t[2] - t[0]).length();
        let nz = normal.z / normal.length();
        let lo = t.iter().map(|v| v.z).fold(f64::INFINITY, f64::min);
        let hi = t.iter().map(|v| v.z).fold(f64::NEG_INFINITY, f64::max);
        // The automatic generator lifts even bed-contact faces.
        if -nz > threshold + 1e-10 {
            metrics.support_area += area * -nz;
            metrics.contact_area += area * -nz * std::f64::consts::PI * 0.04 / 16.0;
            metrics.detail_contact_area += area * -nz * detail;
        }
        if nz.abs() < 1.0 - 1e-8 {
            metrics.stair_area += area * nz.abs().powi(4);
        }
        for (i, section) in sections.iter_mut().enumerate() {
            let z = min.z + size.z * (i as f64 + 0.5) / 24.0;
            if z < lo || z >= hi {
                continue;
            }
            let mut hits = [DVec3::ZERO; 2];
            let mut count = 0;
            for j in 0..3 {
                let a = t[j];
                let b = t[(j + 1) % 3];
                if (a.z <= z && b.z > z) || (b.z <= z && a.z > z) {
                    if count < 2 {
                        hits[count] = a + (b - a) * ((z - a.z) / (b.z - a.z));
                    }
                    count += 1;
                }
            }
            if count == 2 {
                let [a, b] = hits;
                let sign = if (b - a).dot(DVec3::Z.cross(normal)) >= 0.0 {
                    1.0
                } else {
                    -1.0
                };
                *section += sign * (a.x * b.y - b.x * a.y) * 0.5 * sample_weight;
            }
        }
    }
    metrics.peak_area = sections.into_iter().map(f64::abs).fold(0.0, f64::max);
    Candidate {
        rotation,
        min,
        max,
        metrics,
    }
}
fn costs(c: &Candidate) -> [f64; 4] {
    let m = c.metrics;
    [
        m.support_area + m.minima as f64 * 4.0,
        m.height,
        m.stair_area + m.peak_area * 0.5 + m.contact_area * 32.0 + m.detail_contact_area * 0.25,
        m.footprint,
    ]
}
/// Search several hundred sphere directions and large face normals, then refine
/// the best four. Scoring samples at most 4096 triangles; final bounds and yaw
/// use every vertex. Original fitting placement is always available as fallback.
/// A finite directional search cannot prove that an arbitrary mesh never fits.
pub fn search(
    scene: &Scene,
    id: NodeId,
    volume: BuildVolume,
    support_angle_deg: f64,
    weights: [f64; 4],
) -> Option<Proposal> {
    if weights.iter().any(|v| !v.is_finite() || *v < 0.0) || weights.iter().sum::<f64>() <= 0.0 {
        return None;
    }
    let mesh = sc_scene::bake_leaf_to_world(scene, id).ok()?;
    let vertices: Vec<_> = mesh
        .vertices
        .iter()
        .map(|v| DVec3::from_array(v.coords().map(f64::from)))
        .collect();
    let (old_min, old_max) = bounds(vertices.iter().copied())?;
    if vertices.iter().any(|v| !v.is_finite()) || !old_min.is_finite() || !old_max.is_finite() {
        return None;
    }
    let triangles = triangle_sample(&mesh, &vertices);
    if triangles.is_empty() {
        return None;
    }
    let sample_stride = vertices.len().div_ceil(2048).max(1);
    let sample: Vec<_> = vertices.iter().step_by(sample_stride).copied().collect();
    let threshold = support_angle_deg.clamp(0.0, 90.0).to_radians().cos();
    let original = evaluate(
        &vertices,
        &triangles,
        DQuat::IDENTITY,
        volume,
        threshold,
        false,
    );
    let mut directions = vec![
        DVec3::Z,
        -DVec3::Z,
        DVec3::X,
        -DVec3::X,
        DVec3::Y,
        -DVec3::Y,
    ];
    for i in 0..320 {
        let z = 1.0 - 2.0 * (i as f64 + 0.5) / 320.0;
        let angle = i as f64 * std::f64::consts::PI * (3.0 - 5.0f64.sqrt());
        let r = (1.0 - z * z).sqrt();
        directions.push(DVec3::new(r * angle.cos(), r * angle.sin(), z));
    }
    let mut faces: Vec<_> = triangles.iter().map(|(_, n, _)| *n).collect();
    faces.sort_unstable_by(|a, b| b.length_squared().total_cmp(&a.length_squared()));
    let mut clusters = Vec::<DVec3>::new();
    for n in faces {
        let n = n.normalize();
        if clusters.iter().all(|c| c.dot(n) < 0.985) {
            clusters.push(n);
            if clusters.len() == 16 {
                break;
            }
        }
    }
    for n in clusters {
        for n in [n, -n] {
            directions.push(n);
            let tangent = n.any_orthonormal_vector();
            for degrees in [5.0f64, 10.0, 20.0] {
                for sign in [-1.0, 1.0] {
                    directions
                        .push(DQuat::from_axis_angle(tangent, sign * degrees.to_radians()) * n);
                }
            }
        }
    }
    let mut candidates: Vec<_> = directions
        .iter()
        .map(|up| {
            evaluate(
                &sample,
                &triangles,
                DQuat::from_rotation_arc(*up, DVec3::Z),
                volume,
                threshold,
                true,
            )
        })
        .collect();
    candidates.push(original.clone());
    let mut low = [f64::INFINITY; 4];
    let mut high = [f64::NEG_INFINITY; 4];
    for c in candidates.iter().filter(|c| c.metrics.fits) {
        for (i, v) in costs(c).into_iter().enumerate() {
            low[i] = low[i].min(v);
            high[i] = high[i].max(v);
        }
    }
    if !low[0].is_finite() {
        // A refinement may find the first feasible pose. Until then use
        // physical model scales, rather than infinities, for its objectives.
        let area = triangles
            .iter()
            .map(|(_, n, _)| n.length() * 0.5)
            .sum::<f64>()
            .max(1e-12);
        let diagonal = (old_max - old_min).length().max(1e-12);
        low = [0.0; 4];
        high = [area, diagonal, area, diagonal * diagonal];
    }
    let score = |c: &Candidate| {
        costs(c)
            .iter()
            .enumerate()
            .map(|(i, v)| weights[i] * (v - low[i]) / (high[i] - low[i]).max(1e-9))
            .sum::<f64>()
    };
    let rank = |a: &Candidate, b: &Candidate| {
        b.metrics.fits.cmp(&a.metrics.fits).then_with(|| {
            if a.metrics.fits {
                score(a).total_cmp(&score(b))
            } else {
                fit_ratio(a.metrics, volume).total_cmp(&fit_ratio(b.metrics, volume))
            }
        })
    };
    candidates.sort_by(rank);
    let mut refined = Vec::new();
    for c in candidates.iter().take(4) {
        for degrees in [-5.0f64, -2.0, 2.0, 5.0] {
            for axis in [DVec3::X, DVec3::Y] {
                refined.push(evaluate(
                    &sample,
                    &triangles,
                    DQuat::from_axis_angle(axis, degrees.to_radians()) * c.rotation,
                    volume,
                    threshold,
                    true,
                ));
            }
        }
    }
    candidates.extend(refined);
    candidates.sort_by(rank);
    // Never accept approximate extents as evidence of fit. Recheck finalists
    // using the complete mesh, retaining the original as an explicit candidate.
    // Sample vertices are a subset of full vertices. If their exact yaw
    // problem is infeasible, the full projection cannot fit that up-vector.
    let shortlist: Vec<_> = candidates
        .iter()
        .filter(|c| c.metrics.fits)
        .take(16)
        .collect();
    let mut finalists: Vec<_> = shortlist
        .iter()
        .map(|c| evaluate(&vertices, &triangles, c.rotation, volume, threshold, true))
        .collect();
    finalists.push(original.clone());
    finalists.iter_mut().for_each(|c| {
        c.metrics.minima = minimum_patches(&vertices, &mesh.faces, c.rotation);
        c.metrics.contact_area += c.metrics.minima as f64 * std::f64::consts::PI * 0.04;
    });
    if !finalists.iter().any(|c| c.metrics.fits) {
        for c in candidates.iter().filter(|c| c.metrics.fits).skip(16) {
            let mut c = evaluate(&vertices, &triangles, c.rotation, volume, threshold, true);
            if c.metrics.fits {
                c.metrics.minima = minimum_patches(&vertices, &mesh.faces, c.rotation);
                c.metrics.contact_area += c.metrics.minima as f64 * std::f64::consts::PI * 0.04;
                finalists.push(c);
                break;
            }
        }
    }
    let mut original = original;
    original.metrics.minima = minimum_patches(&vertices, &mesh.faces, DQuat::IDENTITY);
    original.metrics.contact_area += original.metrics.minima as f64 * std::f64::consts::PI * 0.04;
    let best = finalists
        .iter()
        .filter(|c| c.metrics.fits)
        .min_by(|a, b| score(a).total_cmp(&score(b)));
    let Some(best) = best else {
        return Some(Proposal {
            delta: Transform3D::Identity,
            original: original.metrics,
            chosen: original.metrics,
            retained: true,
        });
    };
    if original.metrics.fits && score(&original) <= score(best) + 0.005 {
        return Some(Proposal {
            delta: Transform3D::Identity,
            original: original.metrics,
            chosen: original.metrics,
            retained: true,
        });
    }
    let (axis, angle) = best.rotation.to_axis_angle();
    let rotation =
        Transform3D::Affine(AffineTransform3D::from_axis_angle(axis.to_array(), angle).ok()?);
    // Preserve existing packing whenever possible. Clamp the old centre only
    // as far as needed to keep the new footprint inside the active plate.
    let old_centre = (old_min + old_max) * 0.5;
    let spare_x = ((volume.x_mm() - best.metrics.width) * 0.5).max(0.0);
    let spare_y = ((volume.y_mm() - best.metrics.depth) * 0.5).max(0.0);
    let centre_x = old_centre.x.clamp(-spare_x, spare_x);
    let centre_y = old_centre.y.clamp(-spare_y, spare_y);
    let delta = Transform3D::Affine(AffineTransform3D::from_translation([
        centre_x - (best.min.x + best.max.x) * 0.5,
        centre_y - (best.min.y + best.max.y) * 0.5,
        -best.min.z,
    ]))
    .compose(&rotation);
    Some(Proposal {
        delta,
        original: original.metrics,
        chosen: best.metrics,
        retained: false,
    })
}

/// Measure an existing baked orientation without running the search. Uses the
/// same deterministic triangle sample and full vertex bounds as the search.
pub fn measure(
    scene: &Scene,
    id: NodeId,
    volume: BuildVolume,
    support_angle_deg: f64,
) -> Option<Metrics> {
    let mesh = sc_scene::bake_leaf_to_world(scene, id).ok()?;
    let vertices: Vec<_> = mesh
        .vertices
        .iter()
        .map(|v| DVec3::from_array(v.coords().map(f64::from)))
        .collect();
    if vertices.is_empty() {
        return None;
    }
    let triangles = triangle_sample(&mesh, &vertices);
    {
        let mut m = evaluate(
            &vertices,
            &triangles,
            DQuat::IDENTITY,
            volume,
            support_angle_deg.to_radians().cos(),
            false,
        )
        .metrics;
        m.minima = minimum_patches(&vertices, &mesh.faces, DQuat::IDENTITY);
        m.contact_area += m.minima as f64 * std::f64::consts::PI * 0.04;
        Some(m)
    }
}

// Same topological criterion as the real support sampler: flood equal-height
// edges, reject any patch with a lower neighbour, and require a downward face.
fn minimum_patches(
    vertices: &[DVec3],
    faces: &[sc_mesh_core::IndexedTriangle],
    rotation: DQuat,
) -> usize {
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let up = rotation.conjugate() * DVec3::Z;
    let heights: Vec<_> = vertices.iter().map(|v| v.dot(up)).collect();
    let mut parent: Vec<_> = (0..vertices.len()).collect();
    let mut eligible = vec![true; vertices.len()];
    let mut downward = vec![false; vertices.len()];
    for f in faces {
        let ids = f.vertices.map(|i| i.get() as usize);
        let [a, b, c] = ids.map(|i| vertices[i]);
        let down = (b - a).cross(c - a).dot(up) < -1e-12;
        for &i in &ids {
            downward[i] |= down;
            for &j in &ids {
                if heights[j] < heights[i] - 1e-5 {
                    eligible[i] = false;
                } else if i != j && (heights[j] - heights[i]).abs() <= 1e-5 {
                    let a = root(&mut parent, i);
                    let b = root(&mut parent, j);
                    parent[a] = b;
                }
            }
        }
    }
    let mut minima = vec![true; vertices.len()];
    let mut normals = vec![false; vertices.len()];
    for i in 0..vertices.len() {
        let r = root(&mut parent, i);
        minima[r] &= eligible[i];
        normals[r] |= downward[i];
    }
    (0..vertices.len())
        .filter(|&i| parent[i] == i && minima[i] && normals[i])
        .count()
}

// Area-weighted vertex normals provide a geometric detail proxy without a
// semantic surface map. Sharp changes carry a larger contact penalty than a
// smooth patch. The deterministic face sample keeps all score terms bounded.
fn triangle_sample(
    mesh: &sc_mesh_core::IndexedMesh,
    vertices: &[DVec3],
) -> Vec<([DVec3; 3], DVec3, f64)> {
    let mut normals = vec![DVec3::ZERO; vertices.len()];
    for f in &mesh.faces {
        let [Some(a), Some(b), Some(c)] =
            f.vertices.map(|i| vertices.get(i.get() as usize).copied())
        else {
            continue;
        };
        let n = (b - a).cross(c - a);
        for i in f.vertices {
            normals[i.get() as usize] += n;
        }
    }
    let stride = mesh.faces.len().div_ceil(4096).max(1);
    mesh.faces
        .iter()
        .step_by(stride)
        .filter_map(|f| {
            let [a, b, c] = f.vertices.map(|i| vertices.get(i.get() as usize).copied());
            let t = [a?, b?, c?];
            let n = (t[1] - t[0]).cross(t[2] - t[0]);
            if !n.is_finite() || n.length_squared() <= 1e-16 {
                return None;
            }
            let direction = n.normalize();
            let detail = f
                .vertices
                .iter()
                .map(|i| {
                    1.0 - direction
                        .dot(normals[i.get() as usize].normalize_or_zero())
                        .abs()
                        .clamp(0.0, 1.0)
                })
                .sum::<f64>()
                / 3.0;
            Some((t, n * stride as f64, detail))
        })
        .collect()
}

// Infeasible candidates refine around the smallest envelope violation instead
// of an undefined normalized score when there are no fitting coarse poses.
fn fit_ratio(metrics: Metrics, volume: BuildVolume) -> f64 {
    (metrics.width / volume.x_mm())
        .max(metrics.depth / volume.y_mm())
        .max(metrics.height / volume.z_mm())
}

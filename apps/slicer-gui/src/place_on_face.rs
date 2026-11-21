// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! "Place on face", after Orca Slicer's tool of the same name: the flat sides
//! of a model's convex hull are the faces it can stand on. Hovering one
//! highlights it; clicking turns the model so that side rests on the plate.
//!
//! Only sides the model would stand on stably are offered: the centre of mass,
//! dropped straight down onto the side, must land inside it.

use glam::{DVec2, DVec3};

/// One flat side of the hull, in world millimetres.
#[derive(Clone, Debug)]
pub(crate) struct Face {
    /// Outward unit normal: this direction ends up pointing down.
    pub normal: DVec3,
    /// Convex outline, counter-clockwise seen from outside.
    pub outline: Vec<DVec3>,
    pub area: f64,
}

/// Most faces Orca offers at once; more only clutters the view.
const MAX_FACES: usize = 254;
/// Hull facets within this angle (cosine of ~1°) are one flat side.
const SAME_SIDE_COS: f64 = 0.999_85;

/// The flat sides a model with these (world) vertices can be placed on,
/// largest first. `centre` is the centre of mass.
pub(crate) fn faces(points: &[DVec3], centre: DVec3) -> Vec<Face> {
    let hull = hull(points);
    if hull.is_empty() {
        return Vec::new();
    }
    // Group hull facets by normal: on a convex body one outward direction
    // is one side.
    let mut facets: Vec<(DVec3, f64, [usize; 3])> = hull
        .iter()
        .filter_map(|&[a, b, c]| {
            let n = (points[b] - points[a]).cross(points[c] - points[a]);
            let len = n.length();
            (len > 0.0 && len.is_finite()).then(|| (n / len, len * 0.5, [a, b, c]))
        })
        .collect();
    facets.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut groups: Vec<(DVec3, Vec<usize>)> = Vec::new();
    for (n, _, tri) in &facets {
        match groups.iter_mut().find(|(g, _)| g.dot(*n) >= SAME_SIDE_COS) {
            Some((_, vertices)) => vertices.extend_from_slice(tri),
            None => groups.push((*n, tri.to_vec())),
        }
    }
    let mut out: Vec<Face> = groups
        .into_iter()
        .filter_map(|(normal, mut vertices)| {
            vertices.sort_unstable();
            vertices.dedup();
            let u = normal.any_orthonormal_vector();
            let v = normal.cross(u);
            let origin = points[vertices[0]];
            let flat: Vec<DVec2> = vertices
                .iter()
                .map(|&i| {
                    let p = points[i] - origin;
                    DVec2::new(p.dot(u), p.dot(v))
                })
                .collect();
            let polygon = hull_2d(flat);
            if polygon.len() < 3 {
                return None;
            }
            let area = polygon_area(&polygon);
            let c = centre - origin;
            if !inside(&polygon, DVec2::new(c.dot(u), c.dot(v))) {
                return None;
            }
            let outline = polygon.iter().map(|p| origin + u * p.x + v * p.y).collect();
            Some(Face {
                normal,
                outline,
                area,
            })
        })
        .collect();
    let largest = out.iter().map(|f| f.area).fold(0.0, f64::max);
    out.retain(|f| f.area >= 1.0_f64.max(largest * 0.002));
    out.sort_by(|a, b| b.area.total_cmp(&a.area));
    out.truncate(MAX_FACES);
    out
}

/// The face a ray (world space) hits first, if any.
pub(crate) fn pick(faces: &[Face], origin: DVec3, direction: DVec3) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, f) in faces.iter().enumerate() {
        let facing = f.normal.dot(direction);
        if facing >= -1e-9 {
            continue;
        }
        let t = (f.outline[0] - origin).dot(f.normal) / facing;
        if t <= 0.0 || best.is_some_and(|(_, b)| t >= b) {
            continue;
        }
        let hit = origin + direction * t;
        let n = f.outline.len();
        let within = (0..n).all(|k| {
            let a = f.outline[k];
            let b = f.outline[(k + 1) % n];
            (b - a).cross(hit - a).dot(f.normal) >= -1e-6 * (b - a).length().max(1.0)
        });
        if within {
            best = Some((i, t));
        }
    }
    best.map(|(i, _)| i)
}

/// Convex hull of a point cloud as outward-facing triangles (indices into
/// `points`). Quickhull: every step floods out the faces the farthest
/// outside point sees and replaces them with a cone to their horizon.
/// Empty when the points are (nearly) flat.
pub(crate) fn hull(points: &[DVec3]) -> Vec<[usize; 3]> {
    if points.len() < 4 {
        return Vec::new();
    }
    let (lo, hi) = points.iter().fold(
        (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    let size = (hi - lo).length();
    if !size.is_finite() || size <= 0.0 {
        return Vec::new();
    }
    let eps = size * 1e-9;
    // Initial simplex: the two points farthest apart along x, the point
    // farthest from their line, and the point farthest from that plane.
    let extreme = |key: &dyn Fn(&DVec3) -> f64| {
        (0..points.len())
            .max_by(|&a, &b| key(&points[a]).total_cmp(&key(&points[b])))
            .unwrap()
    };
    let axis = {
        let d = hi - lo;
        if d.x >= d.y && d.x >= d.z {
            DVec3::X
        } else if d.y >= d.z {
            DVec3::Y
        } else {
            DVec3::Z
        }
    };
    let a = extreme(&|p| -p.dot(axis));
    let b = extreme(&|p| p.dot(axis));
    let ab = (points[b] - points[a]).normalize_or_zero();
    let c = extreme(&|p| {
        let w = *p - points[a];
        (w - ab * w.dot(ab)).length_squared()
    });
    let plane = (points[b] - points[a]).cross(points[c] - points[a]);
    if plane.length() <= eps * size {
        return Vec::new();
    }
    let plane = plane.normalize();
    let d = extreme(&|p| (*p - points[a]).dot(plane).abs());
    let side = (points[d] - points[a]).dot(plane);
    if side.abs() <= eps * 10.0 {
        return Vec::new();
    }

    struct Facet {
        v: [usize; 3],
        n: DVec3,
        off: f64,
        outside: Vec<usize>,
        alive: bool,
    }
    let make = |v: [usize; 3]| {
        let n = (points[v[1]] - points[v[0]])
            .cross(points[v[2]] - points[v[0]])
            .normalize_or_zero();
        Facet {
            v,
            n,
            off: n.dot(points[v[0]]),
            outside: Vec::new(),
            alive: true,
        }
    };
    // Orient the tetrahedron outward (away from the fourth vertex).
    let mut facets: Vec<Facet> = if side > 0.0 {
        vec![
            make([a, c, b]),
            make([a, b, d]),
            make([b, c, d]),
            make([c, a, d]),
        ]
    } else {
        vec![
            make([a, b, c]),
            make([a, d, b]),
            make([b, d, c]),
            make([c, d, a]),
        ]
    };
    let used = [a, b, c, d];
    for (i, p) in points.iter().enumerate() {
        if used.contains(&i) {
            continue;
        }
        if let Some(f) = facets.iter_mut().find(|f| f.n.dot(*p) - f.off > eps) {
            f.outside.push(i);
        }
    }
    // Directed edge -> facet owning it; the facet across edge (x, y) owns
    // (y, x). Visibility floods out from the eye's facet through this map,
    // so each step only touches the patch it replaces.
    let mut owner = std::collections::HashMap::<(usize, usize), usize>::new();
    for (i, f) in facets.iter().enumerate() {
        let [x, y, z] = f.v;
        owner.extend([((x, y), i), ((y, z), i), ((z, x), i)]);
    }
    let mut work: Vec<usize> = (0..facets.len()).collect();
    while let Some(fi) = work.pop() {
        if !facets[fi].alive || facets[fi].outside.is_empty() {
            continue;
        }
        let f = &facets[fi];
        let eye = *f
            .outside
            .iter()
            .max_by(|&&x, &&y| {
                (f.n.dot(points[x]) - f.off).total_cmp(&(f.n.dot(points[y]) - f.off))
            })
            .unwrap();
        let p = points[eye];
        let mut visible = vec![fi];
        let mut seen = std::collections::HashSet::from([fi]);
        let mut horizon = Vec::new();
        let mut k = 0;
        while k < visible.len() {
            let [x, y, z] = facets[visible[k]].v;
            k += 1;
            for (a, b) in [(x, y), (y, z), (z, x)] {
                let Some(&n) = owner.get(&(b, a)) else {
                    continue;
                };
                if seen.contains(&n) {
                    continue;
                }
                if facets[n].n.dot(p) - facets[n].off > eps {
                    seen.insert(n);
                    visible.push(n);
                } else {
                    horizon.push((a, b));
                }
            }
        }
        // A neighbour first rejected can be reached again and accepted by a
        // later facet; keep only edges whose far side really stays.
        horizon.retain(|&(a, b)| owner.get(&(b, a)).is_none_or(|n| !seen.contains(n)));
        let mut orphans = Vec::new();
        for &i in &visible {
            facets[i].alive = false;
            orphans.append(&mut facets[i].outside);
            let [x, y, z] = facets[i].v;
            for e in [(x, y), (y, z), (z, x)] {
                if owner.get(&e) == Some(&i) {
                    owner.remove(&e);
                }
            }
        }
        let first_new = facets.len();
        for (x, y) in horizon {
            let i = facets.len();
            facets.push(make([x, y, eye]));
            owner.extend([((x, y), i), ((y, eye), i), ((eye, x), i)]);
        }
        for i in orphans {
            if i == eye {
                continue;
            }
            let q = points[i];
            if let Some(f) = facets[first_new..]
                .iter_mut()
                .find(|f| f.n.dot(q) - f.off > eps)
            {
                f.outside.push(i);
            }
        }
        work.extend(first_new..facets.len());
    }
    facets
        .into_iter()
        .filter(|f| f.alive && f.n != DVec3::ZERO)
        .map(|f| f.v)
        .collect()
}

fn hull_2d(mut points: Vec<DVec2>) -> Vec<DVec2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup_by(|a, b| (*a - *b).length_squared() < 1e-18);
    if points.len() < 3 {
        return points;
    }
    let mut result: Vec<DVec2> = Vec::with_capacity(points.len() * 2);
    for pass in 0..2 {
        let start = result.len();
        let iter: Box<dyn Iterator<Item = &DVec2>> = if pass == 0 {
            Box::new(points.iter())
        } else {
            Box::new(points.iter().rev())
        };
        for &p in iter {
            while result.len() >= start + 2 {
                let n = result.len();
                if (result[n - 1] - result[n - 2]).perp_dot(p - result[n - 1]) <= 1e-12 {
                    result.pop();
                } else {
                    break;
                }
            }
            result.push(p);
        }
        result.pop();
    }
    result
}

fn polygon_area(polygon: &[DVec2]) -> f64 {
    let n = polygon.len();
    (0..n)
        .map(|i| polygon[i].perp_dot(polygon[(i + 1) % n]))
        .sum::<f64>()
        .abs()
        * 0.5
}

fn inside(polygon: &[DVec2], p: DVec2) -> bool {
    let n = polygon.len();
    (0..n).all(|i| (polygon[(i + 1) % n] - polygon[i]).perp_dot(p - polygon[i]) >= -1e-9)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_points(lo: DVec3, hi: DVec3) -> Vec<DVec3> {
        (0..8)
            .map(|i| {
                DVec3::new(
                    if i & 1 == 0 { lo.x } else { hi.x },
                    if i & 2 == 0 { lo.y } else { hi.y },
                    if i & 4 == 0 { lo.z } else { hi.z },
                )
            })
            .collect()
    }

    #[test]
    fn a_box_has_six_sides_with_their_true_areas() {
        let mut pts = box_points(DVec3::ZERO, DVec3::new(40.0, 20.0, 10.0));
        // Interior and on-face points must not add sides.
        pts.push(DVec3::new(20.0, 10.0, 5.0));
        pts.push(DVec3::new(20.0, 10.0, 0.0));
        let f = faces(&pts, DVec3::new(20.0, 10.0, 5.0));
        assert_eq!(f.len(), 6, "{f:?}");
        assert!((f[0].area - 800.0).abs() < 1e-6);
        assert!((f[2].area - 400.0).abs() < 1e-6);
        assert!((f[4].area - 200.0).abs() < 1e-6);
        for face in &f {
            // Outline lies in its plane and winds counter-clockwise outside.
            let n = (face.outline[1] - face.outline[0]).cross(face.outline[2] - face.outline[0]);
            assert!(n.normalize().dot(face.normal) > 0.999);
        }
    }

    #[test]
    fn unstable_sides_are_not_offered() {
        // A tall wedge whose centre of mass overhangs its sloped side.
        let pts = vec![
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(10.0, 0.0, 0.0),
            DVec3::new(0.0, 10.0, 0.0),
            DVec3::new(10.0, 10.0, 0.0),
            DVec3::new(0.0, 0.0, 60.0),
            DVec3::new(0.0, 10.0, 60.0),
        ];
        // Mass sits low near x = 0 (the vertical side) so it stays put there.
        let all = faces(&pts, DVec3::new(3.3, 5.0, 20.0));
        let bottom = all.iter().any(|f| f.normal.z < -0.99);
        assert!(bottom);
        // With the mass shifted far outside the base, the base is unstable.
        let none = faces(&pts, DVec3::new(30.0, 5.0, 20.0));
        assert!(!none.iter().any(|f| f.normal.z < -0.99));
    }

    #[test]
    fn three_feet_make_a_face_like_in_orca() {
        // A tripod: the plane through its feet is a side even though no
        // mesh triangle lies in it.
        let mut pts = vec![
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(30.0, 0.0, 0.0),
            DVec3::new(15.0, 26.0, 0.0),
        ];
        for z in [5.0, 10.0, 20.0] {
            pts.push(DVec3::new(15.0, 8.7, z));
        }
        pts.push(DVec3::new(10.0, 8.7, 20.0));
        pts.push(DVec3::new(20.0, 8.7, 20.0));
        let f = faces(&pts, DVec3::new(15.0, 8.7, 8.0));
        let base = f.iter().find(|f| f.normal.z < -0.999).expect("feet plane");
        assert!((base.area - 390.0).abs() < 1.0, "{}", base.area);
    }

    #[test]
    fn picking_returns_the_side_the_ray_meets_first() {
        let pts = box_points(DVec3::ZERO, DVec3::new(40.0, 20.0, 10.0));
        let f = faces(&pts, DVec3::new(20.0, 10.0, 5.0));
        let top = pick(&f, DVec3::new(10.0, 5.0, 100.0), -DVec3::Z).unwrap();
        assert!(f[top].normal.z > 0.999);
        let side = pick(&f, DVec3::new(-50.0, 5.0, 5.0), DVec3::X).unwrap();
        assert!(f[side].normal.x < -0.999);
        assert!(pick(&f, DVec3::new(100.0, 100.0, 100.0), -DVec3::Z).is_none());
    }

    /// A dense mesh (Benchy has ~100k vertices) must hull quickly: the
    /// first version rescanned every facet ever made on each step.
    #[test]
    fn a_dense_box_cloud_still_has_six_sides() {
        let mut pts = box_points(DVec3::ZERO, DVec3::new(30.0, 20.0, 10.0));
        let mut seed = 12345u64;
        let mut rnd = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        for _ in 0..60_000 {
            let mut p = DVec3::new(rnd() * 30.0, rnd() * 20.0, rnd() * 10.0);
            // Many on the surface, like a mesh's vertices.
            match (rnd() * 6.0) as u32 {
                0 => p.x = 0.0,
                1 => p.x = 30.0,
                2 => p.y = 0.0,
                3 => p.y = 20.0,
                4 => p.z = 0.0,
                _ => p.z = 10.0,
            }
            pts.push(p);
        }
        let started = std::time::Instant::now();
        let f = faces(&pts, DVec3::new(15.0, 10.0, 5.0));
        assert_eq!(f.len(), 6);
        assert!((f[0].area - 600.0).abs() < 1e-6);
        assert!(started.elapsed().as_secs_f64() < 5.0, "{:?}", started.elapsed());
    }

    #[test]
    fn a_sphere_like_cloud_hulls_without_panicking() {
        let mut pts = Vec::new();
        for i in 0..2000 {
            let z = 1.0 - 2.0 * (i as f64 + 0.5) / 2000.0;
            let a = i as f64 * 2.399_963;
            let r = (1.0 - z * z).sqrt();
            pts.push(DVec3::new(r * a.cos(), r * a.sin(), z) * 20.0);
        }
        let h = hull(&pts);
        // Euler: a closed triangulated hull of V vertices has 2V - 4 faces.
        assert_eq!(h.len(), 2 * 2000 - 4);
    }
}

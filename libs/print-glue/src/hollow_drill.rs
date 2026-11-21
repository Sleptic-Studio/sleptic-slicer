// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Drain holes cut as exact geometry (R111). Each hole is a convex prism
//! around its axis; only the triangles it touches are clipped, the prism's
//! own faces are split where the model's surface crosses them and kept where
//! they run through material, and the seams are welded closed. The rest of
//! the mesh is untouched, so the model's outside stays the original surface
//! instead of being rebuilt from voxels (the maintainer found drained models
//! "pixelated").

use super::{DrainHole, Tree};
use glam::DVec3;
use sc_mesh_core::{IndexedMesh, IndexedTriangle, Normal, Vertex};
use std::collections::{HashMap, HashSet};

/// Side tolerance for splitting, mm.
const EPS: f64 = 1e-9;
/// Points nearer than this are one vertex, mm. Where the hole's edge meets
/// a surface at a grazing angle the same point comes out of the clipped
/// triangle and of the split hole face a few microns apart (1 um left a
/// hollowed sphere open); 10 um is still far under a printer pixel.
const WELD: f64 = 1e-5;
/// A vertex nearer than this to an edge's line sits on the edge, mm.
const ON_EDGE: f64 = 1e-5;

type Polygon = Vec<DVec3>;

/// Prism faces per hole for a radius: facets at most 0.25 mm, 16 to 64.
fn sides_for(radius: f64) -> usize {
    ((std::f64::consts::TAU * radius / 0.25).ceil() as usize).clamp(16, 64)
}

/// The prism's bounding planes (outward normal, offset) and faces, each face
/// counter-clockwise about its outward normal. It starts a little outside
/// the entry point so it opens the surface cleanly even where the surface is
/// tilted to the axis, and its sides are tangent to the requested circle.
pub(super) fn prism(hole: &DrainHole) -> (Vec<(DVec3, f64)>, Vec<Polygon>) {
    let axis = DVec3::from_array(hole.direction).normalize();
    let radius = hole.diameter_mm * 0.5;
    let start = DVec3::from_array(hole.position) - axis * (radius + 0.5);
    let end = DVec3::from_array(hole.position) + axis * hole.depth_mm;
    let e1 = axis.any_orthonormal_vector();
    let e2 = axis.cross(e1);
    let n = sides_for(radius);
    let corner_radius = radius / (std::f64::consts::PI / n as f64).cos();
    let corner = |i: usize| {
        let a = std::f64::consts::TAU * i as f64 / n as f64;
        (e1 * a.cos() + e2 * a.sin()) * corner_radius
    };
    let mut planes = Vec::with_capacity(n + 2);
    let mut faces = Vec::with_capacity(n + 2);
    for i in 0..n {
        let a = std::f64::consts::TAU * (i as f64 + 0.5) / n as f64;
        let normal = e1 * a.cos() + e2 * a.sin();
        planes.push((normal, normal.dot(start) + radius));
        faces.push(oriented(
            vec![
                start + corner(i),
                start + corner(i + 1),
                end + corner(i + 1),
                end + corner(i),
            ],
            normal,
        ));
    }
    planes.push((-axis, -axis.dot(start)));
    faces.push(oriented((0..n).map(|i| start + corner(i)).collect(), -axis));
    planes.push((axis, axis.dot(end)));
    faces.push(oriented((0..n).map(|i| end + corner(i)).collect(), axis));
    (planes, faces)
}

fn newell(polygon: &[DVec3]) -> DVec3 {
    let mut n = DVec3::ZERO;
    for i in 0..polygon.len() {
        n += polygon[i].cross(polygon[(i + 1) % polygon.len()]);
    }
    n
}

fn oriented(mut polygon: Polygon, normal: DVec3) -> Polygon {
    if newell(&polygon).dot(normal) < 0.0 {
        polygon.reverse();
    }
    polygon
}

fn area(polygon: &[DVec3]) -> f64 {
    newell(polygon).length() * 0.5
}

/// Splits a convex polygon by a plane into the parts in front of it
/// (`normal . p > offset`) and behind it. Degenerate parts are dropped.
fn split(polygon: &[DVec3], normal: DVec3, offset: f64) -> (Polygon, Polygon) {
    let (mut front, mut back) = (Vec::new(), Vec::new());
    let side = |d: f64| {
        if d > EPS {
            1
        } else if d < -EPS {
            -1
        } else {
            0
        }
    };
    for i in 0..polygon.len() {
        let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
        let (da, db) = (normal.dot(a) - offset, normal.dot(b) - offset);
        let (sa, sb) = (side(da), side(db));
        if sa >= 0 {
            front.push(a);
        }
        if sa <= 0 {
            back.push(a);
        }
        if sa * sb < 0 {
            let x = a + (b - a) * (da / (da - db));
            front.push(x);
            back.push(x);
        }
    }
    let keep = |p: Polygon| {
        if p.len() >= 3 && area(&p) > 1e-14 {
            p
        } else {
            Vec::new()
        }
    };
    (keep(front), keep(back))
}

/// The parts of a convex polygon outside a convex solid given by its planes,
/// and whether any of it lies inside.
fn outside_parts(polygon: &[DVec3], planes: &[(DVec3, f64)]) -> (Vec<Polygon>, bool) {
    let mut outside = Vec::new();
    let mut rest = polygon.to_vec();
    for &(normal, offset) in planes {
        let (front, back) = split(&rest, normal, offset);
        if !front.is_empty() {
            outside.push(front);
        }
        rest = back;
        if rest.is_empty() {
            return (outside, false);
        }
    }
    (outside, true)
}

/// Whether `p` is inside the closed surface in `tree`, by the winding of
/// rays along +x. Two slightly offset rays must agree; a third settles a
/// ray that grazed an edge.
fn inside(tree: &Tree, p: DVec3) -> bool {
    let winding = |dy: f64, dz: f64| {
        let mut hits = Vec::new();
        tree.crossings(p.y + dy, p.z + dz, &mut hits);
        hits.sort_by(|a, b| a.0.total_cmp(&b.0));
        hits.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-8 && a.1 == b.1);
        hits.iter()
            .filter(|h| h.0 < p.x)
            .map(|h| h.1)
            .sum::<f64>()
            .abs()
            > 0.5
    };
    let (a, b) = (winding(1.314e-9, 2.718e-9), winding(-2.236e-9, 1.732e-9));
    if a == b {
        a
    } else {
        winding(3.162e-9, -1.414e-9)
    }
}

/// Cuts each hole out of the closed `mesh` in turn.
pub(super) fn drill(mesh: &IndexedMesh, holes: &[DrainHole]) -> Option<IndexedMesh> {
    let mut current = mesh.clone();
    for hole in holes {
        current = drill_one(&current, hole)?;
    }
    Some(current)
}

fn position(mesh: &IndexedMesh, i: u32) -> DVec3 {
    DVec3::from_array(mesh.vertices[i as usize].coords().map(f64::from))
}

fn drill_one(mesh: &IndexedMesh, hole: &DrainHole) -> Option<IndexedMesh> {
    let (planes, faces) = prism(hole);
    let corners: Vec<DVec3> = faces.iter().flatten().copied().collect();
    let lo = corners.iter().fold(DVec3::INFINITY, |a, &p| a.min(p)) - DVec3::splat(1e-6);
    let hi = corners.iter().fold(DVec3::NEG_INFINITY, |a, &p| a.max(p)) + DVec3::splat(1e-6);
    let ids = |f: &IndexedTriangle| f.vertices.map(u32::from);
    let points = |f: &IndexedTriangle| ids(f).map(|i| position(mesh, i));
    let near: Vec<usize> = (0..mesh.faces.len())
        .filter(|&i| {
            let p = points(&mesh.faces[i]);
            let (a, b) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
            a.cmple(hi).all() && b.cmpge(lo).all()
        })
        .collect();
    // The surface where the prism removes material.
    let mut touched = HashSet::new();
    let mut local: Vec<Polygon> = Vec::new();
    for &i in &near {
        let (outside, cut) = outside_parts(&points(&mesh.faces[i]), &planes);
        if cut {
            touched.insert(i);
            local.extend(outside);
        }
    }
    if touched.is_empty() {
        return Some(mesh.clone());
    }
    // The prism's faces, split along every line where the surface crosses
    // them; each piece is then wholly in material or wholly out of it.
    let tree = Tree::from_mesh(mesh);
    for face in &faces {
        let normal = newell(face).normalize();
        let offset = normal.dot(face[0]);
        let mut cuts: Vec<(DVec3, f64)> = Vec::new();
        for &i in &near {
            let p = points(&mesh.faces[i]);
            let d = p.map(|v| normal.dot(v) - offset);
            if d.iter().all(|&v| v > EPS)
                || d.iter().all(|&v| v < -EPS)
                || d.iter().all(|v| v.abs() <= EPS)
            {
                continue;
            }
            let mut crossing = Vec::new();
            for k in 0..3 {
                let (a, b) = (k, (k + 1) % 3);
                if d[a].abs() <= EPS {
                    crossing.push(p[a]);
                } else if d[a] * d[b] < 0.0 && d[b].abs() > EPS {
                    crossing.push(p[a] + (p[b] - p[a]) * (d[a] / (d[a] - d[b])));
                }
            }
            let (Some(&a), Some(&b)) = (
                crossing.first(),
                crossing.iter().max_by(|x, y| {
                    (**x - crossing[0])
                        .length_squared()
                        .total_cmp(&(**y - crossing[0]).length_squared())
                }),
            ) else {
                continue;
            };
            if (b - a).length_squared() < 1e-18 {
                continue;
            }
            // Only crossings that meet the face itself cut it.
            let (mut t0, mut t1) = (0.0f64, 1.0f64);
            for k in 0..face.len() {
                let edge = face[(k + 1) % face.len()] - face[k];
                let inward = normal.cross(edge);
                let (fa, fb) = (inward.dot(a - face[k]), inward.dot(b - face[k]));
                if fa < 0.0 && fb < 0.0 {
                    t1 = -1.0;
                    break;
                }
                if fa < 0.0 {
                    t0 = t0.max(fa / (fa - fb));
                } else if fb < 0.0 {
                    t1 = t1.min(fa / (fa - fb));
                }
            }
            if t1 - t0 <= 1e-9 {
                continue;
            }
            let along = (b - a).normalize();
            let m = normal.cross(along).normalize();
            let cut = (m, m.dot(a));
            let same = cuts.iter().any(|&(n, o)| {
                (n.dot(m) > 1.0 - 1e-12 && (o - cut.1).abs() < 1e-9)
                    || (n.dot(m) < -1.0 + 1e-12 && (o + cut.1).abs() < 1e-9)
            });
            if !same {
                cuts.push(cut);
            }
        }
        let mut pieces = vec![face.clone()];
        for &(m, o) in &cuts {
            let mut next = Vec::with_capacity(pieces.len() + 1);
            for piece in pieces {
                let (front, back) = split(&piece, m, o);
                if !front.is_empty() {
                    next.push(front);
                }
                if !back.is_empty() {
                    next.push(back);
                }
            }
            pieces = next;
        }
        for mut piece in pieces {
            let centre = piece.iter().copied().sum::<DVec3>() / piece.len() as f64;
            if inside(&tree, centre) {
                // The hole's wall faces into the hole.
                piece.reverse();
                local.push(piece);
            }
        }
    }
    Some(stitch(mesh, &touched, local))
}

/// Replaces the `touched` faces with the `local` polygons, welding the
/// polygons to each other and to the untouched mesh, and inserting the
/// vertices that lie along an edge (T-junctions) so the result is closed.
fn stitch(mesh: &IndexedMesh, touched: &HashSet<usize>, local: Vec<Polygon>) -> IndexedMesh {
    let touched_vertices: HashSet<u32> = touched
        .iter()
        .flat_map(|&i| mesh.faces[i].vertices.map(u32::from))
        .collect();
    // Untouched faces beside the cut join in, so their edges can take the
    // cut's new vertices.
    let neighbours: Vec<usize> = (0..mesh.faces.len())
        .filter(|i| !touched.contains(i))
        .filter(|&i| {
            mesh.faces[i]
                .vertices
                .iter()
                .any(|&v| touched_vertices.contains(&u32::from(v)))
        })
        .collect();
    let mut out = IndexedMesh::new(mesh.meta.clone(), mesh.vertices.clone(), Vec::new());
    let skip: HashSet<usize> = touched
        .iter()
        .copied()
        .chain(neighbours.iter().copied())
        .collect();
    out.faces = mesh
        .faces
        .iter()
        .enumerate()
        .filter(|(i, _)| !skip.contains(i))
        .map(|(_, f)| *f)
        .collect();
    let mut positions: Vec<DVec3> = Vec::new();
    let mut ids: Vec<u32> = Vec::new();
    let mut grid: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
    let key = |p: DVec3| (p / (WELD * 4.0)).floor().as_i64vec3().to_array();
    let mut weld = |p: DVec3, existing: Option<u32>, out: &mut IndexedMesh| -> u32 {
        let k = key(p);
        // Existing vertices keep their own ids, even where two coincide.
        for dz in if existing.is_some() { 1..=0 } else { -1..=1 } {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if let Some(list) = grid.get(&[k[0] + dx, k[1] + dy, k[2] + dz]) {
                        for &j in list {
                            if (positions[j] - p).length_squared() <= WELD * WELD {
                                return ids[j];
                            }
                        }
                    }
                }
            }
        }
        let id = existing.unwrap_or_else(|| {
            out.vertices
                .push(Vertex::new(p.to_array().map(|v| v as f32)));
            (out.vertices.len() - 1) as u32
        });
        grid.entry(k).or_default().push(positions.len());
        positions.push(p);
        ids.push(id);
        id
    };
    // Existing vertices first, so the cut reuses them.
    for &v in &touched_vertices {
        weld(position(mesh, v), Some(v), &mut out);
    }
    for &i in &neighbours {
        for v in mesh.faces[i].vertices.map(u32::from) {
            weld(position(mesh, v), Some(v), &mut out);
        }
    }
    let mut rings: Vec<Vec<u32>> = Vec::new();
    for &i in &neighbours {
        rings.push(mesh.faces[i].vertices.map(u32::from).to_vec());
    }
    for polygon in &local {
        let mut ring: Vec<u32> = polygon.iter().map(|&p| weld(p, None, &mut out)).collect();
        ring.dedup();
        while ring.len() > 1 && ring.first() == ring.last() {
            ring.pop();
        }
        if ring.len() >= 3 {
            rings.push(ring);
        }
    }
    // Vertices of the cut, by coarse cell, for finding the ones on an edge.
    let at = |v: u32, out: &IndexedMesh| {
        DVec3::from_array(out.vertices[v as usize].coords().map(f64::from))
    };
    let exact: HashMap<u32, DVec3> = ids.iter().copied().zip(positions.iter().copied()).collect();
    let point = |v: u32, out: &IndexedMesh| exact.get(&v).copied().unwrap_or_else(|| at(v, out));
    let cell = 0.5;
    let mut coarse: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
    let mut seen = HashSet::new();
    for ring in &rings {
        for &v in ring {
            if seen.insert(v) {
                let c = (point(v, &out) / cell).floor().as_i64vec3().to_array();
                coarse.entry(c).or_default().push(v);
            }
        }
    }
    for ring in rings {
        let mut full = Vec::with_capacity(ring.len());
        for k in 0..ring.len() {
            let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
            full.push(a);
            let (pa, pb) = (point(a, &out), point(b, &out));
            let delta = pb - pa;
            let length = delta.length_squared();
            if length < 1e-24 {
                continue;
            }
            let (lo, hi) = (
                (pa.min(pb) / cell).floor().as_i64vec3(),
                (pa.max(pb) / cell).floor().as_i64vec3(),
            );
            let mut on = Vec::new();
            for x in lo.x..=hi.x {
                for y in lo.y..=hi.y {
                    for z in lo.z..=hi.z {
                        for &v in coarse.get(&[x, y, z]).into_iter().flatten() {
                            if v == a || v == b {
                                continue;
                            }
                            let q = point(v, &out) - pa;
                            let t = q.dot(delta) / length;
                            if t > 1e-9
                                && t < 1.0 - 1e-9
                                && (q - delta * t).length_squared() < ON_EDGE * ON_EDGE
                            {
                                on.push((t, v));
                            }
                        }
                    }
                }
            }
            on.sort_by(|x, y| x.0.total_cmp(&y.0));
            on.dedup_by(|x, y| x.1 == y.1);
            full.extend(on.into_iter().map(|(_, v)| v));
        }
        for full in simple_rings(full) {
            let triangles: Vec<[u32; 3]> = if full.len() == 3 {
                vec![[full[0], full[1], full[2]]]
            } else {
                // Convex with collinear points along its edges: fan from the
                // centre, which never makes a sliver along an edge.
                let centre =
                    full.iter().map(|&v| point(v, &out)).sum::<DVec3>() / full.len() as f64;
                out.vertices
                    .push(Vertex::new(centre.to_array().map(|v| v as f32)));
                let c = (out.vertices.len() - 1) as u32;
                (0..full.len())
                    .map(|k| [c, full[k], full[(k + 1) % full.len()]])
                    .collect()
            };
            for t in triangles {
                let [a, b, c] = t.map(|v| point(v, &out));
                let normal = (b - a).cross(c - a);
                if normal.length_squared() < 1e-30 {
                    continue;
                }
                out.faces.push(IndexedTriangle::from_raw_indices(
                    Normal::new(normal.normalize().to_array().map(|v| v as f32)),
                    t,
                ));
            }
        }
    }
    out
}

/// A welded ring as simple rings. A sliver thinner than the weld folds back
/// on itself (`a b c b`), and a fan over it would leave a two-sided fin, so
/// out-and-back spikes are removed; a ring that still meets itself (a figure
/// eight) is split there. Rings under three corners go.
fn simple_rings(mut ring: Vec<u32>) -> Vec<Vec<u32>> {
    loop {
        ring.dedup();
        while ring.len() > 1 && ring.first() == ring.last() {
            ring.pop();
        }
        let n = ring.len();
        if n < 3 {
            return Vec::new();
        }
        match (0..n).find(|&k| ring[(k + n - 1) % n] == ring[(k + 1) % n]) {
            Some(k) => {
                // Drop the spike's tip and one copy of its base.
                let (tip, base) = (k, (k + 1) % n);
                let (first, second) = (tip.max(base), tip.min(base));
                ring.remove(first);
                ring.remove(second);
            }
            None => break,
        }
    }
    let n = ring.len();
    for i in 0..n {
        for j in i + 1..n {
            if ring[i] == ring[j] {
                let inner: Vec<u32> = ring[i..j].to_vec();
                let outer: Vec<u32> = ring[j..].iter().chain(&ring[..i]).copied().collect();
                return simple_rings(inner)
                    .into_iter()
                    .chain(simple_rings(outer))
                    .collect();
            }
        }
    }
    vec![ring]
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Plane sections use topological endpoint keys, never positional welding.
use crate::{contour::*, error::SliceError, geom2d::Point2D};
use i_overlay::{
    core::{fill_rule::FillRule, overlay::Overlay, overlay_rule::OverlayRule},
    i_float::int::point::IntPoint,
};
use sc_mesh_core::{IndexedMesh, indexed_mesh::MeshTopology};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Key(usize, usize, usize);
#[derive(Clone, Copy)]
struct End {
    point: Point2D,
    key: Key,
    order: usize,
}
#[derive(Clone, Copy)]
struct Passage {
    ends: [End; 2],
}
fn square(a: Point2D, b: Point2D) -> f64 {
    (a.x - b.x).powi(2) + (a.y - b.y).powi(2)
}
fn fixed(p: Point2D) -> Result<(i64, i64), SliceError> {
    let x = p.x * 100000.;
    let y = p.y * 100000.;
    if !x.is_finite()
        || !y.is_finite()
        || x.abs() >= (1u64 << 61) as f64
        || y.abs() >= (1u64 << 61) as f64
    {
        return Err(SliceError::TolerantCoordinateOutOfRange);
    }
    Ok((x.round() as i64, y.round() as i64))
}
pub(crate) fn union(rings: Vec<ClosedRing>) -> Result<Vec<Polygon>, SliceError> {
    if rings.is_empty() {
        return Ok(vec![]);
    }
    let mut paths = Vec::with_capacity(rings.len());
    for r in rings {
        let mut path = Vec::with_capacity(r.len());
        for &p in r.vertices() {
            let (x, y) = fixed(p)?;
            path.push(IntPoint::new(x, y))
        }
        paths.push(path)
    }
    let shapes = Overlay::<i64>::from_subj(&paths).overlay(OverlayRule::Subject, FillRule::NonZero);
    Ok(shapes
        .into_iter()
        .filter_map(|s| {
            let mut it = s.into_iter().map(|r| {
                ClosedRing::new(
                    r.into_iter()
                        .map(|p| Point2D::new(p.x as f64 / 100000., p.y as f64 / 100000.))
                        .collect(),
                )
            });
            let outer = it.next()?;
            Some(Polygon::new_unchecked(outer, it.collect()))
        })
        .collect())
}
fn passages(
    mesh: &IndexedMesh,
    faces: &[usize],
    z: f64,
    topology: Option<&MeshTopology>,
    limit: bool,
    tolerant: bool,
) -> Vec<Passage> {
    let mut out = Vec::with_capacity(faces.len());
    for &fi in faces {
        let face = &mesh.faces[fi];
        let ids = face.vertices.map(usize::from);
        let v = ids.map(|i| mesh.vertices[i].coords().map(|x| x as f64));
        let side = v.map(|p| {
            if tolerant {
                if p[2] > z { 1 } else { -1 }
            } else if p[2] - z > 1e-6 {
                1
            } else if p[2] - z < -1e-6 {
                -1
            } else {
                0
            }
        });
        let above = side.iter().filter(|&&s| s > 0).count();
        let below = side.iter().filter(|&&s| s < 0).count();
        let on = 3 - above - below;
        if !limit
            && !tolerant
            && (above == 3 || below == 3 || on == 3 || on == 1 && (above == 2 || below == 2))
        {
            continue;
        }
        let vertex_end = |i: usize| {
            let mut fan = 0;
            let mut order = fi;
            if let Some(t) = topology
                && let Some(fans) = t.vertex_fans(face.vertices[i])
            {
                for (j, fs) in fans.enumerate() {
                    if let Some(k) = fs.iter().position(|&f| usize::from(f) == fi) {
                        fan = j;
                        order = k;
                        break;
                    }
                }
            }
            End {
                point: Point2D::new(v[i][0], v[i][1]),
                key: Key(usize::MAX, ids[i], fan),
                order,
            }
        };
        let mut ends = [End {
            point: Point2D::new(0., 0.),
            key: Key(0, 0, 0),
            order: 0,
        }; 2];
        let mut count = 0;
        if on == 2 && !limit && !tolerant {
            let a = side.iter().position(|&s| s == 0).unwrap();
            let b = (a + 1..3)
                .find(|&i| side[i] == 0)
                .unwrap_or_else(|| (0..a).find(|&i| side[i] == 0).unwrap());
            let c = 3 - a - b;
            let Some(t) = topology else { continue };
            let Some(neighbors) = t.edge_faces(face.vertices[a], face.vertices[b]) else {
                continue;
            };
            let other = neighbors
                .iter()
                .copied()
                .find(|&f| usize::from(f) != fi)
                .map(usize::from)
                .unwrap();
            if fi < other {
                continue;
            }
            let third = mesh.faces[other]
                .vertices
                .iter()
                .copied()
                .find(|i| *i != face.vertices[a] && *i != face.vertices[b])
                .unwrap();
            let dz = mesh.vertices[usize::from(third)][2] as f64 - z;
            let os = if dz > 1e-6 {
                1
            } else if dz < -1e-6 {
                -1
            } else {
                0
            };
            if os * side[c] >= 0 {
                continue;
            }
            ends = [vertex_end(a), vertex_end(b)];
            count = 2;
        } else {
            let s = side.map(|s| if s == 0 && limit { -1 } else { s });
            if !tolerant && !limit {
                for (i, &si) in s.iter().enumerate() {
                    if si == 0 && count < 2 {
                        ends[count] = vertex_end(i);
                        count += 1
                    }
                }
            }
            for i in 0..3 {
                let j = (i + 1) % 3;
                if s[i] * s[j] >= 0 {
                    continue;
                }
                if count == 2 {
                    break;
                }
                let (a, b) = if ids[i] < ids[j] { (i, j) } else { (j, i) };
                let t = (z - v[a][2]) / (v[b][2] - v[a][2]);
                let p = Point2D::new(
                    (v[b][0] - v[a][0]).mul_add(t, v[a][0]),
                    (v[b][1] - v[a][1]).mul_add(t, v[a][1]),
                );
                ends[count] = if side[a] == 0 {
                    vertex_end(a)
                } else if side[b] == 0 {
                    vertex_end(b)
                } else {
                    End {
                        point: p,
                        key: Key(ids[a], ids[b], 0),
                        order: fi,
                    }
                };
                count += 1
            }
        }
        if count != 2 {
            continue;
        }
        let a = v[0];
        let b = v[1];
        let c = v[2];
        let nx = (b[1] - a[1]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[1] - a[1]);
        let ny = (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]);
        let d = Point2D::new(
            ends[1].point.x - ends[0].point.x,
            ends[1].point.y - ends[0].point.y,
        );
        if d.x * (-ny) + d.y * nx < 0. {
            ends.swap(0, 1)
        }
        out.push(Passage { ends });
    }
    out
}
fn strict_rings(segments: &[Passage], z: f64) -> Result<Vec<ClosedRing>, SliceError> {
    let mut records = Vec::with_capacity(2 * segments.len());
    for (i, s) in segments.iter().enumerate() {
        for k in 0..2 {
            records.push((s.ends[k].key, s.ends[k].order, i * 2 + k))
        }
    }
    records.sort_unstable();
    let mut links = vec![usize::MAX; records.len()];
    let mut bad = 0;
    let mut i = 0;
    while i < records.len() {
        let mut j = i + 1;
        while j < records.len() && records[j].0 == records[i].0 {
            j += 1
        }
        if (j - i) % 2 != 0 {
            bad += j - i
        } else {
            for pair in records[i..j].as_chunks::<2>().0 {
                links[pair[0].2] = pair[1].2;
                links[pair[1].2] = pair[0].2
            }
        }
        i = j
    }
    if bad > 0 {
        return Err(SliceError::NonManifoldDuringStitching {
            z_mm: z,
            dangling_endpoints: bad,
        });
    }
    let mut seen = vec![false; segments.len()];
    let mut rings = Vec::new();
    for start in 0..segments.len() {
        if seen[start] {
            continue;
        }
        let mut vertices = Vec::new();
        let mut current = start * 2;
        let mut vote = 0.;
        loop {
            let segment = current / 2;
            if seen[segment] {
                break;
            }
            seen[segment] = true;
            let k = current % 2;
            let a = segments[segment].ends[k].point;
            let b = segments[segment].ends[1 - k].point;
            vote += if k == 0 { square(a, b) } else { -square(a, b) };
            vertices.push(a);
            current = links[current ^ 1];
        }
        if vertices.len() >= 3 {
            let mut r = ClosedRing::new(vertices);
            if vote < -1e-12 {
                r.reverse()
            }
            if r.signed_area() != 0. {
                rings.push(r)
            }
        }
    }
    Ok(rings)
}
fn tolerant_rings(segments: Vec<Passage>) -> Result<(Vec<ClosedRing>, usize), SliceError> {
    // Sort directed grid keys once, retaining the earliest original segment.
    let mut keys = Vec::with_capacity(segments.len());
    for (index, s) in segments.iter().enumerate() {
        let a = fixed(s.ends[0].point)?;
        let b = fixed(s.ends[1].point)?;
        if a != b {
            keys.push((a, b, index));
        }
    }
    keys.sort_unstable();
    keys.dedup_by(|b, a| a.0 == b.0 && a.1 == b.1);
    keys.sort_unstable_by_key(|k| k.2);
    let segs: Vec<_> = keys.into_iter().map(|k| segments[k.2]).collect();
    let mut starts: Vec<_> = segs
        .iter()
        .enumerate()
        .map(|(i, s)| (s.ends[0].point.x, i))
        .collect();
    starts.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut seen = vec![false; segs.len()];
    let mut rings = vec![];
    let mut discarded = 0;
    for i in 0..segs.len() {
        if seen[i] {
            continue;
        }
        seen[i] = true;
        let mut points = vec![segs[i].ends[0].point, segs[i].ends[1].point];
        loop {
            let first = points[0];
            let last = *points.last().unwrap();
            if points.len() >= 4 && square(first, last) <= 1e-10 {
                points.pop();
                rings.push(ClosedRing::new(points));
                break;
            }
            let lo = starts.partition_point(|&(x, _)| x < last.x - 0.01);
            let hi = starts.partition_point(|&(x, _)| x <= last.x + 0.01);
            let mut best = None;
            for &(_, j) in &starts[lo..hi] {
                if seen[j] {
                    continue;
                }
                let d = square(last, segs[j].ends[0].point);
                if d <= 0.0001 && best.is_none_or(|(bd, bi)| d < bd || d == bd && j < bi) {
                    best = Some((d, j))
                }
            }
            if let Some((_, j)) = best {
                seen[j] = true;
                points.push(segs[j].ends[1].point)
            } else {
                if points.len() >= 4 && square(first, last) <= 0.0001 {
                    rings.push(ClosedRing::new(points))
                } else {
                    discarded += 1
                }
                break;
            }
        }
    }
    Ok((rings, discarded))
}
pub(crate) fn section(
    mesh: &IndexedMesh,
    faces: &[usize],
    z: f64,
    topology: Option<&MeshTopology>,
    tolerant: bool,
) -> Result<(ContourLayer, usize), SliceError> {
    if tolerant {
        let (rings, d) = tolerant_rings(passages(mesh, faces, z, None, true, true))?;
        return Ok((ContourLayer::new(z, union(rings)?), d));
    }
    let segments = passages(mesh, faces, z, topology, false, false);
    let rings = match strict_rings(&segments, z) {
        Ok(r) => r,
        Err(_) => strict_rings(&passages(mesh, faces, z, topology, true, false), z)?,
    };
    let polygons = if rings.len() == 1 {
        let mut r = rings.into_iter().next().unwrap();
        if r.signed_area() < 0. {
            r.reverse()
        }
        vec![Polygon::new_unchecked(r, vec![])]
    } else {
        union(rings)?
    };
    Ok((ContourLayer::new(z, polygons), 0))
}

#[cfg(test)]
#[path = "tests_section.rs"]
mod tests_section;

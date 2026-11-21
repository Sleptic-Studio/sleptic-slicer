// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{IndexedMesh, IndexedTriangle, Normal, Vector3D, Vertex, primitives::VertexIndex};
use rayon::prelude::*;
#[derive(Debug)]
pub struct Simplified {
    pub mesh: IndexedMesh,
    pub deviation_bound_mm: f64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimplifyError {
    Cancelled,
    InvalidSettings,
    InvalidMesh,
}
impl std::fmt::Display for SimplifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for SimplifyError {}
type Point = [f64; 3];
type Plane = [f64; 4];
// A plane is stored once per original face. Corner nodes form disjoint
// vertex-owned chains; contraction splices chains without copying planes.
struct PlaneSets {
    planes: Vec<Plane>,
    next: Vec<usize>,
    head: Vec<usize>,
    tail: Vec<usize>,
}
impl PlaneSets {
    fn new(vertices: usize, faces: usize) -> Self {
        Self {
            planes: vec![[0.; 4]; faces],
            next: vec![usize::MAX; faces * 3],
            head: vec![usize::MAX; vertices],
            tail: vec![usize::MAX; vertices],
        }
    }
    fn attach(&mut self, vertex: usize, corner: usize) {
        self.next[corner] = self.head[vertex];
        self.head[vertex] = corner;
        if self.tail[vertex] == usize::MAX {
            self.tail[vertex] = corner;
        }
    }
    fn iter(&self, vertex: usize) -> impl Iterator<Item = Plane> + '_ {
        let mut current = self.head[vertex];
        std::iter::from_fn(move || {
            if current == usize::MAX {
                return None;
            }
            let plane = self.planes[current / 3];
            current = self.next[current];
            Some(plane)
        })
    }
    fn merge(&mut self, a: usize, b: usize) {
        if self.head[b] == usize::MAX {
            return;
        }
        if self.head[a] == usize::MAX {
            self.head[a] = self.head[b];
        } else {
            self.next[self.tail[a]] = self.head[b];
        }
        self.tail[a] = self.tail[b];
        self.head[b] = usize::MAX;
        self.tail[b] = usize::MAX;
    }
}
fn sub(a: Point, b: Point) -> Vector3D<f64> {
    Vector3D::new([a[0] - b[0], a[1] - b[1], a[2] - b[2]])
}
fn cross(points: &[Point], f: [usize; 3]) -> Vector3D<f64> {
    sub(points[f[1]], points[f[0]]).cross_product(&sub(points[f[2]], points[f[0]]))
}
fn plane_distance(p: Plane, x: Point) -> f64 {
    p[0].mul_add(x[0], p[1].mul_add(x[1], p[2].mul_add(x[2], p[3])))
        .abs()
}
fn quadric(planes: &[Plane]) -> [f64; 10] {
    let mut q = [0.; 10];
    for p in planes {
        let mut k = 0;
        for i in 0..4 {
            for j in i..4 {
                q[k] += p[i] * p[j];
                k += 1;
            }
        }
    }
    q
}
fn cost(q: [f64; 10], p: Point) -> f64 {
    let x = [p[0], p[1], p[2], 1.];
    let mut result = 0.;
    let mut k = 0;
    for i in 0..4 {
        for j in i..4 {
            result += (if i == j { 1. } else { 2. }) * q[k] * x[i] * x[j];
            k += 1;
        }
    }
    result.max(0.)
}
fn candidate(a: Point, b: Point, q: [f64; 10]) -> Point {
    let mut best = a;
    let mut score = cost(q, a);
    for p in [b, std::array::from_fn(|i| (a[i] + b[i]) * 0.5)] {
        let s = cost(q, p);
        if s < score {
            best = p;
            score = s;
        }
    }
    let m = [[q[0], q[1], q[2]], [q[1], q[4], q[5]], [q[2], q[5], q[7]]];
    let rhs = [-q[3], -q[6], -q[8]];
    let transform = crate::transform::AffineTransform3D::from_linear_and_translation(m, [0.; 3]);
    if let Ok(inv) = transform.inverse() {
        let p = inv
            .linear()
            .map(|r| r[0].mul_add(rhs[0], r[1].mul_add(rhs[1], r[2] * rhs[2])));
        let s = cost(q, p);
        if p.iter().all(|v| v.is_finite()) && s < score {
            best = p;
        }
    }
    // Acceptance is evaluated at the actual stored f32 position.
    best.map(|v| f64::from(v as f32))
}
#[derive(Clone, Copy)]
struct Candidate {
    a: usize,
    b: usize,
    p: Point,
    cost: f64,
    length_squared: f64,
}
pub fn simplify(
    mesh: &IndexedMesh,
    target: usize,
    max_deviation_mm: f64,
) -> Result<Simplified, SimplifyError> {
    simplify_with_progress(mesh, target, max_deviation_mm, &mut |_| true)
}
pub fn simplify_with_progress(
    mesh: &IndexedMesh,
    target: usize,
    max_deviation_mm: f64,
    mut progress: impl FnMut(f32) -> bool,
) -> Result<Simplified, SimplifyError> {
    if !progress(0.) {
        return Err(SimplifyError::Cancelled);
    }
    if target < 4 || !max_deviation_mm.is_finite() || max_deviation_mm <= 0. {
        return Err(SimplifyError::InvalidSettings);
    }
    if mesh.validate_vertices().is_err() || mesh.validate_faces().is_err() {
        return Err(SimplifyError::InvalidMesh);
    }
    let mut points: Vec<Point> = mesh
        .vertices
        .iter()
        .map(|v| v.coords().map(f64::from))
        .collect();
    let mut faces: Vec<[usize; 3]> = mesh
        .faces
        .iter()
        .map(|f| f.vertices.map(usize::from))
        .collect();
    let mut alive = vec![true; faces.len()];
    let mut count = faces.len();
    let original = count;
    let mut planes = PlaneSets::new(points.len(), faces.len());
    let mut quadrics = vec![[0.; 10]; points.len()];
    let mut fixed = vec![false; points.len()];
    for (i, &f) in faces.iter().enumerate() {
        let n = cross(&points, f);
        let length = n.norm();
        if length < 1e-15 {
            for v in f {
                fixed[v] = true;
            }
        } else {
            let n = n.coords().map(|x| x / length);
            let p = [
                n[0],
                n[1],
                n[2],
                -n[0].mul_add(
                    points[f[0]][0],
                    n[1].mul_add(points[f[0]][1], n[2] * points[f[0]][2]),
                ),
            ];
            planes.planes[i] = p;
            let q = quadric(&[p]);
            for (k, v) in f.into_iter().enumerate() {
                planes.attach(v, i * 3 + k);
                for (entry, added) in quadrics[v].iter_mut().zip(q) {
                    *entry += added;
                }
            }
        }
    }
    while count > target {
        let mut edges = Vec::with_capacity(count * 3);
        let mut offsets = vec![0usize; points.len() + 1];
        for (i, &f) in faces.iter().enumerate() {
            if !alive[i] {
                continue;
            }
            for v in f {
                offsets[v + 1] += 1;
            }
            for k in 0..3 {
                let a = f[k].min(f[(k + 1) % 3]);
                let b = f[k].max(f[(k + 1) % 3]);
                edges.push((a, b));
            }
        }
        for i in 1..offsets.len() {
            offsets[i] += offsets[i - 1];
        }
        let mut cursor = offsets.clone();
        let mut incident = vec![0usize; count * 3];
        for (i, &f) in faces.iter().enumerate() {
            if alive[i] {
                for v in f {
                    incident[cursor[v]] = i;
                    cursor[v] += 1;
                }
            }
        }
        edges.par_sort_unstable();
        let mut unique = Vec::with_capacity(edges.len() / 2);
        let mut start = 0;
        while start < edges.len() {
            let mut end = start + 1;
            while end < edges.len() && edges[end] == edges[start] {
                end += 1;
            }
            let (a, b) = edges[start];
            if end - start != 2 {
                fixed[a] = true;
                fixed[b] = true;
            } else {
                unique.push((a, b));
            }
            start = end;
        }
        let mut candidates = Vec::with_capacity(unique.len());
        for batch in unique.chunks(32768) {
            if !progress(reduction(original, count, target)) {
                return Err(SimplifyError::Cancelled);
            }
            candidates.par_extend(batch.par_iter().filter_map(|&(a, b)| {
                if fixed[a] || fixed[b] {
                    return None;
                }
                let q = std::array::from_fn(|i| quadrics[a][i] + quadrics[b][i]);
                let p = candidate(points[a], points[b], q);
                Some(Candidate {
                    a,
                    b,
                    p,
                    cost: cost(q, p),
                    length_squared: sub(points[a], points[b]).squared_norm(),
                })
            }));
        }
        candidates.par_sort_unstable_by(|a, b| {
            a.cost
                .total_cmp(&b.cost)
                .then(a.length_squared.total_cmp(&b.length_squared))
                .then(a.a.cmp(&b.a))
                .then(a.b.cmp(&b.b))
        });
        let mut locked = vec![false; points.len()];
        let before = count;
        let mut neighborhood = Vec::new();
        let mut neighbors_a = Vec::new();
        let mut neighbors_b = Vec::new();
        let mut opposite = Vec::new();
        let mut keys = Vec::new();
        for (processed, c) in candidates.into_iter().enumerate() {
            if processed % 32768 == 0 && !progress(reduction(original, count, target)) {
                return Err(SimplifyError::Cancelled);
            }
            if count <= target {
                break;
            }
            let Candidate { a, b, p, .. } = c;
            if locked[a] || locked[b] {
                continue;
            }
            neighborhood.clear();
            neighborhood.extend_from_slice(&incident[offsets[a]..offsets[a + 1]]);
            neighborhood.extend_from_slice(&incident[offsets[b]..offsets[b + 1]]);
            neighborhood.sort_unstable();
            neighborhood.dedup();
            if neighborhood
                .iter()
                .any(|&i| faces[i].iter().any(|&v| locked[v]))
            {
                continue;
            }
            neighbors_a.clear();
            neighbors_b.clear();
            opposite.clear();
            for &i in &neighborhood {
                let f = faces[i];
                if f.contains(&a) {
                    for v in f {
                        if v != a && v != b {
                            neighbors_a.push(v);
                        }
                    }
                }
                if f.contains(&b) {
                    for v in f {
                        if v != a && v != b {
                            neighbors_b.push(v);
                        }
                    }
                }
                if f.contains(&a) && f.contains(&b) {
                    for v in f {
                        if v != a && v != b {
                            opposite.push(v);
                        }
                    }
                }
            }
            neighbors_a.sort_unstable();
            neighbors_a.dedup();
            neighbors_b.sort_unstable();
            neighbors_b.dedup();
            opposite.sort_unstable();
            opposite.dedup();
            if opposite.len() != 2
                || neighbors_a
                    .iter()
                    .filter(|v| neighbors_b.binary_search(v).is_ok())
                    .copied()
                    .ne(opposite.iter().copied())
            {
                continue;
            }
            if planes
                .iter(a)
                .chain(planes.iter(b))
                .any(|plane| plane_distance(plane, p) > max_deviation_mm)
            {
                continue;
            }
            let old_a = points[a];
            points[a] = p;
            keys.clear();
            let mut valid = true;
            for &i in &neighborhood {
                let old = faces[i];
                if old.contains(&a) && old.contains(&b) {
                    continue;
                }
                let new = old.map(|v| if v == b { a } else { v });
                let new_n = cross(&points, new);
                points[a] = old_a;
                let old_n = cross(&points, old);
                points[a] = p;
                if old_n.dot_product(&new_n) <= 1e-12 {
                    valid = false;
                    break;
                }
                let mut key = new;
                key.sort_unstable();
                keys.push(key);
            }
            if valid {
                keys.sort_unstable();
                valid = !keys.windows(2).any(|w| w[0] == w[1]);
            }
            if !valid {
                points[a] = old_a;
                continue;
            }
            for &i in &neighborhood {
                let old = faces[i];
                for v in old {
                    locked[v] = true;
                }
                if old.contains(&a) && old.contains(&b) {
                    alive[i] = false;
                    count -= 1;
                } else {
                    faces[i] = old.map(|v| if v == b { a } else { v });
                }
            }
            planes.merge(a, b);
            let other = quadrics[b];
            for (entry, added) in quadrics[a].iter_mut().zip(other) {
                *entry += added;
            }
        }
        if !progress(reduction(original, count, target)) {
            return Err(SimplifyError::Cancelled);
        }
        if count == before {
            break;
        }
    }
    let mut vertices = Vec::new();
    let mut output = Vec::with_capacity(count);
    let mut numbering = vec![None; points.len()];
    let mut bound = 0f64;
    for (i, f) in faces.into_iter().enumerate() {
        if !alive[i] {
            continue;
        }
        let n = cross(&points, f);
        let length = n.norm();
        let normal = if length <= 1e-15 {
            Normal::new([0., 0., 1.])
        } else {
            Normal::new([
                (n[0] / length) as f32,
                (n[1] / length) as f32,
                (n[2] / length) as f32,
            ])
        };
        let ids = f.map(|v| {
            if let Some(id) = numbering[v] {
                id
            } else {
                let id = VertexIndex::from(vertices.len() as u32);
                numbering[v] = Some(id);
                let p = points[v];
                vertices.push(Vertex::new([p[0] as f32, p[1] as f32, p[2] as f32]));
                for plane in planes.iter(v) {
                    bound = bound.max(plane_distance(plane, p));
                }
                id
            }
        });
        output.push(IndexedTriangle {
            normal,
            vertices: ids,
        });
    }
    if !progress(1.) {
        return Err(SimplifyError::Cancelled);
    }
    Ok(Simplified {
        mesh: IndexedMesh::new(mesh.meta.clone(), vertices, output),
        deviation_bound_mm: bound,
    })
}
fn reduction(original: usize, current: usize, target: usize) -> f32 {
    if original <= target {
        0.
    } else {
        ((original - current) as f32 / (original - target) as f32).min(1.)
    }
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Solid mesh edits in world millimetres. Operations stage complete meshes;
//! callers commit the result through their ordinary undo history.
use glam::DVec3;
use sc_mesh_core::{IndexedMesh, IndexedTriangle, Vertex};
use std::collections::{BTreeMap, BTreeSet};

const EPS: f64 = 1e-7;
const MAX_POLYGONS: usize = 100_000;

/// A model edit failed without modifying its inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelToolError {
    /// Repair open, non-manifold, degenerate or inconsistently wound input first.
    InvalidMesh,
    /// Nonfinite plane, dimensions or label coordinates.
    InvalidSettings,
    /// The operation exceeds the bounded geometry workload.
    TooComplex,
    /// Numerical degeneracy prevented a closed result.
    InvalidResult,
    /// Labels require printable ASCII and at most 80 characters.
    InvalidText,
}
impl std::fmt::Display for ModelToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidMesh => "Repair the mesh first: this tool requires closed, consistently wound manifold surfaces.",
            Self::InvalidSettings => "Enter finite coordinates and positive label size and depth.",
            Self::TooComplex => "The model edit exceeds the geometry complexity limit.",
            Self::InvalidResult => "The edit could not produce a closed manifold. The original objects were retained.",
            Self::InvalidText => "Enter 1 to 80 printable ASCII characters.",
        })
    }
}
impl std::error::Error for ModelToolError {}

/// Solid set operation. Subtract removes the second object from the first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BooleanOp {
    /// Combine both solids.
    Union,
    /// Remove the second solid.
    Subtract,
    /// Keep common material.
    Intersect,
}

fn validate(mesh: &IndexedMesh) -> Result<(), ModelToolError> {
    mesh.validate_vertices()
        .map_err(|_| ModelToolError::InvalidMesh)?;
    let valid = mesh
        .validated_faces()
        .map_err(|_| ModelToolError::InvalidMesh)?
        .validated_edge_manifold()
        .map_err(|_| ModelToolError::InvalidMesh)?;
    let topology = sc_mesh_core::indexed_mesh::MeshTopology::from_edge_manifold(&valid);
    if (0..mesh.vertices.len()).any(|i| {
        topology
            .vertex_fans((i as u32).into())
            .is_some_and(|fans| fans.count() != 1)
    }) {
        return Err(ModelToolError::InvalidMesh);
    }
    for face in &mesh.faces {
        // Use the mesh core's scale-invariant degeneracy test. An absolute
        // area threshold wrongly rejected legitimate small outline triangles.
        sc_mesh_core::geometry::compute_normal_from_triangle_vertices(
            &face.vertices.map(|i| mesh.vertices[usize::from(i)]),
        )
        .map_err(|_| ModelToolError::InvalidMesh)?;
    }
    Ok(())
}

#[derive(Clone)]
struct Polygon {
    vertices: Vec<DVec3>,
    normal: DVec3,
    distance: f64,
}
impl Polygon {
    fn new(vertices: Vec<DVec3>) -> Option<Self> {
        let a = *vertices.first()?;
        let normal = vertices
            .windows(2)
            .map(|p| (p[0] - a).cross(p[1] - a))
            .find(|n| n.length_squared() > 1e-28)?
            .normalize();
        Some(Self {
            distance: normal.dot(a),
            normal,
            vertices,
        })
    }
    fn flip(&mut self) {
        self.vertices.reverse();
        self.normal = -self.normal;
        self.distance = -self.distance;
    }
}

#[derive(Default)]
struct Partition {
    plane: Option<(DVec3, f64)>,
    polygons: Vec<Polygon>,
    front: Option<Box<Self>>,
    back: Option<Box<Self>>,
}

fn divide(
    p: Polygon,
    normal: DVec3,
    distance: f64,
    coplanar: &mut Vec<Polygon>,
    front: &mut Vec<Polygon>,
    back: &mut Vec<Polygon>,
) {
    let sides: Vec<_> = p
        .vertices
        .iter()
        .map(|v| {
            let d = normal.dot(*v) - distance;
            if d > EPS {
                1
            } else if d < -EPS {
                -1
            } else {
                0
            }
        })
        .collect();
    let has_front = sides.contains(&1);
    let has_back = sides.contains(&-1);
    match (has_front, has_back) {
        (false, false) => coplanar.push(p),
        (true, false) => front.push(p),
        (false, true) => back.push(p),
        (true, true) => {
            let mut f = Vec::new();
            let mut b = Vec::new();
            for i in 0..p.vertices.len() {
                let j = (i + 1) % p.vertices.len();
                let a = p.vertices[i];
                let z = p.vertices[j];
                if sides[i] >= 0 {
                    f.push(a);
                }
                if sides[i] <= 0 {
                    b.push(a);
                }
                if sides[i] * sides[j] == -1 {
                    let t = (distance - normal.dot(a)) / normal.dot(z - a);
                    let v = a + (z - a) * t;
                    f.push(v);
                    b.push(v);
                }
            }
            // A split fragment lies on its source plane. Recomputing that
            // plane from a thin sliver amplifies rounding and opens cracks
            // when a later BSP partition clips its neighbour.
            if f.len() >= 3 {
                front.push(Polygon {
                    vertices: f,
                    normal: p.normal,
                    distance: p.distance,
                });
            }
            if b.len() >= 3 {
                back.push(Polygon {
                    vertices: b,
                    normal: p.normal,
                    distance: p.distance,
                });
            }
        }
    }
}

impl Partition {
    fn build(
        &mut self,
        polygons: Vec<Polygon>,
        depth: usize,
        work: &mut usize,
    ) -> Result<(), ModelToolError> {
        if polygons.is_empty() {
            return Ok(());
        }
        if depth > 256 || polygons.len() > MAX_POLYGONS {
            return Err(ModelToolError::TooComplex);
        }
        *work = work
            .checked_sub(polygons.len())
            .ok_or(ModelToolError::TooComplex)?;
        let (n, d) = *self
            .plane
            .get_or_insert_with(|| (polygons[0].normal, polygons[0].distance));
        let mut front = Vec::new();
        let mut back = Vec::new();
        for p in polygons {
            divide(p, n, d, &mut self.polygons, &mut front, &mut back);
        }
        if !front.is_empty() {
            self.front
                .get_or_insert_with(Default::default)
                .build(front, depth + 1, work)?;
        }
        if !back.is_empty() {
            self.back
                .get_or_insert_with(Default::default)
                .build(back, depth + 1, work)?;
        }
        Ok(())
    }
    fn invert(&mut self) {
        for p in &mut self.polygons {
            p.flip();
        }
        if let Some((n, d)) = &mut self.plane {
            *n = -*n;
            *d = -*d;
        }
        if let Some(n) = &mut self.front {
            n.invert();
        }
        if let Some(n) = &mut self.back {
            n.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }
    fn clip(
        &self,
        polygons: Vec<Polygon>,
        depth: usize,
        work: &mut usize,
    ) -> Result<Vec<Polygon>, ModelToolError> {
        if polygons.len() > MAX_POLYGONS || depth > 256 {
            return Err(ModelToolError::TooComplex);
        }
        *work = work
            .checked_sub(polygons.len())
            .ok_or(ModelToolError::TooComplex)?;
        let Some((n, d)) = self.plane else {
            return Ok(polygons);
        };
        let mut front = Vec::new();
        let mut back = Vec::new();
        let mut coplanar = Vec::new();
        for p in polygons {
            divide(p, n, d, &mut coplanar, &mut front, &mut back);
        }
        for p in coplanar {
            if n.dot(p.normal) >= 0.0 {
                front.push(p);
            } else {
                back.push(p);
            }
        }
        if let Some(node) = &self.front {
            front = node.clip(front, depth + 1, work)?;
        }
        if let Some(node) = &self.back {
            back = node.clip(back, depth + 1, work)?;
        } else {
            back.clear();
        }
        front.extend(back);
        Ok(front)
    }
    fn clip_to(&mut self, other: &Self, work: &mut usize) -> Result<(), ModelToolError> {
        self.polygons = other.clip(std::mem::take(&mut self.polygons), 0, work)?;
        if let Some(node) = &mut self.front {
            node.clip_to(other, work)?;
        }
        if let Some(node) = &mut self.back {
            node.clip_to(other, work)?;
        }
        Ok(())
    }
    fn take(mut self) -> Vec<Polygon> {
        if let Some(node) = self.front.take() {
            self.polygons.extend(node.take());
        }
        if let Some(node) = self.back.take() {
            self.polygons.extend(node.take());
        }
        self.polygons
    }
}

fn polygons(mesh: &IndexedMesh) -> Result<Vec<Polygon>, ModelToolError> {
    validate(mesh)?;
    mesh.faces
        .iter()
        .map(|face| {
            Polygon::new(
                face.vertices
                    .map(|i| {
                        DVec3::from_array(mesh.vertices[usize::from(i)].coords().map(f64::from))
                    })
                    .to_vec(),
            )
            .ok_or(ModelToolError::InvalidMesh)
        })
        .collect()
}

fn point_key(p: DVec3) -> [i64; 3] {
    p.to_array()
        .map(|v| (f64::from(v as f32) / EPS).round() as i64)
}

// Insert every coincident edge subdivision before triangulating. This removes
// BSP T-junctions. Centroid fans preserve collinear boundary subdivisions.
fn mesh_from_polygons(polygons: Vec<Polygon>, closed: bool) -> Result<IndexedMesh, ModelToolError> {
    let mut map = BTreeMap::new();
    let mut positions = Vec::new();
    for p in &polygons {
        for &v in &p.vertices {
            map.entry(point_key(v)).or_insert_with(|| {
                let id = positions.len();
                positions.push(v);
                id
            });
        }
    }
    let edge_count: usize = polygons.iter().map(|p| p.vertices.len()).sum();
    if edge_count.saturating_mul(positions.len()) > 100_000_000 {
        return Err(ModelToolError::TooComplex);
    }
    let mut indices = Vec::new();
    let boundary_count = positions.len();
    for p in polygons {
        let mut boundary = Vec::new();
        for i in 0..p.vertices.len() {
            let a = positions[map[&point_key(p.vertices[i])]];
            let b = positions[map[&point_key(p.vertices[(i + 1) % p.vertices.len()])]];
            let delta = b - a;
            let len = delta.length_squared();
            if len < EPS * EPS {
                continue;
            }
            let mut along: Vec<_> = positions[..boundary_count]
                .iter()
                .enumerate()
                .filter_map(|(id, &v)| {
                    let t = (v - a).dot(delta) / len;
                    ((-EPS..1.0 - EPS).contains(&t)
                        && (a + delta * t - v).length_squared() < EPS * EPS)
                        .then_some((t, id as u32))
                })
                .collect();
            along.sort_by(|a, b| a.0.total_cmp(&b.0));
            boundary.extend(along.into_iter().map(|(_, id)| id));
        }
        boundary.dedup();
        if boundary.len() < 3 {
            continue;
        }
        if boundary.len() == 3 {
            indices.push([boundary[0], boundary[1], boundary[2]]);
            continue;
        }
        let centre = p.vertices.iter().copied().sum::<DVec3>() / p.vertices.len() as f64;
        let mid = positions.len() as u32;
        // Do not expose new centroids to later boundary subdivision searches.
        for i in 0..boundary.len() {
            indices.push([mid, boundary[i], boundary[(i + 1) % boundary.len()]]);
        }
        positions.push(centre);
    }
    let vertices: Vec<_> = positions
        .into_iter()
        .map(|p| Vertex::new(p.to_array().map(|v| v as f32)))
        .collect();
    let mut faces = Vec::new();
    let mut seen = BTreeSet::new();
    for mut ids in indices {
        // BSP subdivisions can form a short edge far from the fan centre.
        // Evaluate the corner adjacent to that edge before rejecting the
        // triangle: the mesh validator measures the angle at the first vertex.
        let normal = (0..3).find_map(|_| {
            ids.rotate_left(1);
            sc_mesh_core::geometry::compute_normal_from_triangle_vertices(
                &ids.map(|i| vertices[i as usize]),
            )
            .ok()
        });
        let Some(n) = normal else {
            continue;
        };
        let mut key = ids;
        key.sort_unstable();
        if seen.insert(key) {
            faces.push(IndexedTriangle::from_raw_indices(n, ids));
        }
    }
    let mesh = IndexedMesh::new(None, vertices, faces);
    if closed && !mesh.faces.is_empty() {
        validate(&mesh).map_err(|_| ModelToolError::InvalidResult)?;
    }
    Ok(mesh)
}

/// Boolean two closed meshes. Errors retain both inputs; an empty intersection
/// is a valid empty mesh. Coplanar faces use the same material-side convention.
pub fn boolean(
    a: &IndexedMesh,
    b: &IndexedMesh,
    op: BooleanOp,
) -> Result<IndexedMesh, ModelToolError> {
    boolean_polygons(polygons(a)?, polygons(b)?, op)
}

fn boolean_polygons(
    a: Vec<Polygon>,
    b: Vec<Polygon>,
    op: BooleanOp,
) -> Result<IndexedMesh, ModelToolError> {
    let mut work = 5_000_000;
    let mut left = Partition::default();
    left.build(a, 0, &mut work)?;
    let mut right = Partition::default();
    right.build(b, 0, &mut work)?;
    match op {
        BooleanOp::Union => {
            left.clip_to(&right, &mut work)?;
            right.clip_to(&left, &mut work)?;
            right.invert();
            right.clip_to(&left, &mut work)?;
            right.invert();
            left.build(right.take(), 0, &mut work)?;
        }
        BooleanOp::Subtract => {
            left.invert();
            left.clip_to(&right, &mut work)?;
            right.clip_to(&left, &mut work)?;
            right.invert();
            right.clip_to(&left, &mut work)?;
            right.invert();
            left.build(right.take(), 0, &mut work)?;
            left.invert();
        }
        BooleanOp::Intersect => {
            left.invert();
            right.clip_to(&left, &mut work)?;
            right.invert();
            left.clip_to(&right, &mut work)?;
            right.clip_to(&left, &mut work)?;
            left.build(right.take(), 0, &mut work)?;
            left.invert();
        }
    }
    mesh_from_polygons(left.take(), true)
}

/// Separate edge-connected shells. Coordinate welding also handles STL's
/// duplicated vertices. Each returned part has compact, independent indices.
pub fn split_parts(mesh: &IndexedMesh) -> Result<Vec<IndexedMesh>, ModelToolError> {
    mesh.validate_vertices()
        .map_err(|_| ModelToolError::InvalidMesh)?;
    mesh.validate_faces()
        .map_err(|_| ModelToolError::InvalidMesh)?;
    let mut parent: Vec<_> = (0..mesh.faces.len()).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let mut edges = BTreeMap::new();
    for (i, f) in mesh.faces.iter().enumerate() {
        let keys = f.vertices.map(|v| {
            point_key(DVec3::from_array(
                mesh.vertices[usize::from(v)].coords().map(f64::from),
            ))
        });
        for j in 0..3 {
            let a = keys[j];
            let b = keys[(j + 1) % 3];
            let edge = (a.min(b), a.max(b));
            if let Some(&other) = edges.get(&edge) {
                let a = root(&mut parent, i);
                let b = root(&mut parent, other);
                parent[a] = b;
            } else {
                edges.insert(edge, i);
            }
        }
    }
    let mut groups = BTreeMap::<usize, Vec<usize>>::new();
    for i in 0..mesh.faces.len() {
        groups.entry(root(&mut parent, i)).or_default().push(i);
    }
    Ok(groups
        .into_values()
        .map(|ids| {
            let mut map = BTreeMap::new();
            let mut vertices = Vec::new();
            let faces = ids
                .into_iter()
                .map(|i| {
                    let f = &mesh.faces[i];
                    let indices = f.vertices.map(|v| {
                        *map.entry(v).or_insert_with(|| {
                            let id = vertices.len() as u32;
                            vertices.push(mesh.vertices[usize::from(v)]);
                            id
                        })
                    });
                    IndexedTriangle::from_raw_indices(f.normal, indices)
                })
                .collect();
            IndexedMesh::new(mesh.meta.clone(), vertices, faces)
        })
        .collect())
}

/// Clip at a horizontal plane, returning `(lower, upper)`. With caps enabled
/// each half is a closed solid; with caps disabled the cut boundary stays open.
pub fn cut(
    mesh: &IndexedMesh,
    z: f64,
    cap: bool,
) -> Result<(IndexedMesh, IndexedMesh), ModelToolError> {
    if !z.is_finite() {
        return Err(ModelToolError::InvalidSettings);
    }
    validate(mesh)?;
    let bounds = mesh.local_aabb().ok_or(ModelToolError::InvalidMesh)?;
    let lo = bounds.min().coords().map(f64::from);
    let hi = bounds.max().coords().map(f64::from);
    if z <= lo[2] {
        return Ok((IndexedMesh::new(None, vec![], vec![]), mesh.clone()));
    }
    if z >= hi[2] {
        return Ok((mesh.clone(), IndexedMesh::new(None, vec![], vec![])));
    }
    let margin = (hi[0] - lo[0])
        .max(hi[1] - lo[1])
        .max(hi[2] - lo[2])
        .max(1.0);
    let box_mesh = cuboid(
        [lo[0] - margin, lo[1] - margin, lo[2] - margin],
        [hi[0] + margin, hi[1] + margin, z],
    )?;
    let mut lower = boolean(mesh, &box_mesh, BooleanOp::Intersect)?;
    let mut upper = boolean(mesh, &box_mesh, BooleanOp::Subtract)?;
    if !cap {
        for part in [&mut lower, &mut upper] {
            part.faces.retain(|f| {
                !f.vertices
                    .iter()
                    .all(|&i| (f64::from(part.vertices[usize::from(i)][2]) - z).abs() < EPS * 4.0)
            });
        }
    }
    Ok((lower, upper))
}

fn cuboid(lo: [f64; 3], hi: [f64; 3]) -> Result<IndexedMesh, ModelToolError> {
    let vertices: Vec<_> = [
        [0, 0, 0],
        [1, 0, 0],
        [1, 1, 0],
        [0, 1, 0],
        [0, 0, 1],
        [1, 0, 1],
        [1, 1, 1],
        [0, 1, 1],
    ]
    .into_iter()
    .map(|v| {
        Vertex::new(std::array::from_fn(|i| {
            if v[i] == 0 {
                lo[i] as f32
            } else {
                hi[i] as f32
            }
        }))
    })
    .collect();
    let mut faces = Vec::new();
    for q in [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ] {
        for ids in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
            let n = sc_mesh_core::geometry::compute_normal_from_triangle_vertices(
                &ids.map(|i| vertices[i as usize]),
            )
            .map_err(|_| ModelToolError::InvalidSettings)?;
            faces.push(IndexedTriangle::from_raw_indices(n, ids));
        }
    }
    Ok(IndexedMesh::new(None, vertices, faces))
}

/// Emboss or engrave an outline Noto Sans ASCII label at a picked surface point.
/// The outward face normal defines depth; text follows the face's upright
/// tangent. Size is the font em height in millimetres; spacing uses advance widths.
pub fn label(
    mesh: &IndexedMesh,
    text: &str,
    position: [f64; 3],
    normal: [f64; 3],
    size_mm: f64,
    depth_mm: f64,
    engrave: bool,
) -> Result<IndexedMesh, ModelToolError> {
    if text.is_empty()
        || text.len() > 80
        || !text.bytes().all(|b| (32..=126).contains(&b))
        || text.trim().is_empty()
    {
        return Err(ModelToolError::InvalidText);
    }
    let origin = DVec3::from_array(position);
    let n = DVec3::from_array(normal);
    if !origin.is_finite()
        || !n.is_finite()
        || n.length_squared() < EPS
        || !size_mm.is_finite()
        || size_mm <= 0.0
        || !depth_mm.is_finite()
        || depth_mm <= 0.0
    {
        return Err(ModelToolError::InvalidSettings);
    }
    validate(mesh)?;
    let n = n.normalize();
    let up = if n.z.abs() > 0.9 { DVec3::Y } else { DVec3::Z };
    let u = up.cross(n).normalize();
    let v = n.cross(u);
    let (bottom, top) = if engrave {
        (-depth_mm, 0.01)
    } else {
        (-0.01, depth_mm)
    };
    let mut label_polygons = Vec::new();
    let mut advance = 0.0;
    for c in text.bytes() {
        let (width, rings) = crate::label_font::glyph(c);
        let rings: Vec<Vec<_>> = rings
            .into_iter()
            .map(|ring| {
                ring.into_iter()
                    .map(|[x, y]| [size_mm * (x + advance), size_mm * y])
                    .collect()
            })
            .collect();
        advance += width;
        let point = |[x, y]: [f64; 2], z| origin + u * x + v * y + n * z;
        let mut ys: Vec<_> = rings.iter().flatten().map(|p| p[1]).collect();
        ys.sort_by(f64::total_cmp);
        ys.dedup();
        // Horizontal monotone trapezoids cap the actual outline, including counters.
        // Even-odd crossings preserve the holes in O, B, etc. No rasterization.
        for band in ys.windows(2) {
            let [lo, hi] = [band[0], band[1]];
            if hi - lo < EPS {
                continue;
            }
            let mid = (lo + hi) * 0.5;
            let mut edges = Vec::new();
            for ring in &rings {
                for i in 0..ring.len() {
                    let a = ring[i];
                    let b = ring[(i + 1) % ring.len()];
                    if (a[1] <= mid && b[1] > mid) || (b[1] <= mid && a[1] > mid) {
                        let at = |y| a[0] + (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]);
                        edges.push((at(mid), at(lo), at(hi)));
                    }
                }
            }
            edges.sort_by(|a, b| a.0.total_cmp(&b.0));
            for pair in edges.as_chunks::<2>().0 {
                let cap = [
                    [pair[0].1, lo],
                    [pair[1].1, lo],
                    [pair[1].2, hi],
                    [pair[0].2, hi],
                ];
                for (z, reverse) in [(bottom, true), (top, false)] {
                    let mut vertices: Vec<_> = cap.map(|p| point(p, z)).into();
                    vertices.dedup_by(|a, b| (*a - *b).length_squared() < EPS * EPS);
                    if vertices.len() > 2
                        && (vertices[0] - vertices[vertices.len() - 1]).length_squared() < EPS * EPS
                    {
                        vertices.pop();
                    }
                    if reverse {
                        vertices.reverse();
                    }
                    if let Some(p) = Polygon::new(vertices) {
                        label_polygons.push(p);
                    }
                }
            }
        }
        // TrueType contours keep material on their right, opposite our cap winding.
        for ring in &rings {
            for i in 0..ring.len() {
                let a = ring[i];
                let b = ring[(i + 1) % ring.len()];
                if let Some(p) = Polygon::new(vec![
                    point(b, bottom),
                    point(a, bottom),
                    point(a, top),
                    point(b, top),
                ]) {
                    label_polygons.push(p);
                }
            }
        }
    }
    let result = boolean_polygons(
        polygons(mesh)?,
        label_polygons,
        if engrave {
            BooleanOp::Subtract
        } else {
            BooleanOp::Union
        },
    )?;
    // A label that runs off its picked face must not silently create loose
    // glyph solids. Refuse it so the user can reduce size or pick another face.
    if !engrave && split_parts(&result)?.len() > split_parts(mesh)?.len() {
        return Err(ModelToolError::InvalidResult);
    }
    Ok(result)
}

#[cfg(test)]
#[path = "tests_model_tools.rs"]
mod tests;

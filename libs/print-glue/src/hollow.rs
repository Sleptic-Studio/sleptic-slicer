// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Hollowing on a sampled distance field. The original exterior is retained.
//! The cavity's wall is a smooth surface where the distance to the exterior
//! equals the wall thickness (surface nets: one vertex per crossing cell, on the
//! interpolated crossings), not a staircase of cubes (R111: the maintainer found
//! hollowed models "pixelated"). Drain channels still use the cell model:
//! cavity cells are admitted only when their entire bounding sphere is at least
//! the requested wall distance from the surface. Cell pitch starts at 0.125 mm or thickness/16
//! and may coarsen up to 0.5 mm or thickness/4 to respect the memory bound.
//! The staircase interior overestimates thickness by at most sqrt(3)*pitch on
//! resolved watertight inputs. Open surfaces are refused rather than guessed.
use glam::DVec3;
use sc_mesh_core::{
    indexed_mesh::IndexedMesh,
    primitives::{IndexedTriangle, Normal, Vertex},
};
use std::collections::HashMap;

/// Hollowing failures leave the original mesh untouched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HollowError {
    /// Wall thickness must be finite and positive.
    InvalidThickness,
    /// Input must be finite, closed and consistently wound.
    InvalidMesh,
    /// Grid exceeds the explicit memory or axis limit.
    TooLarge,
    /// No interior cell satisfies the requested wall clearance.
    NoCavity,
    /// Drain channels require finite coordinates, positive dimensions and a direction.
    InvalidDrain,
}
/// Plain-English messages: these reach the user verbatim in the hollowing
/// error dialog and the CLI (the bare variant names said nothing useful).
impl std::fmt::Display for HollowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidThickness => "Enter a wall thickness greater than zero.",
            Self::InvalidMesh => {
                "The model has holes or overlapping surfaces even after repair, so its inside can't be found safely. Fix the mesh in your modelling tool, or print it solid."
            }
            Self::TooLarge => {
                "The model is too large to hollow at this wall thickness. Use a thicker wall, or split the model."
            }
            Self::NoCavity => {
                "The model is too thin to hollow with this wall thickness. Use a thinner wall, or print it solid."
            }
            Self::InvalidDrain => {
                "That drain hole is not valid. Use a positive diameter and click on the model."
            }
        })
    }
}
impl std::error::Error for HollowError {}

#[path = "hollow_drill.rs"]
mod drill;

#[derive(Clone)]
struct Triangle {
    p: [DVec3; 3],
    min: DVec3,
    max: DVec3,
}
pub(crate) struct Tree {
    min: DVec3,
    max: DVec3,
    children: Option<(Box<Tree>, Box<Tree>)>,
    triangles: Vec<Triangle>,
}
impl Tree {
    /// Build the existing surface-distance accelerator for geometry queries.
    pub(crate) fn from_mesh(mesh: &IndexedMesh) -> Self {
        Self::new(
            mesh.faces
                .iter()
                .map(|face| {
                    let p = face.vertices.map(|i| {
                        DVec3::from_array(mesh.vertices[usize::from(i)].coords().map(f64::from))
                    });
                    Triangle {
                        min: p[0].min(p[1]).min(p[2]),
                        max: p[0].max(p[1]).max(p[2]),
                        p,
                    }
                })
                .collect(),
        )
    }

    fn new(mut triangles: Vec<Triangle>) -> Self {
        Self::build(&mut triangles)
    }

    /// Halve along the longest axis down to leaves of eight. The halves are
    /// partitioned in place around the median (R86): sorting every level and
    /// copying each half made a 225,000-triangle tree cost a quarter second,
    /// and far more with every orientation finalist building one at once.
    fn build(triangles: &mut [Triangle]) -> Self {
        let min = triangles
            .iter()
            .fold(DVec3::splat(f64::INFINITY), |a, t| a.min(t.min));
        let max = triangles
            .iter()
            .fold(DVec3::splat(f64::NEG_INFINITY), |a, t| a.max(t.max));
        if triangles.len() <= 8 {
            return Self {
                min,
                max,
                children: None,
                triangles: triangles.to_vec(),
            };
        }
        let size = max - min;
        let axis = if size.x >= size.y && size.x >= size.z {
            0
        } else if size.y >= size.z {
            1
        } else {
            2
        };
        let middle = triangles.len() / 2;
        triangles.select_nth_unstable_by(middle, |a, b| {
            (a.min[axis] + a.max[axis]).total_cmp(&(b.min[axis] + b.max[axis]))
        });
        let (low, high) = triangles.split_at_mut(middle);
        Self {
            min,
            max,
            children: Some((Box::new(Self::build(low)), Box::new(Self::build(high)))),
            triangles: Vec::new(),
        }
    }
    /// Bounded ray query used by orientation contact-access checks. The tree
    /// stays in model coordinates and is shared by all rotated finalists.
    pub(crate) fn ray_any(
        &self,
        origin: DVec3,
        direction: DVec3,
        minimum: f64,
        maximum: f64,
    ) -> bool {
        if maximum <= minimum {
            return false;
        }
        let padding = 2e-8 * (self.max - self.min).max_element() + 1e-7;
        let mut near = minimum;
        let mut far = maximum;
        for axis in 0..3 {
            if direction[axis].abs() < 1e-12 {
                if origin[axis] < self.min[axis] - padding
                    || origin[axis] > self.max[axis] + padding
                {
                    return false;
                }
            } else {
                let a = (self.min[axis] - padding - origin[axis]) / direction[axis];
                let b = (self.max[axis] + padding - origin[axis]) / direction[axis];
                near = near.max(a.min(b));
                far = far.min(a.max(b));
                if far < near {
                    return false;
                }
            }
        }
        if let Some((a, b)) = &self.children {
            return a.ray_any(origin, direction, minimum, maximum)
                || b.ray_any(origin, direction, minimum, maximum);
        }
        self.triangles.iter().any(|triangle| {
            let [a, b, c] = triangle.p;
            let edge = b - a;
            let other = c - a;
            let cross = direction.cross(other);
            let determinant = edge.dot(cross);
            if determinant.abs() < 1e-12 {
                return false;
            }
            let relative = origin - a;
            let u = relative.dot(cross) / determinant;
            let q = relative.cross(edge);
            let v = direction.dot(q) / determinant;
            let distance = other.dot(q) / determinant;
            u >= -1e-8
                && v >= -1e-8
                && u + v <= 1.0 + 1e-8
                && distance > minimum
                && distance < maximum
        })
    }

    /// Lowers `best` to the distance from `p` to the nearest triangle, when
    /// that is nearer than `best` already is.
    fn nearest(&self, p: DVec3, best: &mut f64) {
        if (p - p.clamp(self.min, self.max)).length_squared() >= *best * *best {
            return;
        }
        if let Some((a, b)) = &self.children {
            let reach = |t: &Tree| (p - p.clamp(t.min, t.max)).length_squared();
            let (first, second) = if reach(a) <= reach(b) { (a, b) } else { (b, a) };
            first.nearest(p, best);
            second.nearest(p, best);
            return;
        }
        for t in &self.triangles {
            *best = best.min(triangle_distance(p, t.p));
        }
    }

    pub(crate) fn near(&self, p: DVec3, distance: f64) -> bool {
        if (p - p.clamp(self.min, self.max)).length_squared() >= distance * distance {
            return false;
        }
        if let Some((a, b)) = &self.children {
            return a.near(p, distance) || b.near(p, distance);
        }
        self.triangles
            .iter()
            .any(|t| triangle_distance(p, t.p) < distance)
    }
    fn crossings(&self, y: f64, z: f64, out: &mut Vec<(f64, f64)>) {
        if y < self.min.y || y > self.max.y || z < self.min.z || z > self.max.z {
            return;
        }
        if let Some((a, b)) = &self.children {
            a.crossings(y, z, out);
            b.crossings(y, z, out);
            return;
        }
        for t in &self.triangles {
            let [a, b, c] = t.p;
            let n = (b - a).cross(c - a);
            if n.x.abs() < 1e-14 {
                continue;
            }
            let p = DVec3::new(a.x - (n.y * (y - a.y) + n.z * (z - a.z)) / n.x, y, z);
            let v = b - a;
            let w = c - a;
            let q = p - a;
            let d = v.dot(v) * w.dot(w) - v.dot(w).powi(2);
            if d <= 1e-20 {
                continue;
            }
            let u = (q.dot(v) * w.dot(w) - q.dot(w) * v.dot(w)) / d;
            let v = (q.dot(w) * v.dot(v) - q.dot(v) * v.dot(w)) / d;
            if u >= -1e-10 && v >= -1e-10 && u + v <= 1.0 + 1e-10 {
                out.push((p.x, n.x.signum()));
            }
        }
    }
}

fn triangle_distance(p: DVec3, [a, b, c]: [DVec3; 3]) -> f64 {
    let n = (b - a).cross(c - a);
    let norm = n.length_squared();
    if norm > 1e-24 {
        let projection = p - n * ((p - a).dot(n) / norm);
        if [(a, b), (b, c), (c, a)]
            .iter()
            .all(|&(s, e)| (e - s).cross(projection - s).dot(n) >= -1e-12)
        {
            return (p - a).dot(n).abs() / norm.sqrt();
        }
    }
    [(a, b), (b, c), (c, a)]
        .iter()
        .map(|&(s, e)| {
            let d = e - s;
            (p - (s + d * ((p - s).dot(d) / d.length_squared().max(1e-30)).clamp(0.0, 1.0)))
                .length()
        })
        .fold(f64::INFINITY, f64::min)
}

/// Hollowed mesh and conservatively removed cavity volume in cubic millimetres.
#[derive(Debug)]
pub struct HollowedMesh {
    /// Original exterior plus an inward-facing staircase cavity surface.
    pub mesh: IndexedMesh,
    /// Removed cavity and drain volume (relative to the sampled solid for drains).
    pub saved_mm3: f64,
    /// Actual interior grid pitch in millimetres.
    pub pitch_mm: f64,
    /// Two bottom-facing suggested channel starts per connected cavity.
    pub suggested_holes: Vec<[f64; 3]>,
}

/// Finite cylindrical drain channel in the same coordinates as the input mesh.
#[derive(Clone, Debug, PartialEq)]
pub struct DrainHole {
    /// Surface entry point.
    pub position: [f64; 3],
    /// Inward direction (normalized internally).
    pub direction: [f64; 3],
    /// Diameter in millimetres.
    pub diameter_mm: f64,
    /// Channel length in millimetres.
    pub depth_mm: f64,
}

/// The mesh hollowing should work on: the input itself when it already passes
/// validation, otherwise a repaired copy. Real STL files routinely carry
/// zero-length edges or duplicate faces that slicing repairs silently; without
/// this, every such model was refused as `InvalidMesh`. Callers that later
/// relate the hollowed result to the source (outer face count, drain depth)
/// must use the returned mesh, not the original.
pub fn hollow_source(mesh: &IndexedMesh) -> std::borrow::Cow<'_, IndexedMesh> {
    let valid = mesh.validate_vertices().is_ok()
        && mesh
            .validated_faces()
            .is_ok_and(|v| v.validate_edge_manifold().is_ok());
    if valid {
        std::borrow::Cow::Borrowed(mesh)
    } else {
        let mut repaired = mesh.clone();
        repaired.repair(sc_mesh_core::indexed_mesh::RepairPasses::default_repair());
        std::borrow::Cow::Owned(repaired)
    }
}

/// Erode the interior using exact triangle-distance queries accelerated by a BVH.
pub fn hollow(mesh: &IndexedMesh, thickness: f64) -> Result<HollowedMesh, HollowError> {
    hollow_with_holes(mesh, thickness, &[])
}

/// Hollow and subtract finite cylindrical channels. With channels the exterior
/// is sampled on the same grid, with a maximum half-cell-diagonal surface error.
pub fn hollow_with_holes(
    mesh: &IndexedMesh,
    thickness: f64,
    holes: &[DrainHole],
) -> Result<HollowedMesh, HollowError> {
    check_holes(holes)?;
    if holes.is_empty() {
        return process_mesh(mesh, thickness, holes, true);
    }
    let hollowed = process_mesh(mesh, thickness, &[], true)?;
    match drill::drill(&hollowed.mesh, holes).filter(closed) {
        Some(drilled) => Ok(HollowedMesh {
            saved_mm3: hollowed.saved_mm3 + volume(&hollowed.mesh) - volume(&drilled),
            mesh: drilled,
            ..hollowed
        }),
        // The exact cut could not close the surface: the voxel cut still can.
        None => process_mesh(mesh, thickness, holes, true),
    }
}

fn check_holes(holes: &[DrainHole]) -> Result<(), HollowError> {
    if holes.iter().any(|h| {
        !h.position.iter().chain(&h.direction).all(|v| v.is_finite())
            || DVec3::from_array(h.direction).length_squared() < 1e-20
            || !h.diameter_mm.is_finite()
            || h.diameter_mm <= 0.0
            || !h.depth_mm.is_finite()
            || h.depth_mm <= 0.0
    }) {
        return Err(HollowError::InvalidDrain);
    }
    Ok(())
}

/// Whether a mesh is closed and edge-manifold.
fn closed(mesh: &IndexedMesh) -> bool {
    mesh.validated_faces()
        .is_ok_and(|v| v.validate_edge_manifold().is_ok())
}

/// Enclosed volume, mm3.
pub(crate) fn volume(mesh: &IndexedMesh) -> f64 {
    mesh.faces
        .iter()
        .map(|f| {
            let [a, b, c] = f
                .vertices
                .map(|i| DVec3::from_array(mesh.vertices[usize::from(i)].coords().map(f64::from)));
            a.dot(b.cross(c)) / 6.0
        })
        .sum()
}

/// Subtract finite drain cylinders from existing geometry without hollowing it.
/// The same bounded voxel grid and surface accuracy as hollowing apply.
pub fn drill_holes(mesh: &IndexedMesh, holes: &[DrainHole]) -> Result<HollowedMesh, HollowError> {
    if holes.is_empty() {
        return Err(HollowError::InvalidDrain);
    }
    check_holes(holes)?;
    if closed(mesh) && mesh.validate_vertices().is_ok() {
        if let Some(drilled) = drill::drill(mesh, holes).filter(closed) {
            return Ok(HollowedMesh {
                saved_mm3: volume(mesh) - volume(&drilled),
                mesh: drilled,
                pitch_mm: 0.0,
                suggested_holes: Vec::new(),
            });
        }
    }
    process_mesh(mesh, 2.0, holes, false)
}

fn process_mesh(
    mesh: &IndexedMesh,
    thickness: f64,
    holes: &[DrainHole],
    hollowing: bool,
) -> Result<HollowedMesh, HollowError> {
    if holes.iter().any(|h| {
        !h.position.iter().chain(&h.direction).all(|v| v.is_finite())
            || DVec3::from_array(h.direction).length_squared() < 1e-20
            || !h.diameter_mm.is_finite()
            || h.diameter_mm <= 0.0
            || !h.depth_mm.is_finite()
            || h.depth_mm <= 0.0
    }) {
        return Err(HollowError::InvalidDrain);
    }
    if !thickness.is_finite() || thickness <= 0.0 {
        return Err(HollowError::InvalidThickness);
    }
    mesh.validate_vertices()
        .map_err(|_| HollowError::InvalidMesh)?;
    let valid = mesh
        .validated_faces()
        .map_err(|_| HollowError::InvalidMesh)?;
    valid
        .validate_edge_manifold()
        .map_err(|_| HollowError::InvalidMesh)?;
    let triangles: Vec<_> = mesh
        .faces
        .iter()
        .map(|f| {
            let p = f
                .vertices
                .map(|i| DVec3::from_array(mesh.vertices[usize::from(i)].coords().map(f64::from)));
            Triangle {
                min: p[0].min(p[1]).min(p[2]),
                max: p[0].max(p[1]).max(p[2]),
                p,
            }
        })
        .collect();
    if triangles.is_empty() {
        return Err(HollowError::InvalidMesh);
    }
    // A closed manifold made entirely from the six bounding planes is a box.
    // On these planes a cell extends pitch/2 toward the surface, rather than
    // half its diagonal. Keep the sphere bound for all other geometry.
    let min = triangles
        .iter()
        .fold(DVec3::splat(f64::INFINITY), |a, t| a.min(t.min));
    let max = triangles
        .iter()
        .fold(DVec3::splat(f64::NEG_INFINITY), |a, t| a.max(t.max));
    let box_surface = triangles.iter().all(|t| {
        (0..3).any(|axis| {
            t.p.iter().all(|p| p[axis] == min[axis]) || t.p.iter().all(|p| p[axis] == max[axis])
        })
    });
    let tree = Tree::new(triangles);
    let mut pitch = 0.125_f64.min(thickness / 16.0);
    // Large models coarsen further (up to a quarter of the wall) instead of
    // being refused: a 0.25 mm floor capped hollowing at about an 80 mm cube,
    // below a typical figurine on a 192 x 120 mm plate. Walls stay at least
    // the requested thickness; only the conservative overestimate grows.
    let limit = 0.5_f64.min(thickness / 4.0);
    let (dims, count) = loop {
        let size = (tree.max - tree.min) / pitch;
        if size.max_element() <= 2048.0 {
            let dims = size.ceil().as_uvec3().to_array().map(|v| v as usize);
            let count = dims
                .iter()
                .try_fold(1usize, |a, b| a.checked_mul(*b))
                .ok_or(HollowError::TooLarge)?;
            if count <= 32_000_000 {
                break (dims, count);
            }
        }
        if pitch >= limit {
            return Err(HollowError::TooLarge);
        }
        pitch = (pitch * 1.1).min(limit);
    };
    let mut cells = vec![false; count];
    let mut material = if holes.is_empty() {
        Vec::new()
    } else {
        vec![false; count]
    };
    let index = |x, y, z| (z * dims[1] + y) * dims[0] + x;
    let clearance = thickness
        + if box_surface {
            pitch * 0.5
        } else {
            3.0_f64.sqrt() * pitch * 0.5
        };
    let mut saved = 0usize;
    let mut channel_removed = 0usize;
    for z in 0..dims[2] {
        for y in 0..dims[1] {
            let mut hits = Vec::new();
            // Irrational sub-nanometre perturbation avoids grid/triangle edge coincidences.
            let yy = tree.min.y + (y as f64 + 0.5) * pitch + 1.314e-9;
            let zz = tree.min.z + (z as f64 + 0.5) * pitch + 2.718e-9;
            tree.crossings(yy, zz, &mut hits);
            hits.sort_by(|a, b| a.0.total_cmp(&b.0));
            hits.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-8 && a.1 == b.1);
            let mut at = 0;
            let mut winding = 0.0_f64;
            for x in 0..dims[0] {
                let p = tree.min
                    + (DVec3::new(x as f64, y as f64, z as f64) + DVec3::splat(0.5)) * pitch;
                while at < hits.len() && hits[at].0 < p.x {
                    winding += hits[at].1;
                    at += 1;
                }
                // The smooth wall interpolates the real distance, so its
                // samples use the wall itself; drain cells keep the margin.
                let reach = if holes.is_empty() {
                    thickness
                } else {
                    clearance
                };
                if hollowing && winding.abs() > 0.5 && !tree.near(p, reach) {
                    cells[index(x, y, z)] = true;
                    saved += 1;
                }
                if !holes.is_empty() {
                    let cut = holes.iter().any(|hole| {
                        let direction = DVec3::from_array(hole.direction).normalize_or_zero();
                        let q = p - DVec3::from_array(hole.position);
                        let along = q.dot(direction);
                        along >= -pitch
                            && along <= hole.depth_mm
                            && (q - direction * along).length_squared()
                                <= (hole.diameter_mm * 0.5).powi(2)
                    });
                    material[index(x, y, z)] =
                        winding.abs() > 0.5 && !cells[index(x, y, z)] && !cut;
                    if winding.abs() > 0.5 && !cells[index(x, y, z)] && cut {
                        channel_removed += 1;
                    }
                }
            }
        }
    }
    if saved == 0 && hollowing {
        return Err(HollowError::NoCavity);
    }
    let mut suggested_holes = Vec::new();
    let mut visited = vec![false; count];
    for start in 0..count {
        if !cells[start] || visited[start] {
            continue;
        }
        let mut queue = vec![start];
        visited[start] = true;
        let mut lowest = Vec::new();
        let mut low_z = usize::MAX;
        while let Some(i) = queue.pop() {
            let xyz = [
                i % dims[0],
                (i / dims[0]) % dims[1],
                i / (dims[0] * dims[1]),
            ];
            if xyz[2] < low_z {
                lowest.clear();
                low_z = xyz[2];
            }
            if xyz[2] == low_z {
                lowest.push(xyz);
            }
            for axis in 0..3 {
                for delta in [-1isize, 1] {
                    let mut next = xyz;
                    let n = xyz[axis] as isize + delta;
                    if n < 0 || n >= dims[axis] as isize {
                        continue;
                    }
                    next[axis] = n as usize;
                    let j = index(next[0], next[1], next[2]);
                    if cells[j] && !visited[j] {
                        visited[j] = true;
                        queue.push(j);
                    }
                }
            }
        }
        lowest.sort();
        // Keep suggested channels away from side walls. Picking quartiles of
        // lexicographically sorted cells put both starts on a cavity edge,
        // producing diagonal material contacts where the drain meets the void.
        let lo = lowest
            .iter()
            .fold([usize::MAX; 2], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
        let hi = lowest
            .iter()
            .fold([0; 2], |a, p| [a[0].max(p[0]), a[1].max(p[1])]);
        let axis = usize::from(hi[1] - lo[1] > hi[0] - lo[0]);
        for fraction in [0.25, 0.75] {
            let mut target = [(lo[0] + hi[0]) as f64 * 0.5, (lo[1] + hi[1]) as f64 * 0.5];
            target[axis] = lo[axis] as f64 + fraction * (hi[axis] - lo[axis]) as f64;
            let xyz = lowest
                .iter()
                .min_by(|a, b| {
                    let distance = |p: &[usize; 3]| {
                        (p[0] as f64 - target[0]).powi(2) + (p[1] as f64 - target[1]).powi(2)
                    };
                    distance(a).total_cmp(&distance(b))
                })
                .expect("nonempty cavity floor");
            suggested_holes.push([
                tree.min.x + (xyz[0] as f64 + 0.5) * pitch,
                tree.min.y + (xyz[1] as f64 + 0.5) * pitch,
                tree.min.z + (xyz[2] as f64 + 0.5) * pitch,
            ]);
        }
    }
    if hollowing && holes.is_empty() {
        let mut result = mesh.clone();
        let cavity = surface_net(&tree, &cells, dims, pitch, thickness, &mut result);
        return Ok(HollowedMesh {
            mesh: result,
            saved_mm3: cavity,
            pitch_mm: pitch,
            suggested_holes,
        });
    }
    let mut result = if holes.is_empty() {
        mesh.clone()
    } else {
        IndexedMesh::new(mesh.meta.clone(), Vec::new(), Vec::new())
    };
    if !holes.is_empty() {
        cells = material;
    }
    let mut vertices = HashMap::<[usize; 3], u32>::new();
    // CCW viewed from inside a cavity cell: outward model material normals.
    let faces = [
        [[0, 0, 0], [0, 1, 0], [0, 1, 1], [0, 0, 1]],
        [[1, 0, 0], [1, 0, 1], [1, 1, 1], [1, 1, 0]],
        [[0, 0, 0], [0, 0, 1], [1, 0, 1], [1, 0, 0]],
        [[0, 1, 0], [1, 1, 0], [1, 1, 1], [0, 1, 1]],
        [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]],
        [[0, 0, 1], [0, 1, 1], [1, 1, 1], [1, 0, 1]],
    ];
    for z in 0..dims[2] {
        for y in 0..dims[1] {
            for x in 0..dims[0] {
                if !cells[index(x, y, z)] {
                    continue;
                }
                for (side, offsets) in faces.iter().enumerate() {
                    let axis = side / 2;
                    let mut next = [x as isize, y as isize, z as isize];
                    next[axis] += if side % 2 == 0 { -1 } else { 1 };
                    if next
                        .iter()
                        .zip(dims)
                        .all(|(&a, b)| a >= 0 && a < (b as isize))
                        && cells[index(next[0] as usize, next[1] as usize, next[2] as usize)]
                    {
                        continue;
                    }
                    let mut indices = offsets.map(|o| {
                        let key = [x + o[0], y + o[1], z + o[2]];
                        *vertices.entry(key).or_insert_with(|| {
                            let id = result.vertices.len() as u32;
                            result.vertices.push(Vertex::new(
                                (tree.min
                                    + DVec3::new(key[0] as f64, key[1] as f64, key[2] as f64)
                                        * pitch)
                                    .to_array()
                                    .map(|v| v as f32),
                            ));
                            id
                        })
                    });
                    if !holes.is_empty() {
                        indices.reverse();
                    }
                    for ids in [
                        [indices[0], indices[1], indices[2]],
                        [indices[0], indices[2], indices[3]],
                    ] {
                        let [a, b, c] = ids.map(|i| {
                            DVec3::from_array(result.vertices[i as usize].coords().map(f64::from))
                        });
                        result.faces.push(IndexedTriangle::from_raw_indices(
                            Normal::new(
                                (b - a)
                                    .cross(c - a)
                                    .normalize()
                                    .to_array()
                                    .map(|v| v as f32),
                            ),
                            ids,
                        ));
                    }
                }
            }
        }
    }
    Ok(HollowedMesh {
        mesh: result,
        saved_mm3: (saved + channel_removed) as f64 * pitch.powi(3),
        pitch_mm: pitch,
        suggested_holes,
    })
}

/// Appends the cavity wall to `out`: the surface where the distance to the
/// exterior equals `thickness`, by surface nets over the sample lattice
/// (`cavity` marks samples farther than that). Each separate piece of wall
/// in a lattice cube gets one vertex, at the mean of its edges' interpolated
/// crossings;
/// each crossed lattice edge gets a quad of the four cubes around it, facing
/// into the cavity (outward from the material). Returns the cavity volume
/// the new faces enclose, mm3.
fn surface_net(
    tree: &Tree,
    cavity: &[bool],
    dims: [usize; 3],
    pitch: f64,
    thickness: f64,
    out: &mut IndexedMesh,
) -> f64 {
    let index = |x: usize, y: usize, z: usize| (z * dims[1] + y) * dims[0] + x;
    let centre = |x: usize, y: usize, z: usize| {
        tree.min + (DVec3::new(x as f64, y as f64, z as f64) + DVec3::splat(0.5)) * pitch
    };
    // Field at a sample beside the wall: distance minus the wall (> 0 in
    // the cavity). Only samples with a neighbour across the wall need it.
    let mut field: HashMap<usize, f64> = HashMap::new();
    let mut value = |x: usize, y: usize, z: usize| -> f64 {
        let i = index(x, y, z);
        *field.entry(i).or_insert_with(|| {
            let mut best = thickness + 4.0 * pitch;
            tree.nearest(centre(x, y, z), &mut best);
            let f = best - thickness;
            // Keep the sample's side: interpolation must cross between them.
            if cavity[i] { f.max(1e-9) } else { f.min(-1e-9) }
        })
    };
    let solid = |x: isize, y: isize, z: isize| {
        x < 0
            || y < 0
            || z < 0
            || x as usize >= dims[0]
            || y as usize >= dims[1]
            || z as usize >= dims[2]
            || !cavity[index(x as usize, y as usize, z as usize)]
    };
    // A cube's corner (bits x, y, z) is in the cavity.
    let corner_in = |c: [usize; 3], k: usize| {
        cavity[index(c[0] + (k & 1), c[1] + ((k >> 1) & 1), c[2] + (k >> 2))]
    };
    // A cube's 12 edges as corner pairs, and each face's 4 edges in order
    // round it with the corner each pair of consecutive edges meets at.
    const EDGES: [(usize, usize); 12] = [
        (0, 1),
        (2, 3),
        (4, 5),
        (6, 7), // along x
        (0, 2),
        (1, 3),
        (4, 6),
        (5, 7), // along y
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7), // along z
    ];
    let edge_of = |a: usize, b: usize| {
        EDGES
            .iter()
            .position(|&(p, q)| (p, q) == (a.min(b), a.max(b)))
            .expect("cube edge")
    };
    let faces: Vec<[usize; 4]> = (0..3)
        .flat_map(|axis| {
            let (u, v) = (1 << ((axis + 1) % 3), 1 << ((axis + 2) % 3));
            [0, 1 << axis].map(|side| [side, side + u, side + u + v, side + v])
        })
        .collect();
    // Vertices per cube edge: one per separate piece of wall in the cube
    // (R111). A single vertex per cube, as plain surface nets has, pinches
    // two sheets of wall together where cavity samples touch diagonally,
    // and the shared edges then belong to four faces: a hollowed figurine
    // came out with 224 such edges, and no drain hole could be cut into it.
    let mut cube_vertices: HashMap<[usize; 3], [u32; 12]> = HashMap::new();
    let mut vertex_of = |c: [usize; 3],
                         edge: usize,
                         out: &mut IndexedMesh,
                         value: &mut dyn FnMut(usize, usize, usize) -> f64|
     -> Option<u32> {
        if c.iter().zip(dims).any(|(&a, d)| a + 1 >= d) {
            return None;
        }
        if let Some(ids) = cube_vertices.get(&c) {
            return (ids[edge] != u32::MAX).then_some(ids[edge]);
        }
        let crossed: [bool; 12] =
            std::array::from_fn(|e| corner_in(c, EDGES[e].0) != corner_in(c, EDGES[e].1));
        // Group the crossed edges into pieces through the cube's faces. A
        // face with two crossed edges joins them; a face with four (cavity
        // on one diagonal) pairs the edges around each cavity corner, so the
        // cavity corners stay apart. The choice depends only on the face's
        // corners, so the two cubes sharing a face always agree.
        let mut parent: [usize; 12] = std::array::from_fn(|e| e);
        fn root(p: &mut [usize; 12], mut e: usize) -> usize {
            while p[e] != e {
                p[e] = p[p[e]];
                e = p[e];
            }
            e
        }
        for face in &faces {
            let ring: [usize; 4] = std::array::from_fn(|k| edge_of(face[k], face[(k + 1) % 4]));
            let hit: Vec<usize> = (0..4).filter(|&k| crossed[ring[k]]).collect();
            let mut join = |a: usize, b: usize| {
                let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
                parent[ra] = rb;
            };
            if hit.len() == 2 {
                join(ring[hit[0]], ring[hit[1]]);
            } else if hit.len() == 4 {
                // Corner k sits between ring edges k-1 and k.
                for k in 0..4 {
                    if corner_in(c, face[k]) {
                        join(ring[(k + 3) % 4], ring[k]);
                    }
                }
            }
        }
        let mut ids = [u32::MAX; 12];
        for e in 0..12 {
            if !crossed[e] || ids[e] != u32::MAX {
                continue;
            }
            let r = root(&mut parent, e);
            let members: Vec<usize> = (0..12)
                .filter(|&f| crossed[f] && root(&mut parent, f) == r)
                .collect();
            let mut sum = DVec3::ZERO;
            for &f in &members {
                let (a, b) = EDGES[f];
                let pa = [c[0] + (a & 1), c[1] + ((a >> 1) & 1), c[2] + (a >> 2)];
                let pb = [c[0] + (b & 1), c[1] + ((b >> 1) & 1), c[2] + (b >> 2)];
                let (fa, fb) = (value(pa[0], pa[1], pa[2]), value(pb[0], pb[1], pb[2]));
                let t = (fa / (fa - fb)).clamp(0.0, 1.0);
                sum += centre(pa[0], pa[1], pa[2]).lerp(centre(pb[0], pb[1], pb[2]), t);
            }
            let id = out.vertices.len() as u32;
            out.vertices.push(Vertex::new(
                (sum / members.len() as f64).to_array().map(|v| v as f32),
            ));
            for f in members {
                ids[f] = id;
            }
        }
        cube_vertices.insert(c, ids);
        (ids[edge] != u32::MAX).then_some(ids[edge])
    };
    let mut volume = 0.0;
    for z in 0..dims[2] {
        for y in 0..dims[1] {
            for x in 0..dims[0] {
                if !cavity[index(x, y, z)] {
                    continue;
                }
                for axis in 0..3 {
                    for step in [-1isize, 1] {
                        let mut n = [x as isize, y as isize, z as isize];
                        n[axis] += step;
                        if !solid(n[0], n[1], n[2]) {
                            continue;
                        }
                        // The crossed lattice edge runs from its lower sample.
                        let mut lower = [x as isize, y as isize, z as isize];
                        if step < 0 {
                            lower[axis] -= 1;
                        }
                        if lower.iter().any(|&a| a < 0) {
                            continue;
                        }
                        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
                        // The four cubes around the edge, counter-clockwise
                        // about +axis; in each, the edge runs from the
                        // corner offset (du, dv) along the axis.
                        let mut quad = Vec::with_capacity(4);
                        for (du, dv) in [(1, 1), (0, 1), (0, 0), (1, 0)] {
                            let mut c = lower;
                            c[u] -= du;
                            c[v] -= dv;
                            if c.iter().any(|&a| a < 0) {
                                break;
                            }
                            let c = c.map(|a| a as usize);
                            let start = ((du as usize) << u) | ((dv as usize) << v);
                            match vertex_of(c, edge_of(start, start | (1 << axis)), out, &mut value)
                            {
                                Some(id) => quad.push(id),
                                None => break,
                            }
                        }
                        if quad.len() < 4 {
                            continue;
                        }
                        // Facing into the cavity: the cycle above runs
                        // counter-clockwise about +axis, which faces the
                        // upper end, so reverse it when the cavity is below.
                        if step > 0 {
                            quad.reverse();
                        }
                        for ids in [[quad[0], quad[1], quad[2]], [quad[0], quad[2], quad[3]]] {
                            let [a, b, c] = ids.map(|i| {
                                DVec3::from_array(out.vertices[i as usize].coords().map(f64::from))
                            });
                            // A sliver stays: dropping it would open the wall.
                            let normal = (b - a).cross(c - a).try_normalize().unwrap_or(DVec3::Z);
                            volume += a.dot(b.cross(c)) / 6.0;
                            out.faces.push(IndexedTriangle::from_raw_indices(
                                Normal::new(normal.to_array().map(|v| v as f32)),
                                ids,
                            ));
                        }
                    }
                }
            }
        }
    }
    // The wall faces into the cavity, so it encloses negative volume.
    -volume
}

#[cfg(test)]
mod net_tests {
    use super::*;

    /// R111: two cavity samples touching across a lattice face's diagonal
    /// make two separate walls. With one vertex per cube they shared the
    /// cubes between them, and two edges belonged to four faces each (a
    /// hollowed figurine had 224 such edges, so no drain hole could be cut).
    #[test]
    fn diagonal_cavity_samples_give_two_separate_closed_walls() {
        let cube = sc_scene::bake_mesh_to_world(
            &sc_mesh_fixtures::unit_cube_mesh(),
            &sc_mesh_core::transform::Transform3D::Affine(
                sc_mesh_core::transform::AffineTransform3D::from_uniform_scale(16.0),
            ),
        )
        .unwrap();
        let tree = Tree::from_mesh(&cube);
        let dims = [8, 8, 8];
        let mut cavity = vec![false; 512];
        for [x, y, z] in [[2, 2, 2], [3, 3, 2]] {
            cavity[(z * 8 + y) * 8 + x] = true;
        }
        let mut out = IndexedMesh::new(None, Vec::new(), Vec::new());
        surface_net(&tree, &cavity, dims, 1.0, 2.0, &mut out);
        let mut uses: HashMap<(u32, u32), usize> = HashMap::new();
        for f in &out.faces {
            let v = f.vertices.map(u32::from);
            for k in 0..3 {
                let (a, b) = (v[k], v[(k + 1) % 3]);
                *uses.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        // Two closed walls of six quads each.
        assert_eq!(out.faces.len(), 24);
        assert!(uses.values().all(|&n| n == 2), "{uses:?}");
        assert_eq!(out.vertices.len(), 16);
    }
}

#[cfg(test)]
mod message_tests {
    use super::HollowError;

    // Workflow sweep: the dialog text had runs of spaces mid-sentence
    // ("so its                  inside") from a broken line continuation.
    #[test]
    fn hollow_messages_have_no_runs_of_spaces() {
        for e in [
            HollowError::InvalidThickness,
            HollowError::InvalidMesh,
            HollowError::TooLarge,
            HollowError::NoCavity,
            HollowError::InvalidDrain,
        ] {
            let text = e.to_string();
            assert!(!text.contains("  "), "{text}");
        }
    }
}

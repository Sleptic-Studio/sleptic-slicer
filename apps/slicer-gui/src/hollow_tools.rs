// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Editable hollow settings travel with the mesh and its undo snapshot.
use sc_mesh_core::{IndexedMesh, IndexedTriangle, Normal, Vertex, indexed_mesh::MeshMetadata};
use sc_print_glue::hollow::{DrainHole, hollow_with_holes};
use sc_scene::{NodeId, Scene};
use sc_scene_edit::SceneEdit;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const PREFIX: &str = "sleptic-hollow-v1\n";
#[cfg(test)]
#[path = "tests_hollow_tools.rs"]
mod tests;
#[derive(Serialize, Deserialize)]
struct Hole {
    position: [f64; 3],
    direction: [f64; 3],
    diameter: f64,
    depth: f64,
}
#[derive(Serialize, Deserialize)]
struct Settings {
    thickness: f64,
    saved_mm3: f64,
    vertices: Vec<[f32; 3]>,
    faces: Vec<([f32; 3], [u32; 3])>,
    holes: Vec<Hole>,
}

fn settings(mesh: &IndexedMesh) -> Option<Settings> {
    serde_json::from_str(
        mesh.meta
            .as_ref()?
            .header_comment
            .as_ref()?
            .strip_prefix(PREFIX)?,
    )
    .ok()
}

fn volume_scale(transform: &sc_mesh_core::transform::Transform3D) -> f64 {
    match transform {
        sc_mesh_core::transform::Transform3D::Identity => 1.0,
        sc_mesh_core::transform::Transform3D::Affine(a) => a.determinant().abs(),
    }
}

fn cavity_depth(
    mesh: &IndexedMesh,
    first_face: usize,
    origin: [f64; 3],
    direction: [f64; 3],
) -> Option<f64> {
    use glam::DVec3;
    let origin = DVec3::from_array(origin);
    let direction = DVec3::from_array(direction).normalize_or_zero();
    mesh.faces[first_face..]
        .iter()
        .filter_map(|face| {
            let [a, b, c] = face
                .vertices
                .map(|i| DVec3::from_array(mesh.vertices[usize::from(i)].coords().map(f64::from)));
            let e = b - a;
            let f = c - a;
            let h = direction.cross(f);
            let determinant = e.dot(h);
            if determinant.abs() < 1e-12 {
                return None;
            }
            let delta = origin - a;
            let u = delta.dot(h) / determinant;
            let q = delta.cross(e);
            let v = direction.dot(q) / determinant;
            if !(0.0..=1.0).contains(&u) || v < 0.0 || u + v > 1.0 {
                return None;
            }
            let distance = f.dot(q) / determinant;
            (distance > 1e-6).then_some(distance)
        })
        .min_by(f64::total_cmp)
}

/// Which way a placed drain hole runs into the model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DrainAim {
    /// Straight into the surface where it was placed.
    Surface,
    /// Along a world direction (the view ray, or straight down).
    Along([f64; 3]),
}

/// One drain hole to add, in world millimetres (R111).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DrainRequest {
    /// A point on (or next to) the surface; the hole starts at the nearest
    /// surface point.
    pub point: [f64; 3],
    pub aim: DrainAim,
    pub diameter_mm: f64,
    /// Length into the model; `None` runs through the wall into the cavity.
    pub depth_mm: Option<f64>,
    /// Also add the hole mirrored across the model's centre, left to right
    /// (x) and front to back (y); both gives four holes.
    pub mirror: [bool; 2],
}

/// A change to an object's hollowing.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum HoleChange {
    /// Hollow (again) with the current holes.
    Keep,
    /// Add drain holes at the cavity floors, this diameter.
    Auto(f64),
    Add(DrainRequest),
    Remove(usize),
    Clear,
}

/// The nearest point on the mesh's surface to `p`, and that face's normal.
fn surface_at(mesh: &IndexedMesh, p: [f64; 3]) -> Option<([f64; 3], [f64; 3])> {
    use glam::DVec3;
    let p = DVec3::from_array(p);
    let mut best: Option<(f64, DVec3, DVec3)> = None;
    for face in &mesh.faces {
        let [a, b, c] = face
            .vertices
            .map(|i| DVec3::from_array(mesh.vertices[usize::from(i)].coords().map(f64::from)));
        let normal = (b - a).cross(c - a);
        if normal.length_squared() < 1e-24 {
            continue;
        }
        let q = closest_on_triangle(p, a, b, c);
        let d = (q - p).length_squared();
        if best.is_none_or(|(e, _, _)| d < e) {
            best = Some((d, q, normal.normalize()));
        }
    }
    best.map(|(_, q, n)| (q.to_array(), n.to_array()))
}

/// Closest point of triangle abc to p (Ericson, Real-Time Collision
/// Detection, 5.1.5).
fn closest_on_triangle(
    p: glam::DVec3,
    a: glam::DVec3,
    b: glam::DVec3,
    c: glam::DVec3,
) -> glam::DVec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// The drain holes `request` places on `mesh` (world), with mirrored copies:
/// (entry point, inward direction, depth). A copy whose mirror lands on the
/// original (a point on the mirror plane) is left out.
fn placed_holes(
    mesh: &IndexedMesh,
    cavity: &sc_print_glue::hollow::HollowedMesh,
    request: &DrainRequest,
) -> Result<Vec<([f64; 3], [f64; 3], f64)>, String> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for v in &mesh.vertices {
        for (a, c) in v.coords().into_iter().enumerate() {
            lo[a] = lo[a].min(f64::from(c));
            hi[a] = hi[a].max(f64::from(c));
        }
    }
    let centre: [f64; 3] = std::array::from_fn(|a| 0.5 * (lo[a] + hi[a]));
    let mut flips = vec![[false, false]];
    if request.mirror[0] {
        flips.push([true, false]);
    }
    if request.mirror[1] {
        flips.extend(flips.clone().into_iter().map(|[x, _]| [x, true]));
    }
    let mut out: Vec<([f64; 3], [f64; 3], f64)> = Vec::new();
    for flip in flips {
        let mirror = |p: [f64; 3], about: [f64; 3]| -> [f64; 3] {
            std::array::from_fn(|a| {
                if a < 2 && flip[a] {
                    2.0 * about[a] - p[a]
                } else {
                    p[a]
                }
            })
        };
        let point = mirror(request.point, centre);
        let Some((entry, normal)) = surface_at(mesh, point) else {
            continue;
        };
        if out
            .iter()
            .any(|(p, _, _)| (0..3).all(|a| (p[a] - entry[a]).abs() < 1e-3))
        {
            continue;
        }
        let direction = match request.aim {
            DrainAim::Surface => normal.map(|v| -v),
            DrainAim::Along(d) => mirror(d, [0.0; 3]),
        };
        let depth = match request.depth_mm {
            Some(depth) => depth,
            None => {
                cavity_depth(&cavity.mesh, mesh.faces.len(), entry, direction).ok_or_else(|| {
                    crate::i18n::tr!("The drain direction misses the cavity. Choose another point.")
                        .to_string()
                })? + (cavity.pitch_mm * 2.0).max(0.3)
            }
        };
        out.push((entry, direction, depth));
    }
    Ok(out)
}

/// One line per drain hole of the object, for the hole list.
pub(crate) fn hole_rows(scene: &Scene, id: NodeId) -> Vec<String> {
    let Some(s) = scene.mesh_at_leaf(id).ok().and_then(settings) else {
        return Vec::new();
    };
    s.holes
        .iter()
        .map(|h| {
            crate::i18n::tr!(
                "{} mm wide, {} mm long",
                format!("{:.1}", h.diameter),
                format!("{:.1}", h.depth)
            )
            .to_string()
        })
        .collect()
}

pub(crate) fn wall_thickness(scene: &Scene, id: NodeId) -> Option<f64> {
    Some(settings(scene.mesh_at_leaf(id).ok()?)?.thickness)
}

pub(crate) fn stats(scene: &Scene) -> (f64, usize) {
    let mut saved = 0.0;
    let mut warnings = 0;
    let _ = scene.for_each_expanded_leaf::<sc_scene::SceneError, _>(
        sc_scene::CsgPolicy::Descend,
        |_, world, mesh| {
            if let Some(s) = settings(mesh) {
                saved += s.saved_mm3 * volume_scale(world);
                warnings += usize::from(s.holes.is_empty());
            }
            Ok(())
        },
    );
    (saved, warnings)
}

/// Restore the stored source mesh as one reversible mesh swap.
pub(crate) fn remove(scene: &Scene, id: NodeId) -> Option<SceneEdit> {
    let before = scene.leaf_mesh_arc(id)?.clone();
    let s = settings(&before)?;
    let after = Arc::new(IndexedMesh::new(
        None,
        s.vertices.into_iter().map(Vertex::new).collect(),
        s.faces
            .into_iter()
            .map(|(n, i)| IndexedTriangle::from_raw_indices(Normal::new(n), i))
            .collect(),
    ));
    Some(match scene.node(id)?.kind {
        sc_scene::node::NodeKind::Leaf(sc_scene::node::LeafPayload::MeshResource(rid)) => {
            SceneEdit::ReplaceMeshResource { rid, before, after }
        }
        _ => SceneEdit::ReplaceLeafMesh {
            node_id: id,
            before,
            after,
        },
    })
}

/// Rebuild from the saved original, never from a previously voxelized surface.
pub(crate) fn edit(
    scene: &Scene,
    id: NodeId,
    thickness: f64,
    change: HoleChange,
) -> Result<SceneEdit, String> {
    let before = scene.leaf_mesh_arc(id).ok_or("Missing mesh")?.clone();
    let mut s = settings(&before).unwrap_or_else(|| Settings {
        thickness,
        saved_mm3: 0.0,
        vertices: before.vertices.iter().map(|v| v.coords()).collect(),
        faces: before
            .faces
            .iter()
            .map(|f| (f.normal.coords(), f.vertices.map(u32::from)))
            .collect(),
        holes: Vec::new(),
    });
    s.thickness = thickness;
    let source = IndexedMesh::new(
        None,
        s.vertices.iter().copied().map(Vertex::new).collect(),
        s.faces
            .iter()
            .map(|&(n, i)| IndexedTriangle::from_raw_indices(Normal::new(n), i))
            .collect(),
    );
    let world = scene.world_transform_of(id).map_err(|e| e.to_string())?;
    let inverse = world.inverse().map_err(|e| e.to_string())?;
    let baked = sc_scene::bake_mesh_to_world(&source, &world).map_err(|e| e.to_string())?;
    let baked = sc_print_glue::hollow::hollow_source(&baked).into_owned();
    let local_point = |p: [f64; 3]| {
        inverse
            .apply_to_vertex(Vertex::new(p.map(|v| v as f32)))
            .coords()
            .map(f64::from)
    };
    let auto = match &change {
        HoleChange::Auto(diameter) => Some(*diameter),
        _ => None,
    };
    match change {
        HoleChange::Add(request) => {
            if !(request.diameter_mm.is_finite() && request.diameter_mm > 0.0)
                || request
                    .depth_mm
                    .is_some_and(|d| !(d.is_finite() && d > 0.0))
            {
                return Err(sc_print_glue::hollow::HollowError::InvalidDrain.to_string());
            }
            let initial =
                sc_print_glue::hollow::hollow(&baked, thickness).map_err(|e| e.to_string())?;
            for (position, direction, depth) in placed_holes(&baked, &initial, &request)? {
                let p = local_point(position);
                let end = local_point(std::array::from_fn(|i| position[i] + direction[i]));
                s.holes.push(Hole {
                    position: p,
                    direction: std::array::from_fn(|i| end[i] - p[i]),
                    diameter: request.diameter_mm,
                    depth,
                });
            }
        }
        HoleChange::Remove(index) if index < s.holes.len() => {
            s.holes.remove(index);
        }
        HoleChange::Clear => s.holes.clear(),
        _ => {}
    }
    if let Some(diameter) = auto {
        let initial =
            sc_print_glue::hollow::hollow(&baked, thickness).map_err(|e| e.to_string())?;
        for p in initial.suggested_holes {
            let local = local_point(p);
            let end = local_point([p[0], p[1], p[2] - 1.0]);
            let bottom = baked
                .vertices
                .iter()
                .map(|v| f64::from(v.coords()[2]))
                .fold(f64::INFINITY, f64::min);
            s.holes.push(Hole {
                position: local,
                direction: std::array::from_fn(|i| end[i] - local[i]),
                diameter,
                depth: p[2] - bottom + 1.0,
            });
        }
    }
    let world_point = |p: [f64; 3]| {
        world
            .apply_to_vertex(Vertex::new(p.map(|v| v as f32)))
            .coords()
            .map(f64::from)
    };
    let holes: Vec<_> = s
        .holes
        .iter()
        .map(|h| {
            let p = world_point(h.position);
            let end = world_point(std::array::from_fn(|i| h.position[i] + h.direction[i]));
            DrainHole {
                position: p,
                direction: std::array::from_fn(|i| end[i] - p[i]),
                diameter_mm: h.diameter,
                depth_mm: h.depth,
            }
        })
        .collect();
    let result = hollow_with_holes(&baked, thickness, &holes).map_err(|e| e.to_string())?;
    s.saved_mm3 = result.saved_mm3 / volume_scale(&world);
    let mut mesh =
        sc_scene::bake_mesh_to_world(&result.mesh, &inverse).map_err(|e| e.to_string())?;
    mesh.meta = Some(MeshMetadata {
        name: before.meta.as_ref().and_then(|m| m.name.clone()),
        header_comment: Some(format!(
            "{PREFIX}{}",
            serde_json::to_string(&s).map_err(|e| e.to_string())?
        )),
    });
    // Resource-backed leaves share geometry: replace only this object's payload.
    // Remove/add preserves the stable ID, metadata, hierarchy and root order.
    let node = scene.node(id).ok_or("Missing node")?;
    let sc_scene::node::NodeKind::Leaf(payload) = &node.kind else {
        return Err("Not a mesh".into());
    };
    let attachment = scene.attachment_of(id).ok_or("Missing attachment")?;
    Ok(SceneEdit::Compound(vec![
        SceneEdit::RemoveLeaf(sc_scene_edit::LeafRecord {
            node_id: id,
            attachment,
            payload: payload.clone(),
            local_transform: node.local_transform,
            meta: node.meta.clone(),
        }),
        SceneEdit::AddLeaf(sc_scene_edit::LeafRecord {
            node_id: id,
            attachment,
            payload: sc_scene::node::LeafPayload::Mesh(Arc::new(mesh)),
            local_transform: node.local_transform,
            meta: node.meta.clone(),
        }),
    ]))
}

/// Cut a downward vent at a diagnosed cavity floor, preserving the existing
/// material instead of hollowing an already naturally cupped model again.
pub(crate) fn drain_risk(
    scene: &Scene,
    point: [f64; 3],
    diameter: f64,
) -> Result<SceneEdit, String> {
    let mut edits = Vec::new();
    let mut ids = Vec::new();
    scene
        .for_each_expanded_leaf::<sc_scene::SceneError, _>(
            sc_scene::CsgPolicy::Refuse,
            |id, _, _| {
                ids.push(id);
                Ok(())
            },
        )
        .map_err(|e| e.to_string())?;
    let mut candidates = Vec::new();
    for id in ids {
        if scene
            .node(id)
            .and_then(|n| n.meta.as_ref())
            .and_then(|m| m.comment.as_deref())
            .is_some_and(|s| s.starts_with("sc-generated-support"))
        {
            continue;
        }
        let before = scene.leaf_mesh_arc(id).ok_or("Missing mesh")?.clone();
        let world = scene.world_transform_of(id).map_err(|e| e.to_string())?;
        let baked = sc_scene::bake_mesh_to_world(&before, &world).map_err(|e| e.to_string())?;
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for v in &baked.vertices {
            for (a, c) in v.coords().into_iter().enumerate() {
                lo[a] = lo[a].min(f64::from(c));
                hi[a] = hi[a].max(f64::from(c));
            }
        }
        if (0..3).any(|a| point[a] < lo[a] || point[a] > hi[a]) {
            continue;
        }
        if let Some(distance) = floor_distance(&baked, point) {
            candidates.push((distance, id, before, world, baked, lo));
        }
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, id, before, world, baked, lo)) = candidates.into_iter().next() {
        if let Some(s) = settings(&before) {
            return edit(scene, id, s.thickness, HoleChange::Auto(diameter));
        }
        let hole = DrainHole {
            position: point,
            direction: [0.0, 0.0, -1.0],
            diameter_mm: diameter,
            depth_mm: point[2] - lo[2] + 1.0,
        };
        let result =
            sc_print_glue::hollow::drill_holes(&baked, &[hole]).map_err(|e| e.to_string())?;
        if result.saved_mm3 <= 0.0 {
            return Err(crate::i18n::tr!(
                "No model floor was found at this cavity. Place a drain hole manually."
            )
            .to_string());
        }
        let inverse = world.inverse().map_err(|e| e.to_string())?;
        let mut after =
            sc_scene::bake_mesh_to_world(&result.mesh, &inverse).map_err(|e| e.to_string())?;
        // The original hollow source metadata cannot describe a direct geometry
        // subtraction. Preserve its display name and let undo restore all data.
        after.meta = before.meta.clone();
        if let Some(meta) = &mut after.meta {
            meta.header_comment = None;
        }
        let node = scene.node(id).ok_or("Missing node")?;
        let sc_scene::node::NodeKind::Leaf(payload) = &node.kind else {
            return Err("Not a mesh".into());
        };
        let attachment = scene.attachment_of(id).ok_or("Missing attachment")?;
        edits.push(SceneEdit::RemoveLeaf(sc_scene_edit::LeafRecord {
            node_id: id,
            attachment,
            payload: payload.clone(),
            local_transform: node.local_transform,
            meta: node.meta.clone(),
        }));
        edits.push(SceneEdit::AddLeaf(sc_scene_edit::LeafRecord {
            node_id: id,
            attachment,
            payload: sc_scene::node::LeafPayload::Mesh(Arc::new(after)),
            local_transform: node.local_transform,
            meta: node.meta.clone(),
        }));
    }
    if edits.is_empty() {
        return Err(crate::i18n::tr!(
            "No model floor was found at this cavity. Place a drain hole manually."
        )
        .to_string());
    }
    Ok(SceneEdit::Compound(edits))
}

// The nearest downward intersection must enter material. A point already in
// another model would first exit its material and is not a cavity to drill.
fn floor_distance(mesh: &IndexedMesh, point: [f64; 3]) -> Option<f64> {
    use glam::DVec3;
    let origin = DVec3::from_array(point) + DVec3::new(1.314e-9, 2.718e-9, 0.0);
    let direction = DVec3::NEG_Z;
    mesh.faces
        .iter()
        .filter_map(|f| {
            let [a, b, c] = f
                .vertices
                .map(|i| DVec3::from_array(mesh.vertices[usize::from(i)].coords().map(f64::from)));
            let e = b - a;
            let g = c - a;
            let h = direction.cross(g);
            let det = e.dot(h);
            if det.abs() < 1e-12 {
                return None;
            }
            let d = origin - a;
            let u = d.dot(h) / det;
            let q = d.cross(e);
            let v = direction.dot(q) / det;
            let t = g.dot(q) / det;
            if !(0.0..=1.0).contains(&u) || v < 0.0 || u + v > 1.0 || t <= 1e-6 {
                return None;
            }
            Some((t, e.cross(g).dot(direction)))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .filter(|&(_, normal)| normal < 0.0)
        .map(|(t, _)| t)
}

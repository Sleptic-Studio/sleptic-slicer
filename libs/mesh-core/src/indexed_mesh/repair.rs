// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::{
    FixFacesOptions, IndexedMesh, MeshEdgeManifoldError, MeshFaceError, MeshFix, MeshFixError,
    MeshVertexError,
    fix::orientation_plan,
    topology::{Disjoint, corner, edge_key},
    validated::{Incidence, incidences, manifold, manifold_from_incidences, normals},
};
use crate::{Vector3D, primitives::VertexIndex};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RepairPasses {
    pub zero_length_edges: bool,
    pub flipped_faces: bool,
    pub face_normals: bool,
    pub separate_shells: bool,
}
impl RepairPasses {
    pub const fn default_repair() -> Self {
        Self {
            zero_length_edges: true,
            flipped_faces: true,
            face_normals: false,
            separate_shells: true,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum RepairRefusal {
    InvalidVertices(Vec<MeshVertexError>),
    UnfixableFaceErrors(Vec<MeshFaceError>),
    OrientationFixFailed(MeshFixError),
}
impl std::fmt::Display for RepairRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidVertices(e) => {
                write!(f, "mesh has {} invalid vertex coordinates", e.len())
            }
            Self::UnfixableFaceErrors(e) => write!(
                f,
                "{} face errors the enabled repair passes cannot address",
                e.len()
            ),
            Self::OrientationFixFailed(e) => write!(f, "face orientation fix failed: {e}"),
        }
    }
}
impl std::error::Error for RepairRefusal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::OrientationFixFailed(e) => Some(e),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct MeshRepairReport {
    pub fixes: Vec<MeshFix>,
    pub refusal: Option<RepairRefusal>,
    pub edge_manifold_after: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub enum RepairAssessment {
    Clean,
    Refused(RepairRefusal),
    Work,
}
fn preliminary(mesh: &IndexedMesh, p: RepairPasses) -> Result<Vec<MeshFaceError>, RepairRefusal> {
    mesh.validate_vertices()
        .map_err(RepairRefusal::InvalidVertices)?;
    let errors = mesh.validate_faces().err().unwrap_or_default();
    let residue: Vec<_> = errors
        .iter()
        .filter(|e| !p.zero_length_edges || !matches!(e, MeshFaceError::ZeroLengthEdge { .. }))
        .cloned()
        .collect();
    if !residue.is_empty() {
        return Err(RepairRefusal::UnfixableFaceErrors(residue));
    }
    Ok(errors)
}
fn overloaded(errors: &[MeshEdgeManifoldError]) -> bool {
    errors
        .iter()
        .any(|e| matches!(e, MeshEdgeManifoldError::NonManifoldEdge { .. }))
}
pub fn assess_repair(mesh: &IndexedMesh, p: RepairPasses) -> RepairAssessment {
    match preliminary(mesh, p) {
        Err(e) => return RepairAssessment::Refused(e),
        Ok(e) => {
            if !e.is_empty() {
                return RepairAssessment::Work;
            }
        }
    }
    let edges = manifold(mesh).err().unwrap_or_default();
    if p.separate_shells && overloaded(&edges) {
        return RepairAssessment::Work;
    }
    let normal_errors = if p.face_normals {
        normals(mesh).err().unwrap_or_default()
    } else {
        Vec::new()
    };
    match orientation_plan(
        mesh,
        &edges,
        &normal_errors,
        FixFacesOptions {
            flipped: p.flipped_faces,
            normals: p.face_normals,
        },
    ) {
        Err(e) => RepairAssessment::Refused(RepairRefusal::OrientationFixFailed(e)),
        Ok(plan) => {
            if plan.is_empty() {
                RepairAssessment::Clean
            } else {
                RepairAssessment::Work
            }
        }
    }
}
impl IndexedMesh {
    pub fn repair(&mut self, p: RepairPasses) -> MeshRepairReport {
        let mut fixes = Vec::new();
        let mut known_manifold = None;
        let refusal = match preliminary(self, p) {
            Err(e) => Some(e),
            Ok(errors) => {
                if !errors.is_empty() {
                    let (removed, _) = self.fix_zero_length_edges_into_validated(&errors);
                    fixes.extend(removed);
                }
                let records = incidences(self);
                let mut edges = manifold_from_incidences(&records).err().unwrap_or_default();
                if p.separate_shells && overloaded(&edges) {
                    edges = separate(self, &mut fixes, records);
                }
                let ns = if p.face_normals {
                    normals(self).err().unwrap_or_default()
                } else {
                    Vec::new()
                };
                let mut witness = super::ValidatedFacesMut { mesh: self };
                match witness.fix_faces_orientation(
                    &edges,
                    &ns,
                    FixFacesOptions {
                        flipped: p.flipped_faces,
                        normals: p.face_normals,
                    },
                ) {
                    Ok(f) => {
                        if f.is_empty() {
                            known_manifold = Some(edges.is_empty());
                        }
                        fixes.extend(f);
                        None
                    }
                    Err(e) => {
                        known_manifold = Some(edges.is_empty());
                        Some(RepairRefusal::OrientationFixFailed(e))
                    }
                }
            }
        };
        let edge_manifold_after = known_manifold
            .unwrap_or_else(|| self.validate_faces().is_ok() && manifold(self).is_ok());
        MeshRepairReport {
            fixes,
            refusal,
            edge_manifold_after,
        }
    }
}
fn separate(
    mesh: &mut IndexedMesh,
    fixes: &mut Vec<MeshFix>,
    mut edges: Vec<Incidence>,
) -> Vec<MeshEdgeManifoldError> {
    // Orientation parity is invariant under cyclic corner rotations.
    let mut groups: Vec<_> = mesh
        .faces
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let v = f.vertices;
            let parity = (v[0] > v[1]) ^ (v[0] > v[2]) ^ (v[1] > v[2]);
            let mut key = v;
            key.sort_unstable();
            (key, parity, i)
        })
        .collect();
    groups.sort_unstable();
    let mut removed = vec![false; mesh.faces.len()];
    let mut start = 0;
    while start < groups.len() {
        let mut end = start + 1;
        while end < groups.len() && groups[end].0 == groups[start].0 {
            end += 1;
        }
        let split = groups[start..end].partition_point(|e| !e.1) + start;
        let n = (split - start).min(end - split);
        for k in 0..n {
            removed[groups[start + k].2] = true;
            removed[groups[split + k].2] = true;
        }
        start = end;
    }
    for (i, &remove) in removed.iter().enumerate() {
        if remove {
            fixes.push(MeshFix::RemovedDuplicateFace { face_idx: i.into() });
        }
    }
    let mut i = 0;
    mesh.faces.retain(|_| {
        let keep = !removed[i];
        i += 1;
        keep
    });
    if removed.iter().any(|&removed| removed) {
        edges = incidences(mesh);
    }
    let has_overload = edges.windows(3).any(|w| w[0].edge == w[2].edge);
    if !has_overload {
        return manifold_from_incidences(&edges).err().unwrap_or_default();
    }
    let mut sets = Disjoint::new(mesh.faces.len() * 3);
    let mut start = 0;
    while start < edges.len() {
        let mut end = start + 1;
        while end < edges.len() && edges[end].edge == edges[start].edge {
            end += 1;
        }
        let group = &mut edges[start..end];
        if group.len() == 2 {
            join_pair(mesh, &mut sets, group[0], group[1]);
        } else if group.len() > 2 {
            let (lo, hi) = group[0].edge;
            let origin = mesh.vertices[usize::from(lo)].to_f64();
            let axis = mesh.vertices[usize::from(hi)].to_f64().diff(&origin);
            let length = axis.norm();
            let unit = Vector3D::new([axis[0] / length, axis[1] / length, axis[2] / length]);
            let mut least = 0;
            for j in 1..3 {
                if unit[j].abs() < unit[least].abs() {
                    least = j;
                }
            }
            let basis = Vector3D::new([
                if least == 0 { 1. } else { 0. },
                if least == 1 { 1. } else { 0. },
                if least == 2 { 1. } else { 0. },
            ]);
            let zero = unit.cross_product(&basis);
            let quarter = unit.cross_product(&zero);
            let angle = |e: &Incidence| {
                let radial = mesh.vertices[usize::from(e.opposite)]
                    .to_f64()
                    .diff(&origin);
                radial
                    .dot_product(&quarter)
                    .atan2(radial.dot_product(&zero))
            };
            group.sort_by(|a, b| angle(a).total_cmp(&angle(b)).then(a.face.cmp(&b.face)));
            // Rotate the circular parenthesis stream after its minimum prefix.
            let mut depth = 0isize;
            let mut minimum = 0;
            let mut offset = 0;
            for (i, e) in group.iter().enumerate() {
                depth += if e.forward { -1 } else { 1 };
                if depth < minimum {
                    minimum = depth;
                    offset = i + 1;
                }
            }
            // Distinct angular wedges nest. Coincident starts are an ordered
            // bundle: consume their copies in face order, so a duplicated
            // surface does not become a connected double cover at each vertex.
            let mut stack: Vec<(f64, std::collections::VecDeque<Incidence>)> = Vec::new();
            for k in 0..group.len() {
                let e = group[(offset + k) % group.len()];
                if !e.forward {
                    let theta = angle(&e);
                    if let Some((last, bundle)) = stack.last_mut()
                        && *last == theta
                    {
                        bundle.push_back(e);
                        continue;
                    }
                    stack.push((theta, std::collections::VecDeque::from([e])));
                } else if let Some((_, bundle)) = stack.last_mut() {
                    if let Some(begin) = bundle.pop_front() {
                        join_pair(mesh, &mut sets, begin, e);
                    }
                    if bundle.is_empty() {
                        stack.pop();
                    }
                }
            }
        }
        start = end;
    }
    // Each disjoint set belongs to one original vertex. Visit corners in
    // face order and link newly discovered fans per vertex in that order.
    // This is the same (vertex, first corner) ordering without two global sorts.
    let count = mesh.faces.len() * 3;
    let mut heads = vec![usize::MAX; mesh.vertices.len()];
    let mut tails = vec![usize::MAX; mesh.vertices.len()];
    let mut next = vec![usize::MAX; count];
    let mut seen = vec![false; count];
    for c in 0..count {
        let root = sets.root(c);
        if seen[root] {
            continue;
        }
        seen[root] = true;
        let v = usize::from(mesh.faces[c / 3].vertices[c % 3]);
        if heads[v] == usize::MAX {
            heads[v] = root;
        } else {
            next[tails[v]] = root;
        }
        tails[v] = root;
    }
    let mut ids = vec![VertexIndex::from(0); count];
    for (v, &head) in heads.iter().enumerate() {
        let mut root = head;
        while root != usize::MAX {
            ids[root] = if root == head {
                VertexIndex::from(v as u32)
            } else {
                let id = VertexIndex::from(mesh.vertices.len() as u32);
                mesh.vertices.push(mesh.vertices[v]);
                fixes.push(MeshFix::DetachedSharedVertex {
                    vertex: VertexIndex::from(v as u32),
                });
                id
            };
            root = next[root];
        }
    }
    // Splitting fans cannot merge distinct original edges. Recheck each old
    // edge group locally under its new IDs, avoiding another global rebuild.
    let mut errors = Vec::new();
    let mut start = 0;
    while start < edges.len() {
        let mut end = start + 1;
        while end < edges.len() && edges[end].edge == edges[start].edge {
            end += 1;
        }
        let group = &mut edges[start..end];
        // Ordinary paired edges were joined at both endpoints. They remain
        // one consistently wound pair regardless of the assigned fan IDs.
        if group.len() == 2 && group[0].forward != group[1].forward {
            start = end;
            continue;
        }
        for e in group.iter_mut() {
            let a = ids[sets.root(corner(mesh, e.face, e.edge.0))];
            let b = ids[sets.root(corner(mesh, e.face, e.edge.1))];
            e.edge = edge_key(a, b);
            e.forward = if e.forward { a < b } else { b < a };
        }
        group.sort_unstable_by_key(|e| (e.edge, e.face));
        if let Err(es) = manifold_from_incidences(group) {
            errors.extend(es);
        }
        start = end;
    }
    errors.sort_unstable_by_key(|e| match e {
        MeshEdgeManifoldError::BoundaryEdge { edge, .. }
        | MeshEdgeManifoldError::NonManifoldEdge { edge, .. }
        | MeshEdgeManifoldError::InconsistentWindingOrder { edge, .. } => *edge,
        MeshEdgeManifoldError::EmptyMesh => unreachable!(),
    });
    for c in 0..count {
        mesh.faces[c / 3].vertices[c % 3] = ids[sets.root(c)];
    }
    errors
}
fn join_pair(mesh: &IndexedMesh, sets: &mut Disjoint, a: Incidence, b: Incidence) {
    for v in [a.edge.0, a.edge.1] {
        sets.join(corner(mesh, a.face, v), corner(mesh, b.face, v));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IndexedTriangle, Normal, Vertex};
    #[test]
    fn shell_local_edge_verdict_matches_full_rebuild() {
        let mut seed = 7u32;
        for vertices in [4, 9, 23] {
            for _ in 0..20 {
                let mut mesh = IndexedMesh::new(
                    None,
                    (0..vertices)
                        .map(|i| {
                            Vertex::new([i as f32, (i * i % 17) as f32, (i * i * i % 31) as f32])
                        })
                        .collect(),
                    (0..100)
                        .map(|_| {
                            let mut ids = [0; 3];
                            for k in 0..3 {
                                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                                let mut id = seed % vertices;
                                while ids[..k].contains(&id) {
                                    id = (id + 1) % vertices;
                                }
                                ids[k] = id;
                            }
                            IndexedTriangle::from_raw_indices(Normal::new([0., 0., 1.]), ids)
                        })
                        .collect(),
                );
                let records = incidences(&mesh);
                let local = separate(&mut mesh, &mut Vec::new(), records);
                assert_eq!(local, manifold(&mesh).err().unwrap_or_default());
            }
        }
    }
    #[test]
    fn same_winding_shell_split_reuses_records() {
        let mut mesh = IndexedMesh::new(
            None,
            vec![
                Vertex::new([0., 0., 0.]),
                Vertex::new([1., 0., 0.]),
                Vertex::new([0., 1., 0.]),
                Vertex::new([0., 0., 1.]),
            ],
            [[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]]
                .repeat(2)
                .into_iter()
                .map(|f| IndexedTriangle::from_raw_indices(Normal::new([0., 0., 1.]), f))
                .collect(),
        );
        let records = incidences(&mesh);
        super::super::validated::INCIDENCE_BUILDS.with(|n| n.set(0));
        assert!(separate(&mut mesh, &mut Vec::new(), records).is_empty());
        super::super::validated::INCIDENCE_BUILDS.with(|n| assert_eq!(n.get(), 0));
        assert!(manifold(&mesh).is_ok());
    }
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::{
    IndexedMesh,
    topology::{EdgeKey, edge_key},
    validate::verdict,
};
use crate::{
    IndexedTriangle, Vertex, geometry::compute_normal_from_triangle_vertices, primitives::FaceIndex,
};
use std::fmt;
#[derive(Clone, Debug, PartialEq)]
pub enum MeshNormalVectorError {
    ZeroAreaTriangle { face_idx: FaceIndex },
    NonUnitNorm { face_idx: FaceIndex, norm: f32 },
    ExpectationMismatch { face_idx: FaceIndex },
}
impl fmt::Display for MeshNormalVectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroAreaTriangle { face_idx } => {
                write!(f, "face #{face_idx} is degenerate: zero area")
            }
            Self::NonUnitNorm { face_idx, norm } => write!(
                f,
                "face #{face_idx} has a non-unit normal vector (norm = {norm})"
            ),
            Self::ExpectationMismatch { face_idx } => write!(
                f,
                "face #{face_idx} declares a normal vector that disagrees with the triangle plane"
            ),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeshEdgeManifoldError {
    EmptyMesh,
    BoundaryEdge {
        edge: EdgeKey,
        face_idx: FaceIndex,
    },
    NonManifoldEdge {
        edge: EdgeKey,
        face_indices: Vec<FaceIndex>,
    },
    InconsistentWindingOrder {
        edge: EdgeKey,
        face_indices: [FaceIndex; 2],
    },
}
impl fmt::Display for MeshEdgeManifoldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyMesh => f.write_str("mesh has no faces"),
            Self::BoundaryEdge {
                edge: (a, b),
                face_idx,
            } => write!(
                f,
                "edge (#{a}, #{b}) is shared by only face #{face_idx} (a manifold edge must be shared by exactly 2 faces)"
            ),
            Self::NonManifoldEdge {
                edge: (a, b),
                face_indices,
            } => write!(
                f,
                "edge (#{a}, #{b}) is shared by {} faces (a manifold edge must be shared by exactly 2)",
                face_indices.len()
            ),
            Self::InconsistentWindingOrder {
                edge: (a, b),
                face_indices: [first, second],
            } => write!(
                f,
                "edge (#{a}, #{b}) is traversed in the same direction by faces #{first} and #{second} (windings disagree)"
            ),
        }
    }
}
#[derive(Debug)]
pub struct ValidatedFaces<'a> {
    pub(super) mesh: &'a IndexedMesh,
}
#[derive(Debug)]
pub struct ValidatedFacesMut<'a> {
    pub(super) mesh: &'a mut IndexedMesh,
}
#[derive(Debug)]
pub struct ValidatedEdgeManifoldMesh<'a> {
    mesh: &'a IndexedMesh,
}
impl<'a> ValidatedFaces<'a> {
    pub fn validated_edge_manifold(
        self,
    ) -> Result<ValidatedEdgeManifoldMesh<'a>, Vec<MeshEdgeManifoldError>> {
        self.validate_edge_manifold()?;
        Ok(ValidatedEdgeManifoldMesh { mesh: self.mesh })
    }
}
macro_rules! accessors {
    ($name:ident) => {
        impl $name<'_> {
            pub fn vertices(&self) -> &[Vertex] {
                &self.mesh.vertices
            }
            pub fn faces(&self) -> &[IndexedTriangle] {
                &self.mesh.faces
            }
        }
    };
}
accessors!(ValidatedFaces);
accessors!(ValidatedFacesMut);
accessors!(ValidatedEdgeManifoldMesh);
macro_rules! checks {
    ($name:ident) => {
        impl $name<'_> {
            pub fn validate_normal_vectors(&self) -> Result<(), Vec<MeshNormalVectorError>> {
                normals(self.mesh)
            }
            pub fn validate_edge_manifold(&self) -> Result<(), Vec<MeshEdgeManifoldError>> {
                manifold(self.mesh)
            }
        }
    };
}
checks!(ValidatedFaces);
checks!(ValidatedFacesMut);
impl ValidatedEdgeManifoldMesh<'_> {
    pub fn as_indexed_mesh(&self) -> &IndexedMesh {
        self.mesh
    }
}
pub(super) fn normals(mesh: &IndexedMesh) -> Result<(), Vec<MeshNormalVectorError>> {
    let mut errors = Vec::new();
    for (i, f) in mesh.faces.iter().enumerate() {
        let face_idx = i.into();
        let v = f.vertices.map(|v| mesh.vertices[usize::from(v)]);
        match compute_normal_from_triangle_vertices(&v) {
            Err(_) => errors.push(MeshNormalVectorError::ZeroAreaTriangle { face_idx }),
            Ok(expected) => {
                let n = f.normal.to_f64();
                let sq = n.squared_norm();
                if !sq.is_finite() || (sq - 1.).abs() > 1e-6 {
                    errors.push(MeshNormalVectorError::NonUnitNorm {
                        face_idx,
                        norm: sq.sqrt() as f32,
                    });
                } else if expected.to_f64().dot_product(&n) < 0.9961946980917455 {
                    errors.push(MeshNormalVectorError::ExpectationMismatch { face_idx });
                }
            }
        }
    }
    verdict(errors)
}
// One flat incidence record per directed triangle edge, sorted by edge then face.
#[derive(Clone, Copy, Debug)]
pub(super) struct Incidence {
    pub edge: EdgeKey,
    pub face: usize,
    pub forward: bool,
    pub opposite: crate::primitives::VertexIndex,
}
#[cfg(test)]
thread_local! { pub(crate) static INCIDENCE_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
pub(super) fn incidences(mesh: &IndexedMesh) -> Vec<Incidence> {
    #[cfg(test)]
    INCIDENCE_BUILDS.with(|n| n.set(n.get() + 1));
    let mut edges = Vec::with_capacity(mesh.faces.len() * 3);
    for (face, f) in mesh.faces.iter().enumerate() {
        for k in 0..3 {
            let a = f.vertices[k];
            let b = f.vertices[(k + 1) % 3];
            edges.push(Incidence {
                edge: edge_key(a, b),
                face,
                forward: a < b,
                opposite: f.vertices[(k + 2) % 3],
            });
        }
    }
    // Bucket dense imported vertex IDs before sorting their small edge fans;
    // the whole mesh no longer pays for a global comparison sort.
    if edges.is_empty() {
        return edges;
    }
    if mesh.vertices.len() > edges.len().saturating_mul(4) {
        edges.sort_unstable_by_key(|e| (e.edge, e.face));
        return edges;
    }
    let mut offsets = vec![0usize; mesh.vertices.len() + 1];
    for e in &edges {
        offsets[usize::from(e.edge.0) + 1] += 1;
    }
    for i in 1..offsets.len() {
        offsets[i] += offsets[i - 1];
    }
    let mut positions = offsets[..mesh.vertices.len()].to_vec();
    let mut ordered = vec![edges[0]; edges.len()];
    for e in edges {
        let pos = &mut positions[usize::from(e.edge.0)];
        ordered[*pos] = e;
        *pos += 1;
    }
    // Only edges incident to the same lower endpoint need comparison sorting.
    // Their buckets are small even on large regular tessellations.
    for range in offsets.windows(2) {
        ordered[range[0]..range[1]].sort_unstable_by_key(|e| (e.edge.1, e.face));
    }
    ordered
}
pub(super) fn manifold(mesh: &IndexedMesh) -> Result<(), Vec<MeshEdgeManifoldError>> {
    if mesh.faces.is_empty() {
        return Err(vec![MeshEdgeManifoldError::EmptyMesh]);
    }
    manifold_from_incidences(&incidences(mesh))
}
pub(super) fn manifold_from_incidences(
    edges: &[Incidence],
) -> Result<(), Vec<MeshEdgeManifoldError>> {
    if edges.is_empty() {
        return Err(vec![MeshEdgeManifoldError::EmptyMesh]);
    }
    let mut errors = Vec::new();
    let mut start = 0;
    while start < edges.len() {
        let mut end = start + 1;
        while end < edges.len() && edges[end].edge == edges[start].edge {
            end += 1;
        }
        let group = &edges[start..end];
        let edge = group[0].edge;
        let issue = if group.len() == 1 {
            Some(MeshEdgeManifoldError::BoundaryEdge {
                edge,
                face_idx: group[0].face.into(),
            })
        } else if group.len() > 2 {
            Some(MeshEdgeManifoldError::NonManifoldEdge {
                edge,
                face_indices: group.iter().map(|entry| entry.face.into()).collect(),
            })
        } else if group[0].forward == group[1].forward {
            Some(MeshEdgeManifoldError::InconsistentWindingOrder {
                edge,
                face_indices: [group[0].face.into(), group[1].face.into()],
            })
        } else {
            None
        };
        errors.extend(issue);
        start = end;
    }
    verdict(errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counting_order_matches_edge_then_face_sort() {
        let mut seed = 31u32;
        for vertices in [3, 17, 4000] {
            let mesh = IndexedMesh::new(
                None,
                vec![Vertex::new([0.; 3]); vertices],
                (0..300)
                    .map(|_| {
                        IndexedTriangle::from_raw_indices(crate::Normal::new([0.; 3]), {
                            let mut ids = [0; 3];
                            for k in 0..3 {
                                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                                let mut id = seed % vertices as u32;
                                while ids[..k].contains(&id) {
                                    id = (id + 1) % vertices as u32;
                                }
                                ids[k] = id;
                            }
                            ids
                        })
                    })
                    .collect(),
            );
            let actual = incidences(&mesh);
            let mut expected: Vec<_> = mesh
                .faces
                .iter()
                .enumerate()
                .flat_map(|(face, f)| {
                    (0..3).map(move |k| {
                        let a = f.vertices[k];
                        let b = f.vertices[(k + 1) % 3];
                        (edge_key(a, b), face, a < b, f.vertices[(k + 2) % 3])
                    })
                })
                .collect();
            expected.sort_by_key(|e| (e.0, e.1));
            assert_eq!(
                actual
                    .iter()
                    .map(|e| (e.edge, e.face, e.forward, e.opposite))
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }

    #[test]
    fn incidence_classification_contract() {
        let record = |a: u32, b: u32, face, forward| Incidence {
            edge: (a.into(), b.into()),
            face,
            forward,
            opposite: 0.into(),
        };
        assert_eq!(
            manifold_from_incidences(&[]),
            Err(vec![MeshEdgeManifoldError::EmptyMesh])
        );
        let entries = [
            record(0, 1, 2, true),
            record(0, 2, 3, true),
            record(0, 2, 4, true),
            record(1, 2, 5, true),
            record(1, 2, 6, false),
            record(1, 2, 7, true),
            record(2, 3, 8, true),
            record(2, 3, 9, false),
        ];
        assert_eq!(
            manifold_from_incidences(&entries),
            Err(vec![
                MeshEdgeManifoldError::BoundaryEdge {
                    edge: (0.into(), 1.into()),
                    face_idx: 2.into()
                },
                MeshEdgeManifoldError::InconsistentWindingOrder {
                    edge: (0.into(), 2.into()),
                    face_indices: [3.into(), 4.into()]
                },
                MeshEdgeManifoldError::NonManifoldEdge {
                    edge: (1.into(), 2.into()),
                    face_indices: vec![5.into(), 6.into(), 7.into()]
                },
            ])
        );
    }
}

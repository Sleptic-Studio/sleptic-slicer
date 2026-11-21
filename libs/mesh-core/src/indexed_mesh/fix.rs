// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::{
    IndexedMesh, MeshEdgeManifoldError, MeshFaceError, MeshNormalVectorError,
    ValidatedFacesMut,
};
use crate::{
    Normal,
    geometry::{GeometryError, compute_normal_from_triangle_vertices},
    primitives::{FaceIndex, VertexIndex},
};
/// Which face problems a fix pass corrects: faces wound against their
/// neighbours, and stored normals that disagree with the winding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FixFacesOptions {
    pub flipped: bool,
    pub normals: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MeshFix {
    RemovedZeroLengthEdgeFace {
        face_idx: FaceIndex,
    },
    FlippedFaceOrientation {
        face_idx: FaceIndex,
    },
    RecomputedNormal {
        face_idx: FaceIndex,
        old_normal: Normal,
        new_normal: Normal,
    },
    RemovedDuplicateFace {
        face_idx: FaceIndex,
    },
    DetachedSharedVertex {
        vertex: VertexIndex,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshFixError {
    FaceWithTooManyWindingErrors { face_idx: FaceIndex },
    TooManyFlippedFaces,
    NormalRecompute(GeometryError),
}
impl std::fmt::Display for MeshFixError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FaceWithTooManyWindingErrors { face_idx } => {
                write!(f, "face {face_idx} has more than 3 winding-order errors")
            }
            Self::TooManyFlippedFaces => f.write_str("too many flipped faces"),
            Self::NormalRecompute(e) => write!(f, "normal recompute failed: {e}"),
        }
    }
}
impl std::error::Error for MeshFixError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NormalRecompute(e) => Some(e),
            _ => None,
        }
    }
}
impl From<GeometryError> for MeshFixError {
    fn from(e: GeometryError) -> Self {
        Self::NormalRecompute(e)
    }
}
pub(super) fn orientation_plan(
    mesh: &IndexedMesh,
    edges: &[MeshEdgeManifoldError],
    normals: &[MeshNormalVectorError],
    options: FixFacesOptions,
) -> Result<Vec<(usize, bool, Option<Normal>)>, MeshFixError> {
    let mut complaints = vec![0usize; mesh.faces.len()];
    if options.flipped {
        for e in edges {
            if let MeshEdgeManifoldError::InconsistentWindingOrder { face_indices, .. } = e {
                for f in face_indices {
                    complaints[f.get()] += 1;
                }
            }
        }
        for (i, &n) in complaints.iter().enumerate() {
            if n > 3 {
                return Err(MeshFixError::FaceWithTooManyWindingErrors { face_idx: i.into() });
            }
        }
        if complaints.iter().filter(|&&n| n >= 2).count() >= mesh.faces.len() / 2 {
            return Err(MeshFixError::TooManyFlippedFaces);
        }
    }
    let mut wrong = vec![false; mesh.faces.len()];
    if options.normals {
        for e in normals {
            match e {
                MeshNormalVectorError::NonUnitNorm { face_idx, .. }
                | MeshNormalVectorError::ExpectationMismatch { face_idx } => {
                    wrong[face_idx.get()] = true
                }
                _ => {}
            }
        }
    }
    let mut plan = Vec::new();
    for i in 0..mesh.faces.len() {
        let flip = complaints[i] >= 2;
        if !flip && !wrong[i] {
            continue;
        }
        let normal = if wrong[i] {
            let mut ids = mesh.faces[i].vertices;
            if flip {
                ids.reverse();
            }
            Some(compute_normal_from_triangle_vertices(
                &ids.map(|v| mesh.vertices[usize::from(v)]),
            )?)
        } else {
            None
        };
        plan.push((i, flip, normal));
    }
    Ok(plan)
}
impl ValidatedFacesMut<'_> {
    pub fn fix_faces_orientation(
        &mut self,
        edges: &[MeshEdgeManifoldError],
        normals: &[MeshNormalVectorError],
        options: FixFacesOptions,
    ) -> Result<Vec<MeshFix>, MeshFixError> {
        let plan = orientation_plan(self.mesh, edges, normals, options)?;
        let mut fixes = Vec::new();
        for (i, flip, normal) in plan {
            let f = &mut self.mesh.faces[i];
            let old_normal = f.normal;
            if flip {
                f.vertices.reverse();
                f.normal = f.normal.negate();
                fixes.push(MeshFix::FlippedFaceOrientation { face_idx: i.into() });
            }
            if let Some(new_normal) = normal {
                f.normal = new_normal;
                fixes.push(MeshFix::RecomputedNormal {
                    face_idx: i.into(),
                    old_normal,
                    new_normal,
                });
            }
        }
        Ok(fixes)
    }
}
impl IndexedMesh {
    pub fn fix_zero_length_edges_into_validated(
        &mut self,
        errors: &[MeshFaceError],
    ) -> (
        Vec<MeshFix>,
        Result<ValidatedFacesMut<'_>, Vec<MeshFaceError>>,
    ) {
        let mut removed = vec![false; self.faces.len()];
        let mut fixes = Vec::new();
        let mut only = true;
        for e in errors {
            if let MeshFaceError::ZeroLengthEdge { face_idx, .. } = e {
                removed[face_idx.get()] = true;
                fixes.push(MeshFix::RemovedZeroLengthEdgeFace {
                    face_idx: *face_idx,
                });
            } else {
                only = false;
            }
        }
        let mut i = 0;
        self.faces.retain(|_| {
            let keep = !removed[i];
            i += 1;
            keep
        });
        let witness = if only {
            Ok(ValidatedFacesMut { mesh: self })
        } else {
            self.validated_faces_mut()
        };
        (fixes, witness)
    }
}

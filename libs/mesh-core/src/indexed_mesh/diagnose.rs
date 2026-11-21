// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::{
    IndexedMesh, MeshEdgeManifoldError, MeshFaceError, MeshNormalVectorError, MeshVertexError,
};
#[derive(Clone, Debug, PartialEq)]
pub struct MeshDiagnosis {
    pub vertex_errors: Vec<MeshVertexError>,
    pub faces: FaceDiagnosis,
}
#[derive(Clone, Debug, PartialEq)]
pub enum FaceDiagnosis {
    Invalid(Vec<MeshFaceError>),
    Valid {
        normal_vector_errors: Vec<MeshNormalVectorError>,
        edge_manifold_errors: Vec<MeshEdgeManifoldError>,
    },
}
impl FaceDiagnosis {
    fn from_mesh(mesh: &IndexedMesh) -> Self {
        let checked = match mesh.validated_faces() {
            Ok(checked) => checked,
            Err(errors) => return Self::Invalid(errors),
        };
        let normal_vector_errors = checked.validate_normal_vectors().err().unwrap_or_default();
        let edge_manifold_errors = checked.validate_edge_manifold().err().unwrap_or_default();
        Self::Valid {
            normal_vector_errors,
            edge_manifold_errors,
        }
    }

    fn is_clean(&self) -> bool {
        if let Self::Valid {
            normal_vector_errors,
            edge_manifold_errors,
        } = self
        {
            normal_vector_errors.is_empty() && edge_manifold_errors.is_empty()
        } else {
            false
        }
    }
}
impl MeshDiagnosis {
    pub fn has_problems(&self) -> bool {
        !(self.vertex_errors.is_empty() && self.faces.is_clean())
    }
}
impl IndexedMesh {
    pub fn diagnose(&self) -> MeshDiagnosis {
        let faces = FaceDiagnosis::from_mesh(self);
        let vertex_errors = self.validate_vertices().err().unwrap_or_default();
        MeshDiagnosis {
            vertex_errors,
            faces,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_faces_are_a_problem_even_without_reported_errors() {
        let diagnosis = MeshDiagnosis {
            vertex_errors: Vec::new(),
            faces: FaceDiagnosis::Invalid(Vec::new()),
        };
        assert!(diagnosis.has_problems());
    }

    #[test]
    fn each_face_check_can_independently_report_a_problem() {
        for (normals, edges) in [
            (vec![MeshNormalVectorError::ZeroAreaTriangle { face_idx: crate::primitives::FaceIndex::from(0) }], vec![]),
            (vec![], vec![MeshEdgeManifoldError::EmptyMesh]),
            (vec![], vec![]),
        ] {
            let expected = !normals.is_empty() || !edges.is_empty();
            let report = MeshDiagnosis {
                vertex_errors: vec![],
                faces: FaceDiagnosis::Valid {
                    normal_vector_errors: normals,
                    edge_manifold_errors: edges,
                },
            };
            assert_eq!(report.has_problems(), expected);
        }
    }
}

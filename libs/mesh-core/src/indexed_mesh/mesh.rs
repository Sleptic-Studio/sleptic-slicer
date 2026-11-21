// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The mesh itself. Its fields are public: a mesh is plain data until one
//! of the `validated_*` views has checked it.
use super::{MeshFaceError, ValidatedFaces, ValidatedFacesMut};
use crate::{Aabb, IndexedTriangle, Vertex};

/// What a file said about a mesh besides its geometry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MeshMetadata {
    /// The name the file gave it, if any.
    pub name: Option<String>,
    /// A free-text comment, such as a binary STL's header.
    pub header_comment: Option<String>,
}

/// Faces over a shared vertex list. Faces may point out of range or repeat
/// a corner until checked; see [`IndexedMesh::validated_faces`].
#[derive(Clone, Debug, PartialEq)]
pub struct IndexedMesh {
    pub vertices: Vec<Vertex>,
    pub faces: Vec<IndexedTriangle>,
    pub meta: Option<MeshMetadata>,
}

impl IndexedMesh {
    pub fn new(
        meta: Option<MeshMetadata>,
        vertices: Vec<Vertex>,
        faces: Vec<IndexedTriangle>,
    ) -> IndexedMesh {
        IndexedMesh {
            vertices,
            faces,
            meta,
        }
    }

    pub fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }

    pub fn faces(&self) -> &[IndexedTriangle] {
        &self.faces
    }

    /// The box around every vertex (used or not), in the mesh's own
    /// coordinates; none for a mesh without vertices.
    pub fn local_aabb(&self) -> Option<Aabb> {
        let (first, rest) = self.vertices.split_first()?;
        let mut bounds = Aabb::empty();
        bounds.extend(*first);
        rest.iter().for_each(|&corner| bounds.extend(corner));
        Some(bounds)
    }

    /// A read-only view that guarantees every face is well formed, or the
    /// faces that are not.
    pub fn validated_faces(&self) -> Result<ValidatedFaces<'_>, Vec<MeshFaceError>> {
        self.validate_faces()
            .map(|()| ValidatedFaces { mesh: self })
    }

    /// As [`IndexedMesh::validated_faces`], for edits that keep the faces
    /// well formed.
    pub fn validated_faces_mut(&mut self) -> Result<ValidatedFacesMut<'_>, Vec<MeshFaceError>> {
        self.validate_faces()?;
        Ok(ValidatedFacesMut { mesh: self })
    }
}

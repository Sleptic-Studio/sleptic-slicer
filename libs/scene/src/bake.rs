// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Geometry placement without changing stored meshes or metadata.
use crate::*;
use rayon::prelude::*;
use sc_mesh_core::{IndexedMesh, IndexedTriangle, transform::Transform3D};
pub fn bake_mesh_to_world(mesh: &IndexedMesh, t: &Transform3D) -> Result<IndexedMesh, SceneError> {
    if t.is_identity() {
        return Ok(IndexedMesh::new(
            None,
            mesh.vertices.clone(),
            mesh.faces.clone(),
        ));
    }
    let mirrored = t.determinant() < 0.;
    let vertices = if mesh.vertices.len() >= 16_384 {
        mesh.vertices
            .par_iter()
            .map(|v| t.apply_to_vertex(*v))
            .collect()
    } else {
        mesh.vertices
            .iter()
            .map(|v| t.apply_to_vertex(*v))
            .collect()
    };
    let mut faces = mesh.faces.clone();
    let place = |chunk: &mut [IndexedTriangle]| -> Result<(), SceneError> {
        for face in chunk {
            if face.normal.coords() != [0.; 3] {
                face.normal = t.apply_to_normal(face.normal)?;
            }
            if mirrored {
                face.vertices.swap(1, 2);
            }
        }
        Ok(())
    };
    if faces.len() >= 16_384 {
        // Indexed chunk results retain face-order error selection without allocating
        // one Result per triangle on million-face meshes.
        let results: Vec<_> = faces.par_chunks_mut(4096).map(place).collect();
        for result in results {
            result?;
        }
    } else {
        place(&mut faces)?;
    }
    Ok(IndexedMesh::new(None, vertices, faces))
}
pub fn bake_leaf_to_world(scene: &Scene, id: NodeId) -> Result<IndexedMesh, SceneError> {
    bake_mesh_to_world(scene.mesh_at_leaf(id)?, &scene.world_transform_of(id)?)
}
impl Scene {
    pub fn merge_meshes(&self) -> Result<Option<IndexedMesh>, SceneError> {
        let mut vertices = vec![];
        let mut faces = vec![];
        self.for_each_expanded_leaf(CsgPolicy::Refuse, |_, t, m| {
            let baked = bake_mesh_to_world(m, t)?;
            let offset = vertices.len() as u32;
            vertices.extend(baked.vertices);
            faces.extend(baked.faces.into_iter().map(|mut f| {
                f.vertices = f.vertices.map(|v| v.offset(offset));
                f
            }));
            Ok::<_, SceneError>(())
        })?;
        Ok((!faces.is_empty()).then(|| IndexedMesh::new(None, vertices, faces)))
    }
}

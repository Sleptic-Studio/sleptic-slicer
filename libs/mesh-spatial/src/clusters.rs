// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_mesh_core::{Aabb, IndexedMesh, IndexedTriangle, Vertex};
use std::ops::Range;
/// Bounds of consecutive face ranges; indices always address the original face array.
#[derive(Clone, Debug, PartialEq)]
pub struct TriangleClusters {
    bounds: Vec<Aabb>,
    faces: u32,
}
impl TriangleClusters {
    pub const CLUSTER_SIZE: u32 = 64;
    pub fn build(vertices: &[Vertex], faces: &[IndexedTriangle]) -> Option<Self> {
        let bounds: Option<Vec<_>> = faces
            .chunks(Self::CLUSTER_SIZE as usize)
            .map(|chunk| {
                chunk.iter().flat_map(|face| face.vertices).try_fold(
                    Aabb::empty(),
                    |mut bounds, index| {
                        bounds.extend(*vertices.get(index.get() as usize)?);
                        Some(bounds)
                    },
                )
            })
            .collect();
        let bounds = bounds?;
        (!bounds.is_empty()).then_some(Self {
            bounds,
            faces: faces.len() as u32,
        })
    }
    pub fn for_mesh(mesh: &IndexedMesh) -> Option<Self> {
        Self::build(mesh.vertices(), mesh.faces())
    }
    pub fn iter(&self) -> impl Iterator<Item = (Range<u32>, Aabb)> + '_ {
        self.bounds
            .iter()
            .enumerate()
            .map(|(i, b)| ((i as u32 * 64)..((i as u32 + 1) * 64).min(self.faces), *b))
    }
    pub fn len(&self) -> usize {
        self.bounds.len()
    }
    pub fn is_empty(&self) -> bool {
        self.bounds.is_empty()
    }
    pub fn face_count(&self) -> u32 {
        self.faces
    }
    pub fn aabb(&self, index: usize) -> Aabb {
        self.bounds[index]
    }
}

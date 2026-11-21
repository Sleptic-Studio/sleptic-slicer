// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{
    IndexedMesh, Triangle,
    indexed_mesh::MeshMetadata,
    primitives::{IndexedTriangle, VertexIndex},
};
use std::{
    collections::{HashMap, hash_map::Entry},
    hash::{BuildHasherDefault, Hasher},
};
// Array hashing writes the coordinate bytes in one batch. Mix those three
// words directly; the fixed key length needs no separate length hash.
#[derive(Default)]
struct CoordinateHasher(u64);
impl Hasher for CoordinateHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write_usize(&mut self, _: usize) {}
    fn write(&mut self, bytes: &[u8]) {
        let mut h = self.0;
        for word in bytes.as_chunks::<4>().0 {
            h = (h ^ u64::from(u32::from_ne_bytes(*word)))
                .rotate_left(27)
                .wrapping_mul(0x9e3779b185ebca87);
        }
        for &byte in bytes.as_chunks::<4>().1 {
            h = (h ^ u64::from(byte)).wrapping_mul(0x9e3779b185ebca87);
        }
        h ^= h >> 30;
        h = h.wrapping_mul(0xbf58476d1ce4e5b9);
        h ^= h >> 27;
        h = h.wrapping_mul(0x94d049bb133111eb);
        self.0 = h ^ (h >> 31);
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexedMeshBuildError {
    TooManyVertices { limit: usize },
}
impl std::fmt::Display for IndexedMeshBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyVertices { limit } => write!(
                f,
                "indexed-mesh build: unique vertex count exceeds the {limit} index limit"
            ),
        }
    }
}
impl std::error::Error for IndexedMeshBuildError {}
pub fn indexed_mesh_from_soup(
    meta: Option<MeshMetadata>,
    triangles: Vec<Triangle>,
    limit: usize,
) -> Result<IndexedMesh, IndexedMeshBuildError> {
    let capacity = (triangles.len() / 2).min(limit);
    let mut lookup = HashMap::with_capacity_and_hasher(
        capacity,
        BuildHasherDefault::<CoordinateHasher>::default(),
    );
    let mut vertices = Vec::with_capacity(capacity);
    let mut faces = Vec::with_capacity(triangles.len());
    for triangle in triangles {
        let mut indices = [VertexIndex::from(0); 3];
        for (i, v) in triangle.vertices().into_iter().enumerate() {
            let key = v.coords().map(|x| if x == 0. { 0 } else { x.to_bits() });
            indices[i] = match lookup.entry(key) {
                Entry::Occupied(entry) => *entry.get(),
                Entry::Vacant(entry) => {
                    if vertices.len() >= limit {
                        return Err(IndexedMeshBuildError::TooManyVertices { limit });
                    }
                    let index = VertexIndex::from(vertices.len() as u32);
                    vertices.push(v);
                    entry.insert(index);
                    index
                }
            };
        }
        faces.push(IndexedTriangle {
            normal: triangle.normal(),
            vertices: indices,
        });
    }
    Ok(IndexedMesh::new(meta, vertices, faces))
}
pub trait TriangleIterator:
    Iterator<Item = Result<Triangle, <Self as TriangleIterator>::Error>>
{
    type Error: From<IndexedMeshBuildError>;
    fn as_indexed_mesh(
        &mut self,
        meta: Option<MeshMetadata>,
    ) -> Result<IndexedMesh, <Self as TriangleIterator>::Error> {
        let triangles = self.collect::<Result<Vec<_>, _>>()?;
        Ok(indexed_mesh_from_soup(meta, triangles, u32::MAX as usize)?)
    }
}

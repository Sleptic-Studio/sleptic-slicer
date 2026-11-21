// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
pub mod aabb;
pub mod geometry;
pub mod indexed_mesh;
pub mod primitives;
pub mod simplify;
pub mod transform;
pub mod triangle_iterator;
pub use aabb::Aabb;
pub use indexed_mesh::{IndexedMesh, MeshMetadata, ValidatedEdgeManifoldMesh};
pub use primitives::{IndexedTriangle, Normal, Triangle, Vector3D, Vertex};
pub use triangle_iterator::{IndexedMeshBuildError, TriangleIterator, indexed_mesh_from_soup};
#[cfg(test)]
mod tests_contract;

#[cfg(test)]
mod tests_perf;

#[cfg(test)]
mod tests_performance_contract;

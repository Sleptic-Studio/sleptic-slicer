// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The two checks every mesh passes before anything else uses it: each
//! coordinate is a real number, and each face names three different
//! vertices that exist.
use super::IndexedMesh;
use crate::primitives::{CoordAxis, FaceIndex, TriVertexSlot, VertexIndex};
use std::fmt;

/// A vertex coordinate that is not a finite number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeshVertexError {
    InfiniteCoordinate { vertex_idx: usize, coord: CoordAxis },
    NaNCoordinate { vertex_idx: usize, coord: CoordAxis },
}

impl fmt::Display for MeshVertexError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (vertex, axis, kind) = match self {
            MeshVertexError::InfiniteCoordinate { vertex_idx, coord } => {
                (vertex_idx, coord, "an infinite")
            }
            MeshVertexError::NaNCoordinate { vertex_idx, coord } => (vertex_idx, coord, "a NaN"),
        };
        write!(out, "vertex #{vertex} has {kind} {axis} coordinate")
    }
}

/// A face that cannot be a triangle: it names a vertex that is not there,
/// or the same vertex at two corners.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeshFaceError {
    OutOfRangeVertexIndex {
        face_idx: FaceIndex,
        vertex_pos: TriVertexSlot,
        vertex_idx: VertexIndex,
    },
    ZeroLengthEdge {
        face_idx: FaceIndex,
        vertex_pos_a: TriVertexSlot,
        vertex_pos_b: TriVertexSlot,
        vertex_idx: VertexIndex,
    },
}

impl fmt::Display for MeshFaceError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MeshFaceError::OutOfRangeVertexIndex {
                face_idx: face,
                vertex_pos: slot,
                vertex_idx: vertex,
            } => {
                write!(
                    out,
                    "face #{face} slot {slot} references vertex #{vertex}, which is out of range"
                )
            }
            MeshFaceError::ZeroLengthEdge {
                face_idx: face,
                vertex_pos_a: a,
                vertex_pos_b: b,
                vertex_idx: vertex,
            } => {
                write!(
                    out,
                    "face #{face} has a zero-length edge: slots {a} and {b} both bound to vertex #{vertex}"
                )
            }
        }
    }
}

/// A face's three edges as pairs of corner slots, in winding order.
fn edge_slots() -> [(TriVertexSlot, TriVertexSlot); 3] {
    let [first, second, third] = TriVertexSlot::ALL;
    [(first, second), (second, third), (third, first)]
}

impl IndexedMesh {
    /// Every coordinate that is NaN or infinite, vertex by vertex and axis
    /// by axis.
    pub fn validate_vertices(&self) -> Result<(), Vec<MeshVertexError>> {
        let problems = self
            .vertices
            .iter()
            .enumerate()
            .flat_map(|(vertex_idx, vertex)| {
                CoordAxis::ALL.into_iter().filter_map(move |coord| {
                    let value = vertex[coord.as_index()];
                    match (value.is_nan(), value.is_infinite()) {
                        (true, _) => Some(MeshVertexError::NaNCoordinate { vertex_idx, coord }),
                        (_, true) => {
                            Some(MeshVertexError::InfiniteCoordinate { vertex_idx, coord })
                        }
                        _ => None,
                    }
                })
            });
        verdict(problems.collect())
    }

    /// Every corner that points past the vertex list, then every edge whose
    /// two ends are the same vertex, face by face.
    pub fn validate_faces(&self) -> Result<(), Vec<MeshFaceError>> {
        let vertex_count = self.vertices.len();
        let mut problems = Vec::new();
        for (position, face) in self.faces.iter().enumerate() {
            let face_idx = FaceIndex::from(position);
            let at = |slot: TriVertexSlot| face.vertices[slot.as_index()];
            problems.extend(
                TriVertexSlot::ALL
                    .into_iter()
                    .filter(|&slot| usize::from(at(slot)) >= vertex_count)
                    .map(|vertex_pos| MeshFaceError::OutOfRangeVertexIndex {
                        face_idx,
                        vertex_pos,
                        vertex_idx: at(vertex_pos),
                    }),
            );
            problems.extend(
                edge_slots()
                    .into_iter()
                    .filter(|&(a, b)| at(a) == at(b))
                    .map(
                        |(vertex_pos_a, vertex_pos_b)| MeshFaceError::ZeroLengthEdge {
                            face_idx,
                            vertex_pos_a,
                            vertex_pos_b,
                            vertex_idx: at(vertex_pos_a),
                        },
                    ),
            );
        }
        verdict(problems)
    }
}

/// No problems is a pass; any problem fails with all of them.
pub(super) fn verdict<E>(problems: Vec<E>) -> Result<(), Vec<E>> {
    match problems.is_empty() {
        true => Ok(()),
        false => Err(problems),
    }
}

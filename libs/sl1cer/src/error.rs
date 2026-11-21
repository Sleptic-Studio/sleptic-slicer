// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_mesh_core::indexed_mesh::{
    MeshEdgeManifoldError, MeshErrorReport, MeshFaceError, MeshVertexError, group_errors_by_variant,
};
use sc_scene::{NodeId, SceneError};
use std::fmt;
#[derive(Debug, Clone)]
pub enum InvalidLeafGeometry {
    Vertices(Vec<MeshVertexError>),
    Faces(Vec<MeshFaceError>),
    Manifold(Vec<MeshEdgeManifoldError>),
}
#[derive(Debug, Clone)]
pub struct LeafValidationFailure {
    pub leaf: NodeId,
    pub leaf_name: Option<String>,
    pub geometry: InvalidLeafGeometry,
}
#[derive(Debug)]
pub enum SliceError {
    EmptyScene,
    TolerantCoordinateOutOfRange,
    NodeNotFound {
        node_id: NodeId,
    },
    NotALeaf {
        node_id: NodeId,
    },
    CsgNotYetSupported {
        node_id: NodeId,
    },
    SubtreeReferenceNotYetSupported {
        node_id: NodeId,
    },
    VerticesValidationFailed {
        errors: Vec<MeshVertexError>,
    },
    FacesValidationFailed {
        errors: Vec<MeshFaceError>,
    },
    ManifoldCheckFailed {
        errors: Vec<MeshEdgeManifoldError>,
    },
    BakeFailed {
        source: SceneError,
    },
    NonManifoldDuringStitching {
        z_mm: f64,
        dangling_endpoints: usize,
    },
}
fn write_validation<E: MeshErrorReport>(
    f: &mut fmt::Formatter<'_>,
    stage: &str,
    errors: &[E],
) -> fmt::Result {
    write!(
        f,
        "mesh {stage} validation failed: {} error(s)",
        errors.len()
    )?;
    for group in group_errors_by_variant(errors) {
        write!(f, "\n  {}x {}", group.members.len(), group.variant_name)?;
        let mut examples = group.members.iter().filter_map(|error| error.example_face()).take(5);
        if let Some(first) = examples.next() {
            write!(f, " (e.g. faces #{first}")?;
            for face in examples {
                write!(f, ", #{face}")?;
            }
            f.write_str(")")?;
        }
    }
    Ok(())
}
impl fmt::Display for InvalidLeafGeometry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vertices(e) => write_validation(f, "vertex", e),
            Self::Faces(e) => write_validation(f, "face", e),
            Self::Manifold(e) => write_validation(f, "manifold", e),
        }
    }
}
impl fmt::Display for SliceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyScene => f.write_str("scene is empty: no nodes to slice"),
            Self::TolerantCoordinateOutOfRange => {
                f.write_str("coordinates exceed the tolerant fixed-point range")
            }
            Self::NodeNotFound { node_id }
            | Self::NotALeaf { node_id }
            | Self::CsgNotYetSupported { node_id }
            | Self::SubtreeReferenceNotYetSupported { node_id } => {
                let detail = match self {
                    Self::NodeNotFound { .. } => "not found in scene",
                    Self::NotALeaf { .. } => "is a Group, expected a Leaf carrying a mesh",
                    Self::CsgNotYetSupported { .. } => "is a CSG combinator; CSG slicing is not yet supported",
                    Self::SubtreeReferenceNotYetSupported { .. } => "is a subtree reference; slicing scenes with subtree references is not yet supported",
                    _ => unreachable!(),
                };
                write!(f, "node #{} {detail}", node_id.get())
            }
            Self::VerticesValidationFailed { errors } => write_validation(f, "vertex", errors),
            Self::FacesValidationFailed { errors } => write_validation(f, "face", errors),
            Self::ManifoldCheckFailed { errors } => write_validation(f, "manifold", errors),
            Self::BakeFailed { source } => {
                write!(f, "baking the leaf to world space failed: {source}")
            }
            Self::NonManifoldDuringStitching {
                z_mm,
                dangling_endpoints,
            } => write!(
                f,
                "non-manifold geometry detected during stitching at z = {z_mm} mm: {dangling_endpoints} dangling endpoint(s)"
            ),
        }
    }
}
impl std::error::Error for SliceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::BakeFailed { source } => Some(source),
            _ => None,
        }
    }
}
#[derive(Debug)]
pub enum SliceSceneError {
    EmptyScene,
    CsgNotYetSupported {
        node_id: NodeId,
    },
    SubtreeReferenceNotYetSupported {
        node_id: NodeId,
    },
    LeafSlicingFailed {
        leaf: NodeId,
        leaf_name: Option<String>,
        error: SliceError,
    },
    LeavesValidationFailed(Vec<LeafValidationFailure>),
}
fn write_leaf(f: &mut fmt::Formatter<'_>, id: NodeId, name: &Option<String>) -> fmt::Result {
    write!(f, "leaf #{}", id.get())?;
    match name {
        Some(value) => write!(f, " (\"{value}\")"),
        None => Ok(()),
    }
}
impl fmt::Display for SliceSceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyScene => f.write_str("scene has no mesh leaves to slice"),
            Self::CsgNotYetSupported { node_id } => {
                SliceError::CsgNotYetSupported { node_id: *node_id }.fmt(f)
            }
            Self::SubtreeReferenceNotYetSupported { node_id } => {
                SliceError::SubtreeReferenceNotYetSupported { node_id: *node_id }.fmt(f)
            }
            Self::LeafSlicingFailed {
                leaf,
                leaf_name,
                error,
            } => {
                f.write_str("slicing ")?;
                write_leaf(f, *leaf, leaf_name)?;
                write!(f, " failed: {error}")
            }
            Self::LeavesValidationFailed(failures) => {
                write!(
                    f,
                    "mesh validation failed for {} of the scene's leaves:",
                    failures.len()
                )?;
                for failure in failures {
                    f.write_str("\n")?;
                    write_leaf(f, failure.leaf, &failure.leaf_name)?;
                    write!(f, ": {}", failure.geometry)?;
                }
                Ok(())
            }
        }
    }
}
impl std::error::Error for SliceSceneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::LeafSlicingFailed { error, .. } => Some(error),
            _ => None,
        }
    }
}

impl std::error::Error for InvalidLeafGeometry {}

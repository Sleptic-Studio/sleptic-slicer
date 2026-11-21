// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Why a mesh file could not be read or written. Wrapping kinds convert
//! from their cause and name it as their source; the messages are tested
//! word for word, since bug reports quote them.

// Keep wrapper conversions together so each source has one explicit destination.
macro_rules! error_conversions {
    ($( $source:ty => $target:ident :: $variant:ident ),* $(,)?) => {
        $(impl From<$source> for $target {
            fn from(value: $source) -> Self { Self::$variant(value) }
        })*
    };
}

error_conversions! {
    std::io::Error => LoadMeshError::Io,
    InferFileTypeError => LoadMeshError::InferType,
    MeshFileType => LoadMeshError::UnsupportedFormat,
    LoadStlError => LoadMeshError::Stl,
    LoadObjError => LoadMeshError::Obj,
    LoadThreeMfError => LoadMeshError::ThreeMf,
    std::io::Error => WriteSceneError::Io,
    PathBuf => WriteSceneError::PathHasNoFileName,
    SceneError => WriteSceneError::Graph,
    WriteStlError => WriteSceneError::Stl,
    WriteObjError => WriteSceneError::Obj,
    WriteThreeMfError => WriteSceneError::ThreeMf,
    std::io::Error => LoadStlError::Io,
    IndexedMeshBuildError => LoadStlError::IndexedMeshBuild,
    std::io::Error => LoadObjError::Io,
    IndexedMeshBuildError => LoadObjError::IndexedMeshBuild,
    PackageReadError => LoadThreeMfError::Package,
    ModelParseError => LoadThreeMfError::Model,
    std::io::Error => WriteStlError::Io,
    std::io::Error => WriteObjError::Io,
    SceneError => WriteObjError::Graph,
    PackageWriteError => WriteThreeMfError::Package,
    ModelWriteError => WriteThreeMfError::Model,
    std::io::Error => WriteThreeMfError::Io,
    SceneError => WriteThreeMfError::Graph,
}
// List the variants that wrap another error; all remaining variants are terminal.
macro_rules! error_sources {
    ($kind:ty, $cause:ident; $( $pattern:pat_param )|+) => {
        impl std::error::Error for $kind {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                match self {
                    $($pattern => Some($cause),)+
                    _ => None,
                }
            }
        }
    };
}
error_sources!(LoadMeshError, cause; Self::Io(cause) | Self::InferType(cause) | Self::Stl(cause) | Self::Obj(cause) | Self::ThreeMf(cause));
error_sources!(WriteSceneError, cause; Self::Io(cause) | Self::CreateTempFile { source: cause, .. } | Self::Rename { source: cause, .. } | Self::Graph(cause) | Self::Stl(cause) | Self::Obj(cause) | Self::ThreeMf(cause));
error_sources!(LoadStlError, cause; Self::Io(cause) | Self::InvalidFloat { source: cause, .. } | Self::IndexedMeshBuild(cause));
error_sources!(LoadObjError, cause; Self::Io(cause) | Self::InvalidFloat { source: cause, .. } | Self::InvalidFaceIndex { source: cause, .. } | Self::IndexedMeshBuild(cause));
error_sources!(LoadThreeMfError, cause; Self::Package(cause) | Self::Model(cause));
error_sources!(WriteStlError, cause; Self::Io(cause));
error_sources!(WriteObjError, cause; Self::Io(cause) | Self::Graph(cause));
error_sources!(WriteThreeMfError, cause; Self::Package(cause) | Self::Model(cause) | Self::Io(cause) | Self::Graph(cause));
use crate::filetypes::MeshFileType;
use sc_3mf::error::*;
use sc_mesh_core::IndexedMeshBuildError;
use sc_scene::SceneError;
use std::fmt;
use std::path::PathBuf;
// Loading. The top-level error says which step failed; each format's own
// error says where in the file.

/// A path whose extension names no mesh format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InferFileTypeError {
    UnknownExtension { path: PathBuf },
}

impl std::error::Error for InferFileTypeError {}

/// Loading a mesh file of any format.
#[derive(Debug)]
pub enum LoadMeshError {
    /// Choosing the reader.
    InferType(InferFileTypeError),
    UnsupportedFormat(MeshFileType),
    FileTooLarge { limit_bytes: u64, actual_bytes: u64 },
    Io(std::io::Error),
    /// The reader's own error.
    ThreeMf(LoadThreeMfError),
    Obj(LoadObjError),
    Stl(LoadStlError),
}

/// Reading an STL file, binary or text.
#[derive(Debug)]
pub enum LoadStlError {
    /// The file ended early, or claims more triangles than can be read.
    Truncated { what: &'static str },
    DeclaredTriangleCountExceedsCap { cap: usize, declared: usize },
    /// Text files: the keywords and numbers in the wrong place.
    AsciiEmptyInput,
    AsciiHeaderMissing,
    AsciiEofWhileExpecting { what: &'static str },
    AsciiUnexpected { got: Vec<String>, expected: &'static str },
    AsciiInvalidFacetHeader { tokens: Vec<String> },
    AsciiInvalidVertexLine { tokens: Vec<String> },
    /// Numbers that are not usable coordinates.
    InvalidFloat { source: std::num::ParseFloatError, token: String },
    NonFiniteFloat { value: f32, what: &'static str },
    IndexedMeshBuild(IndexedMeshBuildError),
    Io(std::io::Error),
}

/// Reading an OBJ file. Everything but the wrapped errors names its line.
#[derive(Debug)]
pub enum LoadObjError {
    /// Vertices.
    VertexCoordCountInvalid { got: usize, line: usize },
    VertexWZero { line: usize },
    InvalidFloat { source: std::num::ParseFloatError, token: String, line: usize },
    NonFiniteFloat { value: f32, line: usize },
    /// Faces.
    FaceTooFewVertices { got: usize, line: usize },
    FaceIndexZero { line: usize },
    InvalidFaceIndex { source: std::num::ParseIntError, token: String, line: usize },
    FaceIndexOutOfRange { vertex_count: usize, index: i64, line: usize },
    NegativeFaceIndexOutOfRange { index: i64, line: usize },
    /// Limits the reader keeps to.
    VertexCountExceedsLimit { limit: usize, line: usize },
    ObjectCountExceedsLimit { limit: usize, line: usize },
    FaceExceedsTokenLimit { limit: usize, line: usize },
    OutOfMemoryVertices { line: usize },
    OutOfMemoryFaces { line: usize },
    /// Anything else.
    UnrecognizedDirective { directive: String, line: usize },
    IndexedMeshBuild(IndexedMeshBuildError),
    Io(std::io::Error),
}

/// Reading a 3MF package's model.
#[derive(Debug)]
pub enum LoadThreeMfError {
    /// Not a model at all.
    SlicedPrintFile,
    StartPartNotAModel { content_type: String },
    /// The package or the XML would not read.
    Package(PackageReadError),
    Model(ModelParseError),
    /// A model this reader cannot take.
    NonFiniteVertex { object_id: u32 },
    LowresMeshUnsupported { object_id: u32 },
    ProductionReference { object_id: u32, path: String },
    ComponentDepthExceeded { limit: usize },
    LeafBudgetExceeded { limit: usize },
}

// Writing.

/// Writing a scene to a file of any format. The file is written beside
/// the target and renamed over it, so a failure leaves the old file.
#[derive(Debug)]
pub enum WriteSceneError {
    /// Nothing that can be written.
    EmptyScene,
    LeafBudgetExceeded { limit: usize },
    Graph(SceneError),
    /// The target and the temporary file beside it.
    PathHasNoFileName(PathBuf),
    CreateTempFile { source: std::io::Error, path: PathBuf },
    Rename { source: std::io::Error, from: PathBuf, to: PathBuf },
    Io(std::io::Error),
    /// The writer's own error.
    ThreeMf(WriteThreeMfError),
    Obj(WriteObjError),
    Stl(WriteStlError),
}

/// Writing a binary or text STL.
#[derive(Debug)]
pub enum WriteStlError {
    FaceCountExceedsU32 { count: usize },
    Io(std::io::Error),
}

/// Writing an OBJ: indices are 32-bit and count across every object.
#[derive(Debug)]
pub enum WriteObjError {
    VertexCountExceedsU32 { vertex_count: usize },
    VertexOffsetOverflow { added: u32, offset: u32 },
    FaceIndexOverflow { offset: u32, local: u32 },
    LeafBudgetExceeded { limit: usize },
    Graph(SceneError),
    Io(std::io::Error),
}

/// Writing a 3MF package.
#[derive(Debug)]
pub enum WriteThreeMfError {
    CsgUnrepresentable { node_id: sc_scene::NodeId },
    Graph(SceneError),
    Model(ModelWriteError),
    Package(PackageWriteError),
    Io(std::io::Error),
}

/// Writes `inner`'s own message, flags and all.
fn pass_on(inner: &dyn fmt::Display, out: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Display::fmt(inner, out)
}

/// `before`, then `inner`'s message.
fn after(out: &mut fmt::Formatter<'_>, before: &str, inner: &dyn fmt::Display) -> fmt::Result {
    out.write_str(before)?;
    pass_on(inner, out)
}

impl fmt::Display for InferFileTypeError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self::UnknownExtension { path } = self;
        let shown = path.display();
        write!(out, "Unable to infer file format from filename extension for \"{shown}\"")
    }
}

impl fmt::Display for LoadMeshError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InferType(inner) => pass_on(inner, out),
            Self::Stl(inner) => pass_on(inner, out),
            Self::Obj(inner) => pass_on(inner, out),
            Self::ThreeMf(inner) => pass_on(inner, out),
            Self::Io(inner) => after(out, "I/O error while loading mesh: ", inner),
            Self::UnsupportedFormat(kind) => write!(out, "Unsupported mesh file type: {kind:?}"),
            Self::FileTooLarge { actual_bytes: size, limit_bytes: most } => {
                write!(out, "file is {size} bytes, which exceeds the {most} byte limit")
            }
        }
    }
}

impl fmt::Display for WriteSceneError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Graph(inner) => pass_on(inner, out),
            Self::Stl(inner) => pass_on(inner, out),
            Self::Obj(inner) => pass_on(inner, out),
            Self::ThreeMf(inner) => pass_on(inner, out),
            Self::Io(inner) => after(out, "I/O error while writing scene: ", inner),
            Self::EmptyScene => out.write_str("Cannot flatten an empty scene to a single mesh"),
            Self::LeafBudgetExceeded { limit: most } => {
                write!(out, "the scene expands to more than {most} mesh placements")
            }
            Self::PathHasNoFileName(target) => {
                write!(out, "Target path {} has no file name", target.display())
            }
            Self::CreateTempFile { path: temp, source: why } => {
                write!(out, "Failed to create temp file {}: {why}", temp.display())
            }
            Self::Rename { from: old, to: new, source: why } => {
                write!(out, "Failed to rename {} to {}: {why}", old.display(), new.display())
            }
        }
    }
}

impl fmt::Display for LoadStlError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IndexedMeshBuild(inner) => pass_on(inner, out),
            Self::Io(inner) => after(out, "STL I/O error: ", inner),
            Self::AsciiHeaderMissing => out.write_str("Ascii STL does not start with the \"solid\" keyword"),
            Self::AsciiEmptyInput => out.write_str("File seems to be empty"),
            Self::Truncated { what: part } => write!(
                out,
                "The STL file is truncated or malformed: it ended before {part} could be read"
            ),
            Self::DeclaredTriangleCountExceedsCap { declared: claimed, cap: most } => write!(
                out,
                "declared triangle count {claimed} exceeds the {most} maximum supported by this reader"
            ),
            Self::AsciiInvalidFacetHeader { tokens: seen } => write!(out, "invalid facet header: {seen:?}"),
            Self::AsciiInvalidVertexLine { tokens: seen } => {
                write!(out, "Expected vertex f32 f32 f32, got {seen:?}")
            }
            Self::AsciiUnexpected { expected: wanted, got: seen } => {
                write!(out, "expected {wanted:?}, got {seen:?}")
            }
            Self::AsciiEofWhileExpecting { what: wanted } => write!(out, "EOF while expecting {wanted}"),
            Self::InvalidFloat { token: text, source: why } => write!(out, "invalid float '{text}': {why}"),
            Self::NonFiniteFloat { what: field, value: number } => {
                let class = number.classify();
                write!(out, "expected finite f32 for {field}, got {number} which is {class:?}")
            }
        }
    }
}

impl fmt::Display for LoadObjError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Everything but the two wrapped errors names the line it is about.
        let on = |out: &mut fmt::Formatter<'_>, line: usize, rest: fmt::Arguments<'_>| {
            write!(out, "line {line}: ")?;
            out.write_fmt(rest)
        };
        match self {
            Self::IndexedMeshBuild(inner) => pass_on(inner, out),
            Self::Io(inner) => after(out, "OBJ I/O error: ", inner),
            &Self::InvalidFloat { line, ref token, ref source } => {
                on(out, line, format_args!("invalid float '{token}': {source}"))
            }
            &Self::NonFiniteFloat { line, value } => on(out, line, format_args!("expected finite float, got {value}")),
            &Self::VertexCoordCountInvalid { line, got } => {
                on(out, line, format_args!("vertex requires 3 or 4 coordinates, got {got}"))
            }
            &Self::VertexWZero { line } => on(out, line, format_args!("vertex w coordinate must not be zero")),
            &Self::VertexCountExceedsLimit { line, limit } => {
                on(out, line, format_args!("file declares more than {limit} vertices"))
            }
            &Self::ObjectCountExceedsLimit { line, limit } => {
                on(out, line, format_args!("file declares more than {limit} objects"))
            }
            &Self::OutOfMemoryVertices { line } => {
                on(out, line, format_args!("out of memory while parsing OBJ vertices"))
            }
            &Self::OutOfMemoryFaces { line } => {
                on(out, line, format_args!("out of memory while expanding face into triangles"))
            }
            &Self::FaceTooFewVertices { line, got } => {
                on(out, line, format_args!("face requires at least 3 vertices, got {got}"))
            }
            &Self::FaceExceedsTokenLimit { line, limit } => {
                on(out, line, format_args!("face exceeds the {limit}-vertex limit"))
            }
            &Self::FaceIndexZero { line } => on(out, line, format_args!("face index must not be zero")),
            &Self::InvalidFaceIndex { line, ref token, ref source } => {
                on(out, line, format_args!("invalid face index '{token}': {source}"))
            }
            &Self::NegativeFaceIndexOutOfRange { line, index } => {
                on(out, line, format_args!("negative face index {index} out of range"))
            }
            &Self::FaceIndexOutOfRange { line, index, vertex_count } => {
                let counted = if index < 0 { "negative" } else { "vertex" };
                on(
                    out,
                    line,
                    format_args!("{counted} index {index} out of range (only {vertex_count} vertices defined)"),
                )
            }
            &Self::UnrecognizedDirective { line, ref directive } => {
                on(out, line, format_args!("unrecognized directive '{directive}'"))
            }
        }
    }
}

impl fmt::Display for LoadThreeMfError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Package(inner) => after(out, "3MF package error: ", inner),
            Self::Model(inner) => after(out, "3MF model error: ", inner),
            Self::SlicedPrintFile => out.write_str(
                "this is an already-sliced print file (G-code and thumbnails, no model); \
                 open the original project or model instead",
            ),
            Self::StartPartNotAModel { content_type: declared } => write!(
                out,
                "the package's start part declares content type \"{declared}\", not a 3D Model"
            ),
            Self::NonFiniteVertex { object_id: id } => write!(
                out,
                "object {id} has a vertex coordinate too large for 32-bit floats after unit conversion"
            ),
            Self::LeafBudgetExceeded { limit: most } => {
                write!(out, "the build expands to more than {most} mesh placements")
            }
            Self::ComponentDepthExceeded { limit: most } => {
                write!(out, "component objects nest deeper than {most} levels")
            }
            Self::LowresMeshUnsupported { object_id: id } => write!(
                out,
                "object {id} carries a low-resolution stand-in mesh; \
                 loading sliced 3MF geometry is not supported yet"
            ),
            Self::ProductionReference { path: part, object_id: id } => write!(
                out,
                "a component references object {id} in model part \"{part}\", which is missing or invalid"
            ),
        }
    }
}

impl fmt::Display for WriteStlError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(inner) => after(out, "STL writer I/O error: ", inner),
            Self::FaceCountExceedsU32 { count: faces } => {
                write!(out, "binary STL writer: face count {faces} exceeds u32::MAX")
            }
        }
    }
}

impl fmt::Display for WriteObjError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Self::Io(inner) = self {
            return after(out, "OBJ writer I/O error: ", inner);
        }
        out.write_str("OBJ writer: ")?;
        match self {
            Self::Io(_) => Ok(()),
            Self::Graph(inner) => pass_on(inner, out),
            Self::LeafBudgetExceeded { limit: most } => {
                write!(out, "the scene expands to more than {most} mesh placements")
            }
            Self::FaceIndexOverflow { local: index, offset: base } => {
                write!(out, "face vertex index overflow (local={index}, offset={base})")
            }
            Self::VertexCountExceedsU32 { vertex_count: count } => {
                write!(out, "mesh has {count} vertices, exceeds u32::MAX")
            }
            Self::VertexOffsetOverflow { offset: base, added: more } => {
                write!(out, "cumulative vertex offset overflows u32 (offset={base}, +{more})")
            }
        }
    }
}

impl fmt::Display for WriteThreeMfError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(inner) => after(out, "3MF writer I/O error: ", inner),
            Self::Package(inner) => after(out, "3MF writer: ", inner),
            Self::Model(inner) => after(out, "3MF writer: ", inner),
            Self::Graph(inner) => after(out, "3MF writer: ", inner),
            Self::CsgUnrepresentable { node_id: node } => write!(
                out,
                "3MF writer: node {} is a CSG operation, which 3MF cannot represent",
                node.get()
            ),
        }
    }
}

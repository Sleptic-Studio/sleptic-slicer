// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Every error message of this crate, word for word: the GUI shows them and
//! bug reports quote them, so rewording one is a change to review, not a
//! side effect.
use std::path::PathBuf;

use sc_mesh_core::IndexedMeshBuildError;
use sc_scene::{Scene, SceneError};

use crate::error::*;
use crate::filetypes::MeshFileType;

fn disk() -> std::io::Error {
    std::io::Error::other("disk")
}

fn says(error: impl std::fmt::Display, text: &str) {
    assert_eq!(error.to_string(), text);
}

#[test]
fn file_type_and_load_messages() {
    let path = PathBuf::from("part.xyz");
    says(
        InferFileTypeError::UnknownExtension { path: path.clone() },
        "Unable to infer file format from filename extension for \"part.xyz\"",
    );
    says(LoadMeshError::Io(disk()), "I/O error while loading mesh: disk");
    says(
        LoadMeshError::FileTooLarge { actual_bytes: 12, limit_bytes: 10 },
        "file is 12 bytes, which exceeds the 10 byte limit",
    );
    says(
        LoadMeshError::InferType(InferFileTypeError::UnknownExtension { path }),
        "Unable to infer file format from filename extension for \"part.xyz\"",
    );
    says(LoadMeshError::UnsupportedFormat(MeshFileType::Ply), "Unsupported mesh file type: Ply");
    says(LoadMeshError::Stl(LoadStlError::AsciiEmptyInput), "File seems to be empty");
    says(LoadMeshError::Obj(LoadObjError::FaceIndexZero { line: 2 }), "line 2: face index must not be zero");
    says(
        LoadMeshError::ThreeMf(LoadThreeMfError::SlicedPrintFile),
        "this is an already-sliced print file (G-code and thumbnails, no model); open the original project or model instead",
    );
}

#[test]
fn write_scene_messages() {
    let node_id = Scene::new().add_group(None, Default::default());
    let graph = SceneError::NotALeaf { node_id };
    let graph_text = graph.to_string();
    says(WriteSceneError::Io(disk()), "I/O error while writing scene: disk");
    says(WriteSceneError::EmptyScene, "Cannot flatten an empty scene to a single mesh");
    says(WriteSceneError::LeafBudgetExceeded { limit: 5 }, "the scene expands to more than 5 mesh placements");
    says(WriteSceneError::PathHasNoFileName(PathBuf::from("dir")), "Target path dir has no file name");
    says(
        WriteSceneError::CreateTempFile { path: PathBuf::from("a.tmp"), source: disk() },
        "Failed to create temp file a.tmp: disk",
    );
    says(
        WriteSceneError::Rename { from: PathBuf::from("a"), to: PathBuf::from("b"), source: disk() },
        "Failed to rename a to b: disk",
    );
    says(WriteSceneError::Graph(graph), &graph_text);
    says(WriteSceneError::Stl(WriteStlError::Io(disk())), "STL writer I/O error: disk");
    says(WriteSceneError::Obj(WriteObjError::Io(disk())), "OBJ writer I/O error: disk");
    says(WriteSceneError::ThreeMf(WriteThreeMfError::Io(disk())), "3MF writer I/O error: disk");
}

#[test]
fn stl_messages() {
    let bad_float = "x".parse::<f32>().unwrap_err();
    let tokens = vec!["facet".to_string(), "?".to_string()];
    let build = IndexedMeshBuildError::TooManyVertices { limit: 3 };
    let build_text = build.to_string();
    says(LoadStlError::Io(disk()), "STL I/O error: disk");
    says(
        LoadStlError::Truncated { what: "the header" },
        "The STL file is truncated or malformed: it ended before the header could be read",
    );
    says(
        LoadStlError::DeclaredTriangleCountExceedsCap { declared: 9, cap: 8 },
        "declared triangle count 9 exceeds the 8 maximum supported by this reader",
    );
    says(LoadStlError::AsciiHeaderMissing, "Ascii STL does not start with the \"solid\" keyword");
    says(LoadStlError::AsciiEmptyInput, "File seems to be empty");
    says(
        LoadStlError::AsciiInvalidFacetHeader { tokens: tokens.clone() },
        "invalid facet header: [\"facet\", \"?\"]",
    );
    says(
        LoadStlError::AsciiInvalidVertexLine { tokens: tokens.clone() },
        "Expected vertex f32 f32 f32, got [\"facet\", \"?\"]",
    );
    says(
        LoadStlError::AsciiUnexpected { expected: "loop", got: tokens },
        "expected \"loop\", got [\"facet\", \"?\"]",
    );
    says(LoadStlError::AsciiEofWhileExpecting { what: "endloop" }, "EOF while expecting endloop");
    says(
        LoadStlError::InvalidFloat { token: "x".into(), source: bad_float.clone() },
        &format!("invalid float 'x': {bad_float}"),
    );
    says(
        LoadStlError::NonFiniteFloat { what: "vertex x", value: f32::INFINITY },
        "expected finite f32 for vertex x, got inf which is Infinite",
    );
    says(LoadStlError::IndexedMeshBuild(build), &build_text);
}

#[test]
fn obj_messages() {
    let bad_float = "x".parse::<f32>().unwrap_err();
    let bad_int = "y".parse::<i64>().unwrap_err();
    says(LoadObjError::Io(disk()), "OBJ I/O error: disk");
    says(
        LoadObjError::InvalidFloat { line: 3, token: "x".into(), source: bad_float.clone() },
        &format!("line 3: invalid float 'x': {bad_float}"),
    );
    says(LoadObjError::NonFiniteFloat { line: 3, value: f32::NAN }, "line 3: expected finite float, got NaN");
    says(
        LoadObjError::VertexCoordCountInvalid { line: 3, got: 2 },
        "line 3: vertex requires 3 or 4 coordinates, got 2",
    );
    says(LoadObjError::VertexWZero { line: 3 }, "line 3: vertex w coordinate must not be zero");
    says(LoadObjError::VertexCountExceedsLimit { line: 3, limit: 7 }, "line 3: file declares more than 7 vertices");
    says(LoadObjError::ObjectCountExceedsLimit { line: 3, limit: 7 }, "line 3: file declares more than 7 objects");
    says(LoadObjError::OutOfMemoryVertices { line: 3 }, "line 3: out of memory while parsing OBJ vertices");
    says(
        LoadObjError::OutOfMemoryFaces { line: 3 },
        "line 3: out of memory while expanding face into triangles",
    );
    says(
        LoadObjError::FaceTooFewVertices { line: 3, got: 2 },
        "line 3: face requires at least 3 vertices, got 2",
    );
    says(LoadObjError::FaceExceedsTokenLimit { line: 3, limit: 9 }, "line 3: face exceeds the 9-vertex limit");
    says(LoadObjError::FaceIndexZero { line: 3 }, "line 3: face index must not be zero");
    says(
        LoadObjError::InvalidFaceIndex { line: 3, token: "y".into(), source: bad_int.clone() },
        &format!("line 3: invalid face index 'y': {bad_int}"),
    );
    says(
        LoadObjError::NegativeFaceIndexOutOfRange { line: 3, index: -9 },
        "line 3: negative face index -9 out of range",
    );
    says(
        LoadObjError::FaceIndexOutOfRange { line: 3, index: 9, vertex_count: 4 },
        "line 3: vertex index 9 out of range (only 4 vertices defined)",
    );
    says(
        LoadObjError::FaceIndexOutOfRange { line: 3, index: -9, vertex_count: 4 },
        "line 3: negative index -9 out of range (only 4 vertices defined)",
    );
    says(
        LoadObjError::UnrecognizedDirective { line: 3, directive: "q".into() },
        "line 3: unrecognized directive 'q'",
    );
    let build = IndexedMeshBuildError::TooManyVertices { limit: 3 };
    let build_text = build.to_string();
    says(LoadObjError::IndexedMeshBuild(build), &build_text);
}

#[test]
fn three_mf_load_messages() {
    let package = sc_3mf::error::PackageReadError::PartRead { part: "/3D/a.model".into(), source: disk() };
    let package_text = package.to_string();
    says(LoadThreeMfError::Package(package), &format!("3MF package error: {package_text}"));
    says(
        LoadThreeMfError::StartPartNotAModel { content_type: "text/plain".into() },
        "the package's start part declares content type \"text/plain\", not a 3D Model",
    );
    says(
        LoadThreeMfError::NonFiniteVertex { object_id: 4 },
        "object 4 has a vertex coordinate too large for 32-bit floats after unit conversion",
    );
    says(LoadThreeMfError::LeafBudgetExceeded { limit: 6 }, "the build expands to more than 6 mesh placements");
    says(LoadThreeMfError::ComponentDepthExceeded { limit: 6 }, "component objects nest deeper than 6 levels");
    says(
        LoadThreeMfError::LowresMeshUnsupported { object_id: 4 },
        "object 4 carries a low-resolution stand-in mesh; loading sliced 3MF geometry is not supported yet",
    );
    says(
        LoadThreeMfError::ProductionReference { path: "/3D/b.model".into(), object_id: 4 },
        "a component references object 4 in model part \"/3D/b.model\", which is missing or invalid",
    );
    let model = sc_3mf::error::ModelParseError::DtdForbidden;
    let model_text = model.to_string();
    says(LoadThreeMfError::Model(model), &format!("3MF model error: {model_text}"));
}

#[test]
fn writer_messages() {
    says(WriteStlError::Io(disk()), "STL writer I/O error: disk");
    says(
        WriteStlError::FaceCountExceedsU32 { count: 5 },
        "binary STL writer: face count 5 exceeds u32::MAX",
    );
    let node_id = Scene::new().add_group(None, Default::default());
    let graph_text = SceneError::NotALeaf { node_id }.to_string();
    says(WriteObjError::Io(disk()), "OBJ writer I/O error: disk");
    says(WriteObjError::Graph(SceneError::NotALeaf { node_id }), &format!("OBJ writer: {graph_text}"));
    says(
        WriteObjError::LeafBudgetExceeded { limit: 2 },
        "OBJ writer: the scene expands to more than 2 mesh placements",
    );
    says(
        WriteObjError::FaceIndexOverflow { local: 1, offset: 2 },
        "OBJ writer: face vertex index overflow (local=1, offset=2)",
    );
    says(
        WriteObjError::VertexCountExceedsU32 { vertex_count: 8 },
        "OBJ writer: mesh has 8 vertices, exceeds u32::MAX",
    );
    says(
        WriteObjError::VertexOffsetOverflow { offset: 1, added: 2 },
        "OBJ writer: cumulative vertex offset overflows u32 (offset=1, +2)",
    );
    let package = sc_3mf::error::PackageWriteError::Io(disk());
    let package_text = package.to_string();
    says(WriteThreeMfError::Package(package), &format!("3MF writer: {package_text}"));
    let model = sc_3mf::error::ModelWriteError::Io(disk());
    let model_text = model.to_string();
    says(WriteThreeMfError::Model(model), &format!("3MF writer: {model_text}"));
    says(WriteThreeMfError::Io(disk()), "3MF writer I/O error: disk");
    says(WriteThreeMfError::Graph(SceneError::NotALeaf { node_id }), &format!("3MF writer: {graph_text}"));
    says(
        WriteThreeMfError::CsgUnrepresentable { node_id },
        &format!("3MF writer: node {} is a CSG operation, which 3MF cannot represent", node_id.get()),
    );
}

#[test]
fn wrapped_errors_are_their_sources() {
    use std::error::Error as _;
    assert!(LoadMeshError::Io(disk()).source().is_some());
    assert!(LoadMeshError::FileTooLarge { actual_bytes: 1, limit_bytes: 0 }.source().is_none());
    assert!(WriteSceneError::Rename { from: "a".into(), to: "b".into(), source: disk() }.source().is_some());
    assert!(LoadObjError::FaceIndexZero { line: 1 }.source().is_none());
}

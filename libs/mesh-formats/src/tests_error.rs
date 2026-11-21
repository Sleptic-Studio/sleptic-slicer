// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::error::{InferFileTypeError, WriteStlError};
use std::error::Error;

#[test]
fn file_type_and_stl_writer_errors() {
    let error = InferFileTypeError::UnknownExtension { path: "part.xyz".into() };
    assert_eq!(error.to_string(), "Unable to infer file format from filename extension for \"part.xyz\"");
    assert!(error.source().is_none());
    let error = WriteStlError::FaceCountExceedsU32 { count: 42 };
    assert_eq!(error.to_string(), "binary STL writer: face count 42 exceeds u32::MAX");
    assert!(error.source().is_none());
    let error = WriteStlError::from(std::io::Error::other("disk"));
    assert!(matches!(error, WriteStlError::Io(_)));
    assert_eq!(error.to_string(), "STL writer I/O error: disk");
    assert_eq!(error.source().unwrap().to_string(), "disk");
}

#[test]
fn stl_reader_messages_without_causes() {
    use crate::error::LoadStlError as E;
    let cases = [
        (E::Truncated { what: "header" }, "The STL file is truncated or malformed: it ended before header could be read"),
        (E::DeclaredTriangleCountExceedsCap { declared: 9, cap: 8 }, "declared triangle count 9 exceeds the 8 maximum supported by this reader"),
        (E::AsciiHeaderMissing, "Ascii STL does not start with the \"solid\" keyword"),
        (E::AsciiEmptyInput, "File seems to be empty"),
        (E::AsciiInvalidFacetHeader { tokens: vec!["x".into()] }, "invalid facet header: [\"x\"]"),
        (E::AsciiInvalidVertexLine { tokens: vec!["x".into()] }, "Expected vertex f32 f32 f32, got [\"x\"]"),
        (E::AsciiUnexpected { expected: "vertex", got: vec!["x".into()] }, "expected \"vertex\", got [\"x\"]"),
        (E::AsciiEofWhileExpecting { what: "vertex" }, "EOF while expecting vertex"),
        (E::NonFiniteFloat { what: "x", value: f32::INFINITY }, "expected finite f32 for x, got inf which is Infinite"),
    ];
    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        assert!(error.source().is_none());
    }
    let error = E::from(std::io::Error::other("disk"));
    assert!(matches!(error, E::Io(_)));
    assert_eq!(error.to_string(), "STL I/O error: disk");
    assert_eq!(error.source().unwrap().to_string(), "disk");
    let error = E::InvalidFloat { token: "x".into(), source: "x".parse::<f32>().unwrap_err() };
    assert_eq!(error.to_string(), "invalid float 'x': invalid float literal");
    assert_eq!(error.source().unwrap().to_string(), "invalid float literal");
}

#[test]
fn obj_reader_messages_without_causes() {
    use crate::error::LoadObjError as E;
    let cases = [
        (E::NonFiniteFloat { line: 2, value: f32::INFINITY }, "line 2: expected finite float, got inf"),
        (E::VertexCoordCountInvalid { line: 2, got: 5 }, "line 2: vertex requires 3 or 4 coordinates, got 5"),
        (E::VertexWZero { line: 2 }, "line 2: vertex w coordinate must not be zero"),
        (E::VertexCountExceedsLimit { line: 2, limit: 3 }, "line 2: file declares more than 3 vertices"),
        (E::ObjectCountExceedsLimit { line: 2, limit: 3 }, "line 2: file declares more than 3 objects"),
        (E::OutOfMemoryVertices { line: 2 }, "line 2: out of memory while parsing OBJ vertices"),
        (E::OutOfMemoryFaces { line: 2 }, "line 2: out of memory while expanding face into triangles"),
        (E::FaceTooFewVertices { line: 2, got: 1 }, "line 2: face requires at least 3 vertices, got 1"),
        (E::FaceExceedsTokenLimit { line: 2, limit: 3 }, "line 2: face exceeds the 3-vertex limit"),
        (E::FaceIndexZero { line: 2 }, "line 2: face index must not be zero"),
        (E::NegativeFaceIndexOutOfRange { line: 2, index: -3 }, "line 2: negative face index -3 out of range"),
        (E::FaceIndexOutOfRange { line: 2, index: -3, vertex_count: 1 }, "line 2: negative index -3 out of range (only 1 vertices defined)"),
        (E::FaceIndexOutOfRange { line: 2, index: 3, vertex_count: 1 }, "line 2: vertex index 3 out of range (only 1 vertices defined)"),
        (E::UnrecognizedDirective { line: 2, directive: "x".into() }, "line 2: unrecognized directive 'x'"),
    ];
    for (error, expected) in cases { assert_eq!(error.to_string(), expected); assert!(error.source().is_none()); }
    let error = E::from(std::io::Error::other("disk"));
    assert!(matches!(error, E::Io(_)));
    assert_eq!(error.to_string(), "OBJ I/O error: disk");
    assert_eq!(error.source().unwrap().to_string(), "disk");
    let error = E::InvalidFloat { line: 2, token: "x".into(), source: "x".parse::<f32>().unwrap_err() };
    assert_eq!(error.to_string(), "line 2: invalid float 'x': invalid float literal");
    assert_eq!(error.source().unwrap().to_string(), "invalid float literal");
    let error = E::InvalidFaceIndex { line: 2, token: "x".into(), source: "x".parse::<i64>().unwrap_err() };
    assert_eq!(error.to_string(), "line 2: invalid face index 'x': invalid digit found in string");
    assert_eq!(error.source().unwrap().to_string(), "invalid digit found in string");
}

#[test]
fn three_mf_reader_messages_without_causes() {
    use crate::error::LoadThreeMfError as E;
    let cases = [
        (E::StartPartNotAModel { content_type: "x".into() }, "the package's start part declares content type \"x\", not a 3D Model"),
        (E::NonFiniteVertex { object_id: 2 }, "object 2 has a vertex coordinate too large for 32-bit floats after unit conversion"),
        (E::LeafBudgetExceeded { limit: 3 }, "the build expands to more than 3 mesh placements"),
        (E::ComponentDepthExceeded { limit: 3 }, "component objects nest deeper than 3 levels"),
        (E::LowresMeshUnsupported { object_id: 2 }, "object 2 carries a low-resolution stand-in mesh; loading sliced 3MF geometry is not supported yet"),
        (E::ProductionReference { path: "p".into(), object_id: 2 }, "a component references object 2 in model part \"p\", which is missing or invalid"),
        (E::SlicedPrintFile, "this is an already-sliced print file (G-code and thumbnails, no model); open the original project or model instead"),
    ];
    for (error, expected) in cases { assert_eq!(error.to_string(), expected); assert!(error.source().is_none()); }
    let error = E::from(sc_3mf::error::PackageReadError::MissingContentTypes);
    assert!(matches!(error, E::Package(_)));
    assert_eq!(error.to_string(), "3MF package error: package has no [Content_Types].xml part");
    assert_eq!(error.source().unwrap().to_string(), "package has no [Content_Types].xml part");
    let error = E::from(sc_3mf::error::ModelParseError::MissingModelElement);
    assert!(matches!(error, E::Model(_)));
    assert_eq!(error.to_string(), "3MF model error: document contains no <model> element");
    assert_eq!(error.source().unwrap().to_string(), "document contains no <model> element");
}

#[test]
fn obj_writer_messages_without_causes() {
    use crate::error::WriteObjError as E;
    let cases = [
        (E::LeafBudgetExceeded { limit: 3 }, "OBJ writer: the scene expands to more than 3 mesh placements"),
        (E::FaceIndexOverflow { local: 2, offset: 3 }, "OBJ writer: face vertex index overflow (local=2, offset=3)"),
        (E::VertexCountExceedsU32 { vertex_count: 3 }, "OBJ writer: mesh has 3 vertices, exceeds u32::MAX"),
        (E::VertexOffsetOverflow { offset: 2, added: 3 }, "OBJ writer: cumulative vertex offset overflows u32 (offset=2, +3)"),
    ];
    for (error, expected) in cases { assert_eq!(error.to_string(), expected); assert!(error.source().is_none()); }
    let error = E::from(std::io::Error::other("disk"));
    assert!(matches!(error, E::Io(_)));
    assert_eq!(error.to_string(), "OBJ writer I/O error: disk");
    assert_eq!(error.source().unwrap().to_string(), "disk");
}

#[test]
fn mesh_and_scene_wrapper_contracts() {
    use crate::error::*;
    use crate::filetypes::MeshFileType;
    use sc_scene::SceneError;
    use std::path::PathBuf;
    let cases: Vec<(LoadMeshError, &str, bool)> = vec![
        (std::io::Error::other("disk").into(), "I/O error while loading mesh: disk", true),
        (LoadMeshError::FileTooLarge { actual_bytes: 4, limit_bytes: 3 }, "file is 4 bytes, which exceeds the 3 byte limit", false),
        (InferFileTypeError::UnknownExtension { path: "part.xyz".into() }.into(), "Unable to infer file format from filename extension for \"part.xyz\"", true),
        (MeshFileType::Ply.into(), "Unsupported mesh file type: Ply", false),
        (LoadStlError::AsciiEmptyInput.into(), "File seems to be empty", true),
        (LoadObjError::VertexWZero { line: 2 }.into(), "line 2: vertex w coordinate must not be zero", true),
        (LoadThreeMfError::SlicedPrintFile.into(), "this is an already-sliced print file (G-code and thumbnails, no model); open the original project or model instead", true),
    ];
    for (error, expected, source) in cases {
        assert_eq!(error.to_string(), expected);
        assert_eq!(error.source().is_some(), source);
    }
    assert!(matches!(LoadMeshError::from(MeshFileType::Ply), LoadMeshError::UnsupportedFormat(_)));
    assert!(matches!(LoadMeshError::from(LoadStlError::AsciiEmptyInput), LoadMeshError::Stl(_)));
    assert!(matches!(LoadMeshError::from(LoadObjError::VertexWZero { line: 2 }), LoadMeshError::Obj(_)));
    assert!(matches!(LoadMeshError::from(LoadThreeMfError::SlicedPrintFile), LoadMeshError::ThreeMf(_)));
    assert!(matches!(LoadMeshError::from(InferFileTypeError::UnknownExtension { path: "p".into() }), LoadMeshError::InferType(_)));
    assert!(matches!(LoadMeshError::from(std::io::Error::other("disk")), LoadMeshError::Io(_)));

    let cases: Vec<(WriteSceneError, &str, bool)> = vec![
        (std::io::Error::other("disk").into(), "I/O error while writing scene: disk", true),
        (WriteSceneError::EmptyScene, "Cannot flatten an empty scene to a single mesh", false),
        (WriteSceneError::LeafBudgetExceeded { limit: 3 }, "the scene expands to more than 3 mesh placements", false),
        (PathBuf::from("part").into(), "Target path part has no file name", false),
        (WriteSceneError::CreateTempFile { path: "p".into(), source: std::io::Error::other("disk") }, "Failed to create temp file p: disk", true),
        (WriteSceneError::Rename { from: "a".into(), to: "b".into(), source: std::io::Error::other("disk") }, "Failed to rename a to b: disk", true),
        (SceneError::InvalidPlates.into(), "invalid project plate table", true),
        (WriteStlError::FaceCountExceedsU32 { count: 3 }.into(), "binary STL writer: face count 3 exceeds u32::MAX", true),
        (WriteObjError::LeafBudgetExceeded { limit: 3 }.into(), "OBJ writer: the scene expands to more than 3 mesh placements", true),
        (WriteThreeMfError::from(std::io::Error::other("disk")).into(), "3MF writer I/O error: disk", true),
    ];
    for (error, expected, source) in cases {
        assert_eq!(error.to_string(), expected);
        assert_eq!(error.source().is_some(), source);
    }
    assert!(matches!(WriteSceneError::from(std::io::Error::other("disk")), WriteSceneError::Io(_)));
    assert!(matches!(WriteSceneError::from(PathBuf::from("p")), WriteSceneError::PathHasNoFileName(_)));
    assert!(matches!(WriteSceneError::from(SceneError::InvalidPlates), WriteSceneError::Graph(_)));
    assert!(matches!(WriteSceneError::from(WriteStlError::FaceCountExceedsU32 { count: 3 }), WriteSceneError::Stl(_)));
    assert!(matches!(WriteSceneError::from(WriteObjError::LeafBudgetExceeded { limit: 3 }), WriteSceneError::Obj(_)));
    assert!(matches!(WriteSceneError::from(WriteThreeMfError::from(std::io::Error::other("disk"))), WriteSceneError::ThreeMf(_)));
}

#[test]
fn nested_builder_and_writer_conversions() {
    use crate::error::*;
    use sc_mesh_core::IndexedMeshBuildError;
    use sc_scene::SceneError;
    use sc_3mf::error::{ModelWriteError, PackageWriteError};
    let build = || IndexedMeshBuildError::TooManyVertices { limit: 3 };
    let expected = "indexed-mesh build: unique vertex count exceeds the 3 index limit";
    let stl = LoadStlError::from(build());
    assert!(matches!(stl, LoadStlError::IndexedMeshBuild(_)));
    assert_eq!(stl.to_string(), expected);
    assert_eq!(stl.source().unwrap().to_string(), expected);
    let obj = LoadObjError::from(build());
    assert!(matches!(obj, LoadObjError::IndexedMeshBuild(_)));
    assert_eq!(obj.to_string(), expected);
    assert_eq!(obj.source().unwrap().to_string(), expected);
    let obj = WriteObjError::from(SceneError::InvalidPlates);
    assert!(matches!(obj, WriteObjError::Graph(_)));
    assert_eq!(obj.to_string(), "OBJ writer: invalid project plate table");
    assert_eq!(obj.source().unwrap().to_string(), "invalid project plate table");
    let cases = [
        WriteThreeMfError::from(PackageWriteError::DuplicatePartName { name: "a".into() }),
        WriteThreeMfError::from(ModelWriteError::PolygonWithoutSegments { stack_id: 2 }),
        WriteThreeMfError::from(std::io::Error::other("disk")),
        WriteThreeMfError::from(SceneError::InvalidPlates),
    ];
    assert!(matches!(cases[0], WriteThreeMfError::Package(_)));
    assert!(matches!(cases[1], WriteThreeMfError::Model(_)));
    assert!(matches!(cases[2], WriteThreeMfError::Io(_)));
    assert!(matches!(cases[3], WriteThreeMfError::Graph(_)));
    for (error, display, source) in cases.into_iter().zip([
        ("3MF writer: package already contains a part named \"a\"", "package already contains a part named \"a\""),
        ("3MF writer: slice stack 2 has a polygon with no segments", "slice stack 2 has a polygon with no segments"),
        ("3MF writer I/O error: disk", "disk"),
        ("3MF writer: invalid project plate table", "invalid project plate table"),
    ]).map(|(e, (d, s))| (e, d, s)) {
        assert_eq!(error.to_string(), display);
        assert_eq!(error.source().unwrap().to_string(), source);
    }
}

#[test]
fn csg_writer_error_has_no_source() {
    let mut scene = sc_scene::Scene::new();
    let node_id = scene.add_group(None, sc_mesh_core::transform::Transform3D::Identity);
    let error = crate::error::WriteThreeMfError::CsgUnrepresentable { node_id };
    assert_eq!(error.to_string(), "3MF writer: node 1 is a CSG operation, which 3MF cannot represent");
    assert!(error.source().is_none());
}

#[test]
fn cause_identity_and_zero_index_wording() {
    use crate::error::{LoadMeshError, LoadObjError};
    let nested = LoadMeshError::Stl(crate::error::LoadStlError::AsciiEmptyInput);
    assert_eq!(nested.to_string(), "File seems to be empty");
    assert_eq!(LoadObjError::FaceIndexOutOfRange { line: 9, index: -1, vertex_count: 4 }.to_string(),
        "line 9: negative index -1 out of range (only 4 vertices defined)");
    let wrapped = LoadMeshError::Io(std::io::Error::other("cause"));
    let LoadMeshError::Io(inner) = &wrapped else { unreachable!() };
    assert!(std::ptr::eq(inner, wrapped.source().unwrap().downcast_ref::<std::io::Error>().unwrap()));
    assert_eq!(LoadObjError::FaceIndexOutOfRange { line: 9, index: 0, vertex_count: 4 }.to_string(),
        "line 9: vertex index 0 out of range (only 4 vertices defined)");
}

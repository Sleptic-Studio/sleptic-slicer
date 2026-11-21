// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use std::path::PathBuf;
use std::sync::Arc;

use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_mesh_core::{IndexedMesh, IndexedTriangle, Normal, Vertex};
use sc_mesh_formats::filetypes::MeshFileType;
use sc_scene::{LeafPayload, Scene};
use tempfile::TempDir;

use super::*;

/// The triangle (1,0,0) (0,1,0) (0,0,1).
fn triangle() -> Arc<IndexedMesh> {
    Arc::new(IndexedMesh {
        meta: None,
        vertices: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].map(Vertex::new).to_vec(),
        faces: vec![IndexedTriangle::from_raw_indices(Normal::new([1.0, 0.0, 0.0]), [0, 1, 2])],
    })
}

fn moved(x: f64) -> Transform3D {
    Transform3D::Affine(AffineTransform3D::from_translation([x, 0.0, 0.0]))
}

/// Triangles under one group, at these x offsets.
fn triangles(at: &[f64]) -> Scene {
    let mut scene = Scene::new();
    let root = scene.add_group(None, Transform3D::Identity);
    scene.add_root(root).unwrap();
    for &x in at {
        let t = if x == 0.0 { Transform3D::Identity } else { moved(x) };
        let id = scene.add_leaf(None, t, LeafPayload::Mesh(triangle())).unwrap();
        scene.add_child(root, id).unwrap();
    }
    scene
}

/// A triangle, plus a shared group of one triangle used twice, at x 10 and
/// 20: three triangles once expanded.
fn shared_twice() -> Scene {
    let mut scene = triangles(&[0.0]);
    let inner = scene.add_leaf(None, Transform3D::Identity, LeafPayload::Mesh(triangle())).unwrap();
    let group = scene.add_group(None, Transform3D::Identity);
    scene.add_child(group, inner).unwrap();
    let shared = scene.register_subtree_resource(group).unwrap();
    let root = scene.roots()[0];
    for x in [10.0, 20.0] {
        let reference = scene.add_reference(None, moved(x), shared).unwrap();
        scene.add_child(root, reference).unwrap();
    }
    scene
}

fn opened(scene: Scene, path: &str) -> std::rc::Rc<SceneDocument> {
    let doc = SceneDocument::new();
    doc.take_mesh_scene(scene, PathBuf::from(path));
    doc
}

fn reread_parts(path: &Path) -> usize {
    sc_mesh_formats::load_mesh_from_path(path, Some(MeshFileType::_3mf), None).unwrap().graph.reachable_leaf_count()
}

#[test]
fn the_suggested_name_and_format_follow_the_opened_file() {
    let none = SceneDocument::new();
    assert_eq!((suggested_export_name(&none), suggested_export_suffix(&none)), (None, "stl"));
    for (path, stem, ext) in [
        ("/tmp/gear.obj", Some("gear"), "obj"),
        ("/tmp/gear.stla", Some("gear"), "stla"),
        ("/tmp/gear.STL", Some("gear"), "stl"),
        ("/tmp/gear.stlb", Some("gear"), "stl"),
        ("/tmp/gear.3mf", Some("gear"), "3mf"),
        ("/tmp/gear.ply", Some("gear"), "stl"),
        ("/tmp/gear", Some("gear"), "stl"),
    ] {
        let doc = opened(triangles(&[0.0]), path);
        assert_eq!((suggested_export_name(&doc).as_deref(), suggested_export_suffix(&doc)), (stem, ext), "{path}");
    }
}

#[test]
fn the_copy_frees_the_document_and_ignores_later_changes() {
    let dir = tempfile::tempdir().unwrap();
    assert!(scene_copy_for_export(&SceneDocument::new()).is_none());
    assert!(export_now(&SceneDocument::new(), &dir.path().join("x.stl"), WriteFormat::StlBin).is_err());
    let doc = opened(triangles(&[0.0]), "/tmp/widget.obj");
    let copy = scene_copy_for_export(&doc).unwrap();
    assert!(doc.read_scene(|_| ()).is_some());
    doc.take_mesh_scene(triangles(&[0.0, 10.0]), PathBuf::from("/tmp/other.obj"));
    let target = dir.path().join("copy.3mf");
    assert!(write_exported_scene(&copy, &target, WriteFormat::ThreeMf, |_, _, _| {}).unwrap().is_empty());
    assert_eq!(reread_parts(&target), 1);
}

#[test]
fn the_phases_run_from_merging_to_the_written_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut steps = Vec::new();
    let copy = scene_copy_for_export(&opened(triangles(&[0.0, 10.0]), "/tmp/multi.obj")).unwrap();
    write_exported_scene(&copy, &dir.path().join("multi.stl"), WriteFormat::StlBin, |phase, done, total| {
        steps.push((phase, done, total))
    })
    .unwrap();
    assert_eq!(steps.first(), Some(&(ExportPhase::Flatten, 0, 1)));
    assert!(steps.contains(&(ExportPhase::Flatten, 1, 1)));
    assert!(steps.iter().any(|&(phase, done, total)| phase == ExportPhase::Serialize && done == total && total > 0));
    assert_eq!(steps.last(), Some(&(ExportPhase::Write, 1, 1)));
}

/// Exports `doc` to `name` in `format`; the warnings and the file's bytes.
fn export(doc: &SceneDocument, dir: &TempDir, name: &str, format: WriteFormat) -> (Vec<FlattenWarning>, Vec<u8>) {
    let target = dir.path().join(name);
    let warnings = export_now(doc, &target, format).unwrap();
    (warnings, std::fs::read(target).unwrap())
}

#[test]
fn each_format_writes_its_own_kind_of_file_and_stl_warns_of_merging() {
    let dir = tempfile::tempdir().unwrap();
    let one = opened(triangles(&[0.0]), "/tmp/single.obj");
    let (warnings, bytes) = export(&one, &dir, "one.stl", WriteFormat::StlBin);
    assert!(warnings.is_empty() && bytes.len() > 80);
    let (warnings, text) = export(&one, &dir, "one.stla", WriteFormat::StlAscii);
    assert!(warnings.is_empty() && text.starts_with(b"solid"));
    let (warnings, text) = export(&one, &dir, "one.obj", WriteFormat::Obj);
    let text = String::from_utf8(text).unwrap();
    assert!(warnings.is_empty() && text.contains("v ") && text.contains("f "));
    let two = opened(triangles(&[0.0, 10.0]), "/tmp/multi.obj");
    let (warnings, _) = export(&two, &dir, "two.stl", WriteFormat::StlBin);
    assert_eq!(warnings, [FlattenWarning::MultiObjectMerge { object_count: 2 }]);
    // OBJ and 3MF keep parts apart: nothing to warn of.
    assert!(export(&two, &dir, "two.obj", WriteFormat::Obj).0.is_empty());
    assert!(export(&two, &dir, "two.3mf", WriteFormat::ThreeMf).0.is_empty());
    assert_eq!(reread_parts(&dir.path().join("two.3mf")), 2);
}

/// Shared groups are written out in full, every format. (Written from the
/// scene itself: the document's spatial cache refuses shared groups.)
#[test]
fn a_shared_group_is_written_for_each_use() {
    let dir = tempfile::tempdir().unwrap();
    let scene = shared_twice();
    let write = |name: &str, format| {
        let target = dir.path().join(name);
        let warnings = write_exported_scene(&scene, &target, format, |_, _, _| {}).unwrap();
        (warnings, std::fs::read(&target).unwrap())
    };
    let merged = vec![FlattenWarning::MultiObjectMerge { object_count: 3 }];
    let (warnings, obj) = write("shared.obj", WriteFormat::Obj);
    let obj = String::from_utf8(obj).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(obj.lines().filter(|line| line.starts_with("f ")).count(), 3);
    assert!(obj.contains("v 11 0 0") && obj.contains("v 21 0 0"));
    let (warnings, stl) = write("shared.stl", WriteFormat::StlBin);
    assert_eq!((warnings, stl.len()), (merged.clone(), 80 + 4 + 3 * 50));
    let (warnings, ascii) = write("shared.stla", WriteFormat::StlAscii);
    let ascii = String::from_utf8(ascii).unwrap();
    assert_eq!(warnings, merged);
    assert!(ascii.starts_with("solid") && ascii.matches("facet normal").count() == 3);
    // 3MF keeps the sharing; reading it back expands it.
    assert!(write("shared.3mf", WriteFormat::ThreeMf).0.is_empty());
    assert_eq!(reread_parts(&dir.path().join("shared.3mf")), 3);
}

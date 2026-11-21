// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_mesh_core::transform::Transform3D;
use sc_mesh_formats::messages::SharedVertexLeaf;
use sc_scene::Scene;

use super::*;

#[test]
fn obj_warnings_name_the_line_and_what_was_skipped() {
    use LoadObjWarning as W;
    let line = 12;
    let cases = [
        (W::NoVertices, "OBJ file has no vertices"),
        (W::NoFaces, "OBJ file has no faces"),
        (W::SkippedTextureCoordinates { line }, "line 12: skipped texture coordinates"),
        (W::SkippedVertexNormal { line }, "line 12: skipped vertex normal"),
        (W::SkippedGroup { line, name: "base".into() }, "line 12: skipped group 'base'"),
        (W::SkippedSmoothingGroup { line }, "line 12: skipped smoothing group"),
        (W::SkippedMaterialReference { line, material: "red".into() }, "line 12: skipped material reference 'red'"),
        (W::SkippedMaterialLibrary { line, path: "a.mtl".into() }, "line 12: skipped material library 'a.mtl'"),
        (W::SkippedComment { line }, "line 12: skipped comment"),
        (W::SkippedUnknownDirective { line, directive: "cstype".into() }, "line 12: skipped OBJ directive 'cstype'"),
    ];
    for (warning, text) in cases {
        assert_eq!((&warning).explain(), text);
    }
    assert_eq!(
        (&LoadStlWarning::HeaderNotText).explain(),
        "binary STL header is not readable text; no mesh name was taken from it"
    );
}

#[test]
fn shared_vertices_list_each_object_by_name_or_node() {
    let mut scene = Scene::new();
    let ids: Vec<_> = (0..2).map(|_| scene.add_group(None, Transform3D::Identity)).collect();
    let leaves = vec![
        SharedVertexLeaf { node_id: ids[0], object_name: Some("Hull".into()) },
        SharedVertexLeaf { node_id: ids[1], object_name: None },
    ];
    assert_eq!(
        (&SceneLoadWarning::SharedVertices { leaves }).explain(),
        format!("objects share at least one vertex coordinate: 'Hull', node {}", ids[1].get())
    );
}

#[test]
fn threemf_and_project_warnings_read_as_sentences() {
    use LoadThreeMfWarning as W;
    assert_eq!(
        (&W::MaterialsDropped { group_count: 2 }).explain(),
        "dropped 2 material group(s); materials are not supported yet"
    );
    assert_eq!(
        (&W::UnbuiltObjectSkipped { id: 4, name: Some("lid".into()) }).explain(),
        "skipped object 'lid' (id 4): it is not referenced by the build"
    );
    assert_eq!(
        (&W::UnbuiltObjectSkipped { id: 4, name: None }).explain(),
        "skipped object id 4: it is not referenced by the build"
    );
    assert_eq!(
        (&W::SliceDataIgnored { stack_count: 1 }).explain(),
        "ignored 1 slice stack(s); the full-resolution meshes were loaded instead"
    );
    assert_eq!((&W::EmptyBuild).explain(), "the 3MF build section lists no items");
    assert_eq!(
        (&ReadProjectWarning::ThreeMf(W::EmptyBuild)).explain(),
        "the 3MF build section lists no items"
    );
    assert_eq!(
        (&ReadProjectWarning::OtherSlicersData { dialect: ForeignDialect::BambuStudioOrca }).explain(),
        "the file carries project data written by Bambu Studio / OrcaSlicer; only the model data was loaded"
    );
    assert_eq!(
        (&ReadProjectWarning::UnknownProfileFile { part_name: "x.toml".into() }).explain(),
        "ignored an unrecognized part in the embedded-profiles area: x.toml"
    );
}

#[test]
fn a_profile_with_a_copy_in_the_file_is_only_a_note() {
    let printer = sc_print_primitives::PrinterId::new("mars").unwrap();
    let resin = sc_print_primitives::MaterialId::new("grey").unwrap();
    let missing = ProfileRefWarning { profile: NamedProfile::Printer(printer.clone()), embedded: false };
    let copied = ProfileRefWarning { profile: NamedProfile::Resin(resin), embedded: true };
    assert!(!(&missing).is_note());
    assert!((&copied).is_note());
    assert!((&ProfileRefWarning { profile: NamedProfile::Printer(printer), embedded: true }).is_note());
    assert_eq!(
        (&missing).explain(),
        "the project references printer profile \"mars\", which is not installed and has no embedded copy in the file"
    );
    assert_eq!(
        (&copied).explain(),
        "the project references resin profile \"grey\", which is not installed; the file carries an embedded copy"
    );
}

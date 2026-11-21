// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The printed wording is interface: these compare whole outputs, not fragments.
use sc_3mf::model::ObjectType;
use sc_mesh_core::indexed_mesh::{
    FaceDiagnosis, MeshDiagnosis, MeshEdgeManifoldError, MeshFix, MeshNormalVectorError,
    MeshVertexError, RepairRefusal,
};
use sc_mesh_core::primitives::{FaceIndex, Normal, VertexIndex};
use sc_mesh_core::transform::Transform3D;
use sc_mesh_formats::messages::{
    LoadMeshWarning, LoadObjWarning, LoadStlWarning, LoadThreeMfWarning, SceneLoadWarning,
    SharedVertexLeaf,
};
use sc_scene::repair::SceneRepairOutcome;
use sc_scene::mesh_targets::MeshTarget;
use sc_scene::{NodeId, Scene};

use super::*;

fn printed(write: impl FnOnce(&mut Vec<u8>)) -> String {
    let mut bytes = Vec::new();
    write(&mut bytes);
    String::from_utf8(bytes).unwrap()
}

fn nodes(count: usize) -> Vec<NodeId> {
    let mut scene = Scene::new();
    (0..count)
        .map(|_| scene.add_group(None, Transform3D::Identity))
        .collect()
}

fn vertex_error(vertex_idx: usize) -> MeshVertexError {
    MeshVertexError::NaNCoordinate {
        vertex_idx,
        coord: sc_mesh_core::primitives::CoordAxis::Y,
    }
}

#[test]
fn reader_warnings_print_one_exact_line_each() {
    let ids = nodes(2);
    let obj = [
        LoadObjWarning::NoVertices,
        LoadObjWarning::NoFaces,
        LoadObjWarning::SkippedTextureCoordinates { line: 3 },
        LoadObjWarning::SkippedVertexNormal { line: 4 },
        LoadObjWarning::SkippedGroup {
            line: 5,
            name: "hull".into(),
        },
        LoadObjWarning::SkippedSmoothingGroup { line: 6 },
        LoadObjWarning::SkippedMaterialReference {
            line: 7,
            material: "grey".into(),
        },
        LoadObjWarning::SkippedMaterialLibrary {
            line: 8,
            path: "a.mtl".into(),
        },
        LoadObjWarning::SkippedComment { line: 9 },
        LoadObjWarning::SkippedUnknownDirective {
            line: 10,
            directive: "curv".into(),
        },
    ];
    let threemf = [
        LoadThreeMfWarning::MaterialsDropped { group_count: 2 },
        LoadThreeMfWarning::UnbuiltObjectSkipped {
            id: 4,
            name: Some("lid".into()),
        },
        LoadThreeMfWarning::UnbuiltObjectSkipped { id: 5, name: None },
        LoadThreeMfWarning::NonModelObjectTypeFlattened {
            id: 6,
            object_type: ObjectType::Support,
        },
        LoadThreeMfWarning::SliceDataIgnored { stack_count: 3 },
        LoadThreeMfWarning::EmptyBuild,
    ];
    let mut all = vec![LoadMeshWarning::Stl(LoadStlWarning::HeaderNotText)];
    all.extend(obj.into_iter().map(LoadMeshWarning::Obj));
    all.extend(threemf.iter().cloned().map(LoadMeshWarning::ThreeMf));
    all.push(LoadMeshWarning::Scene(SceneLoadWarning::SharedVertices {
        leaves: vec![
            SharedVertexLeaf {
                node_id: ids[0],
                object_name: Some("cap".into()),
            },
            SharedVertexLeaf {
                node_id: ids[1],
                object_name: None,
            },
        ],
    }));
    let expected = format!(
        "WARNING: binary STL header is not readable text; no mesh name was taken from it\n\
WARNING: Loaded OBJ file has no vertices\n\
WARNING: Loaded OBJ file has no faces\n\
WARNING (line 3): skipped texture coordinates\n\
WARNING (line 4): skipped vertex normal\n\
WARNING (line 5): Skipped group 'hull'\n\
WARNING (line 6): Skipped smoothing group\n\
WARNING (line 7): Skipped material reference 'grey'\n\
WARNING (line 8): Skipped material library 'a.mtl'\n\
WARNING (line 9): Skipped comment\n\
WARNING (line 10): Skipped OBJ directive 'curv'\n\
WARNING: Dropped 2 material group(s); materials are not supported yet\n\
WARNING: Skipped object 'lid' (id 4): it is not referenced by the build\n\
WARNING: Skipped object id 5: it is not referenced by the build\n\
WARNING: Object 6 has type 'support'; it was loaded as plain model geometry\n\
WARNING: Ignored 3 slice stack(s); the full-resolution meshes were loaded instead\n\
WARNING: The 3MF build section lists no items\n\
WARNING: Objects share at least one vertex coordinate: 'cap', node {}\n",
        ids[1].get()
    );
    assert_eq!(printed(|out| display_warnings(&all, out)), expected);
    assert_eq!(printed(|out| display_warnings(&[], out)), "");
    assert_eq!(
        printed(|out| display_threemf_warning(&threemf[5], out)),
        "WARNING: The 3MF build section lists no items\n"
    );
}

#[test]
fn a_diagnosis_prints_only_the_lists_that_have_entries() {
    let clean = MeshDiagnosis {
        vertex_errors: vec![],
        faces: FaceDiagnosis::Valid {
            normal_vector_errors: vec![],
            edge_manifold_errors: vec![],
        },
    };
    assert_eq!(printed(|out| show_diagnosis(&clean, out)), "");

    let face = FaceIndex::from(2_usize);
    let normal = MeshNormalVectorError::ZeroAreaTriangle { face_idx: face };
    let open = MeshDiagnosis {
        vertex_errors: vec![vertex_error(1)],
        faces: FaceDiagnosis::Valid {
            normal_vector_errors: vec![normal.clone()],
            edge_manifold_errors: vec![MeshEdgeManifoldError::EmptyMesh],
        },
    };
    assert_eq!(
        printed(|out| show_diagnosis(&open, out)),
        format!(
            "Vertex errors (1):\n\t- {}\nNormal vector errors (1):\n\t- {normal}\nManifold errors (1):\n\t- {}\n",
            vertex_error(1),
            MeshEdgeManifoldError::EmptyMesh
        )
    );

    // Invalid faces replace the two lists that need valid faces, even when empty.
    let broken = MeshDiagnosis {
        vertex_errors: vec![],
        faces: FaceDiagnosis::Invalid(vec![]),
    };
    assert_eq!(
        printed(|out| show_diagnosis(&broken, out)),
        "Face errors (0):\n"
    );
}

fn outcome(node: NodeId, refusal: Option<RepairRefusal>) -> SceneRepairOutcome {
    SceneRepairOutcome {
        target: MeshTarget::InlineLeaf(node),
        fixes: vec![],
        refusal,
        edge_manifold_after: false,
    }
}

#[test]
fn refusals_are_counted_and_numbered_only_among_several_meshes() {
    let ids = nodes(3);
    let lone = [outcome(
        ids[0],
        Some(RepairRefusal::UnfixableFaceErrors(vec![])),
    )];
    let mut count = 0;
    assert_eq!(
        printed(|out| count = show_refusal_warnings(&lone, out)),
        "warning: Unable to fix mesh. Face errors correction is not implemented (yet).\n"
    );
    assert_eq!(count, 1);

    let several = [
        outcome(ids[0], None),
        outcome(
            ids[1],
            Some(RepairRefusal::InvalidVertices(vec![vertex_error(7)])),
        ),
        outcome(ids[2], Some(RepairRefusal::UnfixableFaceErrors(vec![]))),
    ];
    assert_eq!(
        printed(|out| count = show_refusal_warnings(&several, out)),
        format!(
            "Vertex errors (1):\n\t- {}\n\
warning (mesh 2 of 3): Unable to fix mesh, there are invalid vertices.\n\
warning (mesh 3 of 3): Unable to fix mesh. Face errors correction is not implemented (yet).\n",
            vertex_error(7)
        )
    );
    assert_eq!(count, 2);
    assert_eq!(
        printed(|out| count = show_refusal_warnings(&several[..1], out)),
        ""
    );
    assert_eq!(count, 0);
}

#[test]
fn applied_fixes_print_exact_lines() {
    let face = |index: usize| FaceIndex::from(index);
    let fixes = [
        MeshFix::RemovedZeroLengthEdgeFace { face_idx: face(4) },
        MeshFix::FlippedFaceOrientation { face_idx: face(5) },
        MeshFix::RecomputedNormal {
            face_idx: face(6),
            old_normal: Normal::new([1.0, 0.0, 0.0]),
            new_normal: Normal::new([0.0, 1.0, 0.0]),
        },
        MeshFix::RemovedDuplicateFace { face_idx: face(7) },
        MeshFix::DetachedSharedVertex {
            vertex: VertexIndex::from(8_u32),
        },
    ];
    assert_eq!(
        printed(|out| show_applied_fixes(&fixes, out)),
        "Applied fixes (5):\n\
\t- Removed face with ID '4' (had zero-length edge)\n\
\t- Flipped orientation of face with ID '5'\n\
\t- Recomputed normal vector for face with ID '6', from [1.0, 0.0, 0.0] (with norm 1) to [0.0, 1.0, 0.0] (with norm 1)\n\
\t- Removed duplicate face with ID '7'\n\
\t- Gave a separate shell its own copy of vertex '8'\n"
    );
    assert_eq!(
        printed(|out| show_applied_fixes(&[], out)),
        "Applied fixes (0):\n"
    );
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_scene::{LeafPayload, node::SceneMetadata};

#[test]
fn regeneration_removes_nested_supports_and_preserves_model_edits_without_double_lift() {
    let mut scene = Scene::new();
    let root = scene.add_group(
        None,
        Transform3D::Affine(AffineTransform3D::from_translation([2.0, 0.0, 5.0])),
    );
    scene.add_root(root).unwrap();
    let mesh = Arc::new(sc_mesh_fixtures::unit_cube_mesh());
    let model = scene
        .add_leaf(None, Transform3D::Identity, LeafPayload::Mesh(mesh.clone()))
        .unwrap();
    scene.add_child(root, model).unwrap();
    let support = scene
        .add_leaf(
            Some(SceneMetadata {
                name: Some("Support 1".into()),
                comment: Some("sc-generated-support".into()),
            }),
            Transform3D::Identity,
            LeafPayload::Mesh(mesh),
        )
        .unwrap();
    scene.add_child(root, support).unwrap();
    let source = unlifted_model(&scene, 5.0);
    assert!(support_ids(&source).is_empty());
    let bounds = source
        .merge_meshes()
        .unwrap()
        .unwrap()
        .local_aabb()
        .unwrap();
    assert!((bounds.min()[0] - 1.5).abs() < 1e-6);
    assert!((bounds.min()[2] + 0.5).abs() < 1e-6);
}

#[test]
fn manual_surface_contact_lifts_once_builds_raft_and_refuses_a_blocked_shaft() {
    let mut source = Scene::new();
    let leaf = source
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_translation([0.0, 0.0, 0.5])),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    source.add_root(leaf).unwrap();
    let before = source.clone();
    let settings = SupportSettings::default();
    let supported = manual_contact_scene(&source, [0.0, 0.0, 0.0], &settings, true).unwrap();
    assert_eq!(support_ids(&supported).len(), 2); // contact and raft
    assert_eq!(source, before);
    assert_eq!(
        sc_scene::bake_leaf_to_world(&supported, leaf)
            .unwrap()
            .local_aabb()
            .unwrap()
            .min()[2],
        5.0
    );
    assert!(manual_contact_scene(&source, [0.0, 0.0, 1.0], &settings, true).is_err());
    let second = manual_contact_scene(&supported, [0.25, 0.0, 5.0], &settings, false).unwrap();
    assert_eq!(
        sc_scene::bake_leaf_to_world(&second, leaf)
            .unwrap()
            .local_aabb()
            .unwrap()
            .min()[2],
        5.0
    );
    assert_eq!(support_ids(&second).len(), 3);
    let rafts: Vec<_> = support_ids(&second)
        .into_iter()
        .filter(|&id| {
            second
                .node(id)
                .unwrap()
                .meta
                .as_ref()
                .unwrap()
                .comment
                .as_ref()
                .unwrap()
                .starts_with("sc-generated-support-raft")
        })
        .collect();
    assert_eq!(rafts.len(), 1);
    assert_eq!(
        crate::scene_document::support_owner(&second, rafts[0]),
        Some(leaf)
    );
}

#[test]
fn support_snapshot_repairs_collapsed_faces_for_auto_and_manual_contacts() {
    let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
    let mut collapsed = mesh.faces[0];
    collapsed.vertices[1] = collapsed.vertices[0];
    mesh.faces.push(collapsed);
    let mut source = Scene::new();
    let leaf = source
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_translation([0.0, 0.0, 0.5])),
            LeafPayload::Mesh(Arc::new(mesh)),
        )
        .unwrap();
    source.add_root(leaf).unwrap();
    let before = source.clone();
    let settings = SupportSettings::default();
    assert!(sc_print_glue::supports::geometric_supported_scene(&source, &settings).is_err());
    assert!(
        sc_print_glue::supports::geometric_supported_scene(
            &repaired_support_source(&source),
            &settings
        )
        .is_ok()
    );
    assert!(manual_contact_scene(&source, [0.0, 0.0, 0.0], &settings, true).is_ok());
    assert_eq!(source, before);
}

#[test]
fn r14_lattice_and_stale_project_fixture_round_trip() {
    use crate::scene_document::{SceneDocument, SceneEdit};
    let mut source = Scene::new();
    let transform = Transform3D::Affine(AffineTransform3D::from_translation([0.0, 0.0, 10.1]))
        .compose(&Transform3D::Affine(AffineTransform3D::from_uniform_scale(
            20.0,
        )));
    let model = source
        .add_leaf(
            None,
            transform,
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    source.add_root(model).unwrap();
    for stale in [false, true] {
        let settings = SupportSettings {
            lattice_raft: !stale,
            ..Default::default()
        };
        let (generated, _) =
            sc_print_glue::supports::geometric_supported_scene(&source, &settings).unwrap();
        let doc = SceneDocument::new();
        doc.take_mesh_scene(source.clone(), std::path::PathBuf::from("fixture.obj"));
        doc.apply_support_scene(&generated, &[model]).unwrap();
        if stale {
            let before = doc
                .read_scene(|s| s.leaf(model).unwrap().local_transform())
                .unwrap();
            let rotate = Transform3D::Affine(AffineTransform3D::from_rotation_z(0.3));
            let after = rotate.compose(&before);
            doc.commit_edit(SceneEdit::SetLocalTransform {
                node_id: model,
                before,
                after,
            })
            .unwrap();
        }
        let scene = doc.read_scene(|s| s.graph().clone()).unwrap();
        assert_eq!(
            support_ids(&scene)
                .iter()
                .any(|&id| crate::scene_document::support_is_stale(&scene, id)),
            stale
        );
        let temp = tempfile::tempdir().unwrap();
        let name = if stale { "stale" } else { "lattice" };
        let path = std::env::var_os("SC_R14_FIXTURES")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| temp.path().to_path_buf())
            .join(format!("{name}.3mf"));
        let mut file = std::fs::File::create(&path).unwrap();
        sc_project_formats::write_project(
            &scene,
            &sc_project_formats::ProjectPayload::default(),
            sc_project_formats::EmbedProfiles::No,
            None,
            None,
            sc_project_formats::ProjectFileType::ThreeMf,
            &mut file,
        )
        .unwrap();
        drop(file);
        let loaded = sc_project_formats::load_scene(&path, None, |_| {})
            .unwrap()
            .scene;
        assert_eq!(
            support_ids(&loaded)
                .iter()
                .any(|&id| crate::scene_document::support_is_stale(&loaded, id)),
            stale
        );
    }
}

#[test]
fn r14b_painted_project_fixture_round_trip() {
    let mut scene = Scene::new();
    let comment =
        "sc-support-paint -5 -3 20 5 1\nsc-support-paint 5 0 20 6 0\nsc-support-paint 0 0 0 3 1\n";
    let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
    for v in &mut mesh.vertices {
        *v = sc_mesh_core::Vertex::new([v[0] * 20.0, v[1] * 20.0, v[2] * 20.0 + 10.0]);
    }
    let id = scene
        .add_leaf(
            Some(SceneMetadata {
                name: Some("Painted keep and avoid".into()),
                comment: Some(comment.into()),
            }),
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(mesh)),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = std::env::var_os("SC_R14B_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| temp.path().into())
        .join("painted.3mf");
    let mut file = std::fs::File::create(&path).unwrap();
    sc_project_formats::write_project(
        &scene,
        &sc_project_formats::ProjectPayload::default(),
        sc_project_formats::EmbedProfiles::No,
        None,
        None,
        sc_project_formats::ProjectFileType::ThreeMf,
        &mut file,
    )
    .unwrap();
    drop(file);
    let loaded = sc_project_formats::load_scene(&path, None, |_| {})
        .unwrap()
        .scene;
    let id = loaded.first_leaf_id().unwrap();
    assert_eq!(
        loaded
            .node(id)
            .unwrap()
            .meta
            .as_ref()
            .unwrap()
            .comment
            .as_deref(),
        Some(comment)
    );
}

#[test]
fn paint_edit_uses_model_coordinates_preserves_metadata_and_undoes() {
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            Some(SceneMetadata {
                name: Some("Model".into()),
                comment: Some("original".into()),
            }),
            Transform3D::Affine(
                AffineTransform3D::from_translation([10.0, 0.0, 0.0])
                    .compose(&AffineTransform3D::from_non_uniform_scale([2.0; 3])),
            ),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let doc = SceneDocument::new();
    doc.take_mesh_scene(scene.clone(), "paint.obj".into());
    let edit = painted_scene_edit(&scene, id, [10., 0., 1.], 4., false).unwrap();
    doc.commit_edit(edit.clone()).unwrap();
    doc.record_edit(edit);
    doc.read_scene(|s| {
        let comment = s
            .graph()
            .node(id)
            .unwrap()
            .meta
            .as_ref()
            .unwrap()
            .comment
            .as_ref()
            .unwrap();
        assert!(comment.starts_with("original"));
        let regions = sc_print_glue::supports::painted_regions(comment);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].centre, [0., 0., 0.5]);
        assert_eq!(regions[0].radius, 2.);
        assert!(regions[0].keep);
        assert!(painted_scene_edit(s.graph(), id, [10., 0., 1.], 4., false).is_none());
        assert!(painted_scene_edit(s.graph(), id, [10., 0., 1.], 4., true).is_some());
    })
    .unwrap();
    doc.step_back().unwrap();
    assert_eq!(doc.read_scene(|s| s.graph().clone()).unwrap(), scene);
    doc.step_forward().unwrap();
    assert!(
        doc.read_scene(|s| s
            .graph()
            .node(id)
            .unwrap()
            .meta
            .as_ref()
            .unwrap()
            .comment
            .as_ref()
            .unwrap()
            .contains("sc-support-paint"))
            .unwrap()
    );
}

#[test]
fn r22_regeneration_keeps_direct_neighbour_on_the_bed() {
    let mut source = Scene::new();
    let mut ids = Vec::new();
    for (x, size) in [(-20.0, 10.0), (20.0, 1.0)] {
        let transform = Transform3D::Affine(AffineTransform3D::from_translation([x, 0.0, 0.5]))
            .compose(&Transform3D::Affine(
                AffineTransform3D::from_non_uniform_scale([size, size, 1.0]),
            ));
        let id = source
            .add_leaf(
                None,
                transform,
                LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
            )
            .unwrap();
        source.add_root(id).unwrap();
        ids.push(id);
    }
    let (supported, _) =
        sc_print_glue::supports::geometric_supported_scene(&source, &Default::default()).unwrap();
    let reset = unlifted_model(&supported, 5.0);
    for id in ids {
        assert_eq!(
            sc_scene::bake_leaf_to_world(&reset, id).unwrap(),
            sc_scene::bake_leaf_to_world(&source, id).unwrap()
        );
    }
}

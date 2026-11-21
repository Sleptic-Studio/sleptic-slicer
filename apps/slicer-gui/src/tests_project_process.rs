// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
use sc_mesh_core::transform::Transform3D;
use sc_mesh_fixtures::unit_cube_mesh;
use sc_scene::node::LeafPayload;

fn scene(count: usize) -> Scene {
    let mut scene = Scene::new();
    for _ in 0..count {
        let id = scene
            .add_leaf(
                None,
                Transform3D::Identity,
                LeafPayload::Mesh(Arc::new(unit_cube_mesh())),
            )
            .unwrap();
        scene.add_root(id).unwrap();
    }
    scene
}

#[test]
fn single_import_selects_values_and_multiple_imports_show_placeholder() {
    let doc = SceneDocument::new();
    doc.take_mesh_scene(scene(1), PathBuf::from("cube.stl"));
    doc.select_sole_object();
    assert!(doc.main_chosen_node().is_some());
    doc.take_mesh_scene(scene(2), PathBuf::from("two.stl"));
    doc.select_sole_object();
    assert!(doc.main_chosen_node().is_none());
}

#[test]
fn process_and_aa_round_trip_through_real_3mf_and_document_install() {
    let doc = SceneDocument::new();
    let graph = scene(1);
    doc.take_mesh_scene(graph.clone(), PathBuf::from("cube.stl"));
    for (id, value) in [
        (0, "0.04"),
        (1, "2.75"),
        (2, "32"),
        (3, "6"),
        (4, "7"),
        (5, "70"),
        (6, "150"),
        (7, "4"),
        (8, "0.5"),
    ] {
        assert!(crate::process_settings::edit(
            &mut doc.process_overrides.borrow_mut(),
            id,
            value
        ));
    }
    doc.scene_aa.set(Some(sc_print_primitives::AaLevel::X4));
    let payload = doc.project_payload(None, None);
    let mut bytes = vec![];
    sc_project_formats::threemf::write_threemf_project(
        &graph,
        &payload,
        sc_project_formats::EmbedProfiles::No,
        None,
        None,
        &mut bytes,
    )
    .unwrap();
    let loaded =
        sc_project_formats::threemf::read_threemf_project(&mut std::io::Cursor::new(bytes))
            .unwrap();
    let reopened = SceneDocument::new();
    reopened.take_project_scene(
        loaded.scene,
        PathBuf::from("project.3mf"),
        loaded.payload.unwrap(),
    );
    assert_eq!(
        *reopened.process_overrides.borrow(),
        *doc.process_overrides.borrow()
    );
    assert_eq!(reopened.scene_aa.get(), doc.scene_aa.get());
}

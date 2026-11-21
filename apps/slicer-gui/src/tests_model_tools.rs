// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
use crate::scene_document::SceneDocument;
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};

fn fixture() -> (Scene, NodeId, NodeId) {
    let mut scene = Scene::new();
    let model = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_uniform_scale(20.)),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(model).unwrap();
    let support = scene.add_leaf(Some(sc_scene::SceneMetadata {name:Some("Support".into()),comment:Some(format!("sc-generated-support\nposition = [0.0, 0.0, 0.0]\nnormal = [0.0, 0.0, -1.0]\nowner = {}\n",model.get()))}),Transform3D::Identity,LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh()))).unwrap();
    scene.add_root(support).unwrap();
    let mut plates = scene.plates().clone();
    let second = plates.add();
    plates.assignments.insert(model, second);
    scene.set_plates(plates);
    (scene, model, support)
}

#[test]
fn cut_on_plate_two_is_one_undo_and_persists_geometry_staleness() {
    let (scene, model, support) = fixture();
    let original = scene.mesh_at_leaf(model).unwrap().clone();
    let plate = scene.plate_of(model);
    let plan = plan(
        &scene,
        &[model],
        Operation::Cut {
            z: 0.,
            keep: 2,
            cap: true,
        },
    )
    .unwrap();
    let doc = SceneDocument::new();
    doc.take_mesh_scene(scene, "model-tools.3mf".into());
    doc.apply_batch(plan.edit, plan.slots).unwrap();
    let edited = doc.read_scene(|s| s.graph().clone()).unwrap();
    assert_eq!(plan.selected.len(), 2);
    for id in plan.selected {
        assert_eq!(edited.plate_of(id), plate);
        assert!(
            doc.read_scene(|s| s.leaf(id).is_some()).unwrap(),
            "new halves must remain pickable"
        );
    }
    assert!(crate::scene_document::support_is_stale(&edited, support));
    let mut bytes = std::io::Cursor::new(Vec::new());
    sc_project_formats::threemf::write_threemf_project(
        &edited,
        &Default::default(),
        sc_project_formats::EmbedProfiles::No,
        None,
        None,
        &mut bytes,
    )
    .unwrap();
    bytes.set_position(0);
    let loaded = sc_project_formats::threemf::read_threemf_project(&mut bytes)
        .unwrap()
        .scene;
    let loaded_support = crate::auto_support_run::support_ids(&loaded)[0];
    assert!(crate::scene_document::support_is_stale(
        &loaded,
        loaded_support
    ));
    assert!(doc.step_back().unwrap());
    assert_eq!(
        doc.read_scene(|s| s.graph().mesh_at_leaf(model).unwrap().clone())
            .unwrap(),
        original
    );
    assert!(
        !doc.read_scene(|s| crate::scene_document::support_is_stale(s.graph(), support))
            .unwrap()
    );
    assert!(!doc.step_back().unwrap());
    assert!(doc.step_forward().unwrap());
}

#[test]
fn boolean_selection_order_and_refusals_leave_the_original_scene_untouched() {
    let (mut scene, a, _) = fixture();
    let transform = Transform3D::Affine(AffineTransform3D::from_translation([5., 0., 0.])).compose(
        &Transform3D::Affine(AffineTransform3D::from_uniform_scale(20.)),
    );
    let b = scene
        .add_leaf(
            None,
            transform,
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(b).unwrap();
    let mut plates = scene.plates().clone();
    plates.assignments.insert(b, scene.plate_of(a));
    scene.set_plates(plates);
    let plan = plan(&scene, &[b, a], Operation::Boolean(BooleanOp::Subtract)).unwrap();
    let doc = SceneDocument::new();
    doc.take_mesh_scene(scene, "boolean.3mf".into());
    doc.apply_batch(plan.edit, plan.slots).unwrap();
    assert!(doc.read_scene(|s| s.graph().node(a).is_none()).unwrap());
    assert!(doc.step_back().unwrap());
    assert!(doc.read_scene(|s| s.graph().node(a).is_some()).unwrap());
    let original = doc.read_scene(|s| s.graph().clone()).unwrap();
    assert!(super::plan(&original, &[a], Operation::Boolean(BooleanOp::Union)).is_err());
    let after = doc
        .read_scene(|s| s.graph().leaf_mesh_arc(a).unwrap().clone())
        .unwrap();
    assert!(Arc::ptr_eq(original.leaf_mesh_arc(a).unwrap(), &after));
}

#[test]
fn picked_normals_follow_face_geometry_and_mirroring() {
    let mesh = sc_mesh_fixtures::unit_cube_mesh();
    assert_eq!(face_normal(&mesh, [0., 0., 0.5]), Some([0., 0., 1.]));
    assert_eq!(face_normal(&mesh, [0.5, 0., 0.]), Some([1., 0., 0.]));
}

#[test]
fn split_and_label_are_single_undo_steps_with_stale_supports() {
    for split in [false, true] {
        let (mut scene, model, support) = fixture();
        if split {
            let leaf = scene.remove_leaf(model).unwrap();
            let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
            let mut extra = mesh.clone();
            for v in &mut extra.vertices {
                *v = sc_mesh_core::Vertex::new([v[0] + 2., v[1], v[2]]);
            }
            let offset = mesh.vertices.len() as u32;
            mesh.vertices.extend(extra.vertices);
            mesh.faces.extend(extra.faces.into_iter().map(|f| {
                sc_mesh_core::IndexedTriangle::from_raw_indices(
                    f.normal,
                    f.vertices.map(|i| i.get() + offset),
                )
            }));
            scene
                .install_leaf_at(
                    model,
                    leaf.attachment,
                    LeafPayload::Mesh(Arc::new(mesh)),
                    leaf.local_transform,
                    leaf.meta,
                )
                .unwrap();
        }
        let original = scene.leaf_mesh_arc(model).unwrap().clone();
        let operation = if split {
            Operation::Split
        } else {
            Operation::Label {
                text: "A1".into(),
                point: [-8., -8., 10.],
                normal: [0., 0., 1.],
                size: 3.5,
                depth: 0.5,
                engrave: false,
            }
        };
        let plan = plan(&scene, &[model], operation).unwrap();
        let doc = SceneDocument::new();
        doc.take_mesh_scene(scene, "label-split.3mf".into());
        doc.apply_batch(plan.edit, plan.slots).unwrap();
        assert_eq!(plan.selected.len(), if split { 2 } else { 1 });
        assert!(
            doc.read_scene(|s| crate::scene_document::support_is_stale(s.graph(), support))
                .unwrap()
        );
        assert!(doc.step_back().unwrap());
        assert!(
            doc.read_scene(|s| Arc::ptr_eq(s.graph().leaf_mesh_arc(model).unwrap(), &original))
                .unwrap()
        );
        assert!(!doc.step_back().unwrap());
        assert!(doc.step_forward().unwrap());
    }
}

#[test]
fn cut_parts_preserve_parent_transforms_and_plate_membership() {
    let (mut scene, model, _) = fixture();
    let leaf = scene.remove_leaf(model).unwrap();
    let group = scene.add_group(
        None,
        Transform3D::Affine(AffineTransform3D::from_translation([30., 5., 25.])),
    );
    scene.add_root(group).unwrap();
    scene
        .install_leaf_at(
            model,
            LeafAttachment::Parent {
                parent: group,
                position: 0,
            },
            leaf.payload,
            leaf.local_transform,
            leaf.meta,
        )
        .unwrap();
    let plate = scene.plate_of(model);
    let plan = plan(
        &scene,
        &[model],
        Operation::Cut {
            z: 25.,
            keep: 2,
            cap: true,
        },
    )
    .unwrap();
    let doc = SceneDocument::new();
    doc.take_mesh_scene(scene, "parent-cut.3mf".into());
    doc.apply_batch(plan.edit, plan.slots).unwrap();
    for id in plan.selected {
        doc.read_scene(|s| {
            assert_eq!(s.graph().plate_of(id), plate);
            let b = sc_scene::bake_leaf_to_world(s.graph(), id)
                .unwrap()
                .local_aabb()
                .unwrap();
            assert!((b.min()[0] - 20.).abs() < 1e-5 && (b.max()[0] - 40.).abs() < 1e-5);
            assert!(s.leaf(id).is_some());
        })
        .unwrap();
    }
    assert!(doc.step_back().unwrap());
    assert!(!doc.step_back().unwrap());
}

#[test]
fn model_edits_preserve_names_but_drop_the_obsolete_hollow_source() {
    let (mut scene, model, _) = fixture();
    let leaf = scene.remove_leaf(model).unwrap();
    let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
    mesh.meta = Some(sc_mesh_core::indexed_mesh::MeshMetadata {
        name: Some("Original name".into()),
        header_comment: Some("sleptic-hollow-v1\nobsolete source".into()),
    });
    scene
        .install_leaf_at(
            model,
            leaf.attachment,
            LeafPayload::Mesh(Arc::new(mesh)),
            leaf.local_transform,
            Some(sc_scene::SceneMetadata {
                name: Some("Model name".into()),
                comment: Some("User comment\nsc-support-paint 0 0 0 2 1\n".into()),
            }),
        )
        .unwrap();
    let plan = plan(
        &scene,
        &[model],
        Operation::Cut {
            z: 0.,
            keep: 2,
            cap: true,
        },
    )
    .unwrap();
    let doc = SceneDocument::new();
    doc.take_mesh_scene(scene, "metadata.3mf".into());
    doc.apply_batch(plan.edit, plan.slots).unwrap();
    doc.read_scene(|s| {
        let meta = s
            .graph()
            .mesh_at_leaf(model)
            .unwrap()
            .meta
            .as_ref()
            .unwrap();
        assert_eq!(meta.name.as_deref(), Some("Model name"));
        assert!(meta.header_comment.is_none());
        assert_eq!(
            s.graph()
                .node(model)
                .unwrap()
                .meta
                .as_ref()
                .unwrap()
                .comment
                .as_deref(),
            Some("User comment")
        );
    })
    .unwrap();
    assert!(doc.step_back().unwrap());
    assert!(
        doc.read_scene(|s| s
            .graph()
            .mesh_at_leaf(model)
            .unwrap()
            .meta
            .as_ref()
            .unwrap()
            .header_comment
            .is_some())
            .unwrap()
    );
}

#[test]
#[ignore = "writes R15 desktop acceptance source projects"]
fn r15_model_tool_acceptance_sources() {
    let folder = std::path::PathBuf::from(
        std::env::var_os("SC_R15_GUI_FIXTURES").expect("set SC_R15_GUI_FIXTURES"),
    );
    let cube = |size: [f32; 3], centre: [f32; 3]| {
        let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
        for v in &mut mesh.vertices {
            *v = sc_mesh_core::Vertex::new(std::array::from_fn(|i| v[i] * size[i] + centre[i]));
        }
        mesh
    };
    for name in ["cut", "split", "label", "boolean"] {
        let mut scene = Scene::new();
        let meshes = match name {
            "cut" => vec![cube([30., 24., 20.], [0., 0., 10.])],
            "label" => vec![cube([40., 24., 3.], [0., 0., 1.5])],
            "boolean" => vec![
                cube([20.; 3], [0., 0., 10.]),
                cube([12., 12., 30.], [6., -6., 15.]),
            ],
            _ => {
                let mut a = cube([10.; 3], [-12., 0., 5.]);
                let b = cube([10.; 3], [12., 0., 5.]);
                let offset = a.vertices.len() as u32;
                a.vertices.extend(b.vertices);
                a.faces.extend(b.faces.into_iter().map(|f| {
                    sc_mesh_core::IndexedTriangle::from_raw_indices(
                        f.normal,
                        f.vertices.map(|i| i.get() + offset),
                    )
                }));
                vec![a]
            }
        };
        for (i, mesh) in meshes.into_iter().enumerate() {
            let id = scene
                .add_leaf(
                    Some(sc_scene::SceneMetadata {
                        name: Some(format!("{} {}", name, i + 1)),
                        comment: None,
                    }),
                    Transform3D::Identity,
                    LeafPayload::Mesh(Arc::new(mesh)),
                )
                .unwrap();
            scene.add_root(id).unwrap();
        }
        let mut file = std::fs::File::create(folder.join(format!("source-{name}.3mf"))).unwrap();
        sc_project_formats::threemf::write_threemf_project(
            &scene,
            &Default::default(),
            sc_project_formats::EmbedProfiles::No,
            None,
            None,
            &mut file,
        )
        .unwrap();
    }
}

#[test]
fn simplify_undo_restores_exact_original_mesh_and_supports() {
    let (scene, model, support) = fixture();
    let original = scene.mesh_at_leaf(model).unwrap().clone();
    let plan = plan(
        &scene,
        &[model],
        Operation::Simplify {
            target: 50.0,
            percentage: true,
            deviation: 0.02,
        },
    )
    .unwrap();
    assert!(plan.summary.as_ref().unwrap().contains("triangles"));
    let doc = SceneDocument::new();
    doc.take_mesh_scene(scene, "simplify.3mf".into());
    doc.apply_batch(plan.edit, plan.slots).unwrap();
    assert!(
        doc.read_scene(|s| crate::scene_document::support_is_stale(s.graph(), support))
            .unwrap()
    );
    doc.step_back().unwrap();
    let restored = doc
        .read_scene(|s| s.graph().mesh_at_leaf(model).unwrap().clone())
        .unwrap();
    assert_eq!(original.vertices, restored.vertices);
    assert_eq!(original.faces, restored.faces);
    assert!(
        doc.read_scene(|s| s.graph().node(support).is_some())
            .unwrap()
    );
}

#[test]
fn simplify_can_stage_on_a_worker_and_cancel_without_editing_the_scene() {
    let (scene, model, support) = fixture();
    let original = scene.mesh_at_leaf(model).unwrap().clone();
    let worker_scene = scene.clone();
    let result = std::thread::spawn(move || {
        plan_with_progress(
            &worker_scene,
            &[model],
            Operation::Simplify {
                target: 50.0,
                percentage: true,
                deviation: 0.02,
            },
            |_| false,
        )
    })
    .join()
    .unwrap();
    assert!(result.is_err());
    assert_eq!(
        scene.mesh_at_leaf(model).unwrap().vertices,
        original.vertices
    );
    assert_eq!(scene.mesh_at_leaf(model).unwrap().faces, original.faces);
    assert!(!crate::scene_document::support_is_stale(&scene, support));
}

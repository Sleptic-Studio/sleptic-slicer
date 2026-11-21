// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
use sc_scene::node::LeafPayload;
use std::sync::Arc;

#[test]
fn r13_acceptance_project_round_trip_and_fixture() {
    let mut graph = Scene::new();
    let mut table = graph.plates().clone();
    let ids = [1, table.add(), table.add()];
    for (i, plate) in ids.into_iter().enumerate() {
        let mesh = if i == 1 {
            sc_mesh_fixtures::cuboid_with_z_tunnel()
        } else {
            sc_mesh_fixtures::unit_cube_mesh()
        };
        let scale = [20.0, 12.0, 28.0][i];
        let transform = Transform3D::Affine(AffineTransform3D::from_translation([
            -scale / 2.0,
            -scale / 2.0,
            0.0,
        ]))
        .compose(&Transform3D::Affine(AffineTransform3D::from_uniform_scale(
            scale,
        )));
        let leaf = graph
            .add_leaf(
                Some(sc_scene::SceneMetadata {
                    name: Some(format!("Part {}", i + 1)),
                    comment: None,
                }),
                transform,
                LeafPayload::Mesh(Arc::new(mesh)),
            )
            .unwrap();
        graph.add_root(leaf).unwrap();
        let (min, max) = bounds(&points(&graph, leaf).unwrap()).unwrap();
        let delta = Transform3D::Affine(AffineTransform3D::from_translation([
            -(min.x + max.x) / 2.0,
            -(min.y + max.y) / 2.0,
            -min.z,
        ]));
        world_edit(&graph, leaf, delta)
            .unwrap()
            .apply_to_scene(&mut graph)
            .unwrap();
        table.assignments.insert(leaf, plate);
    }
    graph.set_plates(table);
    let temporary = tempfile::tempdir().unwrap();
    let path = std::env::var_os("SC_R13_FIXTURE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| temporary.path().join("three-plates.3mf"));
    let mut file = std::fs::File::create(&path).unwrap();
    sc_project_formats::write_project(
        &graph,
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
    assert_eq!(loaded.plates().order.len(), 3);
    let leaves = loaded
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .unwrap();
    assert_eq!(
        leaves
            .iter()
            .map(|id| loaded.plate_of(*id))
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
}

#[test]
fn crossing_plate_boundary_normalizes_geometry_and_undoes_assignment() {
    let (graph, model) = scene();
    let doc = crate::scene_document::SceneDocument::new();
    doc.take_mesh_scene(graph, "cube.stl".into());
    doc.add_plate();
    doc.set_active_plate(1);
    doc.choose(crate::selection::Selection::One(crate::selection::LeafId(
        model,
    )));
    let before = doc
        .read_scene(|s| s.graph().node(model).unwrap().local_transform)
        .unwrap();
    // Exactly one plate pitch to the right: plate 2's centre.
    let pitch = 192.0 + f64::from(sc_scene::PLATE_GAP_MM);
    let shift = Transform3D::Affine(AffineTransform3D::from_translation([pitch, 0.0, 0.0]));
    let edit = doc
        .read_scene(|s| world_edit(s.graph(), model, shift))
        .flatten()
        .unwrap();
    doc.commit_edit(edit.clone()).unwrap();
    let mut edits = vec![edit];
    doc.finish_plate_drag(&mut edits, model);
    doc.record_edit(SceneEdit::Compound(edits));
    assert_eq!(doc.read_scene(|s| s.graph().plate_of(model)), Some(2));
    let after = doc
        .read_scene(|s| s.graph().node(model).unwrap().local_transform)
        .unwrap();
    for coords in [[0.0, 0.0, 0.0], [1.0, 2.0, 3.0]] {
        let vertex = sc_mesh_core::Vertex::new(coords);
        for (a, b) in before
            .apply_to_vertex(vertex)
            .coords()
            .into_iter()
            .zip(after.apply_to_vertex(vertex).coords())
        {
            assert!((a - b).abs() < 1e-6);
        }
    }
    doc.step_back().unwrap();
    assert_eq!(doc.read_scene(|s| s.graph().plate_of(model)), Some(1));
    doc.step_forward().unwrap();
    assert_eq!(doc.read_scene(|s| s.graph().plate_of(model)), Some(2));
}

#[test]
fn multi_plate_overflow_groups_membership_and_undo() {
    let (mut graph, first) = scene();
    let mut models = vec![first];
    for _ in 0..2 {
        let id = graph
            .add_leaf(
                None,
                Transform3D::Identity,
                LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
            )
            .unwrap();
        graph.add_root(id).unwrap();
        models.push(id);
    }
    let support = graph
        .add_leaf(
            Some(sc_scene::SceneMetadata {
                name: Some("Support".into()),
                comment: Some(format!("sc-generated-support\nowner = {}\n", first.get())),
            }),
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    graph.add_root(support).unwrap();
    let original = graph.clone();
    let edit = arrange_all(&graph, BuildVolume::new(2.0, 2.0, 3.0), 3.0).unwrap();
    edit.apply_to_scene(&mut graph).unwrap();
    assert_eq!(graph.plates().order.len(), 3);
    assert_eq!(graph.plate_of(support), graph.plate_of(first));
    let memberships: std::collections::BTreeSet<_> =
        models.iter().map(|id| graph.plate_of(*id)).collect();
    assert_eq!(memberships.len(), 3);
    for p in &graph.plates().order {
        let isolated = graph.plate_scene(p.id).unwrap();
        assert!(
            isolated
                .reachable_leaves(
                    sc_scene::CsgPolicy::Descend,
                    sc_scene::ReferencePolicy::Skip
                )
                .unwrap()
                .iter()
                .all(|id| graph.plate_of(*id) == p.id)
        );
    }
    edit.inverse().apply_to_scene(&mut graph).unwrap();
    assert_eq!(graph, original);
    edit.apply_to_scene(&mut graph).unwrap();
    assert_eq!(graph.plates().order.len(), 3);
}

#[test]
fn move_to_plate_undo_redo_and_reorder_preserve_implicit_membership() {
    let (graph, model) = scene();
    let doc = crate::scene_document::SceneDocument::new();
    doc.take_mesh_scene(graph, "cube.stl".into());
    doc.add_plate();
    let second = doc.active_plate();
    doc.choose(crate::selection::Selection::One(crate::selection::LeafId(
        model,
    )));
    doc.move_selection_to_plate(second);
    assert_eq!(doc.read_scene(|s| s.graph().plate_of(model)), Some(second));
    doc.set_active_plate(1);
    doc.choose(doc.chosen());
    assert_eq!(doc.active_plate(), second);
    doc.step_back().unwrap();
    assert_eq!(doc.read_scene(|s| s.graph().plate_of(model)), Some(1));
    doc.step_forward().unwrap();
    assert_eq!(doc.read_scene(|s| s.graph().plate_of(model)), Some(second));
    doc.step_back().unwrap();
    doc.edit_plates(|p| p.order.reverse());
    assert_eq!(doc.read_scene(|s| s.graph().plate_of(model)), Some(1));
}

#[test]
fn batch_plan_is_one_undo_step_and_keeps_shared_meshes() {
    let (source, _) = scene();
    let prepared = crate::batch_prepare::plan(
        Scene::new(),
        &[source.clone(), source.clone(), source],
        BuildVolume::new(2.0, 2.0, 3.0),
    )
    .unwrap();
    let doc = crate::scene_document::SceneDocument::new();
    doc.apply_batch(prepared.edit, prepared.node_slots).unwrap();
    assert_eq!(doc.read_scene(|s| s.graph().plates().order.len()), Some(3));
    assert_eq!(doc.read_scene(|s| s.graph().roots().len()), Some(3));
    doc.step_back().unwrap();
    assert_eq!(doc.read_scene(|s| s.graph().roots().len()), Some(0));
    assert!(!doc.has_past());
    doc.step_forward().unwrap();
    assert_eq!(doc.read_scene(|s| s.graph().roots().len()), Some(3));
}

#[test]
fn plate_settings_switch_independently_and_locked_plate_is_not_arranged() {
    let (graph, model) = scene();
    let doc = crate::scene_document::SceneDocument::new();
    doc.take_mesh_scene(graph, "cube.stl".into());
    doc.process_overrides.borrow_mut().layer_height_mm =
        Some(sc_print_primitives::PositiveFiniteMm::new(0.1).unwrap());
    doc.process_changed();
    doc.add_plate();
    doc.process_overrides.borrow_mut().layer_height_mm =
        Some(sc_print_primitives::PositiveFiniteMm::new(0.2).unwrap());
    doc.process_changed();
    doc.step_back().unwrap();
    assert_eq!(
        doc.process_overrides
            .borrow()
            .layer_height_mm
            .unwrap()
            .value(),
        0.1
    );
    doc.step_forward().unwrap();
    assert_eq!(
        doc.process_overrides
            .borrow()
            .layer_height_mm
            .unwrap()
            .value(),
        0.2
    );
    doc.set_active_plate(1);
    assert_eq!(
        doc.process_overrides
            .borrow()
            .layer_height_mm
            .unwrap()
            .value(),
        0.1
    );
    doc.set_active_plate(2);
    assert_eq!(
        doc.process_overrides
            .borrow()
            .layer_height_mm
            .unwrap()
            .value(),
        0.2
    );
    doc.edit_plates(|p| p.order[0].locked = true);
    let mut graph = doc.read_scene(|s| s.graph().clone()).unwrap();
    let before = graph.node(model).unwrap().local_transform;
    let edit = arrange_all(&graph, BuildVolume::new(2.0, 2.0, 3.0), 3.0).unwrap();
    edit.apply_to_scene(&mut graph).unwrap();
    assert_eq!(graph.node(model).unwrap().local_transform, before);
}

fn scene() -> (Scene, NodeId) {
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_rotation_y(0.7)),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    (scene, id)
}

#[test]
fn mirror_all_axes_preserves_xy_centre_drops_and_undoes() {
    for axis in 0..3 {
        let (mut scene, id) = scene();
        let original = points(&scene, id).unwrap();
        let (min, max) = bounds(&original).unwrap();
        let edit = mirror(&scene, id, axis).unwrap();
        edit.apply_to_scene(&mut scene).unwrap();
        let mirrored = points(&scene, id).unwrap();
        let (lo, hi) = bounds(&mirrored).unwrap();
        assert!(lo.z.abs() < 1e-5);
        assert!(((lo + hi) - (min + max)).truncate().length() < 1e-5);
        for (a, b) in original.iter().zip(&mirrored) {
            let expected = if axis == 2 {
                min[axis] + max[axis] - a[axis] - min.z
            } else {
                min[axis] + max[axis] - a[axis]
            };
            assert!((b[axis] - expected).abs() < 1e-5);
        }
        edit.inverse().apply_to_scene(&mut scene).unwrap();
        assert_eq!(points(&scene, id).unwrap(), original);
    }
}

#[test]
fn fill_sleptic_plate_has_deterministic_cube_count_and_undo() {
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_uniform_scale(20.0)),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let (edit, count) =
        fill_plate(&mut scene, id, BuildVolume::new(192.0, 120.0, 200.0), 3.0).unwrap();
    assert_eq!(count, 39); // 8 columns x 5 rows, including the original.
    edit.apply_to_scene(&mut scene).unwrap();
    assert_eq!(scene.roots().len(), 40);
    edit.inverse().apply_to_scene(&mut scene).unwrap();
    assert_eq!(scene.roots(), &[id]);
}

#[test]
fn fill_copies_supports_and_reassigns_owners_in_one_undo() {
    let (mut scene, model) = scene();
    let support = scene
        .add_leaf(
            Some(sc_scene::node::SceneMetadata {
                name: Some("support".into()),
                comment: Some(format!("sc-generated-support\nowner = {}\n", model.get())),
            }),
            Transform3D::Affine(AffineTransform3D::from_translation([2.0, 0.0, 0.0])),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(support).unwrap();
    let (edit, count) =
        fill_plate(&mut scene, model, BuildVolume::new(15.0, 10.0, 10.0), 3.0).unwrap();
    assert!(count > 0);
    edit.apply_to_scene(&mut scene).unwrap();
    assert_eq!(scene.roots().len(), 2 * (count + 1));
    for &id in scene.roots() {
        let Some(comment) = scene
            .node(id)
            .and_then(|n| n.meta.as_ref())
            .and_then(|m| m.comment.as_ref())
            .filter(|c| c.starts_with("sc-generated-support"))
        else {
            continue;
        };
        let owner = comment
            .lines()
            .find_map(|line| line.strip_prefix("owner = "))
            .unwrap()
            .parse::<u32>()
            .unwrap();
        let owner = *scene.roots().iter().find(|m| m.get() == owner).unwrap();
        let (a, b) = bounds(&points(&scene, id).unwrap()).unwrap();
        let (c, d) = bounds(&points(&scene, owner).unwrap()).unwrap();
        assert!((((a + b - c - d) * 0.5).x - 2.0).abs() < 1e-5);
    }
    edit.inverse().apply_to_scene(&mut scene).unwrap();
    assert_eq!(scene.roots(), &[model, support]);
}
#[test]
fn lay_flat_is_an_undoable_rotation_and_drop() {
    let (scene, id) = scene();
    let SceneEdit::SetLocalTransform { before, after, .. } = lay_flat(&scene, id).unwrap() else {
        panic!("transform edit");
    };
    assert_ne!(before, after);
    let mesh = scene.mesh_at_leaf(id).unwrap();
    let points: Vec<DVec3> = mesh
        .vertices
        .iter()
        .map(|v| DVec3::from_array(after.apply_to_vertex(*v).coords().map(f64::from)))
        .collect();
    let (min, max) = bounds(&points).unwrap();
    assert!(min.z.abs() < 1e-5);
    assert!((max.z - min.z - 1.0).abs() < 1e-5);
}
#[test]
fn arrangement_has_spacing_and_refuses_an_overfull_plate() {
    let (mut scene, _) = scene();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let SceneEdit::Compound(edits) = arrange(&scene, BuildVolume::new(50.0, 50.0, 50.0)).unwrap()
    else {
        panic!("compound edit");
    };
    assert_eq!(edits.len(), 2);
    let positions: Vec<_> = edits
        .iter()
        .map(|edit| {
            let SceneEdit::SetLocalTransform { after, .. } = edit else {
                panic!()
            };
            after
                .apply_to_vertex(sc_mesh_core::primitives::Vertex::new([0.0; 3]))
                .coords()
        })
        .collect();
    assert!(positions[1][0] - positions[0][0] >= 3.0);
    assert!(arrange(&scene, BuildVolume::new(1.0, 1.0, 1.0)).is_none());
}

#[test]
fn thin_plate_orientation_is_deterministic_and_fits() {
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_non_uniform_scale([20.0, 20.0, 1.0])),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let first = auto_orient(
        &scene,
        id,
        45.0,
        BuildVolume::new(192.0, 120.0, 200.0),
        sc_print_glue::orient::Preset::Balanced.weights(),
        sc_print_glue::orient::Context::default(),
    )
    .unwrap();
    assert_eq!(
        first,
        auto_orient(
            &scene,
            id,
            45.0,
            BuildVolume::new(192.0, 120.0, 200.0),
            sc_print_glue::orient::Preset::Balanced.weights(),
            sc_print_glue::orient::Context::default()
        )
        .unwrap()
    );
    let SceneEdit::SetLocalTransform { before, after, .. } = first else {
        panic!()
    };
    let _ = before;
    let mesh = scene.mesh_at_leaf(id).unwrap();
    let pts: Vec<_> = mesh
        .vertices
        .iter()
        .map(|v| DVec3::from_array(after.apply_to_vertex(*v).coords().map(f64::from)))
        .collect();
    let (min, max) = bounds(&pts).unwrap();
    // Flat is 1 mm high with a 400 mm2 peel. Vertical is 20 mm high.
    // The selected diagonal trades a much smaller section for less height than vertical.
    assert!(min.z.abs() < 1e-5);
    assert!(max.z >= 1.0 && max.z <= 200.0, "height {}", max.z);
}
#[test]
fn six_cubes_pack_without_overlap_and_report_unfit_ids() {
    let mut scene = Scene::new();
    for _ in 0..6 {
        let id = scene
            .add_leaf(
                None,
                Transform3D::Affine(AffineTransform3D::from_uniform_scale(10.0)),
                LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
            )
            .unwrap();
        scene.add_root(id).unwrap();
    }
    let SceneEdit::Compound(edits) =
        arrange_with_gap(&scene, BuildVolume::new(40.0, 26.0, 20.0), 3.0).unwrap()
    else {
        panic!()
    };
    assert_eq!(edits.len(), 6);
    let mesh = sc_mesh_fixtures::unit_cube_mesh();
    let boxes: Vec<_> = edits
        .iter()
        .map(|e| {
            let SceneEdit::SetLocalTransform { after, .. } = e else {
                panic!()
            };
            bounds(
                &mesh
                    .vertices
                    .iter()
                    .map(|v| DVec3::from_array(after.apply_to_vertex(*v).coords().map(f64::from)))
                    .collect::<Vec<_>>(),
            )
            .unwrap()
        })
        .collect();
    for (i, (min, max)) in boxes.iter().enumerate() {
        assert!(min.x >= -20.0 && max.x <= 20.0 && min.y >= -13.0 && max.y <= 13.0);
        for (other_min, other_max) in &boxes[..i] {
            assert!(
                min.x >= other_max.x + 2.999
                    || other_min.x >= max.x + 2.999
                    || min.y >= other_max.y + 2.999
                    || other_min.y >= max.y + 2.999
            );
        }
    }
    let min = boxes
        .iter()
        .fold(DVec3::splat(f64::INFINITY), |a, b| a.min(b.0));
    let max = boxes
        .iter()
        .fold(DVec3::splat(f64::NEG_INFINITY), |a, b| a.max(b.1));
    assert!((min.x + max.x).abs() < 1e-5);
    assert!((min.y + max.y).abs() < 1e-5);
    let failed = arrange_with_gap(&scene, BuildVolume::new(10.0, 10.0, 20.0), 3.0).unwrap_err();
    assert_eq!(failed.len(), 5);
    assert!(failed.iter().all(|&id| scene.node(id).is_some()));
}
#[test]
fn arrange_keeps_support_relative_to_model() {
    let (mut scene, model) = scene();
    let support = scene
        .add_leaf(
            Some(sc_scene::node::SceneMetadata {
                name: Some("support".into()),
                comment: Some(format!("sc-generated-support\nowner = {}\n", model.get())),
            }),
            Transform3D::Affine(AffineTransform3D::from_translation([2.0, 0.0, 0.0])),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(support).unwrap();
    let SceneEdit::Compound(edits) =
        arrange_with_gap(&scene, BuildVolume::new(50.0, 50.0, 50.0), 3.0).unwrap()
    else {
        panic!()
    };
    let delta: Vec<_> = edits
        .iter()
        .map(|e| {
            let SceneEdit::SetLocalTransform { before, after, .. } = e else {
                panic!()
            };
            after.compose(&before.inverse().unwrap())
        })
        .collect();
    for p in [[0.0; 3], [1.0, 2.0, 3.0]] {
        let v = sc_mesh_core::primitives::Vertex::new(p);
        let a = delta[0].apply_to_vertex(v).coords();
        let b = delta[1].apply_to_vertex(v).coords();
        assert!(a.into_iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5));
    }
}

#[test]
fn machine_import_preserves_plate_centred_pattern_coordinates() {
    let source = sc_print_glue::calibration::machine_scene(
        sc_print_glue::calibration::MachineTool::Orientation,
        [192.0, 120.0, 200.0],
        0.05,
    )
    .unwrap();
    let before = sc_scene::bake_leaf_to_world(&source, source.roots()[0]).unwrap();
    let mut graph = Scene::new();
    let (_, ids) = import_machine_geometry(
        &mut graph,
        &[source],
        BuildVolume::new(192.0, 120.0, 200.0),
        1,
    )
    .unwrap();
    let after = sc_scene::bake_leaf_to_world(&graph, ids[0]).unwrap();
    assert_eq!(before.vertices, after.vertices);
}

#[test]
fn most_per_plate_prediction_matches_actual_fill_plate_packer() {
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_non_uniform_scale([12.0, 8.0, 2.0])),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let volume = sc_print_primitives::BuildVolume::new(60.0, 40.0, 50.0).unwrap();
    let context = sc_print_glue::orient::Context {
        supports: false,
        ..Default::default()
    };
    let proposal = sc_print_glue::orient::search_with_context(
        &scene,
        id,
        volume,
        45.0,
        sc_print_glue::orient::Preset::SmallestFootprint.weights(),
        context,
    )
    .unwrap();
    let (_, original_added) = fill_plate(
        &mut scene.clone(),
        id,
        BuildVolume::new(60.0, 40.0, 50.0),
        3.0,
    )
    .unwrap();
    assert_eq!(original_added + 1, proposal.original.copies);
    world_edit(&scene, id, proposal.delta)
        .unwrap()
        .apply_to_scene(&mut scene)
        .unwrap();
    let (_, chosen_added) =
        fill_plate(&mut scene, id, BuildVolume::new(60.0, 40.0, 50.0), 3.0).unwrap();
    assert_eq!(chosen_added + 1, proposal.chosen.copies);
    assert!(chosen_added >= original_added);
    assert!(proposal.chosen.zero_support);
}

/// R20 captures read "+0 min print" and "1 estimated awkward contacts", and
/// listed "0 estimated awkward contacts" on every clean pose.
#[test]
fn orient_summary_reads_as_plain_words() {
    assert_eq!(
        orient_summary(-41.2, 128.0, 1.0, 1.0, 2),
        "Oriented: 41m shorter print, 128 fewer supports, 1 hard-to-reach support; fits 2 per plate."
    );
    assert_eq!(
        orient_summary(0.3, 12.0, 0.0, 0.0, 1),
        "Oriented: same print time, 12 fewer supports; fits 1 per plate."
    );
    assert_eq!(
        orient_summary(96.2, 48.0, 1.2, 0.0, 6),
        "Oriented: 1h 36m longer print, 48 fewer supports, 1 fewer hard-to-reach support; fits 6 per plate."
    );
    assert_eq!(
        orient_summary(5.0, -1.0, 0.0, 3.0, 4),
        "Oriented: 5m longer print, 1 more support, 3 hard-to-reach supports; fits 4 per plate."
    );
}

#[test]
fn r22_direct_status_explains_the_no_support_pose_and_mixed_support_result() {
    assert_eq!(
        direct_orient_summary(64.0, 84),
        "Oriented: prints flat on the plate with no supports; 1h 4m; fits 84 per plate."
    );
    use sc_print_glue::supports::ModelSupportResult;
    assert_eq!(
        crate::auto_support_run::support_result_text(&[
            ModelSupportResult {
                name: "Lapras".into(),
                contacts: 112,
                columns: 112,
                small: 0,
                direct: false
            },
            ModelSupportResult {
                name: "Jaw".into(),
                contacts: 0,
                columns: 0,
                small: 0,
                direct: true
            },
        ]),
        "Lapras: 112 supports; Jaw: prints directly on the plate, no supports needed"
    );
}

/// Sleptic: Generate supports / Auto prepare on a part that needs none "should
/// say that rather than awkwardly just doing nothing".
#[test]
fn a_part_that_needs_no_supports_gets_a_notice() {
    use crate::auto_support_run::support_notice_text;
    use sc_print_glue::supports::ModelSupportResult;
    let result = |name: &str, direct| ModelSupportResult {
        name: name.into(),
        contacts: if direct { 0 } else { 12 },
        columns: if direct { 0 } else { 12 },
        small: 0,
        direct,
    };
    assert_eq!(
        support_notice_text(&[result("Cube", true)]),
        "No supports needed: Cube prints as it is. Every layer rests on the one below it."
    );
    assert_eq!(
        support_notice_text(&[result("A", true), result("B", true)]),
        "No supports needed: all 2 parts print as they are. Every layer rests on the one below it."
    );
    assert_eq!(
        support_notice_text(&[result("Lid", false), result("Clip", true)]),
        "No supports needed for Clip; the other parts got supports."
    );
    assert_eq!(support_notice_text(&[result("Lid", false)]), "");
}

/// Run the real GUI Fill plate planner on the same selected library poses.
#[test]
#[ignore = "local R22 library and actual Fill plate audit"]
fn r22_real_fill_plate_library_counts() {
    let dir = std::env::var("SLEPTIC_R18_MODELS").unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let printer = sc_print_profiles::load_printer_profile_from_path(
        &root.join("assets/profiles/printers/sleptic-diy-msla.toml"),
    )
    .unwrap();
    let material = sc_print_profiles::load_material_profile_from_path(
        &root.join("assets/profiles/materials/standard-405.toml"),
    )
    .unwrap()
    .into();
    let context =
        sc_print_glue::orient::Context::for_profiles(&printer, &material, &Default::default())
            .unwrap();
    let volume = BuildVolume::new(192.0, 120.0, 200.0);
    let envelope = sc_print_primitives::BuildVolume::new(192.0, 120.0, 200.0).unwrap();
    let names = "Jaw.STL|BELT TENSION NEW.stl|belt tension thing.stl|camera mount.stl|3DBenchy.stl|fly no color.stl|butterfly done repaired.stl|Lapras_Cuerpo (1).STL|Devil-FanShowdown.stl|bottom plate v2 .stl";
    for name in names.split('|') {
        let mut scene =
            sc_project_formats::load_scene(&std::path::Path::new(&dir).join(name), None, |_| {})
                .unwrap()
                .scene;
        scene.repair_meshes(sc_mesh_core::indexed_mesh::RepairPasses::default_repair());
        scene.drop_at([0.0, 0.0], 0.0).unwrap();
        let id = scene
            .reachable_leaves(
                sc_scene::CsgPolicy::Descend,
                sc_scene::ReferencePolicy::Skip,
            )
            .unwrap()[0];
        let mesh = sc_scene::bake_leaf_to_world(&scene, id).unwrap();
        let bb = mesh.local_aabb().unwrap();
        let scale = (190.0 / f64::from(bb.max()[0] - bb.min()[0]))
            .min(118.0 / f64::from(bb.max()[1] - bb.min()[1]))
            .min(195.0 / f64::from(bb.max()[2] - bb.min()[2]))
            .min(1.0);
        if scale < 1.0 {
            let before = scene.node(id).unwrap().local_transform;
            scene
                .set_local_transform(
                    id,
                    Transform3D::Affine(AffineTransform3D::from_uniform_scale(scale))
                        .compose(&before),
                )
                .unwrap();
            scene.drop_at([0.0, 0.0], 0.0).unwrap();
        }
        let (_, original_added) = fill_plate(&mut scene.clone(), id, volume, 3.0).unwrap();
        for (label, weights) in [
            (
                "balanced",
                sc_print_profiles::orient::Preset::Balanced.weights(),
            ),
            ("fill", sc_print_profiles::orient::FILL_PLATE_WEIGHTS),
        ] {
            let p = sc_print_glue::orient::search_with_context(
                &scene, id, envelope, 45.0, weights, context,
            )
            .unwrap();
            let mut chosen = scene.clone();
            world_edit(&chosen, id, p.delta)
                .unwrap()
                .apply_to_scene(&mut chosen)
                .unwrap();
            let (_, added) = fill_plate(&mut chosen, id, volume, 3.0).unwrap();
            let remaining = |m| {
                let costs = sc_print_glue::orient::user_costs(m, p.shortest_minutes);
                costs[..3]
                    .iter()
                    .zip(weights)
                    .map(|(c, w)| c * w)
                    .sum::<f64>()
            };
            println!(
                "R22PACK,{name},{label},actual={},original={},forecast={},zero={},original_contacts={:.1},chosen_contacts={:.1},original_minutes={:.2},chosen_minutes={:.2},original_remaining={:.2},chosen_remaining={:.2}",
                added + 1,
                original_added + 1,
                p.chosen.copies,
                p.chosen.zero_support,
                p.original.contacts,
                p.chosen.contacts,
                p.original.print_minutes,
                p.chosen.print_minutes,
                remaining(p.original),
                remaining(p.chosen)
            );
            if label == "fill" {
                assert_eq!(added + 1, p.chosen.copies);
            }
            if name == "Jaw.STL" && label == "fill" {
                assert!(added + 1 >= 176);
            }
        }
    }
}

#[test]
fn support_summary_distinguishes_columns_short_bridges_and_contacts() {
    let result = sc_print_glue::supports::ModelSupportResult {
        name: "Boss".into(),
        contacts: 17,
        columns: 11,
        small: 3,
        direct: false,
    };
    assert_eq!(
        crate::auto_support_run::support_result_text(&[result]),
        "Boss: 14 supports (3 short)"
    );
}

// R29: Object information shows the model's volume.
#[test]
fn mesh_volume_of_a_closed_cube_and_its_mirror() {
    use sc_mesh_core::IndexedMesh;
    use sc_mesh_core::primitives::{IndexedTriangle, Normal, Vertex};
    let cube = |flip: bool| {
        let s = 10.0_f32;
        let v = |x: f32, y: f32, z: f32| Vertex::new([x * s, y * s, z * s]);
        let vertices = vec![
            v(0., 0., 0.), v(1., 0., 0.), v(1., 1., 0.), v(0., 1., 0.),
            v(0., 0., 1.), v(1., 0., 1.), v(1., 1., 1.), v(0., 1., 1.),
        ];
        let n = Normal::new([0.0, 0.0, 1.0]);
        let tris = [
            [0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4],
            [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7],
        ];
        let faces = tris
            .iter()
            .map(|&[a, b, c]| {
                IndexedTriangle::from_raw_indices(n, if flip { [a, c, b] } else { [a, b, c] })
            })
            .collect();
        IndexedMesh { meta: None, vertices, faces }
    };
    assert!((mesh_volume_mm3(&cube(false)) - 1000.0).abs() < 1e-6);
    assert!((mesh_volume_mm3(&cube(true)) - 1000.0).abs() < 1e-6);
}

// Workflow sweep: a one-support result read "Jaw: 1 supports".
#[test]
fn support_summary_uses_singular_for_one_support() {
    let result = sc_print_glue::supports::ModelSupportResult {
        name: "Jaw".into(),
        contacts: 1,
        columns: 1,
        small: 0,
        direct: false,
    };
    assert_eq!(
        crate::auto_support_run::support_result_text(&[result]),
        "Jaw: 1 support"
    );
}

/// Local audit: print the auto-orient cost breakdown for real models
/// (SLEPTIC_ORIENT_AUDIT = semicolon-separated STL paths).
#[test]
#[ignore = "local orient audit on real models"]
fn orient_audit_real_models() {
    use sc_print_glue::orient::{Context, Preset, measure_with_context, search_with_context, user_costs};
    let paths = std::env::var("SLEPTIC_ORIENT_AUDIT").unwrap();
    let volume = sc_print_primitives::BuildVolume::new(192.0, 120.0, 200.0).unwrap();
    for path in paths.split(';') {
        let mut scene = sc_project_formats::load_scene(std::path::Path::new(path), None, |_| {})
            .unwrap()
            .scene;
        crate::mesh_loader::name_part_after_file(&mut scene, std::path::Path::new(path));
        let id = scene.roots()[0];
        let context = Context { supports: true, ..Default::default() };
        let original = measure_with_context(&scene, id, volume, 45.0, context).unwrap();
        let p = search_with_context(&scene, id, volume, 45.0, Preset::Balanced.weights(), context)
            .unwrap();
        for (name, m) in [("original", original), ("chosen", p.chosen)] {
            eprintln!(
                "{path}\n  {name}: anchored {} zero {} h {:.1} contacts {:.0} blocked {:.2} bed {:.0} peak {:.0} stair {:.0} min {:.0} copies {} costs {:?}",
                m.anchored, m.zero_support, m.height, m.contacts, m.expected_blocked_contacts,
                m.bed_contact_area, m.peak_area, m.stair_area, m.print_minutes, m.copies,
                user_costs(m, p.shortest_minutes).map(|c| (c * 10.0).round() / 10.0)
            );
        }
    }
}

/// Sleptic: arrange should know ahead of time which parts will need supports
/// and leave room for them. Parts that print flat keep the plain gap; parts
/// that will stand on a raft get its margin on each side.
#[test]
fn arrange_leaves_room_for_the_supports_a_part_will_need() {
    let gap_between = |scale: f64| {
        let mut graph = Scene::new();
        let mut ids = Vec::new();
        for _ in 0..2 {
            let id = graph
                .add_leaf(
                    None,
                    Transform3D::Affine(AffineTransform3D::from_uniform_scale(scale)),
                    LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
                )
                .unwrap();
            graph.add_root(id).unwrap();
            ids.push(id);
        }
        let edit = arrange(&graph, BuildVolume::new(200.0, 120.0, 100.0)).unwrap();
        edit.apply_to_scene(&mut graph).unwrap();
        let x = |id| {
            let p = points(&graph, id).unwrap();
            let (lo, hi) = bounds(&p).unwrap();
            (lo.x, hi.x)
        };
        let (a, b) = (x(ids[0]), x(ids[1]));
        if a.0 < b.0 { b.0 - a.1 } else { a.0 - b.1 }
    };
    let settings = sc_print_profiles::support_settings::SupportSettings::default();
    let room = settings.base_diameter_mm / 2.0 + settings.raft_margin_mm;
    // 20 mm cubes print flat on the plate: the 3 mm gap alone.
    assert!((gap_between(20.0) - 3.0).abs() < 1e-6, "{}", gap_between(20.0));
    // 1 mm cubes are too small to stand alone: lifted onto a raft each.
    assert!((gap_between(1.0) - (3.0 + 2.0 * room)).abs() < 1e-6, "{}", gap_between(1.0));
}

/// Sleptic: Fill plate's copies are recognised as copies (only those made by
/// Fill plate), so support generation can work the part out once.
#[test]
fn fill_plate_tags_each_copy_with_its_part() {
    let mut graph = Scene::new();
    let part = graph
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_uniform_scale(10.0)),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    graph.add_root(part).unwrap();
    let mut filled = graph.clone();
    let (edit, count) =
        fill_plate(&mut filled, part, BuildVolume::new(60.0, 60.0, 60.0), 3.0).unwrap();
    assert!(count >= 4, "{count}");
    edit.apply_to_scene(&mut filled).unwrap();
    let copies: Vec<_> = filled
        .reachable_leaves(sc_scene::CsgPolicy::Descend, sc_scene::ReferencePolicy::Skip)
        .unwrap()
        .into_iter()
        .filter(|&id| id != part)
        .collect();
    assert_eq!(copies.len(), count);
    for id in copies {
        let comment = filled.node(id).unwrap().meta.as_ref().unwrap().comment.clone().unwrap();
        assert!(comment.contains(&format!("{FILL_COPY_TAG}{}", part.get())), "{comment}");
    }
    assert_eq!(sc_print_glue::supports::fill_copies(&filled).len(), count);
    // The original is not tagged: it is the part.
    assert!(graph.node(part).unwrap().meta.is_none());
}

/// Sleptic: Auto prepare left five fill copies upright and turned the sixth.
/// Each distinct part is searched once; its copies take the same change,
/// moved with them, and a different part is searched on its own.
#[test]
fn auto_prepare_orients_identical_copies_alike() {
    let mut scene = Scene::new();
    let cube = Arc::new(sc_mesh_fixtures::unit_cube_mesh());
    let rid = scene.add_mesh_resource_arc(cube.clone());
    let mut ids = Vec::new();
    for x in [-30.0, 0.0, 30.0] {
        let leaf = scene
            .add_leaf(
                None,
                Transform3D::Affine(AffineTransform3D::from_translation([x, 10.0, 0.0])),
                LeafPayload::MeshResource(rid),
            )
            .unwrap();
        scene.add_root(leaf).unwrap();
        ids.push(leaf);
    }
    let other = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_translation([0.0, -30.0, 0.0])),
            LeafPayload::Mesh(Arc::new((*cube).clone())),
        )
        .unwrap();
    scene.add_root(other).unwrap();
    ids.push(other);
    let mut calls = 0;
    // A turn about the searched part's own position, different every call.
    let changes = orient_once_per_part(&scene, &ids, |id| {
        calls += 1;
        let Transform3D::Affine(world) = scene.world_transform_of(id).unwrap() else {
            unreachable!()
        };
        let t = *world.translation();
        Some(
            Transform3D::Affine(AffineTransform3D::from_translation(t))
                .compose(&Transform3D::Affine(AffineTransform3D::from_rotation_z(
                    0.3 * f64::from(calls),
                )))
                .compose(&Transform3D::Affine(AffineTransform3D::from_translation([
                    -t[0], -t[1], -t[2],
                ]))),
        )
    });
    assert_eq!(calls, 2, "one search per distinct part");
    assert_eq!(changes.len(), 4);
    // Every copy ends up as the first one moved by its offset.
    let corner = |id: NodeId, delta: &Transform3D| {
        let world = scene.world_transform_of(id).unwrap();
        delta.compose(&world).apply_to_vertex(sc_mesh_core::Vertex::new([1.0, 1.0, 1.0]))
    };
    let first = corner(ids[0], &changes[0].1);
    for (k, (id, delta)) in changes[..3].iter().enumerate() {
        assert_eq!(*id, ids[k]);
        let p = corner(*id, delta);
        let shift = 30.0 * k as f32;
        assert!((p[0] - shift - first[0]).abs() < 1e-4 && (p[1] - first[1]).abs() < 1e-4, "{p:?} {first:?}");
    }
    // The different part was turned by its own (second) search.
    let p = corner(other, &changes[3].1);
    assert!((p[1] - first[1]).abs() > 1.0, "{p:?}");
}

/// Sleptic: Auto prepare turned the motor mount to exactly the plate's 120 mm
/// depth, then arrange refused it ("Objects that do not fit"): baked to
/// world in f32, it measured a few microns over. A part exactly as deep
/// as the plate fits.
#[test]
fn a_part_exactly_as_deep_as_the_plate_still_arranges() {
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            Transform3D::Affine(AffineTransform3D::from_translation([3.37, -7.91, 0.0]))
                .compose(&Transform3D::Affine(AffineTransform3D::from_rotation_z(
                    std::f64::consts::FRAC_PI_2,
                )))
                .compose(&Transform3D::Affine(AffineTransform3D::from_uniform_scale(26.0))),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    let (lo, hi) = bounds(&points(&scene, id).unwrap()).unwrap();
    assert!(hi.y - lo.y >= 26.0, "the fixture measures at least the plate depth");
    assert!(arrange_with_gap(&scene, BuildVolume::new(40.0, 26.0, 30.0), 3.0).is_ok());
}

/// R86: a model bigger than the printer gets a message, not supports
/// standing in mid-air past the plate edge.
#[test]
fn a_model_too_big_for_the_printer_is_refused_supports() {
    use crate::auto_support_run::too_big_text;
    let volume = [224.0, 126.0, 230.0];
    let model = |size| vec![("Mount".to_string(), size)];
    assert_eq!(too_big_text(&model([100.0, 100.0, 100.0]), volume), None);
    // Turned a quarter turn it fits.
    assert_eq!(too_big_text(&model([120.0, 200.0, 50.0]), volume), None);
    // A flat frame wider than the plate is deep fits stood up: not refused.
    assert_eq!(too_big_text(&model([177.0, 177.0, 26.0]), volume), None);
    let text = too_big_text(&model([515.0, 280.0, 265.0]), volume).unwrap();
    assert!(text.starts_with("Mount is too big for this printer: 515 x 280 x 265 mm"), "{text}");
    // Thick in every direction: no way round fits.
    assert!(too_big_text(&model([240.0, 240.0, 240.0]), volume).is_some());
}

/// R92: auto prepare does not orient a part that fits the printer no way
/// round (the support step refuses it with a message); orienting it first
/// kept the user waiting for nothing.
#[test]
fn auto_prepare_skips_orienting_a_part_too_big_for_the_printer() {
    use crate::auto_support_run::part_fits_some_way;
    use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
    let volume = sc_print_primitives::BuildVolume::new(224.0, 126.0, 230.0).unwrap();
    let (mut graph, id) = scene();
    // A 10 mm cube fits; the same cube scaled to 300 mm fits no way round.
    let small = graph.node(id).unwrap().local_transform;
    graph
        .set_local_transform(id, Transform3D::Affine(AffineTransform3D::from_uniform_scale(10.0)).compose(&small))
        .unwrap();
    assert!(part_fits_some_way(&graph, id, volume));
    graph
        .set_local_transform(id, Transform3D::Affine(AffineTransform3D::from_uniform_scale(300.0)).compose(&small))
        .unwrap();
    assert!(!part_fits_some_way(&graph, id, volume));
}

/// R109: with more than two parts the support summary is one line for the
/// plate, not one clause per part.
#[test]
fn support_summary_for_a_crowded_plate_is_one_line() {
    let part = |direct: bool| sc_print_glue::supports::ModelSupportResult {
        name: "3DBenchy".into(),
        contacts: if direct { 0 } else { 4 },
        columns: if direct { 0 } else { 4 },
        small: 0,
        direct,
    };
    let mut plate: Vec<_> = (0..7).map(|_| part(false)).collect();
    plate.extend([part(true), part(true)]);
    assert_eq!(
        crate::auto_support_run::support_result_text(&plate),
        "9 parts: 28 supports (2 print on the plate as they are)"
    );
}

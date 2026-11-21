// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{
    recipe_service::{self, Context},
    scene_document::SceneDocument,
};
use sc_print_profiles::recipes::{Recipe, Step};
use sc_scene::{LeafPayload, Scene};
use std::sync::Arc;

#[test]
fn saved_copy_selects_its_actual_library_position_after_ten_files() {
    let dir = tempfile::tempdir().unwrap();
    let recipe = sc_print_profiles::recipes::defaults().remove(0);
    for n in 1..=12 {
        let index = crate::recipes::save_copy(dir.path(), recipe.clone()).unwrap();
        let entries = sc_print_profiles::recipes::list(dir.path()).unwrap();
        assert_eq!(entries[index].1.name, format!("{} (copy {n})", recipe.name));
    }
}

fn profiles() -> (
    sc_print_profiles::PrinterProfile,
    sc_print_profiles::EffectiveMaterial,
) {
    let library = sc_print_profiles::ProfileLibraries::platform_default();
    let printer = library
        .load_printer_by_id(&"elegoo-mars-3-ctb".parse().unwrap())
        .unwrap();
    let material = library
        .load_material_by_id(&"standard-405".parse().unwrap())
        .unwrap();
    (printer, material.into())
}
fn fixture(open: bool) -> (Scene, sc_scene::NodeId) {
    let mut mesh = sc_mesh_fixtures::unit_cube_mesh();
    if open {
        mesh.faces.pop();
    }
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            None,
            sc_mesh_core::transform::Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(mesh)),
        )
        .unwrap();
    scene.add_root(id).unwrap();
    (scene, id)
}
fn params() -> sc_print_glue::SlicingParams {
    sc_print_glue::SlicingParams {
        process_overrides: Default::default(),
        layer_height_request: None,
        first_layer_height: None,
        aa_level: sc_print_primitives::AaLevel::Off,
    }
}

#[test]
fn scale_support_slice_is_one_undo_and_has_live_step_status() {
    let (scene, id) = fixture(false);
    let (printer, material) = profiles();
    let recipe = Recipe {
        version: 1,
        name: "Fixture".into(),
        steps: vec![
            Step::Scale { percent: 200.0 },
            Step::Supports {
                preset: None,
                settings: Default::default(),
            },
            Step::Slice,
        ],
    };
    let mut statuses = Vec::new();
    let result = recipe_service::run(
        &scene,
        &[id],
        &recipe,
        &Context {
            printer: &printer,
            material: &material,
            params: params(),
            job_name: "fixture",
        },
        &sc_sl1cer::CancelToken::new(),
        |i, s| statuses.push((i, s.to_string())),
    )
    .unwrap();
    assert!(
        result.summary.contains("all steps succeeded"),
        "{}",
        result.summary
    );
    assert_eq!(statuses.len(), 6);
    assert!(statuses.last().unwrap().1.contains("slice — complete"));
    assert!(result.scene.reachable_leaf_count() > 1);
    let bounds = sc_scene::bake_leaf_to_world(&result.scene, id).unwrap();
    let xs: Vec<_> = bounds.vertices.iter().map(|v| v.coords()[0]).collect();
    assert_eq!(
        xs.iter().copied().fold(f32::NEG_INFINITY, f32::max)
            - xs.iter().copied().fold(f32::INFINITY, f32::min),
        2.0
    );
    let doc = SceneDocument::new();
    doc.take_mesh_scene(scene, "recipe.stl".into());
    doc.apply_batch(result.edit, result.slots).unwrap();
    assert!(doc.step_back().unwrap());
    assert!(!doc.step_back().unwrap());
    assert_eq!(
        doc.read_scene(|s| s.graph().reachable_leaf_count()),
        Some(1)
    );
    assert!(doc.step_forward().unwrap());
}

#[test]
fn open_mesh_hollow_failure_names_the_step_and_keeps_original_mesh() {
    let (scene, id) = fixture(true);
    let (printer, material) = profiles();
    let recipe = Recipe {
        version: 1,
        name: "Open".into(),
        steps: vec![Step::Hollow { wall_mm: 0.1 }],
    };
    let result = recipe_service::run(
        &scene,
        &[id],
        &recipe,
        &Context {
            printer: &printer,
            material: &material,
            params: params(),
            job_name: "fixture",
        },
        &sc_sl1cer::CancelToken::new(),
        |_, _| {},
    )
    .unwrap();
    assert!(
        result.summary.contains("hollow skipped:"),
        "{}",
        result.summary
    );
    assert!(Arc::ptr_eq(
        scene.leaf_mesh_arc(id).unwrap(),
        result.scene.leaf_mesh_arc(id).unwrap()
    ));
}

#[test]
fn cancel_after_scale_discards_all_staged_edits() {
    let (scene, id) = fixture(false);
    let (printer, material) = profiles();
    let recipe = Recipe {
        version: 1,
        name: "Cancel".into(),
        steps: vec![Step::Scale { percent: 200.0 }, Step::Slice],
    };
    let token = sc_sl1cer::CancelToken::new();
    let result = recipe_service::run(
        &scene,
        &[id],
        &recipe,
        &Context {
            printer: &printer,
            material: &material,
            params: params(),
            job_name: "fixture",
        },
        &token,
        |_, s| {
            if s.contains("complete") {
                token.cancel();
            }
        },
    );
    assert!(result.err().unwrap().contains("cancelled"));
    assert_eq!(
        scene.node(id).unwrap().local_transform,
        sc_mesh_core::transform::Transform3D::Identity
    );
}

#[test]
fn selected_model_under_parent_keeps_support_lift_without_moving_its_sibling() {
    use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
    let mut scene = Scene::new();
    let parent = scene.add_group(
        None,
        Transform3D::Affine(AffineTransform3D::from_translation([4., 3., 0.])),
    );
    scene.add_root(parent).unwrap();
    let mut ids = Vec::new();
    for x in [0., 10.] {
        let id = scene
            .add_leaf(
                None,
                Transform3D::Affine(AffineTransform3D::from_translation([x, 0., 0.])),
                LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
            )
            .unwrap();
        scene.add_child(parent, id).unwrap();
        ids.push(id);
    }
    let (printer, material) = profiles();
    let recipe = Recipe {
        version: 1,
        name: "Parent".into(),
        steps: vec![Step::Supports {
            preset: None,
            settings: Default::default(),
        }],
    };
    let result = recipe_service::run(
        &scene,
        &ids[..1],
        &recipe,
        &Context {
            printer: &printer,
            material: &material,
            params: params(),
            job_name: "parent",
        },
        &sc_sl1cer::CancelToken::new(),
        |_, _| {},
    )
    .unwrap();
    assert!(
        result.summary.contains("all steps succeeded"),
        "{}",
        result.summary
    );
    let z = |scene: &Scene, id| {
        sc_scene::bake_leaf_to_world(scene, id)
            .unwrap()
            .vertices
            .iter()
            .map(|v| v.coords()[2])
            .fold(f32::INFINITY, f32::min)
    };
    assert!((z(&result.scene, ids[0]) - z(&scene, ids[0]) - 5.0).abs() < 1e-5);
    assert_eq!(
        result.scene.world_transform_of(ids[1]).unwrap(),
        scene.world_transform_of(ids[1]).unwrap()
    );
    assert_eq!(result.scene.parent_of(ids[0]), Some(parent));
}

#[test]
fn all_plates_keep_ids_and_membership_and_locked_targets_refuse() {
    let (mut scene, first) = fixture(false);
    let second = scene
        .add_leaf(
            None,
            sc_mesh_core::transform::Transform3D::Identity,
            LeafPayload::Mesh(scene.leaf_mesh_arc(first).unwrap().clone()),
        )
        .unwrap();
    scene.add_root(second).unwrap();
    let mut plates = scene.plates().clone();
    let plate = plates.add();
    plates.assignments.insert(second, plate);
    assert!(scene.set_plates(plates));
    let (printer, material) = profiles();
    let recipe = Recipe {
        version: 1,
        name: "Plates".into(),
        steps: vec![Step::Scale { percent: 150.0 }, Step::Slice],
    };
    let context = Context {
        printer: &printer,
        material: &material,
        params: params(),
        job_name: "plates",
    };
    let result = recipe_service::run(
        &scene,
        &[first, second],
        &recipe,
        &context,
        &sc_sl1cer::CancelToken::new(),
        |_, _| {},
    )
    .unwrap();
    assert_eq!(result.scene.plate_of(second), plate);
    assert_eq!(result.scene.plates().order.len(), 2);
    assert_eq!(result.scene.reachable_leaf_count(), 2);
    assert!(result.summary.contains("all steps succeeded"));
    let mut plates = scene.plates().clone();
    plates.order[1].locked = true;
    assert!(scene.set_plates(plates));
    assert!(
        recipe_service::run(
            &scene,
            &[first, second],
            &recipe,
            &context,
            &sc_sl1cer::CancelToken::new(),
            |_, _| {}
        )
        .err()
        .unwrap()
        .contains("Unlock")
    );
}

#[test]
fn multiple_exports_reuse_slices_and_edits_invalidate_them() {
    let (scene, id) = fixture(false);
    let (printer, material) = profiles();
    let temp = tempfile::tempdir().unwrap();
    let export = |name: &str| Step::Export {
        format: "svg-combined".into(),
        folder: temp.path().into(),
        name_pattern: name.into(),
    };
    let mut recipe = Recipe {
        version: 1,
        name: "Exports".into(),
        steps: vec![Step::Slice, export("one.svg"), export("two.svg")],
    };
    let context = Context {
        printer: &printer,
        material: &material,
        params: params(),
        job_name: "fixture",
    };
    let result = recipe_service::run(
        &scene,
        &[id],
        &recipe,
        &context,
        &sc_sl1cer::CancelToken::new(),
        |_, _| {},
    )
    .unwrap();
    assert!(
        result.summary.contains("all steps succeeded"),
        "{}",
        result.summary
    );
    assert!(temp.path().join("one.svg").exists());
    assert!(temp.path().join("two.svg").exists());
    recipe.steps = vec![
        Step::Slice,
        Step::Scale { percent: 110.0 },
        export("stale.svg"),
    ];
    let result = recipe_service::run(
        &scene,
        &[id],
        &recipe,
        &context,
        &sc_sl1cer::CancelToken::new(),
        |_, _| {},
    )
    .unwrap();
    assert!(result.summary.contains("export skipped: Run slice"));
    assert!(!temp.path().join("stale.svg").exists());
}

#[test]
fn form_edits_are_typed_validated_and_preserve_other_steps() {
    let mut recipe = sc_print_profiles::recipes::defaults().remove(1);
    let before = recipe.clone();
    let text =
        crate::recipes::edit_field(&recipe.serialize().unwrap(), 0, "wall_mm", "2.5").unwrap();
    recipe = Recipe::parse(&text).unwrap();
    assert_eq!(recipe.steps[0], Step::Hollow { wall_mm: 2.5 });
    assert_eq!(recipe.steps[1..], before.steps[1..]);
    assert!(crate::recipes::edit_field(&text, 0, "wall_mm", "-1").is_err());
    assert!(crate::recipes::edit_field(&text, 1, "count", "2.5").is_err());
    let text = crate::recipes::edit_field(&text, 2, "settings.tree_supports", "false").unwrap();
    assert!(
        matches!(&Recipe::parse(&text).unwrap().steps[2], Step::Supports { settings, .. } if !settings.tree_supports)
    );
    let text = crate::recipes::edit_field(&text, 4, "folder", "folder with spaces").unwrap();
    assert!(
        matches!(&Recipe::parse(&text).unwrap().steps[4], Step::Export { folder, .. } if folder.to_str() == Some("folder with spaces"))
    );
}

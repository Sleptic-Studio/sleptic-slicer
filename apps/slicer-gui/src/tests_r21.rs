// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Everyday consistency regressions use a non-box mesh.
use sc_mesh_core::{
    IndexedMesh,
    primitives::Vertex,
    transform::{AffineTransform3D, Transform3D},
};
use sc_scene::{LeafPayload, Scene};
use std::sync::Arc;

/// A diamond (octahedron) of radius 1, turned 45 degrees about X and lifted
/// 0.75 mm: its box is not its shape, so code that uses the real vertices and
/// code that uses the box give different answers. Its six corners lie on
/// the axes, +X, -X, +Y, -Y, +Z, -Z in that order; it has no faces.
pub(crate) fn tilted_octahedron() -> Scene {
    let corners = (0..3)
        .flat_map(|axis| [1.0, -1.0].map(|sign| {
            let mut at = [0.0; 3];
            at[axis] = sign;
            Vertex::new(at)
        }))
        .collect();
    let diamond = IndexedMesh { meta: None, vertices: corners, faces: Vec::new() };
    let placed = AffineTransform3D::from_translation([0.0, 0.0, 0.75])
        .compose(&AffineTransform3D::from_rotation_x(std::f64::consts::FRAC_PI_4));
    let mut scene = Scene::new();
    let leaf = scene.add_leaf(None, Transform3D::Affine(placed), LeafPayload::Mesh(Arc::new(diamond))).expect("a new scene takes a leaf");
    scene.add_root(leaf).expect("the leaf becomes the root");
    scene
}

#[test]
fn rotated_non_box_project_fit_and_paste_use_real_vertices() {
    let scene = tilted_octahedron();
    let volume = crate::build_volume::BuildVolume::new(2.1, 1.5, 1.5);
    assert!(!volume.holds_box(scene.aabb().unwrap().unwrap()));
    assert!(crate::mesh_loader::fits_printer(&scene, volume));
    let expected = scene.tight_aabb().unwrap().unwrap();
    let fragment = sc_scene_edit::SceneFragment::from_scene(scene);
    let actual = crate::clipboard::copied_box(&fragment).unwrap();
    assert_eq!(actual, expected);
}
#[test]
fn multifile_import_spills_and_is_one_undo_redo_step() {
    let ctrl = crate::viewport::ViewController::for_test();
    let doc = ctrl.scene_doc();
    let volume = crate::build_volume::BuildVolume::new(30.0, 30.0, 30.0);
    let source = sc_scene_edit::SceneFragment::from_leaf_snapshot(
        Arc::new(sc_mesh_fixtures::unit_cube_mesh()),
        Transform3D::Affine(AffineTransform3D::from_uniform_scale(20.0)),
    )
    .into_scene();
    doc.import_geometry(&[source.clone(), source.clone(), source], volume)
        .unwrap();
    let before = doc.read_scene(|s| s.graph().clone()).unwrap();
    assert_eq!(before.plates().order.len(), 3);
    for plate in &before.plates().order {
        let s = before.plate_scene(plate.id).unwrap();
        assert_eq!(
            s.reachable_leaves(
                sc_scene::CsgPolicy::Descend,
                sc_scene::ReferencePolicy::Skip
            )
            .unwrap()
            .len(),
            1
        );
        assert!(crate::mesh_loader::fits_printer(&s, volume));
    }
    doc.step_back().unwrap();
    assert!(!doc.has_past());
    doc.read_scene(|s| {
        assert!(s.graph().roots().is_empty());
        assert_eq!(s.graph().plates().order.len(), 1);
    })
    .unwrap();
    doc.step_forward().unwrap();
    doc.read_scene(|s| {
        assert_eq!(s.graph().plates(), before.plates());
        for id in before.roots() {
            assert_eq!(
                s.graph().node(*id).unwrap().local_transform,
                before.node(*id).unwrap().local_transform
            );
        }
    })
    .unwrap();
}
#[test]
fn printer_orientation_field_has_bounded_width() {
    let source = include_str!("../ui/printer-editor.slint");
    assert!(source.contains(
        "ComboBox {width:85px;horizontal-stretch:0;model:[\"0\",\"90\",\"180\",\"270\"]"
    ));
}

#[path = "tests_r21_library.rs"]
mod library;

#[test]
fn supported_hollow_oriented_multiplate_project_reslices_identically() {
    let mut scene = sc_scene_edit::SceneFragment::from_leaf_snapshot(
        Arc::new(sc_mesh_fixtures::unit_cube_mesh()),
        Transform3D::Affine(AffineTransform3D::from_uniform_scale(12.0)),
    )
    .into_scene();
    let id = scene.first_leaf_id().unwrap();
    crate::hollow_tools::edit(&scene, id, 1.5, crate::hollow_tools::HoleChange::Auto(2.0))
        .unwrap()
        .apply_to_scene(&mut scene)
        .unwrap();
    let before = scene.node(id).unwrap().local_transform;
    scene
        .set_local_transform(
            id,
            Transform3D::Affine(AffineTransform3D::from_rotation_x(0.37)).compose(&before),
        )
        .unwrap();
    scene.drop_at([7.0, -11.0], 0.0).unwrap();
    let (mut scene, structure) =
        sc_print_glue::supports::geometric_supported_scene(&scene, &Default::default()).unwrap();
    assert!(!structure.points.is_empty());
    let other = scene
        .add_leaf(
            Some(sc_scene::SceneMetadata {
                name: Some("Second plate".into()),
                comment: Some("object-settings-marker".into()),
            }),
            Transform3D::Affine(AffineTransform3D::from_translation([3.0, 2.0, 0.5])),
            LeafPayload::Mesh(Arc::new(sc_mesh_fixtures::unit_cube_mesh())),
        )
        .unwrap();
    scene.add_root(other).unwrap();
    let mut plates = scene.plates().clone();
    let second = plates.add();
    plates.assignments.insert(other, second);
    scene.set_plates(plates);
    let doc = crate::scene_document::SceneDocument::new();
    doc.take_mesh_scene(scene.clone(), "source.stl".into());
    doc.choose_printer(Some("sleptic-diy-msla".parse().unwrap()));
    doc.choose_resin(Some("standard-405".parse().unwrap()));
    crate::process_settings::edit(&mut doc.process_overrides.borrow_mut(), 1, "2.75");
    let payload = doc.project_payload(None, None);
    let mut bytes = Vec::new();
    sc_project_formats::threemf::write_threemf_project(
        &scene,
        &payload,
        sc_project_formats::EmbedProfiles::No,
        None,
        None,
        &mut bytes,
    )
    .unwrap();
    let reopened =
        sc_project_formats::threemf::read_threemf_project(&mut std::io::Cursor::new(bytes))
            .unwrap();
    assert_eq!(reopened.payload.as_ref(), Some(&payload));
    assert_eq!(reopened.scene.plates().order, scene.plates().order);
    let leaves = |s: &Scene| {
        s.reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .unwrap()
    };
    let original_ids = leaves(&scene);
    let reopened_ids = leaves(&reopened.scene);
    assert_eq!(original_ids.len(), reopened_ids.len());
    for (a, b) in original_ids.iter().zip(&reopened_ids) {
        assert_eq!(scene.plate_of(*a), reopened.scene.plate_of(*b));
        assert_eq!(
            scene.node(*a).unwrap().local_transform,
            reopened.scene.node(*b).unwrap().local_transform
        );
        assert_eq!(
            scene.mesh_at_leaf(*a).unwrap().meta,
            reopened.scene.mesh_at_leaf(*b).unwrap().meta
        );
        let before = scene.node(*a).unwrap().meta.as_ref();
        let after = reopened.scene.node(*b).unwrap().meta.as_ref();
        assert_eq!(
            before.and_then(|m| m.name.as_ref()),
            after.and_then(|m| m.name.as_ref())
        );
        let normalized = |meta: Option<&sc_scene::SceneMetadata>| {
            let mut lines = meta
                .and_then(|m| m.comment.as_deref())
                .unwrap_or("")
                .lines()
                .filter(|line| !line.starts_with("owner = "))
                .map(str::to_string)
                .collect::<Vec<_>>();
            lines.sort();
            lines
        };
        assert_eq!(normalized(before), normalized(after));
        if let Some(owner) = crate::scene_document::support_owner(&scene, *a) {
            let index = original_ids.iter().position(|i| *i == owner).unwrap();
            assert_eq!(
                crate::scene_document::support_owner(&reopened.scene, *b),
                Some(reopened_ids[index])
            );
        }
    }
    let (printer, material) = library::profiles();
    for p in &scene.plates().order {
        let (a, pa) = library::slice(&scene.plate_scene(p.id).unwrap(), &printer, &material);
        let (b, pb) = library::slice(
            &reopened.scene.plate_scene(p.id).unwrap(),
            &printer,
            &material,
        );
        assert_eq!(a.layers().len(), b.layers().len());
        assert_eq!(
            library::export_first(&a, &pa),
            library::export_first(&b, &pb)
        );
    }
}

#[test]
fn exported_layer_corner_agrees_with_plate_position() {
    let scene = sc_scene_edit::SceneFragment::from_leaf_snapshot(
        Arc::new(sc_mesh_fixtures::unit_cube_mesh()),
        Transform3D::Affine(AffineTransform3D::from_translation([-25.0, 15.0, 5.0])).compose(
            &Transform3D::Affine(AffineTransform3D::from_uniform_scale(10.0)),
        ),
    )
    .into_scene();
    let (printer, material) = library::profiles();
    let (stack, profile) = library::slice(&scene, &printer, &material);
    let bytes = library::export_first(&stack, &profile);
    let image = image::load_from_memory(&bytes).unwrap().to_luma8();
    // Unmirrored 50 um pixels: left=-30, top=+20; PNG Y points down.
    let x = ((96.0 - 30.0) / 0.05) as u32;
    let y = ((60.0 - 20.0) / 0.05) as u32;
    assert!(image.get_pixel(x + 1, y + 1)[0] > 0);
    assert_eq!(image.get_pixel(x - 1, y + 1)[0], 0);
    assert_eq!(image.get_pixel(x + 1, y - 1)[0], 0);
    let bounds = scene.tight_aabb().unwrap().unwrap();
    assert_eq!((bounds.min()[0] + bounds.max()[0]) / 2.0, -25.0);
    assert_eq!((bounds.min()[1] + bounds.max()[1]) / 2.0, 15.0);
    assert_eq!(bounds.min()[2], 0.0);
}

#[path = "tests_r21_history.rs"]
mod history;

// Sleptic: the plate's trash tile deletes the plate. An empty plate goes at once
// (one undo step brings it back); the only plate is never removed.
#[test]
fn r48_empty_plate_is_deleted_by_its_tile_and_the_last_plate_stays() {
    let ctrl = crate::viewport::ViewController::for_test();
    let doc = ctrl.scene_doc();
    let volume = crate::build_volume::BuildVolume::new(30.0, 30.0, 30.0);
    let source = sc_scene_edit::SceneFragment::from_leaf_snapshot(
        Arc::new(sc_mesh_fixtures::unit_cube_mesh()),
        Transform3D::Affine(AffineTransform3D::from_uniform_scale(20.0)),
    )
    .into_scene();
    doc.import_geometry(&[source.clone(), source], volume).unwrap();
    let plates = || doc.read_scene(|s| s.graph().plates().clone()).unwrap();
    assert_eq!(plates().order.len(), 2);
    // Empty the second plate, then its delete tile removes it outright.
    let second = plates().order[1].id;
    doc.set_active_plate(second);
    ctrl.context_action(6);
    assert!(ctrl.delete_empty_plate(second));
    assert_eq!(plates().order.len(), 1);
    doc.step_back().unwrap();
    assert_eq!(plates().order.len(), 2);
    // A plate with objects is not "empty"; the last plate is never removed.
    let first = plates().order[0].id;
    assert!(!ctrl.delete_empty_plate(first));
    ctrl.delete_empty_plate(second);
    assert!(!ctrl.delete_empty_plate(first));
    assert_eq!(plates().order.len(), 1);
}

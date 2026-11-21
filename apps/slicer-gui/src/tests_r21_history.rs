// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Compare live scene state around the daily command history paths.
use super::*;
use crate::{scene_document::SceneDocument, viewport::ViewController};
use sc_scene_edit::SceneEdit;
fn snapshot(doc: &SceneDocument) -> String {
    doc.read_scene(|s| {
        let g = s.graph();
        let leaves = g
            .reachable_leaves(
                sc_scene::CsgPolicy::Descend,
                sc_scene::ReferencePolicy::Skip,
            )
            .unwrap();
        format!(
            "{:?} {:?}",
            g.plates(),
            leaves
                .iter()
                .map(|&id| (
                    id,
                    g.world_transform_of(id).unwrap(),
                    g.node(id).unwrap().meta.clone(),
                    g.mesh_at_leaf(id).unwrap().clone()
                ))
                .collect::<Vec<_>>()
        )
    })
    .unwrap()
}
fn commit(doc: &SceneDocument, edit: SceneEdit) {
    doc.commit_edit(edit.clone()).unwrap();
    doc.record_edit(edit);
}
#[test]
fn daily_commands_restore_exact_geometry_metadata_transforms_and_plates() {
    for name in [
        "position",
        "rotate",
        "scale",
        "mirror",
        "drop",
        "center",
        "arrange",
        "fill",
        "orient",
        "supports-generate",
        "supports-clear",
        "hollow",
        "drains",
        "cut",
        "delete",
        "duplicate",
        "move-plate",
    ] {
        let ctrl = ViewController::for_test();
        ctrl.set_build_volume(crate::build_volume::BuildVolume::new(40.0, 30.0, 100.0));
        let mut source = sc_scene_edit::SceneFragment::from_leaf_snapshot(
            Arc::new(sc_mesh_fixtures::unit_cube_mesh()),
            Transform3D::Affine(AffineTransform3D::from_translation([4.0, 2.0, 11.0]))
                .compose(&Transform3D::Affine(AffineTransform3D::from_rotation_x(
                    0.3,
                )))
                .compose(&Transform3D::Affine(AffineTransform3D::from_uniform_scale(
                    12.0,
                ))),
        )
        .into_scene();
        let model = source.first_leaf_id().unwrap();
        if name == "supports-clear" {
            source =
                sc_print_glue::supports::geometric_supported_scene(&source, &Default::default())
                    .unwrap()
                    .0;
        }
        if name == "drains" {
            crate::hollow_tools::edit(&source, model, 1.5, crate::hollow_tools::HoleChange::Keep)
                .unwrap()
                .apply_to_scene(&mut source)
                .unwrap();
        }
        let mut plates = source.plates().clone();
        plates.add();
        source.set_plates(plates);
        ctrl.put_graph_for_test(source.clone());
        ctrl.switch_section(crate::WorkspaceSection::Transform);
        let doc = ctrl.scene_doc();
        doc.choose(crate::selection::Selection::from_ids(vec![model]));
        let before = snapshot(&doc);
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let received = events.clone();
        let _listener = doc.watch(move |event| received.borrow_mut().push(event));
        match name {
            "position" => ctrl.on_position_committed(0, "9".into()),
            "rotate" => ctrl.on_object_transform_committed(false, 1, "37".into()),
            "scale" => ctrl.on_object_transform_committed(true, 0, "1400".into()),
            "mirror" => ctrl.mirror_action(0),
            "drop" => ctrl.on_drop_to_plate_requested(),
            "center" => ctrl.on_center_on_plate_requested(),
            "arrange" => ctrl.layout_action(true),
            "fill" => {
                assert!(doc.fill_plate(ctrl.printer_volume(), 3.0).is_some());
            }
            "orient" => {
                let context = sc_print_glue::orient::Context {
                    supports: false,
                    ..Default::default()
                };
                let proposal = sc_print_glue::orient::search_with_context(
                    &source,
                    model,
                    sc_print_primitives::BuildVolume::new(40.0, 30.0, 100.0).unwrap(),
                    45.0,
                    sc_print_glue::orient::Preset::Fastest.weights(),
                    context,
                )
                .unwrap();
                commit(
                    &doc,
                    crate::layout_tools::world_edit(&source, model, proposal.delta).unwrap(),
                );
            }
            "supports-generate" => {
                let generated = sc_print_glue::supports::geometric_supported_scene(
                    &source,
                    &Default::default(),
                )
                .unwrap()
                .0;
                doc.apply_support_scene(&generated, &[model]).unwrap();
            }
            "supports-clear" => {
                let ids = crate::auto_support_run::support_ids(&source);
                doc.choose(crate::selection::Selection::from_ids(ids));
                doc.delete_chosen().unwrap();
            }
            "hollow" => commit(
                &doc,
                crate::hollow_tools::edit(
                    &source,
                    model,
                    1.5,
                    crate::hollow_tools::HoleChange::Keep,
                )
                .unwrap(),
            ),
            "drains" => commit(
                &doc,
                crate::hollow_tools::edit(
                    &source,
                    model,
                    1.5,
                    crate::hollow_tools::HoleChange::Auto(2.0),
                )
                .unwrap(),
            ),
            "cut" => {
                let plan = crate::model_tools::plan(
                    &source,
                    &[model],
                    crate::model_tools::Operation::Cut {
                        z: 11.0,
                        keep: 2,
                        cap: true,
                    },
                )
                .unwrap();
                doc.apply_batch(plan.edit, plan.slots).unwrap();
            }
            "delete" => {
                doc.delete_chosen().unwrap();
            }
            "duplicate" => {
                let fragment = crate::clipboard::copy_of_selection(&doc).unwrap();
                doc.paste_parts(&fragment, [15.0, 0.0, 0.0]).unwrap();
            }
            "move-plate" => doc.move_selection_to_plate(2),
            _ => unreachable!(),
        }
        let after = snapshot(&doc);
        assert_ne!(before, after, "{name} must perform a real edit");
        events.borrow_mut().clear();
        assert!(doc.step_back().unwrap(), "{name} undo");
        assert_eq!(snapshot(&doc), before, "{name} undo state");
        assert!(
            events.borrow().iter().any(|e| matches!(
                e,
                crate::scene_document::DocumentEvent::Edited
                    | crate::scene_document::DocumentEvent::Regrouped
            )),
            "{name} undo refresh"
        );
        events.borrow_mut().clear();
        assert!(doc.step_forward().unwrap(), "{name} redo");
        assert_eq!(snapshot(&doc), after, "{name} redo state");
        assert!(
            events.borrow().iter().any(|e| matches!(
                e,
                crate::scene_document::DocumentEvent::Edited
                    | crate::scene_document::DocumentEvent::Regrouped
            )),
            "{name} redo refresh"
        );
        println!("R21HISTORY {name}: undo and redo exact; refresh event emitted");
    }
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! `ViewportController::handle_before_rendering` against a mock renderer:
//! the upload, camera framing and redraw steps that come before the
//! frame needs the window. The rest (target size, gizmo, the draw itself)
//! needs a real window and GPU.
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use glam::Vec3;
use sc_mesh_core::IndexedMesh;
use sc_mesh_core::primitives::Vertex;
use sc_mesh_core::transform::Transform3D;
use sc_scene::{LeafPayload, Scene};

use super::controller::ViewController;
use super::frame_host::Owed;
use crate::mesh_render::mock::{ScriptedRenderer, RendererScript};

type Mock = Rc<RefCell<RendererScript>>;

fn with_mock_renderer() -> (ViewController, Mock) {
    let ctrl = ViewController::for_test();
    let state = RendererScript::shared_script();
    ctrl.put_renderer_for_test(Box::new(ScriptedRenderer::new(state.clone())));
    (ctrl, state)
}

/// A controller with an empty scene and an upload owed, whose next upload
/// fails if `fails`.
fn upload_owed(fails: bool) -> (ViewController, Mock) {
    let (ctrl, mock) = with_mock_renderer();
    if fails {
        mock.borrow_mut()
            .upload_results
            .push_back(Err(anyhow::anyhow!("scripted upload failure")));
    }
    ctrl.put_graph_for_test(Scene::new());
    ctrl.put_frame_for_test(Owed::UPLOAD);
    (ctrl, mock)
}

fn flags(ctrl: &ViewController) -> (bool, bool) {
    let frame = ctrl.frame_for_test();
    (frame.includes(Owed::UPLOAD), frame.includes(Owed::REDRAW))
}

#[test]
fn an_upload_happens_only_when_owed_and_there_is_a_scene() {
    let (ctrl, mock) = with_mock_renderer();
    ctrl.put_graph_for_test(Scene::new());
    ctrl.put_frame_for_test(Owed::NOTHING);
    ctrl.before_frame();
    assert!(mock.borrow().uploads().is_empty(), "not owed");
    // Owed without a scene: nothing to upload, the debt is dropped.
    let (ctrl, mock) = with_mock_renderer();
    ctrl.put_frame_for_test(Owed::UPLOAD);
    ctrl.before_frame();
    assert!(mock.borrow().uploads().is_empty());
    assert_eq!(flags(&ctrl), (false, false));
}

#[test]
fn a_good_upload_owes_a_redraw_and_forgets_earlier_failures() {
    let (ctrl, mock) = upload_owed(false);
    ctrl.scene_loader()
        .record_upload_failure(&anyhow::anyhow!("an earlier failure"));
    ctrl.before_frame();
    assert_eq!(mock.borrow().uploads().len(), 1);
    assert_eq!(flags(&ctrl), (false, true));
    assert_eq!(ctrl.scene_loader().upload_failure_count_for_test(), 0);
    // Nothing drawn here: the test controller has no window.
    assert!(mock.borrow().draws().is_empty());
}

#[test]
fn a_failed_upload_is_counted_drops_the_gpu_scene_and_the_frame_goes_on() {
    let (ctrl, mock) = upload_owed(true);
    assert!(!ctrl.camera_ready_for_test());
    ctrl.before_frame();
    assert_eq!(mock.borrow().uploads().len(), 1);
    assert_eq!(ctrl.scene_loader().upload_failure_count_for_test(), 1);
    // Not owed again: retrying every frame would churn GPU memory for as
    // long as the failure lasts. Whatever changes the scene owes a new
    // upload itself.
    assert!(!flags(&ctrl).0);
    // The GPU's copy no longer matches the scene, so it goes and the
    // bare plate is drawn instead.
    assert_eq!(mock.borrow().scene_drops(), 1);
    // The rest of the frame still ran (a failed upload once froze the
    // view): the camera framing after the upload happened.
    assert!(ctrl.camera_ready_for_test());
}

#[test]
fn deleting_the_only_part_uploads_once_and_leaves_nothing_owed() {
    let ctrl = Rc::new(ViewController::for_test());
    let mock = RendererScript::shared_script();
    ctrl.put_renderer_for_test(Box::new(ScriptedRenderer::new(mock.clone())));
    ctrl.listen_to_document();
    let mut scene = Scene::new();
    let root = scene.add_group(None, Transform3D::Identity);
    scene.add_root(root).unwrap();
    let mesh = IndexedMesh {
        meta: None,
        vertices: vec![Vertex::new([0.0; 3])],
        faces: vec![],
    };
    let part = scene
        .add_leaf(
            None,
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(mesh)),
        )
        .unwrap();
    scene.add_child(root, part).unwrap();
    ctrl.put_graph_for_test(scene);
    ctrl.on_select_by_index(0);
    ctrl.forget_frame_for_test();
    ctrl.delete_selected();
    assert_eq!(ctrl.leaf_total_for_test(), 0);
    assert!(flags(&ctrl).0, "the edit owes an upload");
    ctrl.before_frame();
    assert_eq!(mock.borrow().uploads().len(), 1);
    assert!(!flags(&ctrl).0);
    assert_eq!(ctrl.scene_loader().upload_failure_count_for_test(), 0);
}

#[test]
fn the_camera_is_framed_on_the_first_frame() {
    let (ctrl, mock) = with_mock_renderer();
    mock.borrow_mut().bounds = Some((Vec3::splat(-1.0), Vec3::ONE));
    ctrl.put_frame_for_test(Owed::NOTHING);
    assert!(!ctrl.camera_ready_for_test());
    ctrl.before_frame();
    assert!(ctrl.camera_ready_for_test());
    ctrl.before_frame();
    assert!(ctrl.camera_ready_for_test());
}

#[test]
fn a_lost_device_stops_the_frame_quietly() {
    let (ctrl, mock) = with_mock_renderer();
    ctrl.put_graph_for_test(Scene::new());
    ctrl.scene_loader()
        .lost_device_state()
        .mark("Unknown: test".to_string());
    ctrl.put_frame_for_test(Owed::UPLOAD.and(Owed::REDRAW));
    ctrl.before_frame();
    assert!(mock.borrow().uploads().is_empty() && mock.borrow().draws().is_empty());
    assert_eq!(flags(&ctrl), (false, false));
    // No failure counted, so no failure dialog.
    assert_eq!(
        (
            ctrl.scene_loader().upload_failure_count_for_test(),
            ctrl.scene_loader().render_failure_count_for_test()
        ),
        (0, 0)
    );
}

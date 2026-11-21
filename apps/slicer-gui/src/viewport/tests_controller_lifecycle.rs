// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The controller's renderer lifecycle without a GPU. Setting a renderer
//! up takes wgpu objects, so these drive what happens after it:
//! `post_renderer_install` (built), `post_renderer_refused` (not built)
//! and `handle_rendering_teardown`, against a mock renderer.
use std::cell::RefCell;
use std::rc::Rc;

use sc_scene::Scene;
use slint::wgpu_29::wgpu;

use super::controller::ViewController;
use super::frame_host::Owed;
use crate::graphics_status::NoGraphics;
use crate::mesh_render::mock::{ScriptedRenderer, RendererScript};
use crate::mesh_render::{FrameResource, FrameRefusal, StartRefusal, StartStep};

fn with_mock_renderer() -> (ViewController, Rc<RefCell<RendererScript>>) {
    let ctrl = ViewController::for_test();
    let state = RendererScript::shared_script();
    ctrl.put_renderer_for_test(Box::new(ScriptedRenderer::new(state.clone())));
    (ctrl, state)
}

/// A validation error carrying `message`.
fn refusal(message: &str) -> wgpu::Error {
    wgpu::Error::Validation {
        source: Box::new(std::io::Error::other(message.to_owned())),
        description: message.into(),
    }
}

#[test]
fn a_new_renderer_forgets_earlier_upload_and_render_failures() {
    let (ctrl, _) = with_mock_renderer();
    let loader = ctrl.scene_loader();
    loader.record_upload_failure(&anyhow::anyhow!("an earlier device"));
    let failed_frame = FrameRefusal::Refused {
        resource: FrameResource::Outputs,
        source: refusal("an earlier device"),
    };
    loader.record_render_failure(&failed_frame);
    assert_eq!(
        (
            loader.upload_failure_count_for_test(),
            loader.render_failure_count_for_test()
        ),
        (1, 1)
    );
    ctrl.renderer_ready();
    assert_eq!(
        (
            loader.upload_failure_count_for_test(),
            loader.render_failure_count_for_test()
        ),
        (0, 0)
    );
}

#[test]
fn a_new_renderer_owes_an_upload_only_when_there_is_a_scene() {
    let (ctrl, _) = with_mock_renderer();
    ctrl.put_frame_for_test(Owed::NOTHING);
    ctrl.renderer_ready();
    assert!(!ctrl.frame_for_test().includes(Owed::UPLOAD), "no scene");
    ctrl.put_graph_for_test(Scene::new());
    ctrl.put_frame_for_test(Owed::NOTHING);
    ctrl.renderer_ready();
    assert!(
        ctrl.frame_for_test().includes(Owed::UPLOAD),
        "a scene to upload"
    );
}

#[test]
fn a_refused_renderer_is_recorded_and_fails_the_waiting_load() {
    let (ctrl, _) = with_mock_renderer();
    let folder = tempfile::tempdir().unwrap();
    let waiting = folder.path().join("parked.stl");
    std::fs::write(&waiting, b"").unwrap();
    ctrl.queued_load().hold(waiting.clone());
    let err = StartRefusal {
        step: StartStep::PartsShader,
        source: refusal("too many bind groups"),
    };
    let reason = NoGraphics::Refused(err.to_string());
    ctrl.renderer_refused(err);
    assert_eq!(ctrl.scene_loader().unavailable_graphics_reason(), Some(reason.clone()));
    assert!(ctrl.queued_load().take().is_none(), "no longer waiting");
    // The load ends with the reason, through the usual load-error path.
    let (path, result) = ctrl
        .scene_loader()
        .take_delivery_for_test()
        .expect("the waiting load is answered");
    assert_eq!(path, waiting);
    assert_eq!(result.err().map(|e| e.to_string()), Some(reason.explain()));
}

#[test]
fn teardown_drops_the_renderer_and_the_redraw_owed() {
    let (ctrl, _) = with_mock_renderer();
    assert!(ctrl.renderer_up_for_test());
    ctrl.put_frame_for_test(Owed::REDRAW);
    ctrl.graphics_gone();
    assert!(!ctrl.renderer_up_for_test());
    assert!(!ctrl.frame_for_test().includes(Owed::REDRAW));
}

#[test]
fn a_new_renderer_takes_up_the_file_waiting_for_it() {
    let (ctrl, _) = with_mock_renderer();
    let folder = tempfile::tempdir().unwrap();
    let waiting = folder.path().join("parked.stl");
    std::fs::write(&waiting, b"").unwrap();
    ctrl.queued_load().hold(waiting);
    ctrl.renderer_ready();
    assert!(ctrl.queued_load().take().is_none(), "no longer waiting");
}

#[test]
fn a_refused_frame_size_is_remembered_and_counted() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = crate::MainWindow::new().unwrap();
    let (ctrl, _) = with_mock_renderer();
    ctrl.put_frame_for_test(Owed::REDRAW);
    // A lost device: nothing shown, nothing counted, nothing owed.
    ctrl.show_frame(&ui, Err(FrameRefusal::Lost), (64, 32));
    assert!(!ctrl.frame_for_test().includes(Owed::REDRAW));
    assert_eq!(ctrl.scene_loader().render_failure_count_for_test(), 0);
    assert!(!ctrl.refused_size_for_test((64, 32)));
    // A refusal: that size is skipped from now on, and counted.
    let refused = FrameRefusal::Refused {
        resource: FrameResource::Outputs,
        source: refusal("out of memory"),
    };
    ctrl.show_frame(&ui, Err(refused), (64, 32));
    assert!(ctrl.refused_size_for_test((64, 32)));
    assert_eq!(ctrl.scene_loader().render_failure_count_for_test(), 1);
}

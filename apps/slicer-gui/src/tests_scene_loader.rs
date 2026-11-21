// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The load state machine without a window: `new_for_test` has no window to
//! update, and `drain_deposit_for_test` skips the error dialog, the only
//! part that needs an event loop.
use super::*;

fn loader() -> SceneLoadCoordinator {
    SceneLoadCoordinator::loader_for_test(MessageWindows::new())
}

fn failed() -> anyhow::Result<LoadedSceneBundle> {
    Err(anyhow::anyhow!("worker failed"))
}

fn deliver(ticket: SceneLoadTicket, file: &str) {
    ticket.deliver_loaded_scene(PathBuf::from(file), failed());
}

#[test]
fn one_load_at_a_time_until_its_ticket_goes() {
    let loader = loader();
    let first = loader.request_load_ticket().expect("nothing is loading");
    assert!(loader.request_load_ticket().is_none());
    // A cancelled dialog drops its ticket: loading may start again.
    drop(first);
    let second = loader.request_load_ticket().expect("the first load ended");
    // A delivered load still counts until it is taken.
    deliver(second, "/tmp/a.obj");
    assert!(loader.request_load_ticket().is_none());
    let (path, result) = loader.take_delivery_for_test().expect("current delivery");
    assert_eq!((path, result.err().unwrap().to_string()), (PathBuf::from("/tmp/a.obj"), "worker failed".to_string()));
    assert!(loader.request_load_ticket().is_some());
    assert!(loader.take_delivery_for_test().is_none());
    assert!(loader.take_completed_scene_load().is_none());
}

#[test]
fn each_load_gets_a_new_number_and_closing_the_scene_makes_results_stale() {
    let loader = loader();
    let first = loader.request_load_ticket().unwrap().load_sequence();
    let second = loader.request_load_ticket().unwrap();
    assert_eq!(second.load_sequence(), first + 1);
    // Closed while the worker runs: its later delivery is dropped.
    loader.cancel_and_clear_loads();
    deliver(second, "/tmp/late.obj");
    assert!(loader.take_delivery_for_test().is_none());
    let third = loader.request_load_ticket().expect("the stale delivery ended its load");
    assert_eq!(third.load_sequence(), first + 3);
    // Closed after delivery: the result is freed at once.
    deliver(third, "/tmp/done.obj");
    loader.cancel_and_clear_loads();
    assert!(loader.delivery_slot.lock().unwrap().is_none());
    assert!(loader.request_load_ticket().is_some());
}

#[test]
fn a_closed_window_never_keeps_its_slot_alive() {
    // Closed while the worker runs, then it delivers.
    let loader1 = loader();
    let ticket = loader1.request_load_ticket().unwrap();
    let slot = Arc::downgrade(&loader1.delivery_slot);
    drop(loader1);
    deliver(ticket, "/tmp/a.obj");
    assert!(slot.upgrade().is_none());
    // Delivered, then closed before the UI took it.
    let loader2 = loader();
    let slot = Arc::downgrade(&loader2.delivery_slot);
    deliver(loader2.request_load_ticket().unwrap(), "/tmp/b.obj");
    drop(loader2);
    assert!(slot.upgrade().is_none());
}

#[test]
fn starting_a_load_resets_the_progress() {
    let loader = loader();
    loader.worker_progress_sink().set(WorkerLoadPhase::GpuUpload, 1, 1);
    assert_ne!(loader.load_progress_for_test(), 0);
    loader.start_load_progress_display();
    assert_eq!(loader.load_progress_for_test(), 0);
}

#[test]
fn without_a_device_the_reason_is_known() {
    let loader = loader();
    assert!(loader.gpu_snapshot().is_none());
    loader.remove_graphics_device();
    assert!(loader.gpu_snapshot().is_none());
    assert_eq!(loader.unavailable_graphics_reason(), Some(NoGraphics::NotYet));
    // The latest API wins, and a teardown does not make it usable.
    loader.record_unsupported_graphics_api("GraphicsAPI::NativeOpenGL".into());
    loader.record_unsupported_graphics_api("GraphicsAPI::WebGL".into());
    loader.remove_graphics_device();
    assert_eq!(loader.unavailable_graphics_reason(), Some(NoGraphics::ForeignApi("GraphicsAPI::WebGL".into())));
    // A lost device says so, and the flag is the one the loader shares.
    let shared = loader.lost_device_state();
    loader.record_device_loss();
    assert_eq!(loader.unavailable_graphics_reason(), Some(NoGraphics::Gone));
    shared.mark("Destroyed: test".into());
    assert!(loader.lost_device_state().is_lost());
}

#[test]
fn failures_count_in_runs_that_a_success_or_reset_ends() {
    let loader = loader();
    let refused = || FrameRefusal::Refused {
        resource: FrameResource::Outputs,
        source: wgpu::Error::OutOfMemory { source: Box::new(std::io::Error::other("full")) },
    };
    // The first of a run opens a deferred dialog, which needs no event loop
    // until it runs.
    loader.record_render_failure(&refused());
    loader.record_render_failure(&refused());
    assert_eq!(loader.render_failure_count_for_test(), 2);
    loader.record_render_success();
    assert_eq!(loader.render_failure_count_for_test(), 0);
    loader.record_render_failure(&refused());
    loader.clear_render_failure_gate();
    assert_eq!(loader.render_failure_count_for_test(), 0);
    // A lost device is reported elsewhere, once.
    loader.record_render_failure(&FrameRefusal::Lost);
    assert_eq!(loader.render_failure_count_for_test(), 0);
    let err = anyhow::anyhow!("refused");
    loader.record_upload_failure(&err);
    loader.record_upload_failure(&err);
    assert_eq!(loader.upload_failure_count_for_test(), 2);
    loader.record_upload_success();
    assert_eq!(loader.upload_failure_count_for_test(), 0);
    loader.record_upload_failure(&err);
    loader.cancel_and_clear_loads();
    assert_eq!(loader.upload_failure_count_for_test(), 0);
    loader.record_upload_failure(&err);
    loader.clear_upload_failure_gate();
    assert_eq!(loader.upload_failure_count_for_test(), 0);
}

#[test]
fn the_window_stops_showing_a_load_once_its_last_ticket_goes() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().expect("window");
    let loader = SceneLoadCoordinator::new(&ui, MessageWindows::new(), DeviceLoss::default());
    let ticket = loader.request_load_ticket().expect("nothing is loading");
    loader.start_load_progress_display();
    assert!(ui.get_busy_opening() && ui.get_opening());
    drop(ticket);
    assert!(!ui.get_busy_opening(), "the loading flag goes with the ticket");
    assert!(!ui.get_opening(), "and so does the progress panel");
}

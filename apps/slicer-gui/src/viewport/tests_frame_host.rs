// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use std::rc::Rc;

use super::{FrameHost, Owed};
use crate::mesh_render::mock::{RendererScript, ScriptedRenderer};

/// A windowless host driving a scripted renderer, with the script's log.
fn scripted() -> (FrameHost, Rc<RefCell<RendererScript>>) {
    let host = FrameHost::without_window();
    let log = RendererScript::shared_script();
    host.renderer_slot().replace(Some(Box::new(ScriptedRenderer::new(log.clone()))));
    (host, log)
}
use std::cell::RefCell;

#[test]
fn owed_work_is_a_set_of_independent_flags() {
    let both = Owed::REDRAW.and(Owed::UPLOAD);
    assert!(both.includes(Owed::REDRAW) && both.includes(Owed::UPLOAD) && both.includes(both));
    assert!(!Owed::NOTHING.includes(Owed::REDRAW) && !Owed::REDRAW.includes(Owed::UPLOAD));
    assert!(!both.includes(Owed::NOTHING), "owing nothing is never a debt");
    let host = FrameHost::without_window();
    host.owe(Owed::UPLOAD);
    host.flag_dirty();
    assert_eq!(host.owed(), both);
    host.settle(Owed::REDRAW);
    assert_eq!(host.owed(), Owed::UPLOAD);
    host.uploaded();
    assert_eq!(host.owed(), Owed::REDRAW, "a sent scene still has to be drawn");
    host.settle(Owed::UPLOAD);
    assert_eq!(host.owed(), Owed::REDRAW);
    host.owe_exactly(Owed::NOTHING);
    assert!(!host.owes(Owed::REDRAW) && !host.owes(Owed::UPLOAD));
}

#[test]
fn a_fresh_host_has_no_size_no_renderer_and_no_debt() {
    let host = FrameHost::without_window();
    assert_eq!(host.view_extent(), None);
    assert!(host.renderer_slot().borrow().is_none());
    assert!(!host.is_resizing() && !host.owes(Owed::REDRAW) && !host.size_was_refused((0, 0)));
    // With no window, asking for a frame is harmless.
    host.queue_redraw();
}

#[test]
fn one_resize_is_exact_but_two_in_quick_succession_are_a_drag() {
    let (host, log) = scripted();
    host.take_view_size(1024.0, 700.0);
    let size = host.view_extent().expect("a usable size");
    assert_eq!((size.width(), size.height()), (1024.0, 700.0));
    assert!(host.owes(Owed::REDRAW) && !host.is_resizing());
    assert_eq!(host.rounded_size(1025, 701), (1025, 701));
    host.take_view_size(1040.0, 700.0);
    assert!(host.is_resizing());
    // Whole 64 px steps while dragging; zero stays zero.
    for ((w, h), stepped) in [((1, 1), (64, 64)), ((64, 128), (64, 128)), ((65, 0), (128, 0)), ((1234, 567), (1280, 576))] {
        assert_eq!(host.rounded_size(w, h), stepped, "{w}x{h}");
    }
    host.resize_settled();
    assert!(!host.is_resizing());
    assert_eq!(log.borrow().size_resets(), 1, "the drag's stepped targets are dropped");
    host.resize_settled();
    assert_eq!(log.borrow().size_resets(), 1, "no drag, nothing to drop");
    // The same size again changes nothing; an unusable one clears the size.
    host.owe_exactly(Owed::NOTHING);
    host.take_view_size(1040.0, 700.0);
    assert!(!host.owes(Owed::REDRAW));
    host.take_view_size(0.0, 700.0);
    assert!(host.view_extent().is_none() && host.owes(Owed::REDRAW));
}

#[test]
fn a_refused_target_size_waits_for_a_change() {
    let host = FrameHost::without_window();
    host.flag_dirty();
    host.record_refused_size((320, 200));
    assert!(host.size_was_refused((320, 200)) && !host.size_was_refused((320, 201)));
    assert!(!host.owes(Owed::REDRAW), "the refused frame counts as drawn");
    let changes: [fn(&FrameHost); 3] = [|h| h.take_view_size(640.0, 480.0), |h| h.resize_settled(), |h| h.record_drawn()];
    for change in changes {
        host.record_refused_size((320, 200));
        change(&host);
        assert!(!host.size_was_refused((320, 200)));
    }
}

#[test]
fn losing_the_device_drops_the_renderer_but_clearing_the_scene_does_not() {
    let (host, log) = scripted();
    host.drop_renderer_scene();
    assert_eq!(log.borrow().scene_drops(), 1);
    assert!(host.renderer_slot().borrow().is_some());
    host.flag_dirty();
    host.gpu_departed();
    assert!(host.renderer_slot().borrow().is_none() && !host.owes(Owed::REDRAW));
    host.drop_renderer_scene();
    assert_eq!(log.borrow().scene_drops(), 1, "nothing left to clear");
}

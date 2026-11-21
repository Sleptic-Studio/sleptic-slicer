// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Everything the viewport needs to put a frame on screen, apart from what
//! is in it: the GPU renderer, the view's size, which work the next frame
//! owes, and window resizing. The controller composes frames
//! (`handle_before_rendering`) from what is kept here.
//!
//! Owing a redraw also asks the window for a frame, so callers never have
//! to remember the second step.
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use slint::{ComponentHandle, Weak};

use super::screen::ViewportSize;
use crate::MainWindow;
use crate::mesh_render::{DeviceLoss, DeviceStamp, MeshRenderer, StartRefusal, ViewRenderer};

/// While the window edge is being dragged, render targets are made in
/// steps of this many pixels so each move does not reallocate them; the
/// picture is stretched meanwhile (under 3% on a 1080p view).
const RESIZE_STEP_PX: u32 = 64;

/// A pause this long ends a resize drag (the debounce timer in
/// `wiring.rs` waits for it), and a second size within it starts one.
pub(crate) const RESIZE_QUIET: Duration = Duration::from_millis(200);

/// Work the next frame owes, as a set of flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Owed(u8);

impl Owed {
    pub(super) const NOTHING: Owed = Owed(0);
    /// Something on screen changed.
    pub(super) const REDRAW: Owed = Owed(1);
    /// The scene differs from the GPU's copy (and, once sent, must be drawn).
    pub(super) const UPLOAD: Owed = Owed(2);

    pub(super) fn and(self, more: Owed) -> Owed {
        Owed(self.0 | more.0)
    }

    pub(super) fn includes(self, work: Owed) -> bool {
        self.0 & work.0 == work.0 && work.0 != 0
    }

    fn without(self, work: Owed) -> Owed {
        Owed(self.0 & !work.0)
    }
}

/// The renderer, once a GPU is there, and the one target size the device
/// last refused (frames at it are skipped until something changes).
#[derive(Default)]
struct Gpu {
    renderer: RefCell<Option<Box<dyn MeshRenderer>>>,
    refused: Cell<Option<(u32, u32)>>,
}

/// Whether the window is being resized by dragging, told apart from a lone
/// resize (a panel shown or hidden), which renders at its exact size at once
/// (R116: it drew a stepped frame and again 200 ms later, a visible flash).
#[derive(Default)]
struct Resizing {
    dragging: Cell<bool>,
    last_change: Cell<Option<Instant>>,
}

impl Resizing {
    /// A size change at `now`; whether it is part of a drag.
    fn changed(&self, now: Instant) -> bool {
        let earlier = self.last_change.replace(Some(now));
        let drag = earlier.is_some_and(|then| now.duration_since(then) < RESIZE_QUIET);
        self.dragging.set(drag);
        drag
    }

    fn stepped(&self, width: u32, height: u32) -> (u32, u32) {
        if !self.dragging.get() {
            return (width, height);
        }
        let up = |side: u32| side.div_ceil(RESIZE_STEP_PX) * RESIZE_STEP_PX;
        (up(width), up(height))
    }
}

pub(super) struct FrameHost {
    window: Weak<MainWindow>,
    gpu: Gpu,
    size: Cell<Option<ViewportSize>>,
    owed: Cell<Owed>,
    resizing: Resizing,
}

impl FrameHost {
    pub(super) fn new(ui: &MainWindow) -> Self {
        Self::on(ui.as_weak())
    }

    /// A host with no window: asking for frames does nothing and the owed
    /// work is all there is to look at.
    #[cfg(test)]
    pub(super) fn without_window() -> Self {
        Self::on(Weak::default())
    }

    fn on(window: Weak<MainWindow>) -> Self {
        FrameHost { window, gpu: Gpu::default(), size: Cell::new(None), owed: Cell::default(), resizing: Resizing::default() }
    }

    pub(super) fn renderer_slot(&self) -> &RefCell<Option<Box<dyn MeshRenderer>>> {
        &self.gpu.renderer
    }

    pub(super) fn view_extent(&self) -> Option<ViewportSize> {
        self.size.get()
    }

    /// Whether the next frame owes `work`.
    pub(super) fn owes(&self, work: Owed) -> bool {
        self.owed.get().includes(work)
    }

    /// Adds `work` to what the next frame owes, asking for that frame when
    /// a redraw is part of it.
    pub(super) fn owe(&self, work: Owed) {
        self.owed.set(self.owed.get().and(work));
        if work.includes(Owed::REDRAW) {
            self.queue_redraw();
        }
    }

    /// `work` is done (or no longer wanted).
    pub(super) fn settle(&self, work: Owed) {
        self.owed.set(self.owed.get().without(work));
    }

    /// The scene reached the GPU, so it now has to be drawn.
    pub(super) fn uploaded(&self) {
        self.settle(Owed::UPLOAD);
        self.owed.set(self.owed.get().and(Owed::REDRAW));
    }

    /// Replaces what is owed outright (a fresh session, or a test).
    pub(super) fn owe_exactly(&self, work: Owed) {
        self.owed.set(work);
    }

    #[cfg(test)]
    pub(super) fn owed(&self) -> Owed {
        self.owed.get()
    }

    /// Owes a redraw and asks the window for a frame.
    pub(super) fn flag_dirty(&self) {
        self.owe(Owed::REDRAW);
    }

    /// Asks the window for a frame without owing anything new.
    pub(super) fn queue_redraw(&self) {
        if let Some(open) = self.window.upgrade() {
            open.window().request_redraw();
        }
    }

    /// The view is now `w` x `h`. The same size again (it is read before
    /// every frame too, R116) changes nothing.
    pub(super) fn take_view_size(&self, w: f32, h: f32) {
        let size = ViewportSize::new(w, h);
        if self.size.get() == size {
            return;
        }
        self.size.set(size);
        self.gpu.refused.set(None);
        self.resizing.changed(Instant::now());
        self.flag_dirty();
    }

    /// Resizing paused: a drag's stepped targets give way to exact ones.
    pub(super) fn resize_settled(&self) {
        self.gpu.refused.set(None);
        if self.resizing.dragging.replace(false) {
            self.with_renderer(|renderer| renderer.forget_sizes());
            self.flag_dirty();
        }
    }

    /// The render target size for a `width` x `height` (physical pixels)
    /// view: exact, or rounded up to whole steps while dragging.
    pub(super) fn rounded_size(&self, width: u32, height: u32) -> (u32, u32) {
        self.resizing.stepped(width, height)
    }

    #[cfg(test)]
    pub(super) fn is_resizing(&self) -> bool {
        self.resizing.dragging.get()
    }

    pub(super) fn size_was_refused(&self, size: (u32, u32)) -> bool {
        self.gpu.refused.get() == Some(size)
    }

    /// The device would not make targets of `size`; that frame counts as
    /// drawn so it is not retried every vsync.
    pub(super) fn record_refused_size(&self, size: (u32, u32)) {
        self.gpu.refused.set(Some(size));
        self.settle(Owed::REDRAW);
    }

    pub(super) fn record_drawn(&self) {
        self.gpu.refused.set(None);
    }

    /// A GPU device is ready: a renderer is built on it.
    pub(super) fn gpu_arrived(
        &self,
        device: wgpu::Device,
        stamp: DeviceStamp,
        queue: wgpu::Queue,
        loss: DeviceLoss,
    ) -> Result<(), StartRefusal> {
        let renderer: Box<dyn MeshRenderer> = Box::new(ViewRenderer::new(device, stamp, queue, loss)?);
        self.gpu.renderer.replace(Some(renderer));
        self.gpu.refused.set(None);
        Ok(())
    }

    /// The device is gone, and with it the renderer and any redraw owed.
    pub(super) fn gpu_departed(&self) {
        self.gpu.renderer.replace(None);
        self.settle(Owed::REDRAW);
    }

    /// The scene was cleared: the renderer stays but forgets its meshes.
    pub(super) fn drop_renderer_scene(&self) {
        self.with_renderer(|renderer| renderer.drop_scene());
    }

    fn with_renderer(&self, work: impl FnOnce(&mut dyn MeshRenderer)) {
        if let Some(renderer) = self.gpu.renderer.borrow_mut().as_deref_mut() {
            work(renderer);
        }
    }

    #[cfg(test)]
    pub(super) fn put_view_extent(&self, w: f32, h: f32) {
        self.size.set(ViewportSize::new(w, h));
    }
}

#[cfg(test)]
#[path = "tests_frame_host.rs"]
mod tests;

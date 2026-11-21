// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The viewport's orbit camera: framing a scene and the pointer-driven
//! moves. It lives beside the scene, not in the renderer, so the view
//! survives the GPU being torn down and set up again, and can change
//! before there is a GPU at all.
//!
//! The moves return whether they moved the camera, in which case the
//! controller owes a redraw.
use std::cell::{Cell, RefCell};

use glam::Vec3;

use super::screen::{ScreenDelta, ScreenPoint, ViewportSize};
use crate::build_volume::BuildVolume;
use crate::camera::{ViewAxis, ViewCamera};
use crate::mesh_render::starting_fov_y;

/// Zoom multiplies the distance by `exp(-delta * ZOOM_RATE_PER_DELTA)`
/// per scroll delta, so N notches in and N out end where they began.
const ZOOM_PER_WHEEL_UNIT: f32 = 0.005;

/// The most one scroll event may zoom, in or out: a huge delta would
/// otherwise round the factor to 0 or infinity.
const ZOOM_STEP_LIMIT: f32 = 10.0;

pub(super) struct ViewHolder {
    camera: RefCell<Option<ViewCamera>>,
    /// Perspective or orthographic, for this camera and any framed later.
    /// Perspective until the window applies the View setting.
    perspective: Cell<bool>,
}

impl ViewHolder {
    pub(super) fn new() -> Self {
        Self { camera: RefCell::default(), perspective: Cell::new(true) }
    }

    pub(super) fn set_perspective(&self, perspective: bool) {
        self.perspective.set(perspective);
        self.adjust(|camera| camera.is_perspective = perspective);
    }

    /// The camera itself, for picking, rendering and the gizmo.
    pub(super) fn view_cell(&self) -> &RefCell<Option<ViewCamera>> {
        &self.camera
    }

    /// Whether a camera has been framed.
    pub(super) fn has_view(&self) -> bool {
        self.camera.borrow().is_some()
    }

    /// Drops the camera, so the next scene is framed afresh.
    pub(super) fn forget_view(&self) {
        self.camera.take();
    }

    /// A new camera framing the scene and the build volume together, so
    /// the plate is always in view. It may come as close as the scene's
    /// own size allows (the volume's, without a scene), so a model much
    /// smaller than the plate can still fill the view.
    pub(super) fn frame(&self, scene_bounds: Option<(Vec3, Vec3)>, build_volume: BuildVolume) {
        let (aim, everything) = build_volume.framing_sphere(scene_bounds);
        // Half the scene's diagonal (at least a micron): how close the
        // scene itself lets the camera come.
        let closest = scene_bounds.map_or(everything, |(low, high)| ((high - low).length() * 0.5).max(1e-3));
        let mut fresh = ViewCamera::framing_with_detail(aim, everything, closest);
        fresh.is_perspective = self.perspective.get();
        self.camera.replace(Some(fresh));
    }

    /// Runs `step` on the camera; whether there was one.
    fn adjust(&self, step: impl FnOnce(&mut ViewCamera)) -> bool {
        self.camera.borrow_mut().as_mut().map(step).is_some()
    }

    pub(super) fn orbit_by(&self, delta: ScreenDelta, viewport: Option<ViewportSize>) -> bool {
        viewport.is_some_and(|vp| {
            self.adjust(|camera| camera.turn_by_drag(delta.x, delta.y, vp.height()))
        })
    }

    pub(super) fn pan_by(&self, delta: ScreenDelta, viewport: Option<ViewportSize>) -> bool {
        viewport.is_some_and(|vp| {
            self.adjust(|camera| camera.shift_by_drag(delta.x, delta.y, starting_fov_y(), vp.height()))
        })
    }

    /// Zooms toward the point under the cursor by a scroll of
    /// `wheel_delta` (positive comes closer).
    pub(super) fn zoom_at(&self, wheel_delta: f32, cursor: ScreenPoint, viewport: Option<ViewportSize>) -> bool {
        let Some(view) = viewport else {
            return false;
        };
        let scale = (-wheel_delta * ZOOM_PER_WHEEL_UNIT).exp();
        let scale = scale.clamp(ZOOM_STEP_LIMIT.recip(), ZOOM_STEP_LIMIT);
        let fov = starting_fov_y();
        self.adjust(|camera| camera.zoom_about(scale, cursor.x, cursor.y, fov, view.width(), view.height()))
    }

    /// Back to the framed view.
    pub(super) fn go_home(&self) -> bool {
        self.adjust(ViewCamera::home_view)
    }

    /// Looks from `toward_eye` (a view cube face, edge or corner).
    pub(super) fn look_from(&self, toward_eye: Vec3) -> bool {
        self.adjust(|camera| camera.look_from_direction(toward_eye))
    }

    /// Looks along one of the six axis directions.
    pub(super) fn look_along(&self, view: ViewAxis) -> bool {
        self.adjust(|camera| camera.snap_to_axis(view))
    }
}

#[cfg(test)]
#[path = "tests_orbit.rs"]
mod tests;

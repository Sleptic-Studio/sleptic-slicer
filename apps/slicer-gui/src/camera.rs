// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The 3D view's camera: it orbits a target point, with Z up.
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use glam::{Mat4, Vec3};

/// Pitch stops this short of straight up or down, where the view's
/// sideways direction would be undefined.
const POLE_MARGIN: f32 = 1e-3;
const CLOSEST_SHARE: f32 = 0.05;
const FARTHEST_SHARE: f32 = 50.0;
/// The closest zoom as a share of the view radius: the renderer's near
/// plane sits at 0.02 of it at most (`frame::NEAR_SHARE` 0.01 of a framing
/// diagonal up to twice the radius), so 0.03 keeps the scene in front of it.
const NEAR_PLANE_SHARE: f32 = 0.03;
/// Home is the true isometric view down the view cube's front-right-top
/// corner (front is -Y), exactly what clicking that corner shows. Tool
/// icons on the plate and the cube's labels read upright from here.
const HOME_AZIMUTH: f32 = -FRAC_PI_4;
/// asin(1/√3), 35.26°.
const HOME_ELEVATION: f32 = 0.615_479_7;
pub(crate) const HOME_RANGE_FACTOR: f32 = 3.0;

/// The six views along an axis (the view cube's faces).
#[derive(Copy, Clone, Debug)]
pub enum ViewAxis {
    Front,
    Back,
    Right,
    Left,
    Top,
    Bottom,
}

impl ViewAxis {
    /// Where the eye is, seen from the target. Front looks along +Y (the
    /// plate's front is -Y), and Z is up.
    fn toward_eye(self) -> Vec3 {
        match self {
            ViewAxis::Front => Vec3::NEG_Y,
            ViewAxis::Back => Vec3::Y,
            ViewAxis::Right => Vec3::X,
            ViewAxis::Left => Vec3::NEG_X,
            ViewAxis::Top => Vec3::Z,
            ViewAxis::Bottom => Vec3::NEG_Z,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ViewCamera {
    pub look_at: Vec3,
    pub azimuth: f32,
    pub elevation: f32,
    pub eye_distance: f32,
    pub up_axis: Vec3,
    /// False is orthographic, which the app chooses by default (perspective
    /// is a View setting).
    pub is_perspective: bool,
    /// Where Home and the axis views aim.
    scene_center: Vec3,
    /// Sets the framing distance and how far out one can zoom: the plate
    /// (or plate and scene), so the plate can always be brought back.
    view_radius: f32,
    /// Sets how far in one can zoom: the scene's own size, so a small part
    /// on a big plate can be looked at closely.
    detail_radius: f32,
}

impl ViewCamera {
    /// One radius for both framing and zooming in.
    #[cfg(test)]
    pub fn new(scene_center: Vec3, scene_radius: f32) -> Self {
        Self::framing_with_detail(scene_center, scene_radius, scene_radius)
    }

    /// At Home, aimed at `scene_center`. Radii below a micron are raised
    /// to one so the arithmetic stays sound.
    pub fn framing_with_detail(scene_center: Vec3, view_radius: f32, detail_radius: f32) -> Self {
        let view_radius = view_radius.max(1e-3);
        let mut camera = Self {
            look_at: scene_center,
            azimuth: 0.0,
            elevation: 0.0,
            eye_distance: 0.0,
            up_axis: Vec3::Z,
            is_perspective: true,
            scene_center,
            view_radius,
            detail_radius: detail_radius.max(1e-3),
        };
        camera.home_view();
        camera
    }

    fn aim_home(&mut self) {
        self.look_at = self.scene_center;
        self.eye_distance = self.view_radius * HOME_RANGE_FACTOR;
    }

    /// Home.
    pub fn home_view(&mut self) {
        self.aim_home();
        self.azimuth = HOME_AZIMUTH;
        self.elevation = HOME_ELEVATION;
    }

    /// Centres a box and moves in to show it, keeping the viewing angle.
    pub fn zoom_to_box(&mut self, min: Vec3, max: Vec3) {
        let radius = ((max - min).length() * 0.5).max(self.detail_radius);
        self.look_at = (min + max) * 0.5;
        self.eye_distance = radius * HOME_RANGE_FACTOR * 0.8;
    }

    pub fn snap_to_axis(&mut self, view: ViewAxis) {
        self.look_from_direction(view.toward_eye());
    }

    /// Looks at the scene from `toward_eye` (any length; the view cube's
    /// faces, edges and corners). Looking straight down or up keeps the Top
    /// view's heading, so the plate's front stays at the bottom.
    pub fn look_from_direction(&mut self, toward_eye: Vec3) {
        let d = toward_eye.normalize_or_zero();
        if d == Vec3::ZERO {
            return;
        }
        self.aim_home();
        let vertical = d.x.abs() < 1e-4 && d.y.abs() < 1e-4;
        self.azimuth = if vertical { -FRAC_PI_2 } else { d.y.atan2(d.x) };
        self.elevation = limit_elevation(d.z.clamp(-1.0, 1.0).asin());
    }

    /// Dragging the full view height turns half a circle.
    pub fn turn_by_drag(&mut self, dx_pixels: f32, dy_pixels: f32, viewport_height: f32) {
        let per_pixel = PI / viewport_height.max(1.0);
        self.azimuth -= dx_pixels * per_pixel;
        self.elevation = limit_elevation(self.elevation + dy_pixels * per_pixel);
    }

    /// The world offset that `x` right and `y` down on screen stand for,
    /// at `step` millimetres per pixel.
    fn screen_offset(&self, x: f32, y: f32, step: f32) -> Vec3 {
        let (right, up) = self.right_and_up();
        x * step * right - y * step * up
    }

    /// Moves the target so the scene follows the mouse.
    pub fn shift_by_drag(&mut self, dx_pixels: f32, dy_pixels: f32, fov_y: f32, viewport_height: f32) {
        let step = self.mm_per_pixel(fov_y, viewport_height);
        self.look_at -= self.screen_offset(dx_pixels, dy_pixels, step);
    }

    /// Zooms by `factor` (held between 0.1 and 10 per call) about the point
    /// under the cursor, which keeps its place on screen, within the zoom
    /// limits.
    pub fn zoom_about(
        &mut self,
        factor: f32,
        cursor_x: f32,
        cursor_y: f32,
        fov_y: f32,
        viewport_width: f32,
        viewport_height: f32,
    ) {
        let factor = factor.clamp(0.1, 10.0);
        let step = self.mm_per_pixel(fov_y, viewport_height);
        let from_centre = self.screen_offset(cursor_x - viewport_width * 0.5, cursor_y - viewport_height * 0.5, step);
        let anchor = self.look_at + from_centre;
        self.look_at += (1.0 - factor) * (anchor - self.look_at);
        self.eye_distance = (self.eye_distance * factor).clamp(self.closest_range(), self.farthest_range());
    }

    pub fn world_to_view(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye_position(), self.look_at, self.up_axis)
    }

    pub fn eye_position(&self) -> Vec3 {
        let (sin_pitch, cos_pitch) = self.elevation.sin_cos();
        let (sin_yaw, cos_yaw) = self.azimuth.sin_cos();
        self.look_at
            + Vec3::new(
                self.eye_distance * cos_pitch * cos_yaw,
                self.eye_distance * cos_pitch * sin_yaw,
                self.eye_distance * sin_pitch,
            )
    }

    /// Perspective, or orthographic sized so a millimetre at the target is
    /// as large as in perspective: zoom feels the same and switching does
    /// not jump. The orthographic box reaches `far`/20 behind the eye and
    /// past the target, so zooming in (moving the eye in) never clips.
    pub fn view_to_clip(&self, fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
        if self.is_perspective {
            return Mat4::perspective_rh(fov_y, aspect, near, far);
        }
        let h = self.eye_distance * (fov_y * 0.5).tan();
        let depth = far * 0.05;
        Mat4::orthographic_rh(-h * aspect, h * aspect, -h, h, -depth, depth + self.eye_distance)
    }

    /// The view's right and up directions in the world. Near straight down
    /// or up, the line of sight is almost parallel to Z and round-off would
    /// flip "right"; world Y stands in for Z there so the directions turn
    /// smoothly over the pole.
    pub(crate) fn right_and_up(&self) -> (Vec3, Vec3) {
        let forward = (self.look_at - self.eye_position()).normalize_or_zero();
        let reference = if forward.dot(self.up_axis).abs() > 0.99 { Vec3::Y } else { self.up_axis };
        let right = forward.cross(reference).normalize_or_zero();
        (right, right.cross(forward).normalize_or_zero())
    }

    /// Size of one logical pixel in the world at the target's distance;
    /// panning, zooming and the handles all measure with it.
    pub fn mm_per_pixel(&self, fov_y: f32, viewport_height: f32) -> f32 {
        2.0 * self.eye_distance * (fov_y * 0.5).tan() / viewport_height.max(1.0)
    }

    /// Close enough for the smallest detail, but never inside the near plane.
    fn closest_range(&self) -> f32 {
        (self.detail_radius * CLOSEST_SHARE).max(self.view_radius * NEAR_PLANE_SHARE)
    }

    fn farthest_range(&self) -> f32 {
        self.view_radius * FARTHEST_SHARE
    }
}

fn limit_elevation(pitch: f32) -> f32 {
    let limit = FRAC_PI_2 - POLE_MARGIN;
    pitch.clamp(-limit, limit)
}

#[cfg(test)]
#[path = "tests_camera.rs"]
mod tests;

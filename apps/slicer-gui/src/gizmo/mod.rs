// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The transform handles' maths, free of the GPU so it can be tested
//! alone: where the handles sit, how big they are, what a click hits and
//! how a drag turns into a move, turn or scale.
pub(crate) mod axis;
pub(crate) mod drag_math;
pub(crate) mod frame;
pub(crate) mod hit_test;

use glam::Vec3;
use sc_mesh_spatial::SceneSpatial;
use sc_scene::NodeId;

use crate::camera::ViewCamera;

/// How big the handles are. Pixel sizes stay the same on screen at any
/// zoom (`gizmo_scale`); the others are shares of one handle length.
pub(crate) struct HandleSizes {
    /// A move arrow's length on screen: room for its head without crowding
    /// the centre.
    pub arm_px: f32,
    /// How far from a handle, on screen, a click still takes it; more than
    /// the drawn shaft, so a near miss counts.
    pub grab_radius_px: f32,
    /// Rotate rings' tube radius, about 4 px at the usual length (the ring
    /// weight common slicers and modellers use).
    pub ring_tube: f32,
    /// A scale cube's side on screen.
    pub cube_px: f32,
    /// Where on its shaft the scale cube sits: the end (scale and move are
    /// separate tools and never share a shaft).
    pub cube_at: f32,
    /// Rotate ring radius.
    pub ring_radius: f32,
    /// The stretch of a move arrow that takes clicks: its head and a little
    /// past the tip (1.18), never the shaft.
    pub cone_grab: (f32, f32),
}

pub(crate) const SIZES: HandleSizes = HandleSizes {
    arm_px: 120.0,
    grab_radius_px: 6.0,
    ring_tube: 0.018,
    cube_px: 16.0,
    cube_at: 1.0,
    ring_radius: 0.85,
    cone_grab: (1.0, 1.25),
};

/// When the view makes a handle too unstable to use, and how far a scale
/// drag may go.
pub(crate) struct HandleLimits {
    /// cos 8°: a move axis this close to the line of sight is locked (and
    /// drawn grey), since a pixel of mouse would be a huge move along it.
    pub axis_edge_on_cos: f32,
    /// cos 82°: a rotate ring seen within 8° of edge-on is locked, since
    /// where the mouse meets its plane jumps with every pixel.
    pub ring_edge_on_cos: f32,
    /// The smallest scale factor (the largest is its inverse); it also keeps
    /// handles from mirroring a part.
    pub min_scale: f32,
}

pub(crate) const LIMITS: HandleLimits = HandleLimits { axis_edge_on_cos: 0.990_268_07, ring_edge_on_cos: 0.139_173_1, min_scale: 1e-3 };


/// Where a part's handles sit: the centre of its placed bounding box, so X
/// and Y match the Position fields and Z is half the part's height above
/// it. A part with no usable geometry gets no handles.
pub(crate) fn part_pivot(spatial: &SceneSpatial, node_id: NodeId) -> Option<Vec3> {
    let leaf = spatial.leaf(node_id)?;
    let bounds = leaf.transformed_aabb(leaf.world_transform())?;
    let centre = (Vec3::from(bounds.min().coords()) + Vec3::from(bounds.max().coords())) * 0.5;
    centre.is_finite().then_some(centre)
}

/// Size in world units of one screen pixel at the orbit target, the same
/// measure panning uses.
fn world_per_pixel(camera: &ViewCamera, fov_y: f32, viewport_h: f32) -> f32 {
    2.0 * camera.eye_distance * (fov_y * 0.5).tan() / viewport_h.max(1.0)
}

/// Handle length in world units for a constant on-screen length.
pub(crate) fn gizmo_scale(camera: &ViewCamera, fov_y: f32, viewport_h: f32) -> f32 {
    world_per_pixel(camera, fov_y, viewport_h) * SIZES.arm_px
}

/// Click radius around a handle in world units, constant on screen.
pub(crate) fn hit_radius_world(camera: &ViewCamera, fov_y: f32, viewport_h: f32) -> f32 {
    world_per_pixel(camera, fov_y, viewport_h) * SIZES.grab_radius_px
}

#[cfg(test)]
mod tests;

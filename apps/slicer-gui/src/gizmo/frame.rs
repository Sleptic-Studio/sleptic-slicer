// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The handles to draw this frame, as the renderer wants them.
use sc_mesh_spatial::SceneSpatial;
use sc_scene::NodeId;

use crate::camera::ViewCamera;
use crate::mesh_render::{HandleFrame, HandleHover, HandleShowing, LiveDrag, starting_fov_y};
use crate::picking::PickRay;

use super::axis::ALL_AXES;
use super::hit_test::{arc_pickable, axis_pickable};
use super::{gizmo_scale, part_pivot};

/// Which arrows and which rings are locked, judged along the camera's line
/// of sight: the handles are small and close to the middle of the view, so
/// any ray through them is within about a degree of it.
fn locked_handles(camera: &ViewCamera) -> ([bool; 3], [bool; 3]) {
    let eye = camera.eye_position();
    let sight = PickRay { start: eye, heading: (camera.look_at - eye).normalize_or_zero() };
    (ALL_AXES.map(|axis| !axis_pickable(&sight, axis)), ALL_AXES.map(|axis| !arc_pickable(&sight, axis)))
}

/// The handles for the selected part; `None` when it is not a part or has
/// no geometry to centre them on.
pub(crate) fn handle_frame_for(
    spatial: &SceneSpatial,
    camera: &ViewCamera,
    selection: NodeId,
    viewport_h_logical: f32,
    visibility: HandleShowing,
    hover: Option<HandleHover>,
    drag_visual: Option<LiveDrag>,
) -> Option<HandleFrame> {
    let anchor = part_pivot(spatial, selection)?;
    let (fixed_axes, fixed_rings) = locked_handles(camera);
    let arm_length = gizmo_scale(camera, starting_fov_y(), viewport_h_logical);
    Some(HandleFrame { anchor, arm_length, fixed_axes, fixed_rings, showing: visibility, under_pointer: hover, live_drag: drag_visual })
}

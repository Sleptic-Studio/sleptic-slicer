// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The gizmo the renderer draws this frame: which handles show (from the
//! gizmo state machine's drag or mode, and the hovered handle), built
//! around the selected part.
use sc_mesh_spatial::SceneSpatial;
use sc_scene::NodeId;

use super::handle_drag::{DragKind, HandleDragState};
use super::router::ViewportInputRouter;
use crate::camera::ViewCamera;
use crate::gizmo::frame::handle_frame_for;
use crate::mesh_render::{LiveDrag, HandleFrame, HandleShowing};

pub(super) struct HandleFramer;

impl HandleFramer {
    /// The gizmo for `selection`; `None` with nothing selected or nothing
    /// pickable in it.
    pub(super) fn frame_for(
        input: &ViewportInputRouter,
        spatial: &SceneSpatial,
        camera: &ViewCamera,
        selection: Option<NodeId>,
        viewport_h_logical: f32,
    ) -> Option<HandleFrame> {
        let fsm = input.viewport_gizmo_machine();
        let (visibility, drag) = gizmo_look(&fsm.drag_snapshot(), fsm.mode());
        let hover = fsm.hover_snapshot();
        handle_frame_for(
            spatial,
            camera,
            selection?,
            viewport_h_logical,
            visibility,
            hover,
            drag,
        )
    }
}

/// What the gizmo shows for `state`: the `idle` mode's handles, or, while
/// dragging, the dragged handle (a plate drag shows none, a plane drag the
/// move handles, a uniform scale the scale handles). An axis scale also
/// moves its cube to the pointer, unless it is proportional (Shift), which
/// scales evenly and has no pointer along the axis. A rotation's angle is
/// drawn by the protractor overlay, not the rings.
fn gizmo_look(
    state: &HandleDragState,
    idle: HandleShowing,
) -> (HandleShowing, Option<LiveDrag>) {
    let HandleDragState::HandleHeld { op, pivot, .. } = state else {
        return (idle, None);
    };
    match *op {
        DragKind::Plate { .. } => (HandleShowing::Resting, None),
        DragKind::TranslatePlane { .. } => (HandleShowing::Move, None),
        DragKind::ScaleUniform { .. } => (HandleShowing::Resize, None),
        DragKind::Translate { axis, .. } => (HandleShowing::Moving { axis }, None),
        DragKind::Rotate { axis, .. } => (HandleShowing::Turning { axis }, None),
        DragKind::ScaleAxis {
            axis,
            current_anchor,
            proportional,
            ..
        } => {
            let cube = (!proportional).then(|| LiveDrag::Stretch {
                axis,
                reach: (current_anchor - *pivot).dot(axis.unit_vector()),
            });
            (HandleShowing::Stretching { axis }, cube)
        }
    }
}

#[cfg(test)]
#[path = "tests_handle_frames.rs"]
mod tests;

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Routes viewport pointer and keyboard events to their state machines.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use sc_scene::NodeId;

use super::handle_states::{
    GizmoDragMachine, GizmoHoverContext as GizmoHoverCtx, GizmoMoveContext as GizmoMoveCtx, GizmoPressContext as GizmoStartCtx,
};
use super::buttons::{Button, PointerState, Release};
use super::arrow_keys::{KeyboardIntegrationContext, KeyboardTransformFocus, KeyboardMotionState};
use super::screen::{ScreenPoint, ViewportSize};
use crate::PointerModifiers;
use crate::build_volume::BuildVolume;
use crate::camera::ViewCamera;
use crate::mesh_render::starting_fov_y;
use crate::picking;
use crate::scene_document::SceneDocument;
use crate::selection::Selection;

/// Action the viewport coordinator should take after a pointer release.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum ViewportReleaseResult {
    ViewportDragCommitted,
    ViewportSelectionClick(ScreenPoint),
    HandleClick(ScreenPoint),
    ViewportReleaseIgnored,
}

/// A left press in progress: where it began and whether the pointer has
/// since gone farther than a click allows (even if it came back).
#[derive(Clone, Copy)]
struct Press {
    origin: ScreenPoint,
    travelled: bool,
}

/// The viewport's input: the buttons, the transform handles' drags, the
/// arrow keys, and the pointer's last place.
pub(super) struct ViewportInputRouter {
    buttons: PointerState,
    gizmo: GizmoDragMachine,
    keys: KeyboardMotionState,
    press: Cell<Option<Press>>,
    /// The last pointer position, and whether the pointer is still over the
    /// view (only then is it a hover anchor).
    pointer: Cell<Option<(ScreenPoint, bool)>>,
}

impl ViewportInputRouter {
    pub(super) fn new() -> Self {
        ViewportInputRouter {
            buttons: PointerState::new(),
            gizmo: GizmoDragMachine::new(),
            keys: KeyboardMotionState::new(),
            press: Cell::new(None),
            pointer: Cell::new(None),
        }
    }

    pub(super) fn viewport_gizmo_machine(&self) -> &GizmoDragMachine {
        &self.gizmo
    }

    pub(super) fn viewport_keyboard_motion(&self) -> &KeyboardMotionState {
        &self.keys
    }

    /// Forgets everything held against a scene that may be gone.
    pub(super) fn clear_viewport_input(&self) {
        self.gizmo.reset_drag_state();
        self.gizmo.remove_hover();
        [Button::Left, Button::Right].into_iter().for_each(|button| self.buttons.forget(button));
        self.keys.reset_keyboard_motion();
        self.press.set(None);
        self.pointer.set(None);
    }

    /// Remember the origin for context-menu click classification.
    pub(super) fn begin_context_pointer_press(&self, cursor: ScreenPoint) {
        self.buttons.press(Button::Right, cursor);
    }

    /// Return the press position only for a matching click.
    pub(super) fn finish_context_pointer_press(&self, cursor: ScreenPoint) -> Option<ScreenPoint> {
        match self.buttons.release(Button::Right, cursor) {
            Release::Click(origin) => Some(origin),
            Release::Drag | Release::NoPress => None,
        }
    }

    /// The pointer left the view: its last place is kept but no longer a
    /// hover anchor.
    pub(super) fn leave_viewport_pointer(&self) {
        self.pointer.set(self.pointer.get().map(|(at, _)| (at, false)));
    }

    #[cfg(test)]
    pub(super) fn last_viewport_hover_position(&self) -> Option<ScreenPoint> {
        self.pointer.get().filter(|(_, inside)| *inside).map(|(at, _)| at)
    }

    /// Record the press before attempting a handle or plate drag.
    pub(super) fn begin_viewport_pointer_press(
        &self,
        document: &Rc<SceneDocument>,
        camera: &RefCell<Option<ViewCamera>>,
        viewport: Option<ViewportSize>,
        cursor: ScreenPoint,
        modifiers: PointerModifiers,
        allow_transforms: bool,
    ) -> bool {
        self.buttons.press(Button::Left, cursor);
        self.press.set(Some(Press { origin: cursor, travelled: false }));
        // Selection clicks must still be recorded when transforms are disabled.
        if !allow_transforms {
            return false;
        }
        let start = |modifiers| GizmoStartCtx {
            gizmo_document: document,
            view_camera: camera,
            view_size: viewport,
            selected_node: document.main_chosen_node(),
            press_point: cursor,
            pointer_modifiers: modifiers,
        };
        self.gizmo.begin_handle_drag(start(modifiers.clone()));
        if !self.gizmo.is_dragging() {
            self.gizmo.try_start_plate_drag(start(modifiers));
        }
        self.gizmo.is_dragging()
    }

    /// The pointer is at `cursor` over the view. A press that has gone a
    /// drag's distance stays marked so, even if it comes back.
    pub(super) fn record_viewport_hover_position(&self, cursor: ScreenPoint) {
        self.pointer.set(Some((cursor, true)));
        let far = |origin: ScreenPoint| (cursor.x - origin.x).hypot(cursor.y - origin.y) >= super::buttons::DRAG_START_PX;
        self.press.set(self.press.get().map(|p| Press { travelled: p.travelled || far(p.origin), ..p }));
    }

    /// Update the active drag and report changes to the hover highlight.
    pub(super) fn move_viewport_pointer(
        &self,
        document: &Rc<SceneDocument>,
        camera: &RefCell<Option<ViewCamera>>,
        viewport: Option<ViewportSize>,
        build_volume: BuildVolume,
        cursor: ScreenPoint,
        modifiers: PointerModifiers,
    ) -> bool {
        let move_ctx = GizmoMoveCtx {
            gizmo_document: document,
            view_camera: camera,
            view_size: viewport,
            cursor_point: cursor,
            pointer_modifiers: modifiers,
            plate_volume: build_volume,
        };
        // A scene mutation schedules its own redraw; return hover changes here.
        let _ = self.gizmo.move_active_drag(move_ctx);

        let hover_ctx = GizmoHoverCtx {
            gizmo_document: document,
            view_camera: camera,
            view_size: viewport,
            selected_node: document.main_chosen_node(),
            cursor_point: cursor,
        };
        self.gizmo.refresh_handle_hover(hover_ctx)
    }

    /// Commit a drag or return a click for selection.
    pub(super) fn finish_viewport_pointer_press(
        &self,
        document: &Rc<SceneDocument>,
        cursor: ScreenPoint,
    ) -> ViewportReleaseResult {
        let clicked = matches!(self.buttons.release(Button::Left, cursor), Release::Click(_));
        let travelled = self.press.take().is_some_and(|p| p.travelled);
        let handle_click =
            clicked && !travelled && self.gizmo.is_handle_drag();
        if handle_click {
            self.gizmo.cancel_drag_edit(document);
            ViewportReleaseResult::HandleClick(cursor)
        } else if self.gizmo.finish_drag_edit(document) {
            ViewportReleaseResult::ViewportDragCommitted
        } else if clicked {
            ViewportReleaseResult::ViewportSelectionClick(cursor)
        } else {
            ViewportReleaseResult::ViewportReleaseIgnored
        }
    }

    /// Try the exact ray first, then increasingly distant tolerance rays.
    pub(super) fn pick_viewport_position(
        &self,
        document: &Rc<SceneDocument>,
        camera: &RefCell<Option<ViewCamera>>,
        viewport: Option<ViewportSize>,
        cursor: ScreenPoint,
    ) -> Option<Option<NodeId>> {
        let vp = viewport?;
        let camera_slot = camera.borrow();
        let camera = camera_slot.as_ref()?;
        let plate = Some((document.plate_width.get(), document.plate_depth.get()));
        document.read_scene(|spatial| {
            // Offset order gives the exact ray and nearer rings priority.
            pick_offsets().find_map(|(dx, dy)| {
                let ray = picking::ray_through_pixel(
                    camera,
                    starting_fov_y(),
                    vp.width(),
                    vp.height(),
                    cursor.x + dx,
                    cursor.y + dy,
                );
                picking::part_under_ray_on_plates(&ray, spatial, plate)
            })
        })
    }

    /// Revert an active drag and clear the associated click latch.
    pub(super) fn cancel_viewport_gizmo_drag(&self, document: &Rc<SceneDocument>) -> bool {
        if self.gizmo.cancel_drag_edit(document) {
            self.buttons.forget(Button::Left);
            true
        } else {
            false
        }
    }

    /// Switch keyboard modes only when the requested focus is valid.
    pub(super) fn change_viewport_keyboard_focus(
        &self,
        document: &Rc<SceneDocument>,
        focus: KeyboardTransformFocus,
    ) -> Option<KeyboardTransformFocus> {
        let has_selection = !matches!(document.chosen(), Selection::None);
        if self.keys.set_keyboard_transform_focus(document, focus, has_selection) {
            Some(focus)
        } else {
            None
        }
    }

    /// Ignore new arrow presses while keyboard focus is inactive.
    pub(super) fn press_viewport_gizmo_arrow(&self, document: &Rc<SceneDocument>, code: i32) -> bool {
        if self.keys.keyboard_transform_focus() == KeyboardTransformFocus::NoTransformFocus {
            return false;
        }
        self.update_arrow(document, code, true)
    }

    /// Release held keys even if focus has since been cleared.
    pub(super) fn release_viewport_gizmo_arrow(&self, document: &Rc<SceneDocument>, code: i32) -> bool {
        self.update_arrow(document, code, false)
    }

    fn update_arrow(&self, document: &Rc<SceneDocument>, code: i32, pressed: bool) -> bool {
        self.keys.update_held_arrow(
            document,
            document.main_chosen_node(),
            code,
            pressed,
        )
    }

    /// Apply the held keyboard input for the current render tick.
    pub(super) fn apply_viewport_keyboard_arrows(
        &self,
        document: &Rc<SceneDocument>,
        camera: &RefCell<Option<ViewCamera>>,
        viewport: Option<ViewportSize>,
        build_volume: BuildVolume,
    ) {
        let ctx = KeyboardIntegrationContext {
            keyboard_document: document,
            keyboard_camera: camera,
            keyboard_viewport: viewport,
            keyboard_selection_id: document.main_chosen_node(),
            keyboard_build_volume: build_volume,
        };
        let _ = self.keys.integrate_keyboard_motion(ctx);
    }
}

pub(super) const PICK_TOLERANCE_PX: [f32; 2] = [4.0, 8.0];

pub(super) fn pick_offsets() -> impl Iterator<Item = (f32, f32)> {
    std::iter::once((0.0, 0.0)).chain(PICK_TOLERANCE_PX.into_iter().flat_map(|r| {
        (0..8).map(move |i| {
            let a = i as f32 * std::f32::consts::FRAC_PI_4;
            (r * a.cos(), r * a.sin())
        })
    }))
}

#[cfg(test)]
#[path = "tests_router.rs"]
mod tests;

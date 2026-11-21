// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The gizmo's state machine: a press grabs a handle (or starts moving a
//! part on the plate), moves update the part, release commits one undo
//! step and Esc puts it back. The drag data and its arithmetic live in
//! `gizmo_drag`; this module holds the state and ties it to the scene.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use sc_mesh_core::transform::Transform3D;
use sc_scene::NodeId;

use crate::PointerModifiers;
use crate::build_volume::BuildVolume;
use crate::camera::ViewCamera;
use crate::gizmo;
use crate::mesh_render::{HandleHover, starting_fov_y};
use crate::picking;
use crate::scene_document::{SceneDocument, SceneEdit};

use super::handle_drag::{
    DragKind, HandleDragState, GrabbedHandle, ReanchorView, drag_to_transform, reanchor_axis_scale,
};
use super::move_limits::{clamp_move_edit, settle_on_plate};
use super::screen::{ScreenPoint, ViewportSize};
use crate::gizmo::SIZES;

/// What a press needs.
pub(super) struct GizmoPressContext<'a> {
    pub(super) gizmo_document: &'a Rc<SceneDocument>,
    pub(super) view_camera: &'a RefCell<Option<ViewCamera>>,
    pub(super) view_size: Option<ViewportSize>,
    pub(super) selected_node: Option<NodeId>,
    pub(super) press_point: ScreenPoint,
    pub(super) pointer_modifiers: PointerModifiers,
}

/// What a move needs.
pub(super) struct GizmoMoveContext<'a> {
    pub(super) gizmo_document: &'a Rc<SceneDocument>,
    pub(super) view_camera: &'a RefCell<Option<ViewCamera>>,
    pub(super) view_size: Option<ViewportSize>,
    pub(super) cursor_point: ScreenPoint,
    pub(super) pointer_modifiers: PointerModifiers,
    /// Moves (not turns or scales) are kept inside it.
    pub(super) plate_volume: BuildVolume,
}

/// What hovering needs.
pub(super) struct GizmoHoverContext<'a> {
    pub(super) gizmo_document: &'a Rc<SceneDocument>,
    pub(super) view_camera: &'a RefCell<Option<ViewCamera>>,
    pub(super) view_size: Option<ViewportSize>,
    pub(super) selected_node: Option<NodeId>,
    pub(super) cursor_point: ScreenPoint,
}

pub(super) struct GizmoDragMachine {
    drag_state: Cell<HandleDragState>,
    hovered_handle: Cell<Option<HandleHover>>,
    mode: Cell<crate::mesh_render::HandleShowing>,
    /// False while no transform tool is picked: the handles are not drawn,
    /// so they are not grabbed or hovered either (a part is still dragged
    /// on the plate).
    handles: Cell<bool>,
    /// Other selected objects carried along by an active plate drag, with
    /// their local transforms at activation. Empty unless the grabbed
    /// object is part of a multi-selection.
    companions: RefCell<Vec<(NodeId, Transform3D)>>,
}

impl GizmoDragMachine {
    pub(super) fn new() -> Self {
        Self {
            drag_state: Cell::new(HandleDragState::Resting),
            hovered_handle: Cell::new(None),
            mode: Cell::new(crate::mesh_render::HandleShowing::Resting),
            handles: Cell::new(true),
            companions: RefCell::new(Vec::new()),
        }
    }

    pub(super) fn set_mode(&self, mode: crate::mesh_render::HandleShowing) {
        self.mode.set(mode);
        self.remove_hover();
    }

    pub(super) fn mode(&self) -> crate::mesh_render::HandleShowing {
        self.mode.get()
    }

    pub(super) fn set_handles(&self, shown: bool) {
        if self.handles.replace(shown) != shown {
            self.remove_hover();
        }
    }

    #[cfg(test)]
    pub(super) fn handles(&self) -> bool {
        self.handles.get()
    }

    pub(super) fn drag_snapshot(&self) -> HandleDragState {
        self.drag_state.get()
    }

    pub(super) fn hover_snapshot(&self) -> Option<HandleHover> {
        self.hovered_handle.get()
    }

    /// Back to idle without reverting anything: for a closed scene, whose
    /// part is gone. A finished drag goes through `commit_drag` instead.
    pub(super) fn reset_drag_state(&self) {
        self.drag_state.set(HandleDragState::Resting);
        self.companions.borrow_mut().clear();
    }

    /// The button came up: the drag becomes one undo step (the part, and
    /// any parts carried with it, from where they started to where they
    /// are; an unchanged part adds nothing). `false` when nothing was
    /// dragged, including a plate press that never passed the click
    /// threshold, so the release is handled as a click.
    pub(super) fn finish_drag_edit(&self, document: &Rc<SceneDocument>) -> bool {
        let HandleDragState::HandleHeld {
            op,
            original,
            node_id,
            ..
        } = self.drag_state.get()
        else {
            return false;
        };
        self.drag_state.set(HandleDragState::Resting);
        let companions = std::mem::take(&mut *self.companions.borrow_mut());
        // A plate press that never passed the drag threshold is a click:
        // report "no drag" so the release reaches the pick path.
        if matches!(
            op,
            DragKind::Plate {
                activated: false,
                ..
            }
        ) {
            return false;
        }
        let local = |id: NodeId| {
            document
                .read_scene(|spatial| spatial.leaf(id).map(|q| q.local_transform()))
                .flatten()
        };
        let Some(after) = local(node_id) else {
            return true;
        };
        let lead = SceneEdit::SetLocalTransform {
            node_id,
            before: original,
            after,
        };
        let mut edits = vec![lead];
        for (id, before) in companions {
            if let Some(after) = local(id) {
                edits.push(SceneEdit::SetLocalTransform {
                    node_id: id,
                    before,
                    after,
                });
            }
        }
        if matches!(op, DragKind::Plate { .. }) {
            document.finish_plate_drag(&mut edits, node_id);
        }
        document.record_edit(SceneEdit::Compound(edits));
        true
    }

    /// Forgets the hovered handle (pointer left, selection cleared);
    /// whether there was one.
    pub(super) fn remove_hover(&self) -> bool {
        self.hovered_handle.replace(None).is_some()
    }

    /// Puts the machine straight into `state`, for tests.
    #[cfg(test)]
    pub(super) fn force_drag_state(&self, state: HandleDragState) {
        self.drag_state.set(state);
    }

    /// True while a gizmo handle drag is in flight (armed by a press on a
    /// handle). The viewer uses it to tell a gizmo grab from an empty-space
    /// press, which pans the camera.
    pub(super) fn is_dragging(&self) -> bool {
        matches!(self.drag_state.get(), HandleDragState::HandleHeld { .. })
    }

    /// True while a drag armed by a gizmo handle (not a plate move) is in flight.
    pub(super) fn is_handle_drag(&self) -> bool {
        matches!(self.drag_state.get(), HandleDragState::HandleHeld { op, .. } if !matches!(op, DragKind::Plate { .. }))
    }

    /// Start a plate-plane move after the handle hit test has declined the press.
    pub(super) fn try_start_plate_drag(&self, ctx: GizmoPressContext<'_>) {
        let Some(vp) = ctx.view_size else {
            return;
        };
        let computed = ctx
            .gizmo_document
            .read_scene(|spatial| {
                let camera_slot = ctx.view_camera.borrow();
                let camera = camera_slot.as_ref()?;
                let ray = picking::ray_through_pixel(
                    camera,
                    starting_fov_y(),
                    vp.width(),
                    vp.height(),
                    ctx.press_point.x,
                    ctx.press_point.y,
                );
                let picked = picking::part_under_ray_on_plates(
                    &ray,
                    spatial,
                    Some((
                        ctx.gizmo_document.plate_width.get(),
                        ctx.gizmo_document.plate_depth.get(),
                    )),
                )?;
                if spatial
                    .graph()
                    .plates()
                    .order
                    .iter()
                    .any(|p| p.id == spatial.graph().plate_of(picked) && p.locked)
                {
                    return None;
                }
                let mut ray = ray;
                let [ox, oy] = ctx.gizmo_document.plate_offset(picked);
                ray.start.x -= ox;
                ray.start.y -= oy;
                // Grabbing a support moves the model it holds up (its
                // supports follow); a support on its own never slides
                // away from its contact points.
                let node_id = crate::scene_document::support_owner(spatial.graph(), picked)
                    .or_else(|| {
                        (!crate::auto_support_run::support_ids(spatial.graph()).contains(&picked))
                            .then_some(picked)
                    })?;
                let (x, y) = picking::level_plane_crossing(&ray, 0.0)?;
                Some((
                    node_id,
                    spatial.leaf(node_id)?.local_transform(),
                    glam::Vec3::new(x, y, 0.0),
                ))
            })
            .flatten();
        // Selection is NOT changed on press: a press that never moves past
        // the drag threshold is a click, and the click path (`select_under`,
        // with its Ctrl/Shift multi-select rules) owns selection. `on_move`
        // selects the object once the press actually becomes a drag.
        if let Some((node_id, original, anchor)) = computed {
            self.drag_state.set(HandleDragState::HandleHeld {
                op: DragKind::Plate {
                    anchor,
                    snap: ctx.pointer_modifiers.shift,
                    press: (ctx.press_point.x, ctx.press_point.y),
                    activated: false,
                },
                original,
                pivot: anchor,
                node_id,
            });
        }
    }

    /// A press: grabs the nearest handle of the selected part's gizmo
    /// under the cursor (moves, rings and scale cubes compete by
    /// distance along the ray). With no handle there, or nothing to grab
    /// with (handles hidden, no selection, camera, scene or viewport), the
    /// press stays a click.
    pub(super) fn begin_handle_drag(&self, ctx: GizmoPressContext<'_>) {
        if !self.handles.get() {
            return;
        }
        let (Some(part), Some(viewport)) = (ctx.selected_node, ctx.view_size) else {
            return;
        };
        let grabbed = ctx
            .gizmo_document
            .read_scene(|spatial| {
                let camera = ctx
                    .gizmo_document
                    .local_camera(ctx.view_camera.borrow().as_ref()?, part);
                let pivot = gizmo::part_pivot(spatial, part)?;
                let (w, h) = (viewport.width(), viewport.height());
                let ray = picking::ray_through_pixel(
                    &camera,
                    starting_fov_y(),
                    w,
                    h,
                    ctx.press_point.x,
                    ctx.press_point.y,
                );
                let (handle, _) =
                    nearest_gizmo_handle(&ray, &camera, pivot, starting_fov_y(), h, self.mode.get())?;
                let original = spatial.leaf(part)?.local_transform();
                let op = drag_for_handle(
                    handle,
                    &ray,
                    &camera,
                    pivot,
                    viewport,
                    ctx.press_point,
                    ctx.pointer_modifiers.shift,
                )?;
                Some((op, original, pivot))
            })
            .flatten();
        if let Some((op, original, pivot)) = grabbed {
            self.drag_state.set(HandleDragState::HandleHeld {
                op,
                original,
                pivot,
                node_id: part,
            });
        }
    }

    /// A move during a drag: the part's transform from the cursor, kept on
    /// the plate (and, for moves, inside the build volume). A plate press
    /// first has to pass the click threshold. Shift toggled during an axis
    /// scale switches between per-axis and uniform without a jump: the
    /// part's current transform and the cursor become the new start.
    /// Whether the part moved (the caller then redraws).
    pub(super) fn move_active_drag(&self, ctx: GizmoMoveContext<'_>) -> bool {
        let HandleDragState::HandleHeld {
            mut op,
            mut original,
            pivot,
            node_id,
        } = self.drag_state.get()
        else {
            return false;
        };
        let Some(viewport) = ctx.view_size else {
            return false;
        };
        let (x, y) = (ctx.cursor_point.x, ctx.cursor_point.y);
        if let DragKind::Plate {
            press,
            ref mut activated,
            ..
        } = op
            && !*activated
        {
            if (x - press.0).hypot(y - press.1) < super::buttons::DRAG_START_PX {
                return false;
            }
            *activated = true;
            self.begin_plate_drag(ctx.gizmo_document, node_id);
        }
        // Everything read from the scene and camera happens in here, so
        // both are released before the part is written.
        let moved = ctx
            .gizmo_document
            .read_scene(|spatial| {
                let camera = ctx
                    .gizmo_document
                    .local_camera(ctx.view_camera.borrow().as_ref()?, node_id);
                if let DragKind::ScaleAxis { proportional, .. } = op
                    && proportional != ctx.pointer_modifiers.shift
                    && let Some(running) = spatial.leaf(node_id).map(|part| part.local_transform())
                {
                    let anchor = ReanchorView {
                        lens: &camera,
                        pivot_point: pivot,
                        fov_y: starting_fov_y(),
                        view_px: (viewport.width(), viewport.height()),
                        cursor_px: (x, y),
                    };
                    if let Some((restarted, from)) =
                        reanchor_axis_scale(op, running, &anchor, ctx.pointer_modifiers.shift)
                    {
                        (op, original) = (restarted, from);
                    }
                }
                let ray = picking::ray_through_pixel(
                    &camera,
                    starting_fov_y(),
                    viewport.width(),
                    viewport.height(),
                    x,
                    y,
                );
                if let DragKind::Plate { ref mut snap, .. } = op {
                    *snap = ctx.pointer_modifiers.shift;
                }
                let parent = spatial
                    .leaf(node_id)
                    .map_or(Transform3D::Identity, |part| part.parent_world());
                drag_to_transform(op, &ray, pivot, &parent, &original, x, y)
            })
            .flatten();
        let Some((target, op)) = moved else {
            return false;
        };
        let target = Self::constrain(&ctx, node_id, original, target, op);
        // Kept for the next move: a ring's unwrapped angle advances, and a
        // Shift switch may have moved the start.
        self.drag_state.set(HandleDragState::HandleHeld {
            op,
            original,
            pivot,
            node_id,
        });
        let edit = SceneEdit::SetLocalTransform {
            node_id,
            before: original,
            after: target,
        };
        let applied = ctx.gizmo_document.commit_edit(edit).is_ok();
        if applied && matches!(op, DragKind::Plate { .. }) {
            self.move_companions(ctx.gizmo_document, node_id, &original, &target);
        }
        applied
    }

    /// A plate press became a drag. A part already selected takes the
    /// rest of the selection (but no supports, which follow their models)
    /// along; any other part becomes the selection.
    fn begin_plate_drag(&self, document: &Rc<SceneDocument>, part: NodeId) {
        let selected = document.chosen().ids();
        if !selected.contains(&part) {
            let only = crate::selection::Selection::One(crate::selection::LeafId(part));
            return document.choose(only);
        }
        let carried = document.read_scene(|spatial| {
            let supports = crate::auto_support_run::support_ids(spatial.graph());
            let others = selected
                .iter()
                .filter(|&&id| id != part && !supports.contains(&id));
            others
                .filter_map(|&id| Some((id, spatial.leaf(id)?.local_transform())))
                .collect()
        });
        *self.companions.borrow_mut() = carried.unwrap_or_default();
    }

    /// Where a drag may put the part: moves stay inside the build volume
    /// (a plate move only with a single plate, or no scene; turns and
    /// scales move it only as a side effect of their pivot), and every
    /// tool leaves it resting on the plate (a turn or scale would sink or
    /// lift it, a vertical move drops back). A scene closed meanwhile
    /// leaves `target` as it is; the next move starts over.
    fn constrain(
        ctx: &GizmoMoveContext<'_>,
        part: NodeId,
        original: Transform3D,
        target: Transform3D,
        op: DragKind,
    ) -> Transform3D {
        let document = ctx.gizmo_document;
        let one_plate = || {
            document
                .read_scene(|s| s.graph().plates().order.len() == 1)
                .unwrap_or(true)
        };
        let bounded = matches!(op, DragKind::Translate { .. })
            || (matches!(op, DragKind::Plate { .. }) && one_plate());
        let target = if bounded {
            let clamp = |spatial: &_| {
                clamp_move_edit(spatial, part, original, target, ctx.plate_volume)
            };
            document.read_scene(clamp).unwrap_or(target)
        } else {
            target
        };
        document
            .read_scene(|spatial| settle_on_plate(spatial, part, target))
            .unwrap_or(target)
    }

    /// Carry the plate-drag companions by the lead object's world-space
    /// change (`lead_before` to `lead_after`, both local transforms).
    fn move_companions(
        &self,
        document: &Rc<SceneDocument>,
        lead: NodeId,
        lead_before: &Transform3D,
        lead_after: &Transform3D,
    ) {
        let companions = self.companions.borrow().clone();
        if companions.is_empty() {
            return;
        }
        let edits: Vec<SceneEdit> = document
            .read_scene(|spatial| {
                let lead_parent = spatial.leaf(lead)?.parent_world();
                let delta = lead_parent
                    .compose(lead_after)
                    .compose(&lead_parent.compose(lead_before).inverse().ok()?);
                Some(
                    companions
                        .iter()
                        .filter_map(|&(id, original)| {
                            let query = spatial.leaf(id)?;
                            let parent = query.parent_world();
                            let after = parent
                                .inverse()
                                .ok()?
                                .compose(&delta)
                                .compose(&parent)
                                .compose(&original);
                            Some(SceneEdit::SetLocalTransform {
                                node_id: id,
                                before: query.local_transform(),
                                after,
                            })
                        })
                        .collect(),
                )
            })
            .flatten()
            .unwrap_or_default();
        // Applied one by one so each model's supports follow it.
        for edit in edits {
            let _ = document.commit_edit(edit);
        }
    }

    /// Esc during a drag: the part, and any carried with it, go back to
    /// where they started. Whether there was a drag (the reverts redraw
    /// through the document's events).
    pub(super) fn cancel_drag_edit(&self, document: &Rc<SceneDocument>) -> bool {
        let HandleDragState::HandleHeld {
            original, node_id, ..
        } = self.drag_state.get()
        else {
            return false;
        };
        self.drag_state.set(HandleDragState::Resting);
        let carried = std::mem::take(&mut *self.companions.borrow_mut());
        for (part, start) in std::iter::once((node_id, original)).chain(carried) {
            let now = document
                .read_scene(|spatial| spatial.leaf(part).map(|q| q.local_transform()))
                .flatten();
            // A revert the scene refuses still ends the drag.
            let _ = document.commit_edit(SceneEdit::SetLocalTransform {
                node_id: part,
                before: now.unwrap_or(start),
                after: start,
            });
        }
        true
    }

    /// The handle under the cursor, for its highlight; whether it changed
    /// (the caller redraws). Left alone during a drag, whose handle is lit
    /// anyway.
    pub(super) fn refresh_handle_hover(&self, ctx: GizmoHoverContext<'_>) -> bool {
        if matches!(self.drag_state.get(), HandleDragState::HandleHeld { .. }) {
            return false;
        }
        let hovered = if self.handles.get() {
            compute_hover(&ctx, self.mode.get())
        } else {
            None
        };
        self.hovered_handle.replace(hovered) != hovered
    }
}

/// The drag a press on `handle` starts. Every kind records where it was
/// grabbed; an axis scale also records the screen points the uniform
/// formula needs, even when it starts per-axis, so a Shift switch in
/// either direction re-anchors the same way. `None` (the press stays a
/// click) only where the hit test should already have refused: in a
/// move axis's or ring's lock-out, or with the pivot behind the camera.
fn drag_for_handle(
    handle: GrabbedHandle,
    ray: &picking::PickRay,
    camera: &ViewCamera,
    pivot: glam::Vec3,
    viewport: ViewportSize,
    press: ScreenPoint,
    shift: bool,
) -> Option<DragKind> {
    use gizmo::drag_math::{closest_point_on_axis, ray_hits_axis_plane, rotate_drag_angle};
    let press_screen = (press.x, press.y);
    Some(match handle {
        GrabbedHandle::ScaleUniform => DragKind::ScaleUniform { press_screen },
        GrabbedHandle::TranslatePlane(normal) => DragKind::TranslatePlane {
            normal,
            anchor: ray_hits_axis_plane(ray, pivot, normal)?,
        },
        GrabbedHandle::Translate(axis) => DragKind::Translate {
            axis,
            anchor: closest_point_on_axis(ray, pivot, axis)?,
        },
        GrabbedHandle::Rotate(axis) => {
            let start_angle = rotate_drag_angle(ray, pivot, axis)?;
            DragKind::Rotate {
                axis,
                start_angle,
                last_raw: start_angle,
                current_unwrapped: start_angle,
            }
        }
        GrabbedHandle::ScaleAxis(axis) => {
            let start_anchor = closest_point_on_axis(ray, pivot, axis)?;
            let (w, h) = (viewport.width(), viewport.height());
            let pivot_screen =
                picking::screen_point_of(camera, starting_fov_y(), w, h, pivot)?;
            DragKind::ScaleAxis {
                axis,
                start_anchor,
                current_anchor: start_anchor,
                proportional: shift,
                press_screen,
                pivot_screen,
            }
        }
    })
}

/// The handle `ray` meets first (smallest distance along it) among
/// those `mode` shows, for grabbing and for hover alike.
fn nearest_gizmo_handle(
    ray: &picking::PickRay,
    camera: &ViewCamera,
    pivot: glam::Vec3,
    fov_y: f32,
    h: f32,
    mode: crate::mesh_render::HandleShowing,
) -> Option<(GrabbedHandle, f32)> {
    use crate::gizmo::axis::Axis;
    use crate::mesh_render::HandleShowing::{Resting, Move, Turn, Resize};
    use gizmo::hit_test::{arc_pickable, handle_under, ring_under, cube_under};
    let size = gizmo::gizmo_scale(camera, fov_y, h);
    // Handles are grabbed within the same few pixels as the arrow shafts.
    let reach = gizmo::hit_radius_world(camera, fov_y, h);
    let ring = size * SIZES.ring_radius;
    let cube = camera.mm_per_pixel(fov_y, h) * SIZES.cube_px;
    let shows = |tool| matches!(mode, Resting) || mode == tool;
    let mut nearest: Option<(GrabbedHandle, f32)> = None;
    let mut offer = |hit: GrabbedHandle, t: f32| {
        if !matches!(nearest, Some((_, best)) if best <= t) {
            nearest = Some((hit, t));
        }
    };
    // There is no vertical move arrow: parts always rest on the plate.
    if shows(Move)
        && let Some((axis, t)) = handle_under(ray, pivot, size, reach)
        && axis != Axis::Z
    {
        offer(GrabbedHandle::Translate(axis), t);
    }
    if shows(Turn)
        && let Some((axis, t)) = ring_under(ray, pivot, ring, reach)
    {
        offer(GrabbedHandle::Rotate(axis), t);
    }
    if shows(Resize)
        && let Some((axis, t)) = cube_under(ray, pivot, size, cube)
    {
        offer(GrabbedHandle::ScaleAxis(axis), t);
    }
    // The Move tool's square moves in the plate's plane; vertical squares
    // would lift the part off the plate, which settling would undo.
    if matches!(mode, Move)
        && arc_pickable(ray, Axis::Z)
        && let Some(point) = gizmo::drag_math::ray_hits_axis_plane(ray, pivot, Axis::Z)
    {
        let local = (point - pivot) / size;
        let on_square = |side: glam::Vec3| (0.22..=0.42).contains(&local.dot(side));
        if on_square(glam::Vec3::Y) && on_square(glam::Vec3::X) {
            offer(
                GrabbedHandle::TranslatePlane(Axis::Z),
                (point - ray.start).dot(ray.heading),
            );
        }
    }
    if matches!(mode, Resize) {
        let half = glam::Vec3::splat(cube * 0.5);
        if let Some(t) = picking::box_entry_distance(ray, pivot - half, pivot + half) {
            offer(GrabbedHandle::ScaleUniform, t);
        }
    }
    nearest
}

/// The handle under the cursor, if the selection, scene, camera,
/// viewport and pivot are all there; the state machine is not touched.
fn compute_hover(
    ctx: &GizmoHoverContext<'_>,
    mode: crate::mesh_render::HandleShowing,
) -> Option<HandleHover> {
    let (part, viewport) = (ctx.selected_node?, ctx.view_size?);
    let (w, h, fov_y) = (viewport.width(), viewport.height(), starting_fov_y());
    ctx.gizmo_document
        .read_scene(|spatial| {
            let camera = ctx
                .gizmo_document
                .local_camera(ctx.view_camera.borrow().as_ref()?, part);
            let pivot = gizmo::part_pivot(spatial, part)?;
            // Far from the gizmo, skip the ray tests.
            if !cursor_near_gizmo(&camera, fov_y, w, h, pivot, ctx.cursor_point) {
                return None;
            }
            let ray = picking::ray_through_pixel(&camera, fov_y, w, h, ctx.cursor_point.x, ctx.cursor_point.y);
            let (hit, _) = nearest_gizmo_handle(&ray, &camera, pivot, fov_y, h, mode)?;
            Some(hover_from_handle_hit(hit))
        })
        .flatten()
}

/// Whether the cursor is close enough to the gizmo for any handle to be
/// under it: within the ring (the outermost handle) plus a handle's
/// length, on screen. A pivot that does not project (behind the camera)
/// counts as close, and the ray tests decide.
fn cursor_near_gizmo(
    camera: &ViewCamera,
    fov_y: f32,
    viewport_w: f32,
    viewport_h: f32,
    pivot: glam::Vec3,
    cursor: ScreenPoint,
) -> bool {
    let Some((x, y)) =
        picking::screen_point_of(camera, fov_y, viewport_w, viewport_h, pivot)
    else {
        return true;
    };
    let ring_world = gizmo::gizmo_scale(camera, fov_y, viewport_h)
        * SIZES.ring_radius;
    let reach = ring_world / camera.mm_per_pixel(fov_y, viewport_h)
        + SIZES.arm_px;
    let (dx, dy) = (cursor.x - x, cursor.y - y);
    dx * dx + dy * dy <= reach * reach
}

/// The highlight for a handle: a move square lights its move handle.
fn hover_from_handle_hit(hit: GrabbedHandle) -> HandleHover {
    match hit {
        GrabbedHandle::Translate(axis) | GrabbedHandle::TranslatePlane(axis) => HandleHover::Move(axis),
        GrabbedHandle::Rotate(axis) => HandleHover::Turn(axis),
        GrabbedHandle::ScaleAxis(axis) => HandleHover::Stretch(axis),
        GrabbedHandle::ScaleUniform => HandleHover::Even,
    }
}

#[cfg(test)]
#[path = "tests_handle_states.rs"]
mod tests;

#[cfg(test)]
mod tests_modal_gizmo {
    use super::*;
    use crate::mesh_render::HandleShowing;

    /// Parts always rest on the plate, so only the flat XY square (normal Z)
    /// is a Move handle; the two vertical squares are neither drawn nor picked.
    #[test]
    fn only_the_flat_move_plane_picks_and_it_is_modal() {
        let camera = ViewCamera::new(glam::Vec3::ZERO, 100.0);
        let fov = starting_fov_y();
        let scale = gizmo::gizmo_scale(&camera, fov, 800.0);
        for (normal, point) in [
            (gizmo::axis::Axis::X, glam::Vec3::new(0.0, 0.32, 0.32)),
            (gizmo::axis::Axis::Y, glam::Vec3::new(-0.32, 0.0, 0.32)),
            (gizmo::axis::Axis::Z, glam::Vec3::new(0.32, 0.32, 0.0)),
        ] {
            let ray = picking::PickRay {
                start: point * scale + normal.unit_vector() * 100.0,
                heading: -normal.unit_vector(),
            };
            let picked = matches!(nearest_gizmo_handle(&ray, &camera, glam::Vec3::ZERO, fov, 800.0, HandleShowing::Move),
                Some((GrabbedHandle::TranslatePlane(axis), _)) if axis == normal);
            assert_eq!(picked, normal == gizmo::axis::Axis::Z, "{normal:?}");
            assert!(
                nearest_gizmo_handle(
                    &ray,
                    &camera,
                    glam::Vec3::ZERO,
                    fov,
                    800.0,
                    HandleShowing::Resize
                )
                .is_none()
            );
        }
    }

    /// No vertical move: a ray straight through the Z shaft grabs nothing in
    /// the Move tool (settling would snap the part back to the plate anyway).
    #[test]
    fn move_tool_has_no_vertical_handle() {
        let camera = ViewCamera::new(glam::Vec3::ZERO, 100.0);
        let fov = starting_fov_y();
        let scale = gizmo::gizmo_scale(&camera, fov, 800.0);
        let ray = picking::PickRay {
            start: glam::Vec3::new(100.0, 0.0, scale * 0.7),
            heading: -glam::Vec3::X,
        };
        assert!(!matches!(
            nearest_gizmo_handle(
                &ray,
                &camera,
                glam::Vec3::ZERO,
                fov,
                800.0,
                HandleShowing::Move
            ),
            Some((GrabbedHandle::Translate(gizmo::axis::Axis::Z), _))
        ));
    }

    #[test]
    fn modal_picker_rejects_other_categories() {
        let camera = ViewCamera::new(glam::Vec3::ZERO, 100.0);
        let fov = starting_fov_y();
        let scale = gizmo::gizmo_scale(&camera, fov, 800.0);
        let ray = picking::PickRay {
            start: glam::Vec3::new(scale * 1.1, 0.0, 100.0),
            heading: -glam::Vec3::Z,
        };
        assert!(matches!(
            nearest_gizmo_handle(
                &ray,
                &camera,
                glam::Vec3::ZERO,
                fov,
                800.0,
                HandleShowing::Move
            ),
            Some((GrabbedHandle::Translate(_), _))
        ));
        assert!(
            nearest_gizmo_handle(
                &ray,
                &camera,
                glam::Vec3::ZERO,
                fov,
                800.0,
                HandleShowing::Resize
            )
            .is_none()
        );
        assert!(
            nearest_gizmo_handle(
                &ray,
                &camera,
                glam::Vec3::ZERO,
                fov,
                800.0,
                HandleShowing::Turn
            )
            .is_none()
        );
    }

    #[test]
    fn changing_mode_clears_stale_hover() {
        let fsm = GizmoDragMachine::new();
        fsm.hovered_handle
            .set(Some(HandleHover::Move(gizmo::axis::Axis::X)));
        fsm.set_mode(HandleShowing::Resize);
        assert_eq!(fsm.mode(), HandleShowing::Resize);
        assert_eq!(fsm.hover_snapshot(), None);
    }
}

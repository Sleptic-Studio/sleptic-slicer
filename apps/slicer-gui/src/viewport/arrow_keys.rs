// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Continuous keyboard transforms and their undo grouping.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

use glam::Vec3;
use sc_mesh_core::transform::AffineTransform3D;
use sc_mesh_core::transform::Transform3D;
use sc_scene::NodeId;

use super::handle_drag::{rotation_about, about_pivot};
use super::move_limits::{clamp_move_edit, settle_on_plate};
use super::screen::ViewportSize;
use crate::build_volume::BuildVolume;
use crate::camera::ViewCamera;
use crate::gizmo::axis::Axis;
use crate::mesh_render::starting_fov_y;
use crate::scene_document::{SceneDocument, SceneEdit};

/// The active transform operation, or camera input when no operation is active.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub(super) enum KeyboardTransformFocus {
    #[default]
    NoTransformFocus,
    MoveFocus,
    RotationFocus,
    ScalingFocus,
}

/// Held directions are stored separately so opposing keys cancel.
#[derive(Copy, Clone, Default, Debug)]
pub(super) struct HeldArrowDirections {
    pub positive_x_held: bool,
    pub negative_x_held: bool,
    pub positive_y_held: bool,
    pub negative_y_held: bool,
    pub positive_z_held: bool,
    pub negative_z_held: bool,
}

impl HeldArrowDirections {
    /// Resolve each opposing pair into a signed axis.
    pub(super) fn direction_signs(self) -> (f32, f32, f32) {
        fn direction(positive: bool, negative: bool) -> f32 {
            match (positive, negative) {
                (true, false) => 1.0,
                (false, true) => -1.0,
                _ => 0.0,
            }
        }
        (
            direction(self.positive_x_held, self.negative_x_held),
            direction(self.positive_y_held, self.negative_y_held),
            direction(self.positive_z_held, self.negative_z_held),
        )
    }

    pub(super) fn has_held_arrow(self) -> bool {
        [self.positive_x_held, self.negative_x_held, self.positive_y_held, self.negative_y_held, self.positive_z_held, self.negative_z_held]
            .into_iter()
            .any(|held| held)
    }
}

/// Direction codes shared with the UI callbacks.
pub(super) const KEY_ARROW_POSITIVE_X: i32 = 0;
pub(super) const KEY_ARROW_NEGATIVE_X: i32 = 1;
pub(super) const KEY_ARROW_POSITIVE_Y: i32 = 2;
pub(super) const KEY_ARROW_NEGATIVE_Y: i32 = 3;
pub(super) const KEY_ARROW_POSITIVE_Z: i32 = 4;
pub(super) const KEY_ARROW_NEGATIVE_Z: i32 = 5;

// Translation uses screen distance so its apparent speed follows zoom.
const KEYBOARD_MOVE_PIXELS_PER_SECOND: f32 = 30.0;

// Angular speed in radians per second.
const KEYBOARD_ROTATION_RADIANS_PER_SECOND: f32 = std::f32::consts::FRAC_PI_4;

// Exponential scaling gives inverse factors for opposite directions.
const KEYBOARD_SCALING_BASE: f32 = 1.5;

/// Limit movement after a delayed frame.
pub(super) const KEYBOARD_MAX_FRAME_SECONDS: f32 = 0.1;

/// Compose one input step with the current transform.
/// Rotation applies X, then Y, then Z around the world-space pivot.
pub(super) fn compute_keyboard_transform(
    focus: KeyboardTransformFocus,
    arrows: HeldArrowDirections,
    dt: f32,
    pivot: Vec3,
    parent_world: &Transform3D,
    running: &Transform3D,
    world_per_pixel: f32,
) -> Option<Transform3D> {
    let signs = arrows.direction_signs();
    let (x, y, z) = signs;
    if signs == (0.0, 0.0, 0.0) {
        return None;
    }
    match focus {
        KeyboardTransformFocus::NoTransformFocus => None,
        KeyboardTransformFocus::MoveFocus => {
            let distance = world_per_pixel * KEYBOARD_MOVE_PIXELS_PER_SECOND * dt;
            let offset = [x, y, z].map(|v| (v * distance) as f64);
            Some(Transform3D::Affine(AffineTransform3D::from_translation(offset)).compose(running))
        }
        KeyboardTransformFocus::RotationFocus => {
            let mut result = *running;
            for (axis, sign) in [(Axis::X, x), (Axis::Y, y), (Axis::Z, z)] {
                if sign != 0.0 {
                    if let Some(rotation) = rotation_about(axis, sign * KEYBOARD_ROTATION_RADIANS_PER_SECOND * dt)
                    {
                        result = about_pivot(&rotation, pivot, parent_world, &result);
                    }
                }
            }
            Some(result)
        }
        KeyboardTransformFocus::ScalingFocus => {
            let factors = [x, y, z].map(|sign| {
                if sign == 0.0 {
                    1.0
                } else {
                    KEYBOARD_SCALING_BASE.powf(sign * dt) as f64
                }
            });
            let scale = AffineTransform3D::from_non_uniform_scale(factors);
            Some(about_pivot(&scale, pivot, parent_world, running))
        }
    }
}

/// Inputs borrowed for a single animation tick.
pub(super) struct KeyboardIntegrationContext<'a> {
    pub(super) keyboard_document: &'a Rc<SceneDocument>,
    pub(super) keyboard_camera: &'a RefCell<Option<ViewCamera>>,
    pub(super) keyboard_viewport: Option<ViewportSize>,
    pub(super) keyboard_selection_id: Option<NodeId>,
    /// Bounds used when translating the selection.
    pub(super) keyboard_build_volume: BuildVolume,
}

// The first key captures the pre-edit transform for one undo entry.
struct KeyboardEditBurst {
    armed_keyboard_edit: Cell<Option<(NodeId, Transform3D)>>,
    last_arrow_event_at: Cell<Option<Instant>>,
}

impl KeyboardEditBurst {
    fn new() -> Self {
        Self {
            armed_keyboard_edit: Cell::new(None),
            last_arrow_event_at: Cell::new(None),
        }
    }

    fn has_pending_keyboard_edit(&self) -> bool {
        self.armed_keyboard_edit.get().is_some()
    }

    fn begin_keyboard_edit(&self, node_id: NodeId, before: Transform3D) {
        self.armed_keyboard_edit.set(Some((node_id, before)));
    }

    // Consume the captured transform when the burst ends.
    fn take_keyboard_edit(&self) -> Option<(NodeId, Transform3D)> {
        self.armed_keyboard_edit.take()
    }

    fn discard_keyboard_edit(&self) {
        self.armed_keyboard_edit.set(None);
    }

    fn record_arrow_event(&self) {
        self.last_arrow_event_at.set(Some(Instant::now()));
    }
}

/// Input state and undo grouping for continuous keyboard transforms.
pub(super) struct KeyboardMotionState {
    transform_focus: Cell<KeyboardTransformFocus>,
    held_arrow_directions: Cell<HeldArrowDirections>,
    last_keyboard_tick: Cell<Option<Instant>>,
    keyboard_edit_burst: KeyboardEditBurst,
    companions: RefCell<Vec<(NodeId, Transform3D)>>,
}

impl KeyboardMotionState {
    pub(super) fn new() -> Self {
        Self {
            transform_focus: Cell::new(KeyboardTransformFocus::NoTransformFocus),
            held_arrow_directions: Cell::new(HeldArrowDirections::default()),
            last_keyboard_tick: Cell::new(None),
            keyboard_edit_burst: KeyboardEditBurst::new(),
            companions: RefCell::new(Vec::new()),
        }
    }

    pub(super) fn keyboard_transform_focus(&self) -> KeyboardTransformFocus {
        self.transform_focus.get()
    }

    /// Visible to sibling tests for asserting bitset state.
    #[cfg(test)]
    pub(super) fn held_keyboard_arrows(&self) -> HeldArrowDirections {
        self.held_arrow_directions.get()
    }

    /// Scene replacement invalidates the captured IDs, so discard the burst.
    pub(super) fn reset_keyboard_motion(&self) {
        self.keyboard_edit_burst.discard_keyboard_edit();
        self.companions.take();
        self.clear_input(KeyboardTransformFocus::NoTransformFocus);
    }

    /// Finish the current undo group before switching operations.
    /// A transform mode needs a selection; clearing focus does not.
    pub(super) fn set_keyboard_transform_focus(
        &self,
        document: &Rc<SceneDocument>,
        focus: KeyboardTransformFocus,
        has_selection: bool,
    ) -> bool {
        if self.transform_focus.get() == focus || (focus != KeyboardTransformFocus::NoTransformFocus && !has_selection) {
            return false;
        }
        self.commit_keyboard_edit(document);
        self.clear_input(focus);
        true
    }

    fn clear_input(&self, focus: KeyboardTransformFocus) {
        self.transform_focus.set(focus);
        self.held_arrow_directions.set(HeldArrowDirections::default());
        self.last_keyboard_tick.set(None);
    }

    /// Record a key event and begin or finish an undo group at its edges.
    /// Repeated events are accepted, while unknown codes leave state alone.
    pub(super) fn update_held_arrow(
        &self,
        document: &Rc<SceneDocument>,
        selection_id: Option<NodeId>,
        code: i32,
        pressed: bool,
    ) -> bool {
        let previous = self.held_arrow_directions.get();
        let mut next = previous;
        let target = match code {
            KEY_ARROW_POSITIVE_X => &mut next.positive_x_held,
            KEY_ARROW_NEGATIVE_X => &mut next.negative_x_held,
            KEY_ARROW_POSITIVE_Y => &mut next.positive_y_held,
            KEY_ARROW_NEGATIVE_Y => &mut next.negative_y_held,
            KEY_ARROW_POSITIVE_Z => &mut next.positive_z_held,
            KEY_ARROW_NEGATIVE_Z => &mut next.negative_z_held,
            _ => return false,
        };
        *target = pressed;
        self.held_arrow_directions.set(next);
        self.keyboard_edit_burst.record_arrow_event();

        if !previous.has_held_arrow() && next.has_held_arrow() && pressed {
            self.begin_keyboard_edit_if_possible(document, selection_id);
        } else if previous.has_held_arrow() && !next.has_held_arrow() {
            self.commit_keyboard_edit(document);
        }
        if pressed && self.last_keyboard_tick.get().is_none() {
            self.last_keyboard_tick.set(Some(Instant::now()));
        }
        true
    }

    // Capture the selection only once, at the start of a held-key burst.
    fn begin_keyboard_edit_if_possible(&self, document: &Rc<SceneDocument>, selection_id: Option<NodeId>) {
        if self.keyboard_edit_burst.has_pending_keyboard_edit() {
            return;
        }
        let Some(id) = selection_id else { return };
        let Some((before, companions)) = document
            .read_scene(|scene| {
                let before = scene.leaf(id)?.local_transform();
                let companions = document
                    .chosen()
                    .ids()
                    .into_iter()
                    .filter(|other| *other != id)
                    .filter(|other| {
                        crate::scene_document::support_owner(scene.graph(), *other).is_none()
                    })
                    .filter_map(|other| {
                        scene
                            .leaf(other)
                            .map(|leaf| (other, leaf.local_transform()))
                    })
                    .collect();
                Some((before, companions))
            })
            .flatten()
        else {
            return;
        };
        *self.companions.borrow_mut() = companions;
        self.keyboard_edit_burst.begin_keyboard_edit(id, before);
    }

    // A burst collects the original transforms so the entire movement undoes together.
    fn commit_keyboard_edit(&self, document: &Rc<SceneDocument>) {
        let Some(primary) = self.keyboard_edit_burst.take_keyboard_edit() else {
            return;
        };
        let mut originals = self.companions.take();
        originals.push(primary);
        let edits = document
            .read_scene(|scene| {
                originals
                    .into_iter()
                    .filter_map(|(node_id, before)| {
                        scene
                            .leaf(node_id)
                            .map(|leaf| SceneEdit::SetLocalTransform {
                                node_id,
                                before,
                                after: leaf.local_transform(),
                            })
                    })
                    .collect()
            })
            .unwrap_or_default();
        document.record_edit(SceneEdit::Compound(edits));
    }

    // Advance the clock only after a selected object and viewport are available.
    fn tick_delta(&self) -> f32 {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_keyboard_tick.get().unwrap_or(now));
        self.last_keyboard_tick.set(Some(now));
        elapsed.as_secs_f32().min(KEYBOARD_MAX_FRAME_SECONDS)
    }

    /// Apply one frame of movement when the input and scene are ready.
    pub(super) fn integrate_keyboard_motion(&self, ctx: KeyboardIntegrationContext) -> bool {
        let focus = self.transform_focus.get();
        if focus == KeyboardTransformFocus::NoTransformFocus {
            return false;
        }
        let arrows = self.held_arrow_directions.get();
        if !arrows.has_held_arrow() {
            self.last_keyboard_tick.take();
            return false;
        }
        let (Some(node_id), Some(viewport)) = (ctx.keyboard_selection_id, ctx.keyboard_viewport) else {
            return false;
        };
        let dt = self.tick_delta();
        if dt <= 0.0 {
            return false;
        }

        // Release the scene and camera borrows before applying an edit.
        let prepared = ctx
            .keyboard_document
            .read_scene(|scene| {
                let pivot = crate::gizmo::part_pivot(scene, node_id)?;
                let leaf = scene.leaf(node_id)?;
                let camera = ctx.keyboard_camera.borrow();
                let pixel_size = camera
                    .as_ref()?
                    .mm_per_pixel(starting_fov_y(), viewport.height());
                Some((
                    pivot,
                    leaf.local_transform(),
                    leaf.parent_world(),
                    pixel_size,
                ))
            })
            .flatten();
        let Some((pivot, running, parent_world, world_per_pixel)) = prepared else {
            return false;
        };

        let Some(new_transform) = compute_keyboard_transform(
            focus,
            arrows,
            dt,
            pivot,
            &parent_world,
            &running,
            world_per_pixel,
        ) else {
            return false;
        };

        // Only direct translation is bounded; rotation and scale may move the pivot.
        let new_transform = match focus {
            KeyboardTransformFocus::MoveFocus => ctx
                .keyboard_document
                .read_scene(|scene| {
                    clamp_move_edit(scene, node_id, running, new_transform, ctx.keyboard_build_volume)
                })
                .unwrap_or(new_transform),
            _ => new_transform,
        };
        // Arrow keys follow the same rest-on-the-plate rule as the gizmo.
        let new_transform = ctx
            .keyboard_document
            .read_scene(|spatial| settle_on_plate(spatial, node_id, new_transform))
            .unwrap_or(new_transform);

        let edit = match focus {
            KeyboardTransformFocus::MoveFocus => self.group_translation(&ctx, node_id, running, new_transform),
            _ => Some(SceneEdit::SetLocalTransform {
                node_id,
                before: running,
                after: new_transform,
            }),
        };
        edit.is_some_and(|edit| ctx.keyboard_document.commit_edit(edit).is_ok())
    }

    // Each companion can reduce the shared world displacement before it is applied.
    fn group_translation(
        &self,
        ctx: &KeyboardIntegrationContext<'_>,
        node_id: NodeId,
        before: Transform3D,
        proposed: Transform3D,
    ) -> Option<SceneEdit> {
        ctx.keyboard_document
            .read_scene(|scene| {
                let parent = scene.leaf(node_id)?.parent_world();
                let original_world = parent.compose(&before);
                let mut displacement = parent
                    .compose(&proposed)
                    .compose(&original_world.inverse().ok()?);
                let companions = self.companions.borrow();
                for &(id, _) in companions.iter() {
                    let leaf = scene.leaf(id)?;
                    let parent = leaf.parent_world();
                    let candidate = parent
                        .inverse()
                        .ok()?
                        .compose(&displacement)
                        .compose(&leaf.world_transform());
                    let limited = clamp_move_edit(
                        scene,
                        id,
                        leaf.local_transform(),
                        candidate,
                        ctx.keyboard_build_volume,
                    );
                    displacement = parent
                        .compose(&limited)
                        .compose(&leaf.world_transform().inverse().ok()?);
                }
                let stationary = match displacement {
                    Transform3D::Identity => true,
                    Transform3D::Affine(a) => a.translation().iter().all(|v| v.abs() < 1e-12),
                };
                if stationary {
                    return None;
                }
                let primary_after = parent
                    .inverse()
                    .ok()?
                    .compose(&displacement)
                    .compose(&original_world);
                let mut edits = vec![SceneEdit::SetLocalTransform {
                    node_id,
                    before,
                    after: primary_after,
                }];
                for &(id, _) in companions.iter() {
                    let leaf = scene.leaf(id)?;
                    let after = leaf
                        .parent_world()
                        .inverse()
                        .ok()?
                        .compose(&displacement)
                        .compose(&leaf.world_transform());
                    edits.push(SceneEdit::SetLocalTransform {
                        node_id: id,
                        before: leaf.local_transform(),
                        after,
                    });
                }
                Some(SceneEdit::Compound(edits))
            })
            .flatten()
    }
}

#[cfg(test)]
#[path = "tests_arrow_keys.rs"]
mod tests;

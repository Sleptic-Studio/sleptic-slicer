// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! What the gizmo pass is given each frame: the gizmo's state, and from it
//! one entry per handle (where it points and its colour).
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use crate::gizmo::axis::Axis;
use crate::gizmo::SIZES;

/// The gizmo pass's uniforms: the camera, and in one vec4 the pivot (xyz)
/// and the axis length (w).
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(super) struct HandleUniforms {
    pub(super) clip_from_world: [[f32; 4]; 4],
    pub(super) anchor_and_length: [f32; 4],
}

/// One handle's entry, as `Arrow` in `gizmo.wgsl`: the rotation that
/// turns the handle's +X onto its axis, and its colour.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(super) struct HandleEntry {
    orient: [[f32; 4]; 4],
    colour: [f32; 4],
}

pub(super) const HANDLE_ENTRY_BYTES: u64 = size_of::<HandleEntry>() as u64;

/// The kinds of handle, in the order their entries sit in the entries
/// buffer: one entry per axis, and one for the centre cube.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HandleGroup {
    Arrows,
    Rings,
    Cubes,
    Centre,
}

impl HandleGroup {
    const IN_BUFFER: [HandleGroup; 4] = [HandleGroup::Arrows, HandleGroup::Rings, HandleGroup::Cubes, HandleGroup::Centre];

    /// How many entries the group has.
    pub(super) const fn count(self) -> u64 {
        if let HandleGroup::Centre = self { 1 } else { 3 }
    }

    /// The group's first entry: after every group before it.
    pub(super) fn first(self) -> u64 {
        HandleGroup::IN_BUFFER.iter().take_while(|&&group| group != self).map(|group| group.count()).sum()
    }

    /// The entry of the group's `axis`-th handle (0, 1, 2 for X, Y, Z).
    pub(super) fn at(self, axis: u64) -> u64 {
        debug_assert!(axis < self.count(), "{self:?} has no handle {axis}");
        self.first() + axis
    }
}

/// Entries in the gizmo's entries buffer: every group's.
pub(super) const HANDLE_ENTRY_COUNT: u64 = 3 + 3 + 3 + 1;

/// The gizmo as it is to be drawn this frame.
#[derive(Copy, Clone, Debug)]
pub struct HandleFrame {
    /// Where it stands, in world space.
    pub anchor: Vec3,
    /// Its axes' length in world units, so it keeps one size on screen.
    pub arm_length: f32,
    /// Which handles show, and which one is being dragged.
    pub showing: HandleShowing,
    /// X, Y, Z: axes that cannot be grabbed now, drawn grey. Covers both the
    /// move cone and the scale cube of the axis, which share the same
    /// point-on-axis maths and so the same limit (`gizmo::axis_pickable`).
    pub fixed_axes: [bool; 3],
    /// X, Y, Z: turn rings that cannot be grabbed now (`gizmo::arc_pickable`).
    pub fixed_rings: [bool; 3],
    /// The handle under the pointer with no button held, drawn between dim
    /// and bright. Not used while dragging: the dragged handle is bright.
    pub under_pointer: Option<HandleHover>,
    /// A drag the gizmo shows as it happens.
    pub live_drag: Option<LiveDrag>,
}

/// A drag in progress, shown on the gizmo.
#[derive(Copy, Clone, Debug)]
pub enum LiveDrag {
    /// Scaling along `axis`: the pointer's signed distance from the pivot
    /// along it, in world units; the axis's cube follows it there.
    Stretch { axis: Axis, reach: f32 },
}

/// The handle under the pointer.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HandleHover {
    Even,
    Move(Axis),
    Turn(Axis),
    Stretch(Axis),
}

/// Which handles are shown. `Resting` shows them all with none highlighted;
/// while dragging, only the dragged kind shows, its axis bright and the
/// others dim.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HandleShowing {
    Resting,
    Move,
    Turn,
    Resize,
    Moving { axis: Axis },
    Turning { axis: Axis },
    Stretching { axis: Axis },
}

/// The three axes in slot order, each with the turn that lays a handle
/// built along +X onto it.
fn axis_turns() -> [(Axis, Mat4); 3] {
    let quarter = std::f32::consts::FRAC_PI_2;
    [
        (Axis::X, Mat4::IDENTITY),
        (Axis::Y, Mat4::from_rotation_z(quarter)),
        (Axis::Z, Mat4::from_rotation_y(-quarter)),
    ]
}

/// Grey for a handle that cannot be grabbed now.
const FIXED_GREY: f32 = 0.35;
/// How much of its axis colour a handle shows when idle; one dragged shows
/// all of it, one under the pointer the midpoint of the two.
const IDLE_SHARE: f32 = 0.65;

/// A handle's RGBA colour.
fn handle_colour(axis: Axis, dragged: bool, hovered: bool, fixed: bool) -> [f32; 4] {
    if fixed {
        return [FIXED_GREY, FIXED_GREY, FIXED_GREY, 1.0];
    }
    let share = if dragged {
        1.0
    } else if hovered {
        (1.0 + IDLE_SHARE) / 2.0
    } else {
        IDLE_SHARE
    };
    let full = match axis {
        Axis::X => [1.0, 0.25, 0.25],
        Axis::Y => [0.25, 1.0, 0.25],
        Axis::Z => [0.35, 0.45, 1.0],
    };
    [full[0] * share, full[1] * share, full[2] * share, 1.0]
}

/// A row of per-axis handles: the axis being dragged and the one under the
/// pointer (if any), and which of the three are fixed.
struct AxisRow {
    dragged: Option<Axis>,
    hovered: Option<Axis>,
    fixed: [bool; 3],
}

impl AxisRow {
    /// One entry per axis; `placement` gets the axis and its turn and
    /// returns the handle's full orientation.
    fn entries(&self, placement: impl Fn(Axis, Mat4) -> Mat4) -> [HandleEntry; 3] {
        axis_turns().map(|(axis, turn)| HandleEntry {
            orient: placement(axis, turn).to_cols_array_2d(),
            colour: handle_colour(
                axis,
                self.dragged == Some(axis),
                self.hovered == Some(axis),
                self.fixed[axis as usize],
            ),
        })
    }
}

/// The move arrows (a line and a cone per axis).
pub(super) fn arrow_entries(frame: &HandleFrame) -> [HandleEntry; 3] {
    let row = AxisRow {
        dragged: match frame.showing {
            HandleShowing::Moving { axis } => Some(axis),
            _ => None,
        },
        hovered: match frame.under_pointer {
            Some(HandleHover::Move(axis)) => Some(axis),
            _ => None,
        },
        fixed: frame.fixed_axes,
    };
    row.entries(|_, turn| turn)
}

/// The turn rings: flat in their axis's plane and in its colour, never
/// turned towards the camera, the way Orca, Bambu Studio, PrusaSlicer and
/// Blender draw them (earlier cyan and yellow arcs faced the camera and
/// swung about as the view moved).
pub(super) fn ring_entries(frame: &HandleFrame) -> [HandleEntry; 3] {
    let row = AxisRow {
        dragged: match frame.showing {
            HandleShowing::Turning { axis } => Some(axis),
            _ => None,
        },
        hovered: match frame.under_pointer {
            Some(HandleHover::Turn(axis)) => Some(axis),
            _ => None,
        },
        fixed: frame.fixed_rings,
    };
    row.entries(|_, turn| turn)
}

/// The scale cubes at the ends of the axes, fixed whenever their axis's
/// arrow is. The shader puts a cube `SIZES.cube_at` of the arm along
/// the handle; while one is stretched it follows the pointer instead, so
/// it is pushed along the handle's +X by the difference (after the turn's
/// frame is applied, before the shader's scaling).
pub(super) fn stretch_cube_entries(frame: &HandleFrame) -> [HandleEntry; 3] {
    let row = AxisRow {
        dragged: match frame.showing {
            HandleShowing::Stretching { axis } => Some(axis),
            _ => None,
        },
        hovered: match frame.under_pointer {
            Some(HandleHover::Stretch(axis)) => Some(axis),
            _ => None,
        },
        fixed: frame.fixed_axes,
    };
    let resting_at = frame.arm_length * SIZES.cube_at;
    row.entries(|axis, turn| {
        let push = match frame.live_drag {
            Some(LiveDrag::Stretch { axis: pulled, reach }) if pulled == axis => reach - resting_at,
            _ => 0.0,
        };
        // Also false for NaN: a cube never goes somewhere undefined.
        if push.abs() > 0.0 { turn * Mat4::from_translation(Vec3::X * push) } else { turn }
    })
}

/// The grey centre cube that scales evenly: an axis cube brought back to
/// the pivot, a little brighter under the pointer.
pub(super) fn even_cube_entry(frame: &HandleFrame) -> HandleEntry {
    let grey = match frame.under_pointer {
        Some(HandleHover::Even) => 1.0,
        _ => 0.8,
    };
    let to_pivot = Vec3::X * -(frame.arm_length * SIZES.cube_at);
    HandleEntry {
        orient: Mat4::from_translation(to_pivot).to_cols_array_2d(),
        colour: [grey, grey, grey, 1.0],
    }
}

#[cfg(test)]
#[path = "tests_gizmo_entries.rs"]
mod tests;

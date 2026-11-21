// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The slice extension: stacks of 2D layer outlines. Sleptic keeps a
//! project's slices this way.
use super::{ResourceId, ResourceIndex};

/// A `<slicestack>` resource, starting at `z_bottom`.
#[derive(Clone, Debug, PartialEq)]
pub struct SliceStack {
    pub content: SliceStackContent,
    pub z_bottom: f64,
    pub id: ResourceId,
}

/// A stack holds its own layers, or points at stacks in other parts.
#[derive(Clone, Debug, PartialEq)]
pub enum SliceStackContent {
    Refs(Vec<SliceRef>),
    Slices(Vec<Slice>),
}

/// Where another part's stack is: the part's path and the stack's id there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SliceRef {
    pub slice_path: String,
    pub slice_stack: ResourceId,
}

/// One layer, up to `z_top`: outline points, and polygons that run through
/// them by position.
#[derive(Clone, Debug, PartialEq)]
pub struct Slice {
    pub polygons: Vec<SlicePolygon>,
    pub vertices: Vec<SliceVertex>,
    pub z_top: f64,
}

/// A polygon: where it starts, then where each segment ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlicePolygon {
    pub segments: Vec<SliceSegment>,
    pub start: ResourceIndex,
}

impl SlicePolygon {
    /// Whether the polygon ends where it started.
    pub fn is_closed(&self) -> bool {
        self.segments.last().map(|last| last.end) == Some(self.start)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SliceSegment {
    pub end: ResourceIndex,
}

/// An outline point, in model units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliceVertex {
    pub x: f64,
    pub y: f64,
}

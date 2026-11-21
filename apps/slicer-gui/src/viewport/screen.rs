// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Pointer places and movements in the viewport's logical pixels, and the
//! viewport's size.
use std::marker::PhantomData;

/// Two logical-pixel numbers. `K` says what they mean, so a place cannot be
/// handed over where a movement is wanted, or the other way round.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Screen<K> {
    pub(super) x: f32,
    pub(super) y: f32,
    meaning: PhantomData<K>,
}

/// A place in the viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Place {}

/// A movement between two pointer events.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Movement {}

pub(crate) type ScreenPoint = Screen<Place>;
pub(crate) type ScreenDelta = Screen<Movement>;

impl<K> Screen<K> {
    pub(crate) fn new(x: f32, y: f32) -> Self {
        Screen { x, y, meaning: PhantomData }
    }
}

/// The viewport's width and height in logical pixels, both finite and
/// above zero (a collapsed or not yet laid out view has no size at all).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ViewportSize([f32; 2]);

impl ViewportSize {
    pub(super) fn new(width: f32, height: f32) -> Option<Self> {
        [width, height].iter().all(|side| side.is_finite() && *side > 0.0).then_some(ViewportSize([width, height]))
    }

    pub(super) fn width(self) -> f32 {
        self.0[0]
    }

    pub(super) fn height(self) -> f32 {
        self.0[1]
    }
}

#[cfg(test)]
mod tests {
    use super::{ScreenDelta, ScreenPoint, ViewportSize};

    #[test]
    fn sizes_must_be_positive_and_finite_on_both_sides() {
        let full_hd = ViewportSize::new(1920.0, 1080.0).expect("a real size");
        assert_eq!([full_hd.width(), full_hd.height()], [1920.0, 1080.0]);
        for broken in [0.0, -3.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(ViewportSize::new(broken, 50.0), None, "width {broken}");
            assert_eq!(ViewportSize::new(50.0, broken), None, "height {broken}");
        }
    }

    #[test]
    fn places_and_movements_keep_their_numbers() {
        let place = ScreenPoint::new(12.5, -4.0);
        let step = ScreenDelta::new(-1.0, 2.0);
        assert_eq!((place.x, place.y, step.x, step.y), (12.5, -4.0, -1.0, 2.0));
    }
}

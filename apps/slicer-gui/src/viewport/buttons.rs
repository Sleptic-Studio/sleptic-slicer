// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The mouse buttons held down in the viewport, and whether letting go
//! makes a click or ends a drag.
use std::cell::Cell;

use super::screen::ScreenPoint;

/// The farthest, in logical pixels, a button may come up from where it went
/// down and still count as a click. Smaller and shaky hands lose clicks;
/// larger and short drags turn into clicks.
pub(super) const DRAG_START_PX: f32 = 4.0;

/// The two buttons the viewport tells apart. Each has its own press, so
/// cancelling a left-button gesture leaves a right-button orbit going.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Button {
    Left,
    Right,
}

/// What letting go of a button turned out to be.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Release {
    /// Close to where it went down, which it carries (the context menu
    /// opens and picks there).
    Click(ScreenPoint),
    Drag,
    /// The button was not down here (focus moved, or a control took it).
    NoPress,
}

/// Where each button went down, if it is down.
pub(super) struct PointerState {
    down_at: [Cell<Option<ScreenPoint>>; 2],
}

impl PointerState {
    pub(super) fn new() -> Self {
        PointerState { down_at: [Cell::new(None), Cell::new(None)] }
    }

    fn slot(&self, button: Button) -> &Cell<Option<ScreenPoint>> {
        &self.down_at[button as usize]
    }

    /// `button` went down at `at` (a later press replaces an earlier one).
    pub(super) fn press(&self, button: Button, at: ScreenPoint) {
        self.slot(button).set(Some(at));
    }

    /// Drops a press that will get no release (a cancelled gesture, a
    /// closed scene).
    pub(super) fn forget(&self, button: Button) {
        self.slot(button).set(None);
    }

    /// `button` came up at `at`; the press is used up. A position that is
    /// not a number never makes a click.
    pub(super) fn release(&self, button: Button, at: ScreenPoint) -> Release {
        let Some(start) = self.slot(button).take() else {
            return Release::NoPress;
        };
        let travel = (at.x - start.x).hypot(at.y - start.y);
        if travel <= DRAG_START_PX { Release::Click(start) } else { Release::Drag }
    }
}

#[cfg(test)]
mod tests {
    use super::{Button, PointerState, Release};
    use crate::viewport::ScreenPoint;

    fn spot(x: f32, y: f32) -> ScreenPoint {
        ScreenPoint::new(x, y)
    }

    #[test]
    fn up_within_the_threshold_clicks_and_beyond_it_drags() {
        let buttons = PointerState::new();
        let expect = [(2.0, 3.0, true), (4.0, 0.0, true), (0.0, 4.0, true), (4.01, 0.0, false), (60.0, 0.0, false), (f32::NAN, 1.0, false)];
        for (x, y, clicks) in expect {
            buttons.press(Button::Left, spot(0.0, 0.0));
            let outcome = buttons.release(Button::Left, spot(x, y));
            assert_eq!(outcome == Release::Click(spot(0.0, 0.0)), clicks, "{x},{y}: {outcome:?}");
            assert!(clicks || outcome == Release::Drag);
        }
    }

    #[test]
    fn a_press_ends_with_its_release_or_when_forgotten() {
        let buttons = PointerState::new();
        assert_eq!(buttons.release(Button::Right, spot(5.0, 5.0)), Release::NoPress);
        buttons.press(Button::Right, spot(30.0, 40.0));
        assert_eq!(buttons.release(Button::Right, spot(32.0, 41.0)), Release::Click(spot(30.0, 40.0)));
        assert_eq!(buttons.release(Button::Right, spot(32.0, 41.0)), Release::NoPress);
        buttons.press(Button::Left, spot(1.0, 1.0));
        buttons.forget(Button::Left);
        assert_eq!(buttons.release(Button::Left, spot(1.0, 1.0)), Release::NoPress);
        // The newest press is the one that counts.
        buttons.press(Button::Left, spot(0.0, 0.0));
        buttons.press(Button::Left, spot(90.0, 90.0));
        assert_eq!(buttons.release(Button::Left, spot(91.0, 90.0)), Release::Click(spot(90.0, 90.0)));
    }

    #[test]
    fn each_button_keeps_its_own_press() {
        let buttons = PointerState::new();
        buttons.press(Button::Left, spot(10.0, 10.0));
        buttons.press(Button::Right, spot(70.0, 70.0));
        buttons.forget(Button::Left);
        assert_eq!(buttons.release(Button::Right, spot(70.0, 70.0)), Release::Click(spot(70.0, 70.0)));
        buttons.press(Button::Left, spot(10.0, 10.0));
        buttons.forget(Button::Right);
        assert_eq!(buttons.release(Button::Left, spot(10.0, 10.0)), Release::Click(spot(10.0, 10.0)));
    }
}

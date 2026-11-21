// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! In-window confirmation (Orca / Bambu style): a modal card over the main
//! window instead of a separate OS dialog window. One question at a time per
//! window; the answer runs the caller's continuation.

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use crate::MainWindow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    Primary,
    Secondary,
    Cancel,
}

pub(crate) fn from_index(index: i32) -> Choice {
    match index {
        0 => Choice::Primary,
        1 => Choice::Secondary,
        _ => Choice::Cancel,
    }
}

/// The text of one question. `secondary` empty = no second button;
/// `destructive` paints the primary button red (deleting things).
pub(crate) struct Question<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub primary: &'a str,
    pub secondary: &'a str,
    pub destructive: bool,
}

/// Ask `question` in `ui`; `answer` runs once with the choice (Escape and
/// the Cancel button give `Choice::Cancel`).
pub(crate) fn ask(ui: &MainWindow, question: Question<'_>, answer: impl FnOnce(Choice) + 'static) {
    let pending: Rc<RefCell<Option<Box<dyn FnOnce(Choice)>>>> =
        Rc::new(RefCell::new(Some(Box::new(answer))));
    let weak = ui.as_weak();
    ui.on_confirm_chosen(move |index| {
        if let Some(ui) = weak.upgrade() {
            ui.set_confirm_visible(false);
        }
        if let Some(answer) = pending.borrow_mut().take() {
            answer(from_index(index));
        }
    });
    ui.set_confirm_title(question.title.into());
    ui.set_confirm_body(question.body.into());
    ui.set_confirm_primary(question.primary.into());
    ui.set_confirm_secondary(question.secondary.into());
    ui.set_confirm_destructive(question.destructive);
    ui.set_confirm_visible(true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_indices_map_to_choices() {
        assert_eq!(from_index(0), Choice::Primary);
        assert_eq!(from_index(1), Choice::Secondary);
        assert_eq!(from_index(2), Choice::Cancel);
        assert_eq!(from_index(-1), Choice::Cancel);
    }
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The error, information, re-slice and about windows, through the Slint
//! testing backend: what text each shows and when, and their buttons and
//! Escape.
use std::cell::RefCell;
use std::rc::Rc;

use i_slint_backend_testing::{AccessibleRole, ElementHandle};
use slint::ComponentHandle;
use slint::platform::{Key, WindowEvent};

use crate::{AboutWindow, ErrorDialog, InfoDialog, SliceOverwriteDialog};

type Log = Rc<RefCell<Vec<&'static str>>>;

fn noting(log: &Log, entry: &'static str) -> impl Fn() + 'static {
    let log = Rc::clone(log);
    move || log.borrow_mut().push(entry)
}

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(50));
}

fn shown(root: &impl i_slint_backend_testing::ElementRoot, text: &str) -> usize {
    ElementHandle::find_by_accessible_label(root, text).count()
}

/// The labels of the window's text elements (an element can be listed
/// more than once, once per component it is reached through).
fn text_labels(root: &impl i_slint_backend_testing::ElementRoot) -> Vec<String> {
    i_slint_backend_testing::ElementQuery::from_root(root)
        .match_descendants()
        .match_accessible_role(AccessibleRole::Text)
        .find_all()
        .iter()
        .map(|text| text.accessible_label().unwrap_or_default().to_string())
        .collect()
}

fn press(root: &impl i_slint_backend_testing::ElementRoot, label: &str) {
    let button = ElementHandle::find_by_accessible_label(root, label)
        .find(|e| e.accessible_role() == Some(AccessibleRole::Button))
        .unwrap_or_else(|| panic!("a {label:?} button"));
    button.invoke_accessible_default_action();
}

fn escape(window: &slint::Window) {
    window.dispatch_event(WindowEvent::KeyPressed { text: Key::Escape.into() });
}

#[test]
fn the_error_dialog_shows_what_it_is_given_and_closes() {
    i_slint_backend_testing::init_no_event_loop();
    let dialog = ErrorDialog::new().unwrap();
    let log: Log = Rc::default();
    dialog.on_dismiss_error_requested(noting(&log, "close"));
    dialog.set_error_heading_text("Could not open".into());
    dialog.set_error_message_text("The file is damaged.".into());
    dialog.show().unwrap();
    settle();
    assert_eq!(shown(&dialog, "Could not open"), 1);
    assert_eq!(shown(&dialog, "The file is damaged."), 1);
    assert_eq!(shown(&dialog, "Error"), 1, "the badge's label");
    // No context, no context line: not even an empty one, which would
    // still take a row. With one, it shows.
    assert!(!text_labels(&dialog).iter().any(String::is_empty), "{:?}", text_labels(&dialog));
    assert_eq!(shown(&dialog, "File: part.stl"), 0);
    dialog.set_error_context_text("File: part.stl".into());
    settle();
    assert_eq!(shown(&dialog, "File: part.stl"), 1);
    // Plain details show as text; others behind the technical toggle.
    dialog.set_error_details_text("Disk full".into());
    dialog.set_details_inline(true);
    settle();
    assert_eq!(shown(&dialog, "Disk full"), 1);
    assert_eq!(shown(&dialog, "Show technical details"), 0);
    dialog.set_details_inline(false);
    settle();
    assert_eq!(shown(&dialog, "Show technical details"), 1);
    press(&dialog, "OK");
    escape(dialog.window());
    assert_eq!(*log.borrow(), ["close", "close"]);
}

#[test]
fn the_information_dialog_names_its_details_plainly_and_closes() {
    i_slint_backend_testing::init_no_event_loop();
    let dialog = InfoDialog::new().unwrap();
    let log: Log = Rc::default();
    dialog.on_done_clicked(noting(&log, "close"));
    dialog.set_body_text("Saved.".into());
    dialog.set_more_text("12 parts".into());
    dialog.show().unwrap();
    settle();
    assert_eq!(shown(&dialog, "Saved."), 1);
    assert_eq!(shown(&dialog, "Show details"), 1);
    assert_eq!(shown(&dialog, "Show technical details"), 0);
    press(&dialog, "OK");
    escape(dialog.window());
    assert_eq!(*log.borrow(), ["close", "close"]);
}

#[test]
fn the_reslice_question_confirms_or_keeps_the_slice() {
    i_slint_backend_testing::init_no_event_loop();
    let dialog = SliceOverwriteDialog::new().unwrap();
    let log: Log = Rc::default();
    dialog.on_reslice_approved(noting(&log, "re-slice"));
    dialog.on_reslice_dismissed(noting(&log, "cancel"));
    dialog.show().unwrap();
    press(&dialog, "Re-slice");
    press(&dialog, "Cancel");
    escape(dialog.window());
    assert_eq!(*log.borrow(), ["re-slice", "cancel", "cancel"]);
}

#[test]
fn the_about_window_shows_the_version_and_closes() {
    i_slint_backend_testing::init_no_event_loop();
    let window = AboutWindow::new().unwrap();
    let log: Log = Rc::default();
    window.on_close_requested(noting(&log, "close"));
    window.set_about_release_label("1.2.3".into());
    window.show().unwrap();
    settle();
    assert_eq!(shown(&window, "Version 1.2.3"), 1);
    assert!(shown(&window, "#MadeWithSlint") >= 1, "Slint attribution is visible");
    press(&window, "Close");
    assert_eq!(*log.borrow(), ["close"]);
}


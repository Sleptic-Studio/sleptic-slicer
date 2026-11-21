// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Error, information and "re-slice?" windows. Each stays open, owned here,
//! until the user closes it.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use slint::ComponentHandle;

use crate::{ErrorDialog, InfoDialog, SliceOverwriteDialog};

/// What "Save changes?" offers before another project opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SaveChoice {
    /// The scene has a project file: "Save" writes over it.
    OverwriteCurrent,
    /// No project file yet: "Save as..." asks for one.
    KeepAsCopy,
}

/// The windows of one kind that are open, by id.
type Open<D> = RefCell<HashMap<u64, D>>;

/// A confirmation waiting for its answer; the first button takes it.
type Pending = Rc<Cell<Option<Box<dyn FnOnce()>>>>;

/// The open windows. Close handlers hold a weak reference back, so a window
/// never keeps the manager alive. Ids come from one counter for all kinds.
pub(crate) struct MessageWindows {
    errors: Open<ErrorDialog>,
    infos: Open<InfoDialog>,
    overwrites: Open<SliceOverwriteDialog>,
    next_id: Cell<u64>,
}

/// Whether the details are a few plain lines worth showing under the
/// message, rather than technical output kept behind a disclosure (R112: a
/// failure's reason was hidden behind "Show technical details").
pub(crate) fn inline_details(details: &str) -> bool {
    const TECHNICAL: [&str; 8] = [
        "::", "{", "}", "\\", "os error", "panicked", "    at ", "Err(",
    ];
    let details = details.trim();
    if details.is_empty() || details.chars().count() > 360 || details.lines().count() > 8 {
        return false;
    }
    !TECHNICAL.iter().any(|mark| details.contains(mark))
}

/// Hides a window and forgets it.
fn close<D: ComponentHandle + 'static>(
    window: &slint::Weak<D>,
    manager: &Weak<MessageWindows>,
    id: u64,
    open: fn(&MessageWindows) -> &Open<D>,
) {
    if let Some(window) = window.upgrade() {
        let _ = window.hide();
    }
    if let Some(manager) = manager.upgrade() {
        open(&manager).borrow_mut().remove(&id);
    }
}

impl MessageWindows {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self {
            errors: RefCell::default(),
            infos: RefCell::default(),
            overwrites: RefCell::default(),
            next_id: Cell::new(0),
        })
    }

    fn take_id(&self) -> u64 {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        id
    }

    /// Shows the window and keeps it until it closes; `kind` names it in
    /// the log if it cannot be shown.
    fn keep<D: ComponentHandle + 'static>(
        &self,
        window: D,
        id: u64,
        kind: &str,
        open: fn(&MessageWindows) -> &Open<D>,
    ) {
        match window.show() {
            Ok(()) => {
                open(self).borrow_mut().insert(id, window);
            }
            Err(err) => eprintln!("Failed to show {kind} dialog: {err}"),
        }
    }

    /// Shows an error now, and logs it. UI thread only.
    pub(crate) fn show_failure(
        self: &Rc<Self>,
        heading: &str,
        context: Option<String>,
        message: String,
        details: String,
    ) {
        eprintln!("[error] {heading}: {message}\n{details}");
        let window = match ErrorDialog::new() {
            Ok(window) => window,
            Err(err) => return eprintln!("Failed to create error dialog: {err}"),
        };
        crate::theme_sync::track(&window, ErrorDialog::set_theme);
        window.set_error_heading_text(heading.into());
        if let Some(context) = context {
            window.set_error_context_text(context.into());
        }
        window.set_error_message_text(message.into());
        window.set_details_inline(inline_details(&details));
        window.set_error_details_text(details.into());
        let id = self.take_id();
        let (handle, manager) = (window.as_weak(), Rc::downgrade(self));
        window.on_dismiss_error_requested(move || close(&handle, &manager, id, |m| &m.errors));
        self.keep(window, id, "error", |m| &m.errors);
    }

    /// Shows a neutral notice now: an error's window, without its styling
    /// or log line. UI thread only.
    pub(crate) fn show_notice(self: &Rc<Self>, heading: &str, message: String, details: String) {
        let window = match InfoDialog::new() {
            Ok(window) => window,
            Err(err) => return eprintln!("Failed to create info dialog: {err}"),
        };
        crate::theme_sync::track(&window, InfoDialog::set_theme);
        window.set_headline(heading.into());
        window.set_body_text(message.into());
        window.set_details_inline(inline_details(&details));
        window.set_more_text(details.into());
        let id = self.take_id();
        let (handle, manager) = (window.as_weak(), Rc::downgrade(self));
        window.on_done_clicked(move || close(&handle, &manager, id, |m| &m.infos));
        self.keep(window, id, "info", |m| &m.infos);
    }

    /// Shows an error on the next turn of the event loop, for callers inside
    /// a Slint hook (a rendering notifier) where opening a window would
    /// re-enter the loop. A timer, not `invoke_from_event_loop`, so this
    /// stays on the UI thread and needs no `Send`.
    pub(crate) fn show_failure_later(
        self: &Rc<Self>,
        heading: String,
        context: Option<String>,
        message: String,
        details: String,
    ) {
        let manager = Rc::clone(self);
        slint::Timer::single_shot(core::time::Duration::ZERO, move || {
            manager.show_failure(&heading, context, message, details);
        });
    }

    /// Asks before a new slice replaces the one held. `on_confirm` runs once,
    /// only on a yes; cancelling, or a window that cannot be shown, keeps the
    /// held slice. UI thread only.
    pub(crate) fn ask_before_reslice(
        self: &Rc<Self>,
        on_confirm: impl FnOnce() + 'static,
    ) {
        let window = match SliceOverwriteDialog::new() {
            Ok(window) => window,
            Err(err) => return eprintln!("Failed to create slice-overwrite dialog: {err}"),
        };
        crate::theme_sync::track(&window, SliceOverwriteDialog::set_theme);
        let id = self.take_id();
        let pending: Pending = Rc::new(Cell::new(Some(Box::new(on_confirm))));
        let (handle, manager) = (window.as_weak(), Rc::downgrade(self));
        let yes = Rc::clone(&pending);
        window.on_reslice_approved(move || {
            if let Some(confirm) = yes.take() {
                confirm();
            }
            close(&handle, &manager, id, |m| &m.overwrites);
        });
        let (handle, manager) = (window.as_weak(), Rc::downgrade(self));
        window.on_reslice_dismissed(move || {
            drop(pending.take());
            close(&handle, &manager, id, |m| &m.overwrites);
        });
        self.keep(window, id, "slice-overwrite", |m| &m.overwrites);
    }
}

#[cfg(test)]
impl MessageWindows {
    /// The headings of the error windows open now.
    pub(crate) fn error_headings(&self) -> Vec<String> {
        self.errors.borrow().values().map(|window| window.get_error_heading_text().to_string()).collect()
    }

    /// The heading, file and message of each error window open now,
    /// oldest first.
    pub(crate) fn errors(&self) -> Vec<(String, String, String)> {
        let errors = self.errors.borrow();
        let mut ids: Vec<_> = errors.keys().copied().collect();
        ids.sort_unstable();
        let text = |w: &ErrorDialog| (w.get_error_heading_text().into(), w.get_error_context_text().into(), w.get_error_message_text().into());
        ids.iter().map(|id| text(&errors[id])).collect()
    }

    /// The heading and message of each notice open now.
    pub(crate) fn info_messages(&self) -> Vec<(String, String)> {
        self.infos.borrow().values().map(|w| (w.get_headline().into(), w.get_body_text().into())).collect()
    }

    /// The "re-slice?" windows open now.
    pub(crate) fn overwrite_windows(&self) -> Vec<SliceOverwriteDialog> {
        self.overwrites.borrow().values().map(ComponentHandle::clone_strong).collect()
    }
}

/// An error's causes, one per line under its own message, for a dialog's
/// details; empty when there is no cause to show.
pub(crate) fn error_with_causes(err: &anyhow::Error) -> String {
    let mut chain = err.chain();
    let (Some(top), Some(cause)) = (chain.next(), chain.next()) else {
        return String::new();
    };
    let mut text = format!("{top}\n  caused by: {cause}");
    for cause in chain {
        text.push_str(&format!("\n  caused by: {cause}"));
    }
    text
}

#[cfg(test)]
#[path = "tests_error_dialog.rs"]
mod tests;

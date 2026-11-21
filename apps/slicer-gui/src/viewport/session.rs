// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! What one window's viewport works on for its whole life, apart from the
//! camera, input and rendering: the open document and the services around
//! it (loading, the save and export progress panels, message windows, a file
//! waiting for the GPU), plus the printer's volume and the open section.
//!
//! The document subscription is made by the controller, whose listener
//! needs a weak handle on it, and only kept here so it ends with the view.
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::build_volume::{BuildVolume, FALLBACK_VOLUME};
use crate::error_dialog::MessageWindows;
use crate::export_progress_panel::ExportProgressPanel;
use crate::pending_load::WaitingFile;
use crate::save_progress_panel::SaveProgressPanel;
use crate::scene_document::{SceneDocument, Watch};
use crate::scene_loader::SceneLoadCoordinator;
use crate::{MainWindow, WorkspaceSection};

/// The two progress panels a window shows over its view.
pub(super) struct ProgressPanels {
    pub(super) save: Rc<SaveProgressPanel>,
    pub(super) export: Rc<ExportProgressPanel>,
}

/// The printer volume translations are clamped to (settings, and tests with
/// a small one, change it while the window is open) and the open section
/// (each window has its own; not saved). Setting either says whether it
/// changed, so callers redraw or switch tools only when something did.
pub(super) struct Placement {
    volume: Cell<BuildVolume>,
    section: Cell<WorkspaceSection>,
}

impl Default for Placement {
    /// The default printer, in Prepare, until settings and the user say
    /// otherwise.
    fn default() -> Self {
        Placement { volume: Cell::new(FALLBACK_VOLUME), section: Cell::new(WorkspaceSection::Prepare) }
    }
}

impl Placement {
    pub(super) fn volume(&self) -> BuildVolume {
        self.volume.get()
    }

    pub(super) fn set_volume(&self, next: BuildVolume) -> bool {
        self.volume.replace(next) != next
    }

    pub(super) fn section(&self) -> WorkspaceSection {
        self.section.get()
    }

    pub(super) fn set_section(&self, next: WorkspaceSection) -> bool {
        self.section.replace(next) != next
    }
}

pub(super) struct ViewSession {
    pub(super) document: Rc<SceneDocument>,
    pub(super) loader: Rc<SceneLoadCoordinator>,
    pub(super) panels: ProgressPanels,
    pub(super) waiting_file: Rc<WaitingFile>,
    pub(super) messages: Rc<MessageWindows>,
    pub(super) place: Placement,
    /// Held only so the document subscription lives as long as the view.
    subscription: RefCell<Option<Watch>>,
}

impl ViewSession {
    /// The session of `window`, showing `document`.
    pub(super) fn for_window(
        window: &MainWindow,
        document: Rc<SceneDocument>,
        waiting_file: Rc<WaitingFile>,
        gpu_loss: crate::mesh_render::DeviceLoss,
    ) -> Self {
        use slint::ComponentHandle;
        let messages = MessageWindows::new();
        ViewSession {
            loader: Rc::new(SceneLoadCoordinator::new(window, messages.clone(), gpu_loss)),
            panels: ProgressPanels {
                save: Rc::new(SaveProgressPanel::new(window.as_weak())),
                export: Rc::new(ExportProgressPanel::new(window.as_weak())),
            },
            document,
            waiting_file,
            messages,
            place: Placement::default(),
            subscription: RefCell::default(),
        }
    }

    /// A session with no window behind it: panels and dialogs show nothing.
    #[cfg(test)]
    pub(super) fn detached() -> Self {
        let messages = MessageWindows::new();
        let nowhere = slint::Weak::default;
        ViewSession {
            loader: Rc::new(SceneLoadCoordinator::loader_for_test(messages.clone())),
            panels: ProgressPanels {
                save: Rc::new(SaveProgressPanel::new(nowhere())),
                export: Rc::new(ExportProgressPanel::new(nowhere())),
            },
            document: SceneDocument::new(),
            waiting_file: Rc::new(WaitingFile::new()),
            messages,
            place: Placement::default(),
            subscription: RefCell::default(),
        }
    }

    pub(super) fn keep_watch(&self, subscription: Watch) {
        self.subscription.replace(Some(subscription));
    }
}

#[cfg(test)]
mod tests {
    use super::Placement;
    use crate::WorkspaceSection;
    use crate::build_volume::{BuildVolume, FALLBACK_VOLUME};

    #[test]
    fn volume_changes_are_reported_only_when_real() {
        let session = Placement::default();
        assert_eq!(session.volume(), FALLBACK_VOLUME);
        let tiny = BuildVolume::new(20.0, 30.0, 40.0);
        assert!(!session.set_volume(FALLBACK_VOLUME), "same as before");
        assert!(session.set_volume(tiny));
        assert!(!session.set_volume(tiny));
        assert_eq!(session.volume(), tiny);
        assert!(session.set_volume(FALLBACK_VOLUME));
    }

    #[test]
    fn sections_start_at_prepare_and_report_switches() {
        let session = Placement::default();
        assert_eq!(session.section(), WorkspaceSection::Prepare);
        assert!(!session.set_section(WorkspaceSection::Prepare));
        assert!(session.set_section(WorkspaceSection::Slice));
        assert!(!session.set_section(WorkspaceSection::Slice));
        assert!(session.set_section(WorkspaceSection::Transform));
        assert_eq!(session.section(), WorkspaceSection::Transform);
    }
}

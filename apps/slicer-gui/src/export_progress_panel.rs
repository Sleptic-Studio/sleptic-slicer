// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Export selects its phases and sink over the shared save-panel driver.

use crate::export_progress::{ExportPhase, ExportProgressSink};
use crate::save_progress_panel::ProgressPanel;
use crate::{MainWindow, WriteStep as UiPhase};

pub(crate) struct ExportProgressPanel(ProgressPanel);

impl ExportProgressPanel {
    pub(crate) fn new(ui: slint::Weak<MainWindow>) -> Self {
        Self(ProgressPanel::new(ui))
    }
    pub(crate) fn sink(&self) -> ExportProgressSink {
        ExportProgressSink::new(self.0.cell())
    }
    pub(crate) fn begin(&self) {
        self.0.begin(UiPhase::Flattening, export_snapshot);
    }
    pub(crate) fn end(&self) {
        self.0.end();
    }

    #[cfg(test)]
    pub(crate) fn status_for_test(&self) -> (u32, bool) {
        self.0.status_for_test()
    }

    #[cfg(test)]
    pub(crate) fn timer_for_test(&self) -> std::rc::Weak<slint::Timer> {
        self.0.timer_for_test()
    }
}

fn export_snapshot(value: u32) -> (UiPhase, u32) {
    let (phase, fraction) = crate::export_progress::decode(value);
    let phase = match phase {
        ExportPhase::Flatten => UiPhase::Flattening,
        ExportPhase::Serialize => UiPhase::Serializing,
        ExportPhase::Compress => UiPhase::Compressing,
        ExportPhase::Write => UiPhase::Writing,
    };
    (phase, fraction)
}

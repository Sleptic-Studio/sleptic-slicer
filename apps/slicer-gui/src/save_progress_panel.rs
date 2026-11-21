// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Save and export share one timed view of their worker progress cell.

use crate::save_progress::{WriteStep, SaveProgressSink};
use crate::{MainWindow, WriteStep as UiPhase};
use std::{
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

pub(crate) struct ProgressPanel {
    ui: slint::Weak<MainWindow>,
    cell: Arc<AtomicU32>,
    timer: Rc<slint::Timer>,
}

impl ProgressPanel {
    pub(crate) fn new(ui: slint::Weak<MainWindow>) -> Self {
        Self {
            ui,
            cell: Arc::new(AtomicU32::new(0)),
            timer: Rc::new(slint::Timer::default()),
        }
    }

    pub(crate) fn cell(&self) -> Arc<AtomicU32> {
        self.cell.clone()
    }

    pub(crate) fn begin(&self, initial: UiPhase, decode: fn(u32) -> (UiPhase, u32)) {
        self.cell.store(0, Ordering::Relaxed);
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        ui.set_save_step(initial);
        ui.set_save_fraction(0.0);
        ui.set_saving(true);
        let window = self.ui.clone();
        let progress = self.cell.clone();
        let timer = Rc::downgrade(&self.timer);
        self.timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(100),
            move || {
                if let Some(ui) = window.upgrade().filter(|ui| ui.get_saving()) {
                    let (phase, fraction) = decode(progress.load(Ordering::Relaxed));
                    ui.set_save_step(phase);
                    ui.set_save_fraction(
                        fraction as f32 / crate::load_progress::PROGRESS_PERMILLE_MAX as f32,
                    );
                } else if let Some(timer) = timer.upgrade() {
                    timer.stop();
                }
            },
        );
    }

    pub(crate) fn end(&self) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_saving(false);
        }
    }

    #[cfg(test)]
    pub(crate) fn status_for_test(&self) -> (u32, bool) {
        (self.cell.load(Ordering::Relaxed), self.timer.running())
    }
}

#[cfg(test)]
impl ProgressPanel {
    pub(crate) fn timer_for_test(&self) -> std::rc::Weak<slint::Timer> {
        Rc::downgrade(&self.timer)
    }
}

pub(crate) struct SaveProgressPanel(ProgressPanel);

impl SaveProgressPanel {
    pub(crate) fn new(ui: slint::Weak<MainWindow>) -> Self {
        Self(ProgressPanel::new(ui))
    }
    pub(crate) fn sink(&self) -> SaveProgressSink {
        SaveProgressSink::new(self.0.cell())
    }
    pub(crate) fn begin(&self) {
        self.0.begin(UiPhase::Serializing, save_snapshot);
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

fn save_snapshot(value: u32) -> (UiPhase, u32) {
    let (phase, fraction) = crate::save_progress::decode(value);
    let phase = match phase {
        WriteStep::Serialize => UiPhase::Serializing,
        WriteStep::Compress => UiPhase::Compressing,
        WriteStep::Writing => UiPhase::Writing,
    };
    (phase, fraction)
}

#[cfg(test)]
#[path = "tests_progress_panels.rs"]
mod tests_progress_panels;

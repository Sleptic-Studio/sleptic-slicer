// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

use crate::export_progress::{ExportPhase, ExportProgressSink};
use crate::export_progress_panel::ExportProgressPanel;
use crate::save_progress::{WriteStep, SaveProgressSink};
use crate::save_progress_panel::SaveProgressPanel;
use crate::{MainWindow, WriteStep as UiPhase};
use slint::ComponentHandle;

macro_rules! live_panel {
    ($name:ident, $panel:ident, $first:path, [$(($phase:path, $shown:path)),+]) => {
        #[test]
        fn $name() {
            i_slint_backend_testing::init_no_event_loop();
            let ui = MainWindow::new().unwrap();
            let panel = $panel::new(ui.as_weak());
            panel.begin();
            assert_eq!(panel.status_for_test(), (0, true));
            assert!(ui.get_saving());
            assert_eq!(ui.get_save_step(), $first);
            assert_eq!(ui.get_save_fraction(), 0.0);
            $(
                panel.sink().set($phase, 250, 1000);
                i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(100));
                assert_eq!(ui.get_save_step(), $shown);
                assert_eq!(ui.get_save_fraction(), 0.25);
            )+
            panel.end();
            assert!(!ui.get_saving());
            assert!(panel.status_for_test().1, "end hides before the next poll");
            i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(100));
            assert!(!panel.status_for_test().1);
            panel.begin();
            assert_eq!(panel.status_for_test(), (0, true));
            assert_eq!(ui.get_save_step(), $first);
            assert_eq!(ui.get_save_fraction(), 0.0);
            panel.end();
        }
    };
}

live_panel!(
    save_lifecycle_and_phase_mapping,
    SaveProgressPanel,
    UiPhase::Serializing,
    [
        (WriteStep::Serialize, UiPhase::Serializing),
        (WriteStep::Compress, UiPhase::Compressing),
        (WriteStep::Writing, UiPhase::Writing)
    ]
);
live_panel!(
    export_lifecycle_and_phase_mapping,
    ExportProgressPanel,
    UiPhase::Flattening,
    [
        (ExportPhase::Flatten, UiPhase::Flattening),
        (ExportPhase::Serialize, UiPhase::Serializing),
        (ExportPhase::Compress, UiPhase::Compressing),
        (ExportPhase::Write, UiPhase::Writing)
    ]
);

#[test]
fn polling_waits_for_100ms_and_begin_rearms_the_same_worker_cell() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    let panel = SaveProgressPanel::new(ui.as_weak());
    let sink = panel.sink();
    panel.begin();
    sink.set(WriteStep::Writing, 1, 1);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(99));
    assert_eq!(ui.get_save_fraction(), 0.0);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(1));
    assert_eq!(ui.get_save_fraction(), 1.0);
    panel.begin();
    assert_eq!(panel.status_for_test(), (0, true));
    assert_eq!(ui.get_save_step(), UiPhase::Serializing);
    sink.set(WriteStep::Compress, 1, 3);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(100));
    assert_eq!(ui.get_save_step(), UiPhase::Compressing);
    assert_eq!(ui.get_save_fraction(), 333.0 / 1000.0);
}

#[test]
fn headless_panels_still_reset_and_do_not_start_timers() {
    let save = SaveProgressPanel::new(slint::Weak::default());
    let export = ExportProgressPanel::new(slint::Weak::default());
    let _: SaveProgressSink = save.sink();
    let _: ExportProgressSink = export.sink();
    save.sink().set(WriteStep::Writing, 1, 1);
    export.sink().set(ExportPhase::Write, 1, 1);
    assert_ne!(save.status_for_test().0, 0);
    assert_ne!(export.status_for_test().0, 0);
    save.begin();
    export.begin();
    assert_eq!(save.status_for_test(), (0, false));
    assert_eq!(export.status_for_test(), (0, false));
    save.end();
    export.end();
}

#[test]
fn dropping_an_active_panel_releases_its_timer_without_a_tick() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    let save = SaveProgressPanel::new(ui.as_weak());
    save.begin();
    let timer = save.timer_for_test();
    drop(save);
    assert!(timer.upgrade().is_none());
    let export = ExportProgressPanel::new(ui.as_weak());
    export.begin();
    let timer = export.timer_for_test();
    drop(export);
    assert!(timer.upgrade().is_none());
}

#[test]
fn closing_windows_stops_polling_and_dropping_panels_releases_their_timers() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    let weak = ui.as_weak();
    let save = SaveProgressPanel::new(ui.as_weak());
    let export = ExportProgressPanel::new(ui.as_weak());
    save.begin();
    export.begin();
    let save_timer = save.timer_for_test();
    let export_timer = export.timer_for_test();
    drop(ui);
    assert!(weak.upgrade().is_none());
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(100));
    assert!(!save.status_for_test().1);
    assert!(!export.status_for_test().1);
    save.begin();
    export.begin();
    save.end();
    export.end();
    drop(save);
    drop(export);
    assert!(save_timer.upgrade().is_none());
    assert!(export_timer.upgrade().is_none());
}

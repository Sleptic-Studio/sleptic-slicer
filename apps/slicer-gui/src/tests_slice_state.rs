// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::SliceState;
#[test]
fn success_edit_and_reslice() {
    let mut state = SliceState::default();
    state.changed();
    assert_eq!(state, SliceState::Idle);
    state.start();
    state.progress(0.4);
    assert_eq!(state, SliceState::Slicing { progress: 0.4 });
    state.finish(None);
    assert_eq!(state, SliceState::Sliced);
    state.changed();
    assert_eq!(state, SliceState::Stale);
    state.start();
    state.finish(None);
    assert_eq!(state, SliceState::Sliced);
}
#[test]
fn cancellation_failure_and_retry() {
    let mut state = SliceState::Idle;
    state.start();
    state.cancel(false);
    assert_eq!(state, SliceState::Idle);
    state.start();
    state.cancel(true);
    assert_eq!(state, SliceState::Stale);
    state.start();
    state.finish(Some("worker failed".into()));
    assert_eq!(
        state,
        SliceState::Failed {
            msg: "worker failed".into()
        }
    );
    state.start();
    state.finish(None);
    assert_eq!(state, SliceState::Sliced);
}
#[test]
fn late_progress_cannot_revive_finished_run() {
    let mut state = SliceState::Sliced;
    state.progress(0.5);
    assert_eq!(state, SliceState::Sliced);
    state.start();
    state.progress(2.0);
    assert_eq!(state, SliceState::Slicing { progress: 1.0 });
}

#[test]
fn status_summary_keeps_only_the_first_line() {
    let msg = "Slicing failed: mesh validation failed for 1 of the scene's leaves.\n\
               leaf #1: mesh manifold validation failed: 1215 error(s)\n  \
               1215x NonManifoldEdge (e.g. faces #363, #408)";
    assert_eq!(
        super::status_summary(msg),
        "Slicing failed: mesh validation failed for 1 of the scene's leaves."
    );
}

#[test]
fn status_summary_skips_leading_blank_lines_and_cuts_long_lines() {
    assert_eq!(super::status_summary("\n  \n  short  "), "short");
    let long = "x".repeat(500);
    let summary = super::status_summary(&long);
    assert!(summary.ends_with("..."));
    assert_eq!(summary.chars().count(), 140);
    assert_eq!(super::status_summary(""), "");
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! The slice lifecycle, independent of Slint and worker implementation.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum SliceState {
    #[default]
    Idle,
    Slicing {
        progress: f32,
    },
    Sliced,
    Stale,
    Failed {
        msg: String,
    },
}

impl SliceState {
    pub(crate) fn changed(&mut self) {
        if matches!(self, Self::Sliced | Self::Stale) {
            *self = Self::Stale;
        }
    }
    pub(crate) fn start(&mut self) {
        *self = Self::Slicing { progress: 0.0 };
    }
    pub(crate) fn progress(&mut self, value: f32) {
        if matches!(self, Self::Slicing { .. }) {
            *self = Self::Slicing {
                progress: value.clamp(0.0, 1.0),
            };
        }
    }
    pub(crate) fn finish(&mut self, error: Option<String>) {
        *self = error.map_or(Self::Sliced, |msg| Self::Failed { msg });
    }
    pub(crate) fn cancel(&mut self, has_result: bool) {
        *self = if has_result { Self::Stale } else { Self::Idle };
    }
    pub(crate) fn apply(&self, ui: &crate::MainWindow) {
        ui.set_slicing(matches!(self, Self::Slicing { .. }));
        ui.set_slice_estimates_stale(matches!(self, Self::Stale));
        ui.set_slice_export_ready(matches!(self, Self::Sliced));
        ui.set_slice_failed(matches!(self, Self::Failed { .. }));
        ui.set_slice_failure_message(if let Self::Failed { msg } = self {
            status_summary(msg).into()
        } else {
            "".into()
        });
        if let Self::Slicing { progress } = self {
            ui.set_slice_fraction(*progress);
        }
    }
}

/// Longest status-bar summary, in characters, before it is cut with "...".
const STATUS_SUMMARY_MAX_CHARS: usize = 140;

/// One-line status-bar form of a slice failure: the first non-empty line,
/// cut to [`STATUS_SUMMARY_MAX_CHARS`]. Validation failures span many lines
/// (one per error group), which overflowed the single-line status bar; the
/// full text is shown in the error dialog that opens with every failure.
pub(crate) fn status_summary(msg: &str) -> String {
    let line = msg
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if line.chars().count() <= STATUS_SUMMARY_MAX_CHARS {
        line.to_owned()
    } else {
        let cut: String = line.chars().take(STATUS_SUMMARY_MAX_CHARS - 3).collect();
        format!("{}...", cut.trim_end())
    }
}

#[cfg(test)]
#[path = "tests_slice_state.rs"]
mod tests;

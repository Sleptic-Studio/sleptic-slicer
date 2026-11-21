// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! How a worker (loading, saving, exporting) reports progress to the UI
//! thread: one atomic word holding the stage and how far into it, which
//! only ever grows, so an update that arrives late cannot move the bar
//! back.
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// A stage counts in thousandths.
pub(crate) const PROGRESS_PERMILLE_MAX: u32 = 1000;

/// The stage sits above the fraction (`stage << 16 | thousandths`), so any
/// later stage outranks every fraction of an earlier one.
const STAGE_SHIFT: u32 = 16;

/// `done` of `total` in thousandths, rounded down; nothing to do, or more
/// done than there was, counts as complete.
pub(crate) fn per_mille_of(done: u64, total: u64) -> u32 {
    if total == 0 || done >= total {
        return PROGRESS_PERMILLE_MAX;
    }
    // In 128 bits so huge byte counts cannot overflow.
    (u128::from(done) * u128::from(PROGRESS_PERMILLE_MAX) / u128::from(total)) as u32
}

/// The stages of one kind of job, in the order they run.
pub(crate) trait Stages: Copy + PartialEq + 'static {
    const ORDER: &'static [Self];

    /// Where the stage comes in `ORDER`, from 0.
    fn rank(self) -> u32 {
        Self::ORDER.iter().position(|stage| *stage == self).unwrap_or(0) as u32
    }
}

/// The stage and thousandths stored in `word`. A stage number past the
/// last reads as the last stage.
pub(crate) fn read_stage<S: Stages>(word: u32) -> (S, u32) {
    let stage = ((word >> STAGE_SHIFT) as usize).min(S::ORDER.len() - 1);
    (S::ORDER[stage], word & ((1 << STAGE_SHIFT) - 1))
}

/// The writing end, held by the worker; the panel reads the shared word.
pub(crate) struct StagedProgress<S> {
    word: Arc<AtomicU32>,
    kind: PhantomData<S>,
}

impl<S> Clone for StagedProgress<S> {
    fn clone(&self) -> Self {
        StagedProgress { word: Arc::clone(&self.word), kind: PhantomData }
    }
}

impl<S: Stages> StagedProgress<S> {
    /// Writes into `word`, leaving what it holds until the first report.
    pub(crate) fn new(word: Arc<AtomicU32>) -> Self {
        StagedProgress { word, kind: PhantomData }
    }

    /// Back to nothing done (a new job on the same panel).
    pub(crate) fn reset(&self) {
        self.word.store(0, Ordering::Relaxed);
    }

    /// `done` of `total` through `stage`; ignored when the word is already
    /// further along.
    pub(crate) fn set(&self, stage: S, done: u64, total: u64) {
        let thousandths = per_mille_of(done, total).min(PROGRESS_PERMILLE_MAX);
        self.word.fetch_max((stage.rank() << STAGE_SHIFT) | thousandths, Ordering::Relaxed);
    }
}

/// Opening a file.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum OpenStep {
    Parse,
    Priming,
    GpuUpload,
}

impl Stages for OpenStep {
    const ORDER: &'static [Self] = &[OpenStep::Parse, OpenStep::Priming, OpenStep::GpuUpload];
}

pub(crate) type LoadProgressSink = StagedProgress<OpenStep>;

pub(crate) fn decode(word: u32) -> (OpenStep, u32) {
    read_stage(word)
}

#[cfg(test)]
#[path = "tests_progress.rs"]
mod tests_progress;

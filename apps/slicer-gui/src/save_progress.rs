// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The stages of saving a project, reported like loading's
//! (`load_progress::StagedProgress`).
use crate::load_progress::{StagedProgress, Stages, read_stage};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum WriteStep {
    Serialize,
    Compress,
    Writing,
}

impl Stages for WriteStep {
    const ORDER: &'static [Self] = &[WriteStep::Serialize, WriteStep::Compress, WriteStep::Writing];
}

pub(crate) type SaveProgressSink = StagedProgress<WriteStep>;

pub(crate) fn decode(word: u32) -> (WriteStep, u32) {
    read_stage(word)
}

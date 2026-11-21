// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The stages of exporting a scene, reported like loading's
//! (`load_progress::StagedProgress`).
use crate::load_progress::{StagedProgress, Stages, read_stage};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum ExportPhase {
    Flatten,
    Serialize,
    Compress,
    Write,
}

impl Stages for ExportPhase {
    const ORDER: &'static [Self] = &[ExportPhase::Flatten, ExportPhase::Serialize, ExportPhase::Compress, ExportPhase::Write];
}

pub(crate) type ExportProgressSink = StagedProgress<ExportPhase>;

pub(crate) fn decode(word: u32) -> (ExportPhase, u32) {
    read_stage(word)
}

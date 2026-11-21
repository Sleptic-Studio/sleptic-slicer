// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{PlateSlices, ProjectFileType, ProjectPayload, WriteProjectError};

/// Whether a written project carries copies of the profiles it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbedProfiles {
    Yes,
    No,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteProgress {
    Serialize { done: u64, total: u64 },
    Compress { done: u64, total: u64 },
}
#[allow(clippy::too_many_arguments)]
pub fn write_project(
    s: &sc_scene::Scene,
    p: &ProjectPayload,
    e: EmbedProfiles,
    l: Option<&PlateSlices>,
    fingerprint: Option<&str>,
    f: ProjectFileType,
    w: &mut impl std::io::Write,
) -> Result<(), WriteProjectError> {
    write_project_with_progress(s, p, e, l, fingerprint, f, w, &mut |_| {})
}
#[allow(clippy::too_many_arguments)]
pub fn write_project_with_progress(
    s: &sc_scene::Scene,
    p: &ProjectPayload,
    e: EmbedProfiles,
    l: Option<&PlateSlices>,
    fingerprint: Option<&str>,
    f: ProjectFileType,
    w: &mut impl std::io::Write,
    mut progress: impl FnMut(WriteProgress),
) -> Result<(), WriteProjectError> {
    match f {
        ProjectFileType::ThreeMf => crate::threemf::write_threemf_project_with_progress(
            s,
            p,
            e,
            l,
            fingerprint,
            w,
            &mut progress,
        )
        .map_err(WriteProjectError::ThreeMf),
    }
}

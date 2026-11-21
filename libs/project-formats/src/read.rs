// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{ProjectFileType, ProjectPayload, ReadProjectError};

/// Another slicer whose project data a 3MF can carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForeignDialect {
    PrusaSlicer,
    BambuStudioOrca,
    Cura,
}
/// Something reading a project noticed but read past.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadProjectWarning {
    ThreeMf(sc_mesh_formats::messages::LoadThreeMfWarning),
    NewerProjectData { version: i64 },
    UnknownProfileFile { part_name: String },
    OtherSlicersData { dialect: ForeignDialect },
}

#[derive(Debug)]
pub struct LoadedProject {
    pub scene: sc_scene::Scene,
    pub warnings: Vec<ReadProjectWarning>,
    pub payload: Option<ProjectPayload>,
    pub slices: Option<sc_sl1cer::contour::ContourStack>,
    pub slice_fingerprint: Option<String>,
}
pub fn read_project(
    r: &mut (impl std::io::Read + std::io::Seek),
    f: ProjectFileType,
) -> Result<LoadedProject, ReadProjectError> {
    match f {
        ProjectFileType::ThreeMf => {
            crate::threemf::read_threemf_project(r).map_err(ReadProjectError::ThreeMf)
        }
    }
}

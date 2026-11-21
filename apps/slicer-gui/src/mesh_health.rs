// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Mesh validation chooses whether a loaded document needs preparation.

use crate::WorkspaceSection;
use sc_scene::Scene;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum MeshHealth {
    #[default]
    Unknown,
    ProblemFree,
    HasProblems,
}

impl MeshHealth {
    fn from_problems(present: bool) -> Self {
        if present {
            Self::HasProblems
        } else {
            Self::ProblemFree
        }
    }
}

pub(crate) fn scene_mesh_health(scene: &Scene) -> MeshHealth {
    MeshHealth::from_problems(!scene.non_edge_manifold_leaves().is_empty())
}

pub(crate) fn opening_section(health: MeshHealth) -> WorkspaceSection {
    if health == MeshHealth::ProblemFree {
        WorkspaceSection::Transform
    } else {
        WorkspaceSection::Prepare
    }
}

#[cfg(test)]
#[path = "tests_mesh_health.rs"]
mod tests_mesh_health;

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Each workspace section enables a fixed set of editing operations.

use crate::WorkspaceSection;

const LAYOUT: u8 = 1;
const HISTORY: u8 = 2;
const REPAIR: u8 = 4;
const CLIP: u8 = 8;

fn permitted(section: WorkspaceSection, operation: u8) -> bool {
    let permissions = match section {
        WorkspaceSection::Prepare => HISTORY | REPAIR,
        WorkspaceSection::Transform => LAYOUT | HISTORY,
        WorkspaceSection::Slice => CLIP,
    };
    permissions & operation != 0
}

pub(crate) fn layout_tools_allowed_in(section: WorkspaceSection) -> bool {
    permitted(section, LAYOUT)
}

pub(crate) fn undo_allowed_in(section: WorkspaceSection) -> bool {
    permitted(section, HISTORY)
}

pub(crate) fn repair_allowed_in(section: WorkspaceSection) -> bool {
    permitted(section, REPAIR)
}

pub(crate) fn cut_height_for(section: WorkspaceSection, layer_top_z: Option<f64>) -> Option<f32> {
    permitted(section, CLIP)
        .then_some(layer_top_z)
        .flatten()
        .map(|height| height as f32)
}

#[cfg(test)]
#[path = "tests_workspace_section.rs"]
mod tests_workspace_section;

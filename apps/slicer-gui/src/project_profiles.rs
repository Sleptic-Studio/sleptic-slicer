// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Profiles and project files: a save carries the printer and resin
//! profiles it names, and an open checks which named profiles are installed.
//! Profile folders are searched in their order; the first that has a file
//! gives it.
use std::fs;
use std::path::{Path, PathBuf};

use sc_print_primitives::{MaterialId, PrinterId};
use sc_print_profiles::{
    ProfileLibraries, material_extension_path_in_library, material_profile_path_in_library,
    printer_profile_path_in_library,
};
use sc_project_formats::{EmbeddedProfile, ProjectPayload};

/// Whether any profile folder holds the file `path` names there.
fn installed(libraries: &ProfileLibraries, path: impl Fn(&Path) -> PathBuf) -> bool {
    libraries.roots().any(|(root, _)| path(root).is_file())
}

/// The file's bytes from the first profile folder that can read it.
fn first_copy(libraries: &ProfileLibraries, path: impl Fn(&Path) -> PathBuf) -> Option<Vec<u8>> {
    libraries
        .roots()
        .find_map(|(root, _)| fs::read(path(root)).ok())
}

/// Whether a printer profile with this id is installed.
pub(crate) fn printer_installed(libraries: &ProfileLibraries, id: &PrinterId) -> bool {
    installed(libraries, |root| printer_profile_path_in_library(root, id))
}

/// Whether a resin profile with this id is installed.
pub(crate) fn material_installed(libraries: &ProfileLibraries, id: &MaterialId) -> bool {
    installed(libraries, |root| material_profile_path_in_library(root, id))
}

/// Before a save: adds a copy of each named profile the project does not
/// carry yet (printer, resin, and the resin's extension if it has one).
/// Copies already carried are kept as they are; a name found in no folder
/// stays a bare name.
pub(crate) fn with_embedded_profiles(
    payload: &mut ProjectPayload,
    libraries: &ProfileLibraries,
) {
    if let Some(id) = payload.printer.clone()
        && !payload.embeds_printer(&id)
        && let Some(bytes) =
            first_copy(libraries, |root| printer_profile_path_in_library(root, &id))
    {
        payload
            .embedded
            .push(EmbeddedProfile { profile: sc_project_formats::NamedProfile::Printer(id), extension: false, bytes });
    }
    let Some(id) = payload.resin.clone() else {
        return;
    };
    if !payload.embeds_material(&id)
        && let Some(bytes) = first_copy(libraries, |root| {
            material_profile_path_in_library(root, &id)
        })
    {
        payload.embedded.push(EmbeddedProfile { profile: sc_project_formats::NamedProfile::Resin(id.clone()), extension: false, bytes });
    }
    let has_extension = payload.embedded.iter().any(
        |profile| matches!(profile, EmbeddedProfile { profile: sc_project_formats::NamedProfile::Resin(held), extension: true, .. } if *held == id),
    );
    if !has_extension
        && let Some(bytes) = first_copy(libraries, |root| {
            material_extension_path_in_library(root, &id)
        })
    {
        payload
            .embedded
            .push(EmbeddedProfile { profile: sc_project_formats::NamedProfile::Resin(id), extension: true, bytes });
    }
}

#[cfg(test)]
#[path = "tests_project_profiles.rs"]
mod tests;

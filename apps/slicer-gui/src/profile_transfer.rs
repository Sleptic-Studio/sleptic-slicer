// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Preferences package actions use the same profile format as the CLI.

use std::fs::File;
use std::io::{Cursor, Write};
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use rfd::MessageDialogResult;
use sc_print_profiles::{Collision, CollisionReport, ExportManifest, ExportSelection, ImportError};
use sc_print_profiles::{export_package, import_package};

use crate::i18n::ctr;

/// What an exported package holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PackageKind {
    PrinterProfiles,
    ResinProfiles,
}

impl PackageKind {
    pub(crate) fn as_export_selection(self) -> ExportSelection {
        if self == Self::PrinterProfiles { ExportSelection::Printers } else { ExportSelection::Materials }
    }

    /// The save dialog's suggested name.
    pub(crate) fn suggested_file_name(self) -> &'static str {
        if self == Self::PrinterProfiles { "printers.zip" } else { "materials.zip" }
    }
}

/// How an import ended when nothing went wrong.
pub(crate) enum ImportResult {
    Added,
    /// Nothing was written: these profiles exist already (ask, then import
    /// again with `overwrite`).
    WouldOverwrite(CollisionReport),
}

/// Writes the user library's `selection` as a package at `dest`. The
/// package is built in memory first, so a failure never leaves a broken or
/// emptied file where an older package was.
pub(crate) fn write_package(user_library: &Path, selection: &ExportSelection, dest: &Path) -> Result<()> {
    let stamp = ExportManifest::stamped_now(env!("CARGO_PKG_VERSION"));
    let mut zip = Cursor::new(Vec::new());
    let failed = |e: &dyn std::fmt::Display| anyhow!("could not export the profile package: {e}");
    export_package(user_library, selection, &stamp, &mut zip).map_err(|e| failed(&e))?;
    let mut file = File::create(dest)
        .with_context(|| format!("could not create the output archive {}", dest.display()))?;
    file.write_all(zip.get_ref()).map_err(|e| failed(&e))
}

/// Installs the package at `src` into the user library; with `overwrite`
/// existing profiles are replaced, without it they are reported.
pub(crate) fn install_package(user_library: &Path, src: &Path, overwrite: bool) -> Result<ImportResult> {
    let package = File::open(src)
        .with_context(|| format!("could not open the package archive {}", src.display()))?;
    match import_package(package, user_library, overwrite) {
        Ok(()) => Ok(ImportResult::Added),
        Err(ImportError::Collisions(existing)) => Ok(ImportResult::WouldOverwrite(existing)),
        Err(other) => Err(anyhow!("could not import the profile package: {other}")),
    }
}

/// The confirmation text: what the package would replace, by kind.
pub(crate) fn overwrite_warning(report: &CollisionReport) -> String {
    let intro = ctr!("PreferencesPage", "Importing this package would overwrite existing profiles:");
    let groups = [
        (ctr!("PreferencesPage", "Printers"), &report.printers),
        (ctr!("PreferencesPage", "Materials"), &report.materials),
        (ctr!("PreferencesPage", "Material extensions"), &report.material_extensions),
    ];
    let mut text = intro.to_string();
    for (title, clashes) in groups.iter().filter(|(_, clashes)| !clashes.is_empty()) {
        text.push_str(&format!("\n\n{title}:"));
        for Collision { id, .. } in clashes.iter() {
            text.push_str(&format!("\n  - {id}"));
        }
    }
    text
}

/// Whether the user pressed the dialog's overwrite button.
pub(crate) fn chose_overwrite(answer: &MessageDialogResult, overwrite_button: &str) -> bool {
    matches!(answer, MessageDialogResult::Custom(pressed) if pressed == overwrite_button)
}

#[cfg(test)]
mod tests;

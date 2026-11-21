#![deny(missing_docs)]
// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Sleptic Slicer's command line: slices models and projects, checks,
//! repairs and converts meshes, and manages the user's profile library.

mod commands;
mod config;
mod filesystem;
#[path = "recipe/service.rs"]
mod recipe_service;

use std::io::{stderr, stdout};

use anyhow::Result;

use crate::commands::{convert, diagnose, fix, inspect, profiles, slice};
use crate::config::args::{Command, parser};
use crate::config::profile_libraries::resolve_profile_libraries;

fn main() -> Result<()> {
    // Profiles saved before the app had its own folders are copied over once.
    sc_app_paths::migrate_legacy_dirs();
    let request = parser().run();
    let libraries = resolve_profile_libraries();
    // Mesh findings are diagnostics: they go to stderr with the errors.
    let mut notes = stderr();
    match request {
        Command::Slice(job) => slice::slice(*job, &libraries),
        Command::Convert { source: from, source_format: read_as, target: to, target_format: write_as } => {
            convert::convert((from, read_as), (to, write_as))
        }
        Command::Fix { source: from, source_format: read_as, target: to, target_format: write_as, passes } => {
            fix::fix(from, read_as, to, write_as, passes, &mut notes)
        }
        Command::Diagnose { source: from, source_format: read_as } => diagnose::diagnose(from, read_as, &mut notes),
        Command::Inspect { source: from, source_format: read_as } => inspect::inspect(from, read_as, &mut stdout()),
        Command::ProfilesList { kind: which } => profiles::list(which, &libraries),
        Command::ProfilesImport { input: archive, force: overwrite } => profiles::import(archive, overwrite, &libraries),
        Command::ProfilesExport { output: archive, printers: only_printers, materials: only_materials, ids: chosen } => {
            profiles::export(archive, only_printers, only_materials, chosen, &libraries)
        }
        Command::Version => {
            println!("Version: {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        // `help` prints what `--help` prints; the parser returns that page
        // as a "failure" meant for stdout.
        Command::Help => {
            if let Err(page) = parser().run_inner(&["--help"]) {
                println!("{}", page.unwrap_stdout());
            }
            Ok(())
        }
    }
}

// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{
    config::args::ListKind,
    filesystem::{display_path, validate_targetfile_path},
};
use anyhow::{Context, Result, anyhow, bail};
use sc_print_profiles::{
    CollisionReport, ExportManifest, ExportSelection, ImportError, ProfileLibraries,
    export_package, import_package, list_material_ids, list_printer_ids,
    load_material_profile_from_library, load_printer_profile_from_library,
};
use std::{
    ffi::OsString,
    fs,
    io::{self, Cursor, Write},
    path::{Path, PathBuf},
};

fn user_root(libraries: &ProfileLibraries) -> Result<&Path> {
    libraries
        .user
        .as_deref()
        .context("No user profile-library directory is available")
}

pub(crate) fn list(kind: ListKind, libraries: &ProfileLibraries) -> Result<()> {
    render_list(user_root(libraries)?, kind, &mut io::stdout().lock())
}

/// One line per profile in the library at `root`, printers then materials
/// (as `kind` asks): id, name, then size and pixels or brand and family,
/// tab-separated.
fn render_list(root: &Path, kind: ListKind, out: &mut dyn Write) -> Result<()> {
    let (show_printers, show_materials) = (kind != ListKind::Materials, kind != ListKind::Printers);
    for id in if show_printers { list_printer_ids(root)? } else { Vec::new() } {
        let printer = load_printer_profile_from_library(root, &id)
            .with_context(|| format!("could not load printer profile '{id}'"))?;
        let (size, grid) = (printer.hardware.build_volume(), printer.hardware.pixel_resolution());
        let mm = format!("{} x {} x {} mm", size.x_mm(), size.y_mm(), size.z_mm());
        writeln!(out, "{id}	{}	{mm}, {} x {} px", printer.identity.name, grid.x(), grid.y())?;
    }
    for id in if show_materials { list_material_ids(root)? } else { Vec::new() } {
        let resin = load_material_profile_from_library(root, &id)
            .with_context(|| format!("could not load material profile '{id}'"))?;
        let who = &resin.identity;
        writeln!(out, "{id}	{}	{}, {}", who.name, who.brand, who.family)?;
    }
    Ok(())
}

/// Which profiles an export takes: the ids given (alone: they cannot be
/// mixed with a kind), else printers or materials when exactly one is asked
/// for, else everything.
fn export_kind(printers: bool, materials: bool, ids: Vec<String>) -> Result<ExportSelection> {
    let by_kind = match (printers, materials) {
        (true, false) => ExportSelection::Printers,
        (false, true) => ExportSelection::Materials,
        _ => ExportSelection::All,
    };
    match (ids.is_empty(), printers || materials) {
        (true, _) => Ok(by_kind),
        (false, false) => Ok(ExportSelection::Ids(ids)),
        (false, true) => bail!("--id cannot be combined with --printers or --materials"),
    }
}

pub(crate) fn export(
    output: OsString,
    printers: bool,
    materials: bool,
    ids: Vec<String>,
    libraries: &ProfileLibraries,
) -> Result<()> {
    let selection = export_kind(printers, materials, ids)?;
    let target = PathBuf::from(output);
    let resolved = validate_targetfile_path(target.clone())?;
    let manifest = ExportManifest::stamped_now(env!("CARGO_PKG_VERSION"));
    // Construct first: a bad ID or unreadable library must leave an existing archive intact.
    let mut bytes = Cursor::new(Vec::new());
    export_package(user_root(libraries)?, &selection, &manifest, &mut bytes)
        .map_err(|e| anyhow!("could not export the profile package: {e}"))?;
    fs::write(&resolved, bytes.into_inner())
        .with_context(|| format!("Cannot write profile archive {}", display_path(&target)))?;
    writeln!(
        io::stdout().lock(),
        "Exported profiles to {}",
        display_path(&target)
    )?;
    Ok(())
}

fn collision_text(report: &CollisionReport) -> String {
    let groups = [
        ("Printer conflicts:", &report.printers),
        ("Material conflicts:", &report.materials),
        ("Material extension conflicts:", &report.material_extensions),
    ];
    // Ids are padded to the longest in any group, so the paths line up.
    let width = groups.iter().flat_map(|(_, items)| items.iter()).fold(0, |widest, item| widest.max(item.id.len()));
    let mut text = "import would overwrite existing profiles. Use --force to overwrite.".to_owned();
    for (heading, items) in groups {
        if items.is_empty() {
            continue;
        }
        text.push_str("\n\n");
        text.push_str(heading);
        for item in items {
            text.push_str(&format!(
                "\n  - {:width$} (library file: {})",
                item.id,
                display_path(&item.library_path)
            ));
        }
    }
    text
}

pub(crate) fn import(input: OsString, force: bool, libraries: &ProfileLibraries) -> Result<()> {
    let path = PathBuf::from(input);
    let file = fs::File::open(&path)
        .with_context(|| format!("Cannot open profile archive {}", display_path(&path)))?;
    let root = user_root(libraries)?;
    match import_package(file, root, force) {
        Ok(()) => writeln!(
            io::stdout().lock(),
            "Imported profiles into {}",
            display_path(root)
        )?,
        Err(ImportError::Collisions(report)) => bail!("{}", collision_text(&report)),
        Err(error) => return Err(error).context("could not import the profile package"),
    }
    Ok(())
}

#[cfg(test)]
mod tests_packages {
    use super::*;
    use sc_print_profiles::Collision;
    #[test]
    fn selectors_are_explicit_and_conflicting_modes_fail() {
        for (p, m, expected) in [
            (false, false, ExportSelection::All),
            (true, true, ExportSelection::All),
            (true, false, ExportSelection::Printers),
            (false, true, ExportSelection::Materials),
        ] {
            assert_eq!(export_kind(p, m, vec![]).unwrap(), expected);
        }
        let ids = vec!["a".to_owned(), "b".to_owned(), "a".to_owned()];
        assert_eq!(
            export_kind(false, false, ids.clone()).unwrap(),
            ExportSelection::Ids(ids.clone())
        );
        for (p, m) in [(true, false), (false, true), (true, true)] {
            assert!(export_kind(p, m, ids.clone()).is_err());
        }
    }
    #[test]
    fn collision_sections_cover_extensions_and_align_across_groups() {
        let item = |id: &str| Collision {
            id: id.into(),
            library_path: PathBuf::from(format!("profiles/{id}.toml")),
        };
        let report = CollisionReport {
            printers: vec![item("p")],
            materials: vec![],
            material_extensions: vec![item("long-extension")],
        };
        let text = collision_text(&report);
        assert!(text.contains("Use --force to overwrite."));
        assert!(text.contains("Printer conflicts:"));
        assert!(!text.contains("Material conflicts:"));
        assert!(text.contains("Material extension conflicts:"));
        assert!(text.contains("  - p              (library file: profiles/p.toml)"));
        assert!(text.contains("  - long-extension (library file: profiles/long-extension.toml)"));
    }
    #[test]
    fn unavailable_roots_and_output_failures_are_errors() {
        assert!(user_root(&ProfileLibraries::new(None, vec!["bundled".into()])).is_err());
        let dir = tempfile::tempdir().unwrap();
        let mut out = Vec::new();
        render_list(dir.path(), ListKind::All, &mut out).unwrap();
        assert!(out.is_empty());
        fs::create_dir(dir.path().join("printers")).unwrap();
        fs::write(dir.path().join("printers/invalid.toml"), b"invalid").unwrap();
        assert!(render_list(dir.path(), ListKind::Printers, &mut out).is_err());
    }
}

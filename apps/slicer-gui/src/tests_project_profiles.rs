// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use std::fs;
use std::path::Path;

use tempfile::TempDir;

use super::*;

fn printer(id: &str) -> PrinterId {
    PrinterId::new(id).unwrap()
}

fn resin(id: &str) -> MaterialId {
    MaterialId::new(id).unwrap()
}

/// A profile folder holding these files (path inside the folder, text).
fn folder(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}

/// The user folder first, then the bundled ones.
fn folders(user: &Path, bundled: &[&Path]) -> ProfileLibraries {
    ProfileLibraries::new(
        Some(user.to_path_buf()),
        bundled.iter().map(|p| p.to_path_buf()).collect(),
    )
}

fn project(
    printer: Option<PrinterId>,
    resin: Option<MaterialId>,
    embedded: Vec<EmbeddedProfile>,
) -> ProjectPayload {
    ProjectPayload {
        printer,
        resin,
        first_layer_height: None,
        process_overrides: None,
        aa_level: None,
        embedded,
    }
}

fn printer_copy(id: &str, text: &str) -> EmbeddedProfile {
    EmbeddedProfile { profile: sc_project_formats::NamedProfile::Printer(printer(id)), extension: false, bytes: text.as_bytes().to_vec() }
}

fn resin_copy(id: &str, text: &str) -> EmbeddedProfile {
    EmbeddedProfile { profile: sc_project_formats::NamedProfile::Resin(resin(id)), extension: false, bytes: text.as_bytes().to_vec() }
}

#[test]
fn a_profile_counts_as_installed_in_any_folder() {
    let user = folder(&[]);
    let bundled = folder(&[("printers/p.toml", "x = 1"), ("materials/r.toml", "x = 1")]);
    let libraries = folders(user.path(), &[bundled.path()]);
    assert!(printer_installed(&libraries, &printer("p")));
    assert!(material_installed(&libraries, &resin("r")));
    assert!(!printer_installed(&libraries, &printer("r")));
    assert!(!material_installed(&libraries, &resin("p")));
    // A folder of that name is not a profile.
    let odd = folder(&[("printers/dir.toml/inside", "")]);
    assert!(!printer_installed(
        &folders(odd.path(), &[]),
        &printer("dir")
    ));
}

#[test]
fn the_user_folder_wins_over_a_bundled_copy() {
    let user = folder(&[("printers/p.toml", "from user")]);
    let bundled = folder(&[
        ("printers/p.toml", "bundled"),
        ("printers/q.toml", "bundled q"),
    ]);
    let libraries = folders(user.path(), &[bundled.path()]);
    let mut payload = project(Some(printer("p")), None, Vec::new());
    with_embedded_profiles(&mut payload, &libraries);
    assert_eq!(payload.embedded, vec![printer_copy("p", "from user")]);
    let mut payload = project(Some(printer("q")), None, Vec::new());
    with_embedded_profiles(&mut payload, &libraries);
    assert_eq!(payload.embedded, vec![printer_copy("q", "bundled q")]);
}

#[test]
fn names_found_nowhere_stay_bare_names() {
    let user = folder(&[]);
    let mut payload = project(Some(printer("p")), Some(resin("r")), Vec::new());
    with_embedded_profiles(&mut payload, &folders(user.path(), &[]));
    assert_eq!(
        payload,
        project(Some(printer("p")), Some(resin("r")), Vec::new())
    );
}

#[test]
fn a_resin_brings_its_extension_after_it() {
    let user = folder(&[
        ("materials/r.toml", "resin"),
        ("materials/extensions/r.toml", "extension"),
    ]);
    let mut payload = project(None, Some(resin("r")), Vec::new());
    with_embedded_profiles(&mut payload, &folders(user.path(), &[]));
    let extension = EmbeddedProfile { profile: sc_project_formats::NamedProfile::Resin(resin("r")), extension: true, bytes: b"extension".to_vec() };
    assert_eq!(
        payload.embedded,
        vec![resin_copy("r", "resin"), extension.clone()]
    );
    // Saving again adds nothing.
    with_embedded_profiles(&mut payload, &folders(user.path(), &[]));
    assert_eq!(payload.embedded, vec![resin_copy("r", "resin"), extension]);
}

#[test]
fn copies_already_carried_are_kept_and_the_rest_added() {
    let user = folder(&[
        ("printers/p.toml", "installed"),
        ("materials/r.toml", "installed resin"),
    ]);
    let mut payload = project(
        Some(printer("p")),
        Some(resin("r")),
        vec![printer_copy("p", "carried")],
    );
    with_embedded_profiles(&mut payload, &folders(user.path(), &[]));
    assert_eq!(
        payload.embedded,
        vec![
            printer_copy("p", "carried"),
            resin_copy("r", "installed resin")
        ]
    );
    // A carried resin copy for another resin does not count.
    let mut payload = project(None, Some(resin("r")), vec![resin_copy("s", "other")]);
    with_embedded_profiles(&mut payload, &folders(user.path(), &[]));
    assert_eq!(
        payload.embedded,
        vec![resin_copy("s", "other"), resin_copy("r", "installed resin")]
    );
}
